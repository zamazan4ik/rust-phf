//! Lookup throughput of `phf::Map`, `phf::StrMap` and `std::HashMap` on the
//! key sets of the ConstexprCore/perfect_hash benchmarks, for streams of
//! hits only, misses only, and a uniform mix of the two pools.
//!
//! Run with `--features ptrhash` to measure the ptrhash layout of `phf::Map`.

use std::collections::HashMap;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use phf_bench::DATA_SETS;

const STREAM_LEN: usize = 1 << 14;

fn stream(pool: &[&'static str], seed: u64) -> Vec<&'static str> {
    let mut rng = fastrand::Rng::with_seed(seed);
    (0..STREAM_LEN).map(|_| pool[rng.usize(..pool.len())]).collect()
}

fn lookups(c: &mut Criterion) {
    let map_name = if cfg!(feature = "ptrhash") {
        "phf::Map(ptrhash)"
    } else {
        "phf::Map"
    };
    for set in DATA_SETS {
        let std_map: HashMap<&str, u32> = set.hits.iter().enumerate().map(|(i, k)| (*k, i as u32)).collect();
        let mixed_pool: Vec<&str> = set.hits.iter().chain(set.misses).copied().collect();
        let workloads = [
            ("hits", stream(set.hits, 42)),
            ("misses", stream(set.misses, 42)),
            ("mixed", stream(&mixed_pool, 42)),
        ];

        let mut group = c.benchmark_group(set.name);
        group.throughput(Throughput::Elements(STREAM_LEN as u64));
        for (workload, input) in &workloads {
            // Sum the values found so the lookups cannot be optimised out.
            group.bench_with_input(BenchmarkId::new("phf::StrMap", workload), input, |b, input| {
                b.iter(|| input.iter().map(|k| set.str_map.get(black_box(k)).copied().unwrap_or(0)).sum::<u32>())
            });
            group.bench_with_input(BenchmarkId::new(map_name, workload), input, |b, input| {
                b.iter(|| input.iter().map(|k| set.map.get(black_box(k)).copied().unwrap_or(0)).sum::<u32>())
            });
            group.bench_with_input(BenchmarkId::new("std::HashMap", workload), input, |b, input| {
                b.iter(|| input.iter().map(|k| std_map.get(black_box(k)).copied().unwrap_or(0)).sum::<u32>())
            });
        }
        group.finish();
    }
}

criterion_group!(benches, lookups);
criterion_main!(benches);
