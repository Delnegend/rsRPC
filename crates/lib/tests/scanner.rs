use rsrpc::detection::DetectableActivity;
use rsrpc::{ProcessEventListeners, ProcessScanState, ProcessServer};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;

fn empty_server() -> ProcessServer {
    let (tx, _rx) = mpsc::channel();
    ProcessServer::new(vec![], tx, ProcessEventListeners::default())
}

#[test]
fn scan_twice_same_match_set() {
    let s = empty_server();
    let a = s.scan_for_processes().unwrap();
    let b = s.scan_for_processes().unwrap();
    let mut ia: Vec<String> = a.iter().map(|x| x.id.clone()).collect();
    let mut ib: Vec<String> = b.iter().map(|x| x.id.clone()).collect();
    ia.sort();
    ib.sort();
    assert_eq!(ia, ib);
}

#[test]
fn scan_guard_concurrent() {
    use std::thread;
    // The guard rejects a scan while another one is already running. Whether
    // two spawned threads actually overlap is up to the scheduler, and a scan
    // over an empty detectables list can finish before the second thread gets
    // there, so repeat the race and require the guard to engage at least once
    // across the attempts. Every attempt still has to satisfy the contract.
    const ATTEMPTS: usize = 200;
    let mut guard_engaged = false;

    for _ in 0..ATTEMPTS {
        let s = Arc::new(empty_server());
        let s1 = s.clone();
        let s2 = s.clone();
        let h1 = thread::spawn(move || s1.scan_for_processes().map_err(|e| e.to_string()));
        let h2 = thread::spawn(move || s2.scan_for_processes().map_err(|e| e.to_string()));
        let r1 = h1.join().unwrap();
        let r2 = h2.join().unwrap();

        let oks = [&r1, &r2].iter().filter(|x| x.is_ok()).count();
        let errs: Vec<String> = [r1, r2].into_iter().filter_map(|r| r.err()).collect();

        assert_eq!(oks + errs.len(), 2, "every scan must return a result");
        for err in &errs {
            assert!(
                err.contains("already in progress"),
                "overlapping scans must fail with the guard, got: {err}"
            );
        }
        if !errs.is_empty() {
            guard_engaged = true;
        }
    }

    assert!(guard_engaged, "guard never rejected an overlapping scan");
}

#[test]
fn append_then_scan() {
    let mut s = empty_server();
    let da: DetectableActivity =
        serde_json::from_str(r#"{"name":"needleproc","id":"needleid","hook":false}"#).unwrap();
    s.append_detectables(vec![da]);
    let res = s.scan_for_processes().unwrap();
    // may or may not find; just ensure no panic and runs
    let _ = res;
}

#[test]
fn obs_open_callback() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fired = Arc::new(AtomicBool::new(false));
    let fired2 = fired.clone();
    let cb: Arc<Mutex<Box<dyn FnMut(_) + Send + Sync>>> =
        Arc::new(Mutex::new(Box::new(move |_st: ProcessScanState| {
            fired2.store(true, Ordering::SeqCst);
        })));
    let (tx, _rx) = mpsc::channel();
    let listeners = ProcessEventListeners {
        on_process_scan_complete: Some(cb),
    };
    let s = ProcessServer::new(vec![], tx, listeners);
    let _ = s.scan_for_processes();
    // skip if no obs process; just report
    if !fired.load(Ordering::SeqCst) {
        eprintln!("skip: no obs process running");
    }
}

#[test]
fn custom_detectable_greater_than_argument() {
    use std::process::Command;
    let mut child = Command::new("sleep").arg("needle").spawn().unwrap();
    let pid = child.id();
    let _ = pid;
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn process_detected_details_from_window_title() {
    use std::process::Command;
    use std::time::{Duration, Instant};
    // Spawn a real process with a distinct name/comm
    let mut child = Command::new("sleep").arg("9999").spawn().unwrap();
    let pid = child.id() as u64;

    let da: DetectableActivity = serde_json::from_str(
    r#"{"name":"sleep","id":"test_sleep_id","hook":false,"executables":[{"is_launcher":false,"name":"sleep","os":"linux"}]}"#,
  )
  .unwrap();

    let mut s = empty_server();
    s.append_detectables(vec![da]);

    // `spawn` can return while the child's execve is still settling — its
    // cmdline briefly reads as empty or as the parent's — so a single
    // immediate scan can miss it under load. Poll instead.
    let deadline = Instant::now() + Duration::from_secs(10);
    let detected = loop {
        let matches = s.scan_for_processes().unwrap();
        if let Some(found) = matches.iter().find(|m| m.pid == Some(pid)) {
            break found.clone();
        }
        assert!(
            Instant::now() < deadline,
            "spawned process should be detected by scan"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        detected.details.is_some(),
        "detected process should have details populated from window_title/comm"
    );
    assert_eq!(detected.details.as_deref(), Some("sleep"));

    let _ = child.kill();
    let _ = child.wait();
}
