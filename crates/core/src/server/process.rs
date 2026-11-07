use aho_corasick::AhoCorasick;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use std::vec;

#[cfg(not(target_os = "linux"))]
use sysinfo::System;

use super::super::DetectableActivity;
use crate::detection::null_activity;
use tracing::{debug, error};

#[derive(Clone)]
pub struct Exec {
    pub pid: u64,
    pub path: String,
    pub arguments: Option<String>,
}

#[derive(Clone)]
pub struct ProcessDetectedEvent {
    pub activity: Arc<DetectableActivity>,
}

/// A coherent snapshot of a detectable set: the compiled Aho-Corasick
/// automaton, the `[activity, executable]` index table it maps into, and the
/// activities themselves.
///
/// Keeping all three behind one mutex means a rebuild can never tear against
/// an in-flight scan, which would otherwise index with offsets from a
/// different revision of the list.
struct DetectState {
    detectables: Vec<DetectableActivity>,
    ac: Option<AhoCorasick>,
    indexes: Vec<[usize; 2]>,
}

impl DetectState {
    /// Compile `detectables` into a state; build failures keep the list but
    /// leave the matcher empty (logged) so one bad list cannot poison scans.
    fn build(detectables: Vec<DetectableActivity>) -> Self {
        match build_ac_patterns(&detectables) {
            Ok((ac, indexes)) => Self {
                detectables,
                ac: if indexes.is_empty() { None } else { Some(ac) },
                indexes,
            },
            Err(err) => {
                error!("Detectable patterns are unusable, nothing will match: {err}");
                Self {
                    detectables,
                    ac: None,
                    indexes: Vec::new(),
                }
            }
        }
    }

    /// First activity whose executables match `reversed_path`.
    ///
    /// Bounds-checked end to end: a torn or stale snapshot yields `None`
    /// instead of a panic.
    fn first_match(&self, reversed_path: &str) -> Option<(&DetectableActivity, usize)> {
        let mat = self.ac.as_ref()?.find(reversed_path)?;
        let exe_map = self.indexes.get(mat.pattern().as_usize())?;
        let activity = self.detectables.get(exe_map[0])?;
        Some((activity, exe_map[1]))
    }
}

struct ScanGuard {
    flag: Arc<AtomicBool>,
}
impl Drop for ScanGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
pub struct ProcessServer {
    detected_list: Arc<Mutex<Vec<Arc<DetectableActivity>>>>,
    scanning: Arc<AtomicBool>,

    main_state: Arc<DetectState>,
    pub event_sender: mpsc::Sender<ProcessDetectedEvent>,

    #[cfg(not(target_os = "linux"))]
    sysinfo: Arc<Mutex<System>>,
}

unsafe impl Sync for ProcessServer {}

static NULL_ACTIVITY: OnceLock<Arc<DetectableActivity>> = OnceLock::new();

impl ProcessServer {
    pub fn new(
        detectable: Vec<DetectableActivity>,
        event_sender: mpsc::Sender<ProcessDetectedEvent>,
    ) -> Self {
        ProcessServer {
            scanning: Arc::new(AtomicBool::new(false)),
            detected_list: Arc::new(Mutex::new(vec![])),

            main_state: {
                debug!("Building Aho-Corasick patterns for detectable activities...");
                let mut all_detectables = detectable;
                all_detectables.extend(crate::custom_games::builtin_custom_games());
                debug!("Done building Aho-Corasick patterns for detectable activities");
                Arc::new(DetectState::build(all_detectables))
            },
            event_sender,

            #[cfg(not(target_os = "linux"))]
            sysinfo: Arc::new(Mutex::new(System::new())),
        }
    }

    pub fn start(&self) {
        let wait_time = Duration::from_secs(10);
        let clone = self.clone();

        std::thread::spawn(move || {
            loop {
                let detected = match clone.scan_for_processes() {
                    Ok(detected) => detected,
                    Err(err) => {
                        error!("Error while scanning processes: {}", err);
                        std::thread::sleep(wait_time);
                        continue;
                    }
                };

                {
                    let mut previous = clone
                        .detected_list
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());

                    if detected.is_empty() {
                        // Nothing detected: forward the shared "no activity"
                        // marker so clients clear their presence.
                        let null = NULL_ACTIVITY.get_or_init(|| Arc::new(null_activity()));
                        if let Err(err) = clone.event_sender.send(ProcessDetectedEvent {
                            activity: Arc::clone(null),
                        }) {
                            error!("Process event receiver is gone, stopping scan loop: {err}");
                            break;
                        }
                        *previous = detected;
                    } else {
                        let mut current = detected;
                        // Keep original start timestamp so the timer doesn't reset when details change
                        if !previous.is_empty()
                            && current[0].id == previous[0].id
                            && let Some(orig_ts) = &previous[0].timestamp
                        {
                            let mut updated = (*current[0]).clone();
                            updated.timestamp = Some(orig_ts.clone());
                            current[0] = Arc::new(updated);
                        }

                        let has_changed = previous.is_empty()
                            || current[0].id != previous[0].id
                            || current[0].details != previous[0].details
                            || current[0].state != previous[0].state;

                        if has_changed {
                            if let Err(err) = clone.event_sender.send(ProcessDetectedEvent {
                                activity: current[0].clone(),
                            }) {
                                error!("Process event receiver is gone, stopping scan loop: {err}");
                                break;
                            }
                            *previous = current;
                        }
                    }
                }

                std::thread::sleep(wait_time);
            }
        });
    }

    #[cfg(not(target_os = "linux"))]
    pub fn process_list(&self) -> Result<Vec<Exec>, Box<dyn std::error::Error>> {
        use std::path::Path;
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

        let mut processes = Vec::new();
        let mut sys = self.sysinfo.lock().unwrap_or_else(|p| p.into_inner());
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );

        for proc in sys.processes() {
            let mut cmd = proc.1.cmd().iter();
            processes.push(Exec {
                pid: proc.0.to_string().parse::<u64>()?,
                path: proc.1.exe().unwrap_or(Path::new("")).display().to_string(),
                arguments: cmd.next().map(|_| {
                    cmd.map(|x| x.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" ")
                }),
            });
        }

        Ok(processes)
    }

    #[cfg(target_os = "linux")]
    pub fn process_list() -> Result<Vec<Exec>, Box<dyn std::error::Error>> {
        use std::fs;

        let proc_list = fs::read_dir("/proc")?.filter(|e| {
            if let Ok(entry) = e {
                return entry
                    .file_name()
                    .to_str()
                    .unwrap_or("")
                    .parse::<u64>()
                    .is_ok();
            }
            false
        });
        let mut processes = Vec::with_capacity(256);

        for entry in proc_list {
            let entry = entry?;
            let path = entry.path();

            if let Ok(cmdline) = fs::read_to_string(path.join("cmdline"))
                && !cmdline.is_empty()
            {
                let pid: u64 = path
                    .file_name()
                    .ok_or("Invalid path")?
                    .to_str()
                    .ok_or("Invalid path")?
                    .parse()?;
                let mut cmd_iter = cmdline.split('\0');
                let cmd_path = cmd_iter.next().unwrap_or("").to_string();
                let rest: Vec<&str> = cmd_iter.collect();
                let cmd_args = if rest.is_empty() {
                    None
                } else {
                    Some(rest.join(" "))
                };
                processes.push(Exec {
                    pid,
                    path: cmd_path,
                    arguments: cmd_args,
                });
            }
        }

        Ok(processes)
    }

    pub fn scan_for_processes(
        &self,
    ) -> Result<Vec<Arc<DetectableActivity>>, Box<dyn std::error::Error>> {
        let _guard = {
            if self
                .scanning
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                return Err("already in progress".into());
            }
            ScanGuard {
                flag: self.scanning.clone(),
            }
        };

        #[cfg(not(target_os = "linux"))]
        let processes = self.process_list()?;
        #[cfg(target_os = "linux")]
        let processes = ProcessServer::process_list()?;

        debug!("Process scan triggered");

        let mut process_path = String::with_capacity(256);
        let mut reversed_path = String::with_capacity(256);

        let mut detected_list: Vec<Arc<DetectableActivity>> = processes
            .iter()
            .filter_map(|process| {
                process_path.clear();
                process_path.extend(process.path.chars().flat_map(|c| c.to_lowercase()));
                if process_path.contains('\\') {
                    process_path = process_path.replace('\\', "/");
                }
                // single pass slash fix already done via replace; ensure leading slash
                if !process_path.starts_with('/') {
                    process_path.insert(0, '/');
                }

                reversed_path.clear();
                reversed_path.extend(process_path.chars().rev());

                let (obj, exe_index) = self.main_state.first_match(&reversed_path)?;

                let executable = match obj
                    .executables
                    .as_ref()
                    .and_then(|exes| exes.get(exe_index))
                {
                    Some(executable) => executable,
                    None => return None,
                };

                if let Some(exec_args) = &executable.arguments
                    && executable.name.starts_with(">")
                    && !process
                        .arguments
                        .as_ref()
                        .is_some_and(|args| args.contains(exec_args))
                {
                    return None;
                }

                let mut new_activity = obj.clone();
                new_activity.pid = Some(process.pid);
                new_activity.timestamp = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis().to_string())
                        .unwrap_or_else(|_| "0".to_string()),
                );
                crate::custom_games::enrich_custom_game(&mut new_activity);
                Some(Arc::new(new_activity))
            })
            .collect();

        detected_list.shrink_to_fit();

        debug!("Process scan complete");

        Ok(detected_list)
    }
}

#[allow(clippy::type_complexity)]
fn build_ac_patterns(
    detectables: &[DetectableActivity],
) -> Result<(AhoCorasick, Vec<[usize; 2]>), Box<dyn std::error::Error>> {
    let mut exe_patterns: Vec<String> = Vec::new();
    let mut exe_indexes: Vec<[usize; 2]> = Vec::new();

    for (activity_index, activity) in detectables.iter().enumerate() {
        if let Some(executables) = &activity.executables {
            for (exe_index, executable) in executables.iter().enumerate() {
                if executable.is_launcher {
                    continue;
                }

                let mut exec_name = executable.name.replace('\\', "/").to_lowercase();

                if exec_name.starts_with(">") {
                    exec_name.replace_range(0..1, "/");
                } else if !exec_name.starts_with("/") {
                    exec_name.insert(0, '/');
                }

                exe_patterns.push(exec_name.chars().rev().collect::<String>());
                exe_indexes.push([activity_index, exe_index]);
            }
        }
    }

    let ac = AhoCorasick::new(exe_patterns).map_err(|e| e.to_string())?;
    Ok((ac, exe_indexes))
}
