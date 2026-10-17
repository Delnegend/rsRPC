use criterion::{criterion_group, criterion_main, Criterion};
use rsrpc::{encode, encode_into, get_url_params, PacketType};

fn bench_encode(c: &mut Criterion) {
    c.bench_function("ipc/encode frame 1KiB", |b| {
        let payload = "x".repeat(1024);
        b.iter(|| {
            let _ = encode(PacketType::Frame, &payload);
        });
    });
}

fn bench_encode_into(c: &mut Criterion) {
    c.bench_function("ipc/encode_into frame 1KiB", |b| {
        let body = vec![b'x'; 1024];
        b.iter(|| {
            let mut out = Vec::with_capacity(8 + body.len());
            encode_into(&mut out, PacketType::Frame as u32, &body);
            let _ = out;
        });
    });
}

fn bench_from_u32(c: &mut Criterion) {
    c.bench_function("ipc/PacketType::from_u32", |b| {
        b.iter(|| {
            let _ = PacketType::from_u32(1);
        });
    });
}

fn bench_get_url_params(c: &mut Criterion) {
    // New signature: get_url_params(&str) -> UrlParams (borrowed struct).
    c.bench_function("ipc/get_url_params", |b| {
        b.iter(|| {
            let _ = get_url_params("/callback?code=abc123&state=xyz&client_id=123456789");
        });
    });
}

criterion_group!(
    benches,
    bench_encode,
    bench_encode_into,
    bench_from_u32,
    bench_get_url_params
);
criterion_main!(benches);
