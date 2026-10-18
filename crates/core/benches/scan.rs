use criterion::{Criterion, criterion_group, criterion_main};
use rsrpc_core::{ProcessEventListeners, ProcessServer};
use std::sync::mpsc;

fn bench_scan_for_processes(c: &mut Criterion) {
    c.bench_function("scan/scan_for_processes real /proc", |b| {
        // Empty detectable set: measures raw /proc enumeration + matcher overhead.
        let (tx, _rx) = mpsc::channel();
        let server = ProcessServer::new(vec![], tx, ProcessEventListeners::default());
        b.iter(|| {
            let _ = server.scan_for_processes();
        });
    });
}

criterion_group!(benches, bench_scan_for_processes);
criterion_main!(benches);
