//! Generators for sets of at most 255 keys shorter than 255 bytes, ported
//! from `detail/gperf_generator.h` of ConstexprCore/perfect_hash.
//!
//! Three tiers, cheapest lookup first, each tried at power-of-two table sizes
//! from `next_power_of_two(N)` up to 256:
//!
//! 1. gperf-style position hash, solved by a partition search;
//! 2. hash-and-displace over the first/last byte and length;
//! 3. seeded whole-key hash with displacement.

use phf_shared::strhash::{fh_hash, hd_bucket, hd_key_hash};

use super::{StrHashKind, StrHashState};

/// Most byte positions the position hash may read.
pub(crate) const MAX_POSITIONS: usize = 16;
/// Largest table: slots and positions are byte-indexed.
const MAX_TABLE: usize = 256;
/// Candidate value for the last byte of the key.
const LAST_CHAR: usize = usize::MAX;
/// Most `count_undistinguished_pairs` evaluations in the backtracking search.
const SEARCH_BUDGET: usize = 5000;
/// Seeds tried by the whole-key tier per table size.
const FULLHASH_SEEDS: u64 = 512;

/// Character at `pos`, or 256 when the key has no such byte.
fn char_at(key: &[u8], pos: usize) -> usize {
    let idx = if pos == LAST_CHAR {
        key.len().wrapping_sub(1)
    } else {
        pos
    };
    key.get(idx).map_or(256, |&b| usize::from(b))
}

/// Pairs of keys whose lengths agree modulo `modulus`; only these need a
/// distinguishing position, since the length term separates the rest.
fn colliding_pairs(keys: &[&[u8]], modulus: usize) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for i in 0..keys.len() {
        for j in i + 1..keys.len() {
            if keys[i].len() % modulus == keys[j].len() % modulus {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

fn count_undistinguished(keys: &[&[u8]], pairs: &[(usize, usize)], positions: &[usize]) -> usize {
    pairs
        .iter()
        .filter(|&&(i, j)| {
            positions
                .iter()
                .all(|&p| char_at(keys[i], p) == char_at(keys[j], p))
        })
        .count()
}

/// Number of distinct `(len % modulus, char_at(key, pos))` pairs.
fn discriminating_power(keys: &[&[u8]], pos: usize, modulus: usize) -> usize {
    let mut seen: Vec<(usize, usize)> = keys
        .iter()
        .map(|k| (k.len() % modulus, char_at(k, pos)))
        .collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

/// Depth-first search for a small set of positions, over the best candidates.
fn backtracking_search(
    keys: &[&[u8]],
    pairs: &[(usize, usize)],
    candidates: &[usize],
    budget: &mut usize,
) -> Option<Vec<usize>> {
    const MAX_DEPTH: usize = if MAX_POSITIONS < 8 { MAX_POSITIONS } else { 8 };
    let breadth = candidates.len().min(20);

    #[derive(Clone, Copy, Default)]
    struct Frame {
        next_ci: usize,
        parent_count: usize,
    }

    let initial = count_undistinguished(keys, pairs, &[]);
    *budget = budget.saturating_sub(1);
    if initial == 0 {
        return Some(Vec::new());
    }

    let mut positions = [0usize; MAX_DEPTH];
    let mut stack = [Frame::default(); MAX_DEPTH];
    stack[0] = Frame {
        next_ci: 0,
        parent_count: initial,
    };
    let mut sp = 0;

    while *budget > 0 {
        let frame = stack[sp];
        if frame.next_ci >= breadth {
            if sp == 0 {
                break;
            }
            sp -= 1;
            stack[sp].next_ci += 1;
            continue;
        }

        positions[sp] = candidates[frame.next_ci];
        *budget -= 1;
        let count = count_undistinguished(keys, pairs, &positions[..=sp]);
        if count == 0 {
            return Some(positions[..=sp].to_vec());
        }

        if count < frame.parent_count && sp + 1 < MAX_DEPTH {
            stack[sp + 1] = Frame {
                next_ci: frame.next_ci + 1,
                parent_count: count,
            };
            sp += 1;
        } else {
            stack[sp].next_ci += 1;
        }
    }
    None
}

/// Choose byte positions that tell apart every pair of keys whose lengths
/// agree modulo `modulus`. `None` if no set of at most [`MAX_POSITIONS`] was
/// found.
pub(crate) fn select_positions(keys: &[&[u8]], modulus: usize) -> Option<Vec<usize>> {
    let pairs = colliding_pairs(keys, modulus);
    let distinguishes = |positions: &[usize]| count_undistinguished(keys, &pairs, positions) == 0;

    // Strategy 1: lengths alone.
    if pairs.is_empty() {
        return Some(Vec::new());
    }

    // Candidates are every position, plus the last byte, by discriminating power.
    let max_len = keys.iter().map(|k| k.len()).max().unwrap_or(0);
    let mut candidates: Vec<(usize, usize)> = (0..max_len)
        .chain([LAST_CHAR])
        .map(|pos| (pos, discriminating_power(keys, pos, modulus)))
        .collect();
    candidates.sort_by_key(|&(_, power)| std::cmp::Reverse(power));
    let candidates: Vec<usize> = candidates.into_iter().map(|(pos, _)| pos).collect();

    // Strategy 2: the single best position.
    if distinguishes(&candidates[..1]) {
        return Some(candidates[..1].to_vec());
    }

    // Strategy 3: first and last byte.
    if MAX_POSITIONS >= 2 && distinguishes(&[0, LAST_CHAR]) {
        return Some(vec![0, LAST_CHAR]);
    }

    // Strategy 4: bounded backtracking over the best candidates.
    let mut budget = SEARCH_BUDGET;
    if let Some(found) = backtracking_search(keys, &pairs, &candidates, &mut budget) {
        return Some(found);
    }

    // Strategy 5: add candidates greedily.
    let mut positions = Vec::new();
    for &candidate in &candidates {
        if positions.len() == MAX_POSITIONS {
            break;
        }
        positions.push(candidate);
        if distinguishes(&positions) {
            return Some(positions);
        }
    }
    None
}

/// Association values for a table of `m` slots, set one `(position, byte)`
/// symbol at a time as GNU gperf does. Returns the tables and the slot of
/// each key.
fn try_generate_gperf(
    keys: &[&[u8]],
    positions: &[usize],
    m: usize,
) -> Option<(Vec<[usize; 256]>, Vec<usize>)> {
    let n = keys.len();
    let np = positions.len();
    let mut asso = vec![[0usize; 256]; np];

    // Each key's byte at each position (256 when absent).
    let kchars: Vec<Vec<usize>> = keys
        .iter()
        .map(|k| positions.iter().map(|&p| char_at(k, p)).collect())
        .collect();

    // Unique (position, byte) symbols, most frequent first: they affect the
    // most keys, so fixing them while the partition is coarse leaves the
    // most freedom.
    struct Sym {
        pos: usize,
        ch: usize,
        freq: usize,
    }
    let mut syms = Vec::new();
    for p in 0..np {
        let mut freq = [0usize; 256];
        for kc in &kchars {
            if kc[p] < 256 {
                freq[kc[p]] += 1;
            }
        }
        for (ch, &f) in freq.iter().enumerate() {
            if f > 0 {
                syms.push(Sym {
                    pos: p,
                    ch,
                    freq: f,
                });
            }
        }
    }
    syms.sort_by_key(|sym| std::cmp::Reverse(sym.freq));

    // Partial hash: length plus the values fixed so far.
    let mut phash: Vec<usize> = keys.iter().map(|k| k.len()).collect();

    // sig[k] is the XOR of a salt over the symbols of key k not yet fixed.
    // Keys with equal sig can no longer be separated by later symbols, so
    // their partial hashes must already differ modulo m.
    let mut salt = vec![[0u64; 256]; np];
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    for row in &mut salt {
        for v in row.iter_mut() {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *v = s;
        }
    }
    let mut sig: Vec<u64> = kchars
        .iter()
        .map(|kc| {
            (0..np)
                .filter(|&p| kc[p] < 256)
                .fold(0, |acc, p| acc ^ salt[p][kc[p]])
        })
        .collect();
    let mut order: Vec<usize> = (0..n).collect();

    // Generation counters avoid clearing the slot marks per class.
    let mut slot_gen = vec![0usize; m];
    let mut generation = 0usize;
    let search_limit = m.next_power_of_two().max(32);

    for sym in &syms {
        let (sp, sc) = (sym.pos, sym.ch);
        let sym_salt = salt[sp][sc];
        for (k, kc) in kchars.iter().enumerate() {
            if kc[sp] == sc {
                sig[k] ^= sym_salt;
            }
        }
        // Stable and nearly sorted already.
        order.sort_by_key(|&k| sig[k]);

        let mut found = None;
        'values: for v in 0..search_limit {
            let mut ci = 0;
            while ci < n {
                let class_sig = sig[order[ci]];
                let mut cj = ci;
                while cj < n && sig[order[cj]] == class_sig {
                    cj += 1;
                }
                if cj - ci > 1 {
                    generation += 1;
                    for &k in &order[ci..cj] {
                        let mut h = phash[k];
                        if kchars[k][sp] == sc {
                            h += v;
                        }
                        let h = h % m;
                        if slot_gen[h] == generation {
                            continue 'values;
                        }
                        slot_gen[h] = generation;
                    }
                }
                ci = cj;
            }
            found = Some(v);
            break;
        }

        let v = found?;
        asso[sp][sc] = v;
        for (k, kc) in kchars.iter().enumerate() {
            if kc[sp] == sc {
                phash[k] += v;
            }
        }
    }

    let slots: Vec<usize> = phash.iter().map(|h| h % m).collect();
    if !all_distinct(&slots, m) {
        return None;
    }
    Some((asso, slots))
}

fn all_distinct(slots: &[usize], m: usize) -> bool {
    let mut taken = vec![false; m];
    slots
        .iter()
        .all(|&s| !std::mem::replace(&mut taken[s], true))
}

/// Place buckets largest first, giving each the smallest displacement below
/// `max_d` that sends all its keys to free slots. `bucket[i]` is key i's
/// bucket (below 256) and `hash[i]` its slot before displacement. Returns
/// each key's slot and the displacement per bucket.
fn displace(
    bucket: &[usize],
    hash: &[u64],
    m: usize,
    max_d: usize,
) -> Option<(Vec<usize>, [u8; 256])> {
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); 256];
    for (i, &b) in bucket.iter().enumerate() {
        members[b].push(i);
    }
    let mut order: Vec<usize> = (0..256).filter(|&b| !members[b].is_empty()).collect();
    order.sort_by(|&a, &b| members[b].len().cmp(&members[a].len()));

    let mut taken = vec![false; m];
    let mut slots = vec![0usize; bucket.len()];
    let mut disp = [0u8; 256];
    let mut tried = Vec::new();
    for b in order {
        let placed = (0..max_d).find(|&d| {
            tried.clear();
            for &k in &members[b] {
                let slot = (hash[k].wrapping_add(d as u64) % m as u64) as usize;
                if taken[slot] || tried.contains(&slot) {
                    return false;
                }
                tried.push(slot);
            }
            true
        })?;
        disp[b] = placed as u8;
        for (&k, &slot) in members[b].iter().zip(&tried) {
            taken[slot] = true;
            slots[k] = slot;
        }
    }
    Some((slots, disp))
}

fn try_hash_and_displace(keys: &[&[u8]], m: usize) -> Option<(StrHashKind, Vec<usize>)> {
    let bucket: Vec<usize> = keys.iter().map(|k| hd_bucket(k)).collect();
    // Displacements are stored in a byte.
    let max_d = m.min(255);
    for four_byte in [false, true] {
        let hash: Vec<u64> = keys
            .iter()
            .map(|k| hd_key_hash(k, four_byte) as u64)
            .collect();
        if let Some((slots, asso)) = displace(&bucket, &hash, m, max_d) {
            return Some((StrHashKind::Displace { asso, four_byte }, slots));
        }
    }
    None
}

fn try_seeded_fullhash(keys: &[&[u8]], m: usize) -> Option<(StrHashKind, Vec<usize>)> {
    for seed in 0..FULLHASH_SEEDS {
        let hash: Vec<u64> = keys.iter().map(|k| fh_hash(k, seed)).collect();
        let bucket: Vec<usize> = hash.iter().map(|h| ((h >> 48) & 0xFF) as usize).collect();
        // Two keys in one bucket with equal slots can never be separated:
        // move to the next seed rather than search.
        let mut seen: Vec<(usize, u64)> = bucket
            .iter()
            .zip(&hash)
            .map(|(&b, &h)| (b, h % m as u64))
            .collect();
        seen.sort_unstable();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            continue;
        }
        if let Some((slots, asso)) = displace(&bucket, &hash, m, m.min(256)) {
            return Some((StrHashKind::FullHash { asso, seed }, slots));
        }
    }
    None
}

fn table_sizes(n: usize) -> impl Iterator<Item = usize> {
    let start = n.next_power_of_two();
    let cap = (start * 8).min(MAX_TABLE);
    std::iter::successors(Some(start), |&m| Some(m * 2)).take_while(move |&m| m <= cap)
}

fn finish(keys: &[&[u8]], m: usize, hash: StrHashKind, key_slots: Vec<usize>) -> StrHashState {
    let mut slots = vec![None; m];
    for (i, &s) in key_slots.iter().enumerate() {
        slots[s] = Some(i);
    }
    StrHashState {
        hash,
        max_len: keys.iter().map(|k| k.len()).max().unwrap_or(0),
        slots,
    }
}

/// Position-hash tier alone.
pub(crate) fn try_positions(keys: &[&[u8]]) -> Option<StrHashState> {
    // The pairs needing a position at size m only grow as m shrinks, so if
    // no position set exists at the largest table, none exists at all.
    select_positions(keys, MAX_TABLE)?;
    for m in table_sizes(keys.len()) {
        let Some(positions) = select_positions(keys, m) else {
            continue;
        };
        if let Some((asso, key_slots)) = try_generate_gperf(keys, &positions, m) {
            let asso = asso.iter().map(|row| row.map(|v| (v % m) as u8)).collect();
            let positions = positions
                .iter()
                .map(|&p| if p == LAST_CHAR { 255 } else { p as u8 })
                .collect();
            return Some(finish(
                keys,
                m,
                StrHashKind::Positions { asso, positions },
                key_slots,
            ));
        }
    }
    None
}

/// Hash-and-displace tier alone.
pub(crate) fn try_displace(keys: &[&[u8]]) -> Option<StrHashState> {
    table_sizes(keys.len()).find_map(|m| {
        try_hash_and_displace(keys, m).map(|(hash, slots)| finish(keys, m, hash, slots))
    })
}

/// Whole-key hash tier alone.
pub(crate) fn try_fullhash(keys: &[&[u8]]) -> Option<StrHashState> {
    table_sizes(keys.len()).find_map(|m| {
        try_seeded_fullhash(keys, m).map(|(hash, slots)| finish(keys, m, hash, slots))
    })
}

/// Whether the classic tiers can hold this key set at all.
pub(crate) fn fits(keys: &[&[u8]]) -> bool {
    keys.len() <= 255 && keys.iter().all(|k| k.len() < 255)
}

/// All classic tiers in order, for a non-empty set of distinct keys that
/// [`fits`].
pub(crate) fn generate(keys: &[&[u8]]) -> Option<StrHashState> {
    debug_assert!(!keys.is_empty() && fits(keys));
    try_positions(keys)
        .or_else(|| try_displace(keys))
        .or_else(|| try_fullhash(keys))
}
