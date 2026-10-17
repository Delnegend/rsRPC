use aho_corasick::{AhoCorasick, PatternID};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use std::vec;

#[cfg(not(target_os = "linux"))]
use sysinfo::System;

use super::super::DetectableActivity;
use crate::log;
use crate::ProcessCallback;

#[derive(Default, Clone)]
pub struct ProcessScanState {
  pub obs_open: bool,
}

#[derive(Default)]
pub struct ProcessEventListeners {
  pub on_process_scan_complete: Option<Arc<Mutex<ProcessCallback>>>,
}

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
  custom_detectables: Arc<Mutex<Vec<Arc<DetectableActivity>>>>,
  scanning: Arc<AtomicBool>,

  detectable_indexes: Arc<Mutex<Vec<[usize; 2]>>>,
  detectable_ac: Arc<Mutex<AhoCorasick>>,

  custom_detectable_indexes: Arc<Mutex<Vec<[usize; 2]>>>,
  custom_detectable_ac: Arc<Mutex<Option<AhoCorasick>>>,

  pub detectable_list: Vec<Arc<DetectableActivity>>,
  pub event_sender: mpsc::Sender<ProcessDetectedEvent>,

  event_listeners: Arc<Mutex<ProcessEventListeners>>,

  #[cfg(not(target_os = "linux"))]
  sysinfo: Arc<Mutex<System>>,
}

unsafe impl Sync for ProcessServer {}

static NULL_ACTIVITY: OnceLock<Arc<DetectableActivity>> = OnceLock::new();

impl ProcessServer {
  pub fn new(
    detectable: Vec<Arc<DetectableActivity>>,
    event_sender: mpsc::Sender<ProcessDetectedEvent>,
    event_listeners: ProcessEventListeners,
  ) -> Self {
    log!("[Process Scanner] Building Aho-Corasick patterns for main detectable activities...");
    let (ac, idx) = build_ac_patterns(&detectable).expect("failed to build AC patterns");
    log!("[Process Scanner] Done!");

    ProcessServer {
      scanning: Arc::new(AtomicBool::new(false)),
      detected_list: Arc::new(Mutex::new(vec![])),
      custom_detectables: Arc::new(Mutex::new(vec![])),
      detectable_list: detectable,
      event_sender,

      detectable_indexes: Arc::new(Mutex::new(idx)),
      detectable_ac: Arc::new(Mutex::new(ac)),
      custom_detectable_indexes: Arc::new(Mutex::new(vec![])),
      custom_detectable_ac: Arc::new(Mutex::new(None)),

      event_listeners: Arc::new(Mutex::new(event_listeners)),

      #[cfg(not(target_os = "linux"))]
      sysinfo: Arc::new(Mutex::new(System::new())),
    }
  }

  fn update_custom_detectables(&self) {
    log!("[Process Scanner] Updating Aho-Corasick patterns for custom detectable activities...");
    let cd = self.custom_detectables.lock().unwrap().clone();
    let (ac, idx) = build_ac_patterns(&cd).expect("failed to build AC patterns");
    if !idx.is_empty() {
      *self.custom_detectable_ac.lock().unwrap() = Some(ac);
    } else {
      *self.custom_detectable_ac.lock().unwrap() = None;
    }
    *self.custom_detectable_indexes.lock().unwrap() = idx;
    log!("[Process Scanner] Done!");
  }

  pub fn append_detectables(&mut self, detectable: Vec<DetectableActivity>) {
    self
      .custom_detectables
      .lock()
      .unwrap()
      .extend(detectable.into_iter().map(Arc::new));
    self.update_custom_detectables();
  }

  pub fn remove_detectable_by_name(&mut self, name: String) {
    self
      .custom_detectables
      .lock()
      .unwrap()
      .retain(|x| x.name != name);
    self.update_custom_detectables();
  }

  pub fn start(&self) {
    let wait_time = Duration::from_secs(10);
    let clone = self.clone();

    self.update_custom_detectables();

    std::thread::spawn(move || loop {
      let detected = match clone.scan_for_processes() {
        Ok(detected) => detected,
        Err(err) => {
          log!("[Process Scanner] Error while scanning processes: {}", err);
          std::thread::sleep(wait_time);
          continue;
        }
      };

      if !detected.is_empty() {
        let send = {
          let detected_list = clone.detected_list.lock().unwrap();
          if detected_list.is_empty() {
            true
          } else {
            detected[0].id != detected_list[0].id
          }
        };
        if send {
          clone
            .event_sender
            .send(ProcessDetectedEvent {
              activity: detected[0].clone(),
            })
            .unwrap();
        }
      }

      if detected.is_empty() {
        let null = NULL_ACTIVITY.get_or_init(|| {
          Arc::new(DetectableActivity {
            bot_public: None,
            bot_require_code_grant: None,
            cover_image: None,
            description: None,
            developers: None,
            executables: None,
            flags: None,
            guild_id: None,
            hook: false,
            icon: None,
            id: "null".to_string(),
            name: "".to_string(),
            publishers: None,
            rpc_origins: None,
            splash: None,
            third_party_skus: None,
            type_field: None,
            verify_key: None,
            primary_sku_id: None,
            slug: None,
            aliases: None,
            overlay: None,
            overlay_compatibility_hook: None,
            privacy_policy_url: None,
            terms_of_service_url: None,
            eula_id: None,
            deeplink_uri: None,
            tags: None,
            pid: None,
            timestamp: None,
          })
        });
        clone
          .event_sender
          .send(ProcessDetectedEvent {
            activity: null.clone(),
          })
          .unwrap();
      }

      if !detected.is_empty() {
        let send = {
          let detected_list = clone.detected_list.lock().unwrap();
          detected_list.is_empty() || detected[0].id != detected_list[0].id
        };
        if send {
          *clone.detected_list.lock().unwrap() = detected;
        }
      } else {
        *clone.detected_list.lock().unwrap() = detected;
      }

      std::thread::sleep(wait_time);
    });
  }

  #[cfg(not(target_os = "linux"))]
  pub fn process_list(&self) -> Result<Vec<Exec>, Box<dyn std::error::Error>> {
    use std::path::Path;
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

    let mut processes = Vec::new();
    let mut sys = self.sysinfo.lock().unwrap();
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
          cmd
            .map(|x| x.to_string_lossy())
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

      if let Ok(cmdline) = fs::read_to_string(path.join("cmdline")) {
        if !cmdline.is_empty() {
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

    log!("[Process Scanner] Process scan triggered");

    let detectable_ac = self.detectable_ac.lock().unwrap().clone();
    let custom_ac = self.custom_detectable_ac.lock().unwrap().clone();
    let detectable_indexes = self.detectable_indexes.lock().unwrap().clone();
    let custom_detectable_indexes = self.custom_detectable_indexes.lock().unwrap().clone();
    let custom_detectables = self.custom_detectables.lock().unwrap().clone();
    let detectable_list = self.detectable_list.clone();

    let mut obs_open = false;

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
        // OBS substring check on normalized path
        if !obs_open && (process_path.contains("obs64") || process_path.contains("streamlabs")) {
          obs_open = true;
        }

        reversed_path.clear();
        reversed_path.extend(process_path.chars().rev());

        let (obj, exe_index) = if let Some(mat) = detectable_ac.find(&reversed_path) {
          let pattern_id: PatternID = mat.pattern();
          let exe_index = detectable_indexes[pattern_id.as_usize()];
          (&detectable_list[exe_index[0]], exe_index[1])
        } else {
          let mat = custom_ac
            .as_ref()
            .and_then(|custom| custom.find(&reversed_path))?;
          let pattern_id: PatternID = mat.pattern();
          let exe_index = custom_detectable_indexes[pattern_id.as_usize()];
          (&custom_detectables[exe_index[0]], exe_index[1])
        };

        let executable = &obj.executables.as_ref().unwrap()[exe_index];

        if let Some(exec_args) = &executable.arguments {
          if executable.name.starts_with(">")
            && !process
              .arguments
              .as_ref()
              .is_some_and(|args| args.contains(exec_args))
          {
            return None;
          }
        }

        let mut new_activity = (**obj).clone();
        new_activity.pid = Some(process.pid);
        new_activity.timestamp = Some(format!(
          "{:?}",
          std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
        ));
        Some(Arc::new(new_activity))
      })
      .collect();

    if let Some(callback) = self
      .event_listeners
      .lock()
      .unwrap()
      .on_process_scan_complete
      .as_ref()
    {
      callback.lock().unwrap()(ProcessScanState { obs_open });
    }

    detected_list.shrink_to_fit();

    log!("[Process Scanner] Process scan complete");

    Ok(detected_list)
  }
}

#[allow(clippy::type_complexity)]
fn build_ac_patterns(
  detectables: &[Arc<DetectableActivity>],
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

  let ac = AhoCorasick::new(exe_patterns).map_err(|e| format!("...: {e}"))?;
  Ok((ac, exe_indexes))
}
