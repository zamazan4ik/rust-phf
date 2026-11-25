//! Generator for any number of keys of any length, ported from
//! `detail/wide_generator.h` of ConstexprCore/perfect_hash: a PTHash-style
//! pilot table over a multiplicative hash of the key's 64-bit lanes.
//!
//! For each seed, every key gets a bucket (top hash bits) and a base slot
//! (the next bits). Buckets are placed largest first; a bucket's pilot is the
//! smallest offset that moves all its keys onto free slots. Two keys sharing a
//! bucket and a base can never be separated, so such seeds are skipped
//! without a search.

use phf_shared::strhash::{wide_base, wide_bucket, wide_hash};

use super::{StrHashKind, StrHashState};

/// Seeds tried per (table size, hash variant).
const SEEDS: u64 = 48;

/// splitmix64 finaliser.
fn splitmix(i: u64) -> u64 {
    let mut z = i.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The odd multiplier of `lane` for `seed`.
fn multiplier(seed: u64, lane: u64) -> u64 {
    splitmix((seed << 32) ^ lane) | 1
}

/// Lanes the hash reads for keys of at most `max_len` bytes. Up to 15 bytes,
/// the length is folded into the top byte of the last lane.
fn lanes_for(max_len: usize) -> (usize, bool) {
    match max_len {
        0..=7 => (1, true),
        8..=15 => (2, true),
        _ => (max_len.div_ceil(8), false),
    }
}

/// Shift that leaves the top `bits` bits of a `u64`. With no bits, any shift
/// works because the result is masked to zero; 63 keeps it in range.
fn top_bits_shift(bits: u32) -> u32 {
    if bits == 0 { 63 } else { 64 - bits }
}

struct Params {
    muls: Vec<u64>,
    len_mul: u64,
    len_in_lane: bool,
    strong: bool,
    buckets: usize,
    bucket_shift: u32,
    base_shift: u32,
}

fn try_place(keys: &[&[u8]], m: usize, p: &Params) -> Option<(Vec<u32>, Vec<usize>)> {
    let mask = m - 1;
    let mut bucket = Vec::with_capacity(keys.len());
    let mut base = Vec::with_capacity(keys.len());
    for key in keys {
        let h = wide_hash(key, &p.muls, p.len_mul, p.len_in_lane, p.strong);
        bucket.push(wide_bucket(h, p.bucket_shift, p.buckets));
        base.push(wide_base(h, p.base_shift, mask));
    }

    // Counting sort keys by bucket.
    let mut start = vec![0usize; p.buckets + 1];
    for &b in &bucket {
        start[b + 1] += 1;
    }
    for b in 0..p.buckets {
        start[b + 1] += start[b];
    }
    let mut order = vec![0usize; keys.len()];
    let mut fill = start.clone();
    for (i, &b) in bucket.iter().enumerate() {
        order[fill[b]] = i;
        fill[b] += 1;
    }

    // Dead pair: same bucket and base, unfixable for this seed.
    for b in 0..p.buckets {
        let members = &order[start[b]..start[b + 1]];
        for (x, &i) in members.iter().enumerate() {
            if members[x + 1..].iter().any(|&j| base[i] == base[j]) {
                return None;
            }
        }
    }

    let mut by_size: Vec<usize> = (0..p.buckets)
        .filter(|&b| start[b + 1] > start[b])
        .collect();
    by_size.sort_by(|&a, &b| (start[b + 1] - start[b]).cmp(&(start[a + 1] - start[a])));

    let mut taken = vec![false; m];
    let mut pilots = vec![0u32; p.buckets];
    let mut key_slots = vec![0usize; keys.len()];
    for b in by_size {
        let members = &order[start[b]..start[b + 1]];
        // Bases within a bucket are distinct, so only taken slots can clash.
        let d = (0..m).find(|&d| members.iter().all(|&i| !taken[(base[i] + d) & mask]))?;
        pilots[b] = d as u32;
        for &i in members {
            let slot = (base[i] + d) & mask;
            taken[slot] = true;
            key_slots[i] = slot;
        }
    }
    Some((pilots, key_slots))
}

fn try_seeds(keys: &[&[u8]], max_len: usize, m: usize, strong: bool) -> Option<StrHashState> {
    let n = keys.len();
    let buckets = (n.next_power_of_two() / 2).max(1);
    let b_bits = buckets.trailing_zeros();
    let m_bits = m.trailing_zeros();
    let (lanes, len_in_lane) = lanes_for(max_len);
    for seed in 0..SEEDS {
        let params = Params {
            muls: (0..lanes as u64).map(|l| multiplier(seed, l)).collect(),
            len_mul: multiplier(seed, u64::from(u32::MAX)),
            len_in_lane,
            strong,
            buckets,
            bucket_shift: top_bits_shift(b_bits),
            base_shift: top_bits_shift(b_bits + m_bits),
        };
        if let Some((pilots, key_slots)) = try_place(keys, m, &params) {
            let mut slots = vec![None; m];
            for (i, &s) in key_slots.iter().enumerate() {
                slots[s] = Some(i);
            }
            return Some(StrHashState {
                hash: StrHashKind::Wide {
                    muls: params.muls,
                    len_mul: params.len_mul,
                    len_in_lane,
                    strong,
                    pilots,
                    bucket_shift: params.bucket_shift,
                    base_shift: params.base_shift,
                },
                max_len,
                slots,
            });
        }
    }
    None
}

/// Wide tier for a non-empty set of distinct keys. Tries the tight table
/// first, the plain hash before the strong one, then larger tables.
pub(crate) fn generate(keys: &[&[u8]]) -> Option<StrHashState> {
    let max_len = keys.iter().map(|k| k.len()).max().unwrap_or(0);
    let min_m = keys.len().next_power_of_two();
    [min_m, min_m * 2, min_m * 4].into_iter().find_map(|m| {
        try_seeds(keys, max_len, m, false).or_else(|| try_seeds(keys, max_len, m, true))
    })
}
