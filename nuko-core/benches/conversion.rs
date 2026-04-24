//! 変換エンジンのベンチマーク

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nuko_core::input::RomajiConverter;

fn romaji_conversion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("romaji_conversion");

    group.bench_function("simple", |b| {
        b.iter(|| {
            let mut conv = RomajiConverter::new();
            conv.convert(black_box("nihongo")).unwrap()
        })
    });

    group.bench_function("complex", |b| {
        b.iter(|| {
            let mut conv = RomajiConverter::new();
            conv.convert(black_box("toukyoutokkyokyokakyoku")).unwrap()
        })
    });

    group.bench_function("with_sokuon", |b| {
        b.iter(|| {
            let mut conv = RomajiConverter::new();
            conv.convert(black_box("kitte")).unwrap()
        })
    });

    group.finish();
}

criterion_group!(benches, romaji_conversion_benchmark);
criterion_main!(benches);
