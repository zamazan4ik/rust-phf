use std::collections::HashSet;

use fastrand::Rng;

use super::*;

const PROTOCOLS: &[&str] = &["http", "https", "ftp", "ws", "wss", "file"];
const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "asm",
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "constexpr",
    "continue",
    "default",
];
const HTTP_METHODS: &[&str] = &[
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "HEAD",
    "OPTIONS",
    "TRACE",
    "CONNECT",
    "PROPFIND",
    "PROPPATCH",
    "MKCOL",
    "COPY",
    "MOVE",
    "LOCK",
    "UNLOCK",
    "REPORT",
    "MKACTIVITY",
    "CHECKOUT",
    "MERGE",
    "MSEARCH",
    "NOTIFY",
    "SUBSCRIBE",
    "UNSUBSCRIBE",
    "PURGE",
    "LINK",
    "UNLINK",
];
const HTTP_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "connection",
    "content-length",
    "content-type",
    "cookie",
    "date",
    "etag",
    "host",
    "if-modified-since",
    "if-none-match",
    "last-modified",
    "location",
    "referer",
    "server",
    "set-cookie",
    "user-agent",
];

/// Every key hits its own slot, and keys outside the set miss.
fn check<K: AsRef<[u8]>>(state: &StrHashState, keys: &[K], misses: &[Vec<u8>]) {
    let slot_keys = state.slot_keys(keys);
    let lookup = |key: &[u8]| -> Option<usize> {
        if key.len() > state.max_len {
            return None;
        }
        let slot = &slot_keys[state.slot_of(key)];
        slot.matches(key, |i| keys[i].as_ref())
            .then(|| slot.index().unwrap())
    };
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(lookup(key.as_ref()), Some(i), "key {i} of {}", keys.len());
    }
    let set: HashSet<&[u8]> = keys.iter().map(AsRef::as_ref).collect();
    for miss in misses {
        if !set.contains(miss.as_slice()) {
            assert_eq!(lookup(miss), None, "false hit for {miss:?}");
        }
    }
}

fn random_keys(
    rng: &mut Rng,
    n: usize,
    lens: std::ops::RangeInclusive<usize>,
    alphabet: &[u8],
) -> Vec<Vec<u8>> {
    let mut seen = HashSet::new();
    let mut keys = Vec::with_capacity(n);
    while keys.len() < n {
        let len = rng.usize(lens.clone());
        let key: Vec<u8> = (0..len)
            .map(|_| alphabet[rng.usize(..alphabet.len())])
            .collect();
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    keys
}

/// Misses near the keys: each key with one byte changed, one byte added, and
/// one byte removed, plus a few unrelated keys.
fn near_misses<K: AsRef<[u8]>>(rng: &mut Rng, keys: &[K]) -> Vec<Vec<u8>> {
    let mut misses = vec![
        Vec::new(),
        vec![0],
        b"\xff\xff\xff\xff\xff\xff\xff\xff\xff".to_vec(),
    ];
    for key in keys.iter().take(2000) {
        let key = key.as_ref();
        let mut extended = key.to_vec();
        extended.push(rng.u8(..));
        misses.push(extended);
        if !key.is_empty() {
            let mut changed = key.to_vec();
            let i = rng.usize(..key.len());
            changed[i] = changed[i].wrapping_add(1);
            misses.push(changed);
            misses.push(key[..key.len() - 1].to_vec());
            misses.push(key[1..].to_vec());
        }
    }
    misses
}

fn all_tiers() -> [Tier; 5] {
    [
        Tier::Auto,
        Tier::Positions,
        Tier::Displace,
        Tier::FullHash,
        Tier::Wide,
    ]
}

#[test]
fn small_sets_use_positions() {
    let mut rng = Rng::with_seed(1);
    for keys in [PROTOCOLS, CPP_KEYWORDS, HTTP_METHODS, HTTP_HEADERS] {
        let state = generate_str_hash(keys);
        assert_eq!(state.hash.name(), "positions", "{keys:?}");
        assert!(state.slots.len() <= 256);
        check(&state, keys, &near_misses(&mut rng, keys));
    }
}

#[test]
fn every_tier_is_correct_when_it_succeeds() {
    let mut rng = Rng::with_seed(2);
    let mut solved = [0usize; 5];
    for keys in [PROTOCOLS, CPP_KEYWORDS, HTTP_METHODS, HTTP_HEADERS] {
        let misses = near_misses(&mut rng, keys);
        for (t, tier) in all_tiers().into_iter().enumerate() {
            if let Ok(state) = try_generate_str_hash(keys, tier) {
                solved[t] += 1;
                check(&state, keys, &misses);
            }
        }
    }
    // Auto, Positions, FullHash and Wide always solve these sets.
    assert_eq!([solved[0], solved[1], solved[3], solved[4]], [4, 4, 4, 4]);
}

#[test]
fn scattered_positions_fall_back() {
    // 18 keys of 18 bytes; key i differs from the base only at byte i, so a
    // distinguishing position set needs 17 positions, beyond the limit of 16.
    let keys: Vec<Vec<u8>> = (0..18)
        .map(|i| {
            let mut k = vec![b'a'; 18];
            k[i] = b'b';
            k
        })
        .collect();
    assert_eq!(
        try_generate_str_hash(&keys, Tier::Positions),
        Err(StrHashError::Unsolvable)
    );
    let state = generate_str_hash(&keys);
    assert_ne!(state.hash.name(), "positions");
    check(&state, &keys, &near_misses(&mut Rng::with_seed(3), &keys));
}

#[test]
fn sizes_across_the_tier_boundary() {
    let mut rng = Rng::with_seed(4);
    for n in [0, 1, 2, 3, 7, 64, 200, 255, 256, 257, 1000, 5000] {
        let keys = random_keys(
            &mut rng,
            n,
            1..=12,
            b"abcdefghijklmnopqrstuvwxyz0123456789_-",
        );
        let state = generate_str_hash(&keys);
        if n > 255 {
            assert!(state.hash.name().starts_with("wide"), "n = {n}");
        }
        check(&state, &keys, &near_misses(&mut rng, &keys));
    }
}

#[test]
fn large_wide_set() {
    let mut rng = Rng::with_seed(5);
    let keys: Vec<String> = (0..100_000)
        .map(|i| format!("key-{i}-{}", rng.u32(..1000)))
        .collect();
    let state = generate_str_hash(&keys);
    assert!(state.slots.len() <= 4 * keys.len().next_power_of_two());
    check(&state, &keys, &near_misses(&mut rng, &keys));
}

#[test]
fn long_and_varied_lengths() {
    let mut rng = Rng::with_seed(6);
    for (n, lens) in [
        (20, 0..=300),
        (300, 0..=64),
        (50, 1000..=4096),
        (40, 24..=54),
    ] {
        let keys = random_keys(&mut rng, n, lens, b"ab.:/");
        let misses = near_misses(&mut rng, &keys);
        for tier in all_tiers() {
            if let Ok(state) = try_generate_str_hash(&keys, tier) {
                check(&state, &keys, &misses);
            }
        }
    }
}

#[test]
fn edge_case_keys() {
    let mut rng = Rng::with_seed(7);
    let prefix = "x".repeat(40);
    let sets: Vec<Vec<Vec<u8>>> = vec![
        vec![b"".to_vec()],
        vec![b"".to_vec(), b"a".to_vec(), b"ab".to_vec()],
        vec![
            b"\0".to_vec(),
            b"\0\0".to_vec(),
            b"".to_vec(),
            b"a\0".to_vec(),
            b"a".to_vec(),
        ],
        // differ only by trailing NULs, past the length lane limit
        (0..8)
            .map(|i| [b"abcdefghijklmnopq".as_slice(), &vec![0; i]].concat())
            .collect(),
        // long shared prefix, differing at the end
        (0..30)
            .map(|i| format!("{prefix}{i:03}").into_bytes())
            .collect(),
        // every byte value
        (0..=255u8).map(|b| vec![b]).collect(),
        (0..=254u8).map(|b| vec![b, b]).collect(),
        // keys of 254 bytes, the longest the classic tiers take
        (0..10).map(|i| vec![b'a' + i; 254]).collect(),
    ];
    for keys in &sets {
        let misses = near_misses(&mut rng, keys);
        for tier in all_tiers() {
            if let Ok(state) = try_generate_str_hash(keys, tier) {
                check(&state, keys, &misses);
            }
        }
        // Auto and Wide always succeed.
        try_generate_str_hash(keys, Tier::Auto).unwrap();
        try_generate_str_hash(keys, Tier::Wide).unwrap();
    }
}

#[test]
fn duplicates_are_rejected() {
    assert_eq!(
        try_generate_str_hash(&["a", "b", "a"], Tier::Auto),
        Err(StrHashError::DuplicateKey(2))
    );
}

#[test]
fn generation_is_deterministic() {
    let mut rng = Rng::with_seed(8);
    let keys = random_keys(&mut rng, 600, 1..=20, b"abc");
    assert_eq!(generate_str_hash(&keys), generate_str_hash(&keys));
    assert_eq!(
        generate_str_hash(HTTP_HEADERS),
        generate_str_hash(HTTP_HEADERS)
    );
}

#[test]
fn fields_source_lists_every_slot() {
    let state = generate_str_hash(PROTOCOLS);
    let source = state.fields_source("::phf", PROTOCOLS);
    assert!(source.starts_with("hash: ::phf::StrHash::Positions"));
    assert_eq!(
        source.matches("::phf::SlotKey::raw(").count(),
        state.slots.len()
    );
}
