//! Perfect hash generation for string keys (the `strhash` feature), backing
//! `phf::StrMap` and `phf::StrSet`.
//!
//! The algorithms are ported from [ConstexprCore/perfect_hash] (MIT License,
//! Copyright (c) 2026 ConstexprCore). The hash functions themselves live in
//! [`phf_shared::strhash`], so that generation and lookup share one
//! definition. Generation is deterministic.
//!
//! [ConstexprCore/perfect_hash]: https://github.com/ConstexprCore/perfect_hash

use std::collections::HashSet;
use std::fmt;

use phf_shared::strhash::{SlotKey, StrHash};

mod classic;
mod wide;

/// The hash function chosen for a key set, with owned tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrHashKind {
    /// See [`StrHash::Positions`].
    Positions {
        /// One association table per position.
        asso: Vec<[u8; 256]>,
        /// The positions read.
        positions: Vec<u8>,
    },
    /// See [`StrHash::Displace`].
    Displace {
        /// Displacement per bucket.
        asso: [u8; 256],
        /// Whether the key hash reads four bytes.
        four_byte: bool,
    },
    /// See [`StrHash::FullHash`].
    FullHash {
        /// Displacement per bucket.
        asso: [u8; 256],
        /// Seed of the whole-key hash.
        seed: u64,
    },
    /// See [`StrHash::Wide`].
    Wide {
        /// Multiplier per lane.
        muls: Vec<u64>,
        /// Multiplier for the length.
        len_mul: u64,
        /// Whether the length is folded into the last lane.
        len_in_lane: bool,
        /// Whether the strong hash is used.
        strong: bool,
        /// Pilot per bucket.
        pilots: Vec<u32>,
        /// Shift for the bucket bits.
        bucket_shift: u32,
        /// Shift for the base-slot bits.
        base_shift: u32,
    },
}

impl StrHashKind {
    /// Borrow as the runtime representation.
    pub fn as_hash(&self) -> StrHash<'_> {
        match self {
            StrHashKind::Positions { asso, positions } => StrHash::Positions { asso, positions },
            StrHashKind::Displace { asso, four_byte } => StrHash::Displace {
                asso,
                four_byte: *four_byte,
            },
            StrHashKind::FullHash { asso, seed } => StrHash::FullHash { asso, seed: *seed },
            StrHashKind::Wide {
                muls,
                len_mul,
                len_in_lane,
                strong,
                pilots,
                bucket_shift,
                base_shift,
            } => StrHash::Wide {
                muls,
                len_mul: *len_mul,
                len_in_lane: *len_in_lane,
                strong: *strong,
                pilots,
                bucket_shift: *bucket_shift,
                base_shift: *base_shift,
            },
        }
    }

    /// Short name of the algorithm, for diagnostics.
    pub fn name(&self) -> &'static str {
        match self {
            StrHashKind::Positions { .. } => "positions",
            StrHashKind::Displace { .. } => "displace",
            StrHashKind::FullHash { .. } => "fullhash",
            StrHashKind::Wide { strong: false, .. } => "wide",
            StrHashKind::Wide { strong: true, .. } => "wide/strong",
        }
    }
}

/// A solved perfect hash for a set of string keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrHashState {
    /// The hash function.
    pub hash: StrHashKind,
    /// Length of the longest key; longer inputs are rejected before hashing.
    pub max_len: usize,
    /// Key index stored in each slot. The length is a power of two.
    pub slots: Vec<Option<usize>>,
}

impl StrHashState {
    /// The slot that `key` hashes to.
    pub fn slot_of(&self, key: &[u8]) -> usize {
        self.hash.as_hash().slot(key, self.slots.len() - 1)
    }

    /// The runtime slot table for `keys`, the keys this state was built for.
    pub fn slot_keys<K: AsRef<[u8]>>(&self, keys: &[K]) -> Vec<SlotKey> {
        self.slots
            .iter()
            .map(|slot| match *slot {
                Some(i) => SlotKey::new(keys[i].as_ref(), i),
                None => SlotKey::EMPTY,
            })
            .collect()
    }

    /// Write the hash fields of a `StrMap` as Rust source, with `path` the
    /// path to the `phf` crate:
    /// `hash: ..., max_len: ..., slots: &[...],`.
    pub fn write_fields<K: AsRef<[u8]>>(
        &self,
        out: &mut impl fmt::Write,
        path: &str,
        keys: &[K],
    ) -> fmt::Result {
        write!(out, "hash: {path}::StrHash::")?;
        match &self.hash {
            StrHashKind::Positions { asso, positions } => {
                write!(out, "Positions {{ asso: &[")?;
                for row in asso {
                    write_row(out, row)?;
                    out.write_str(",")?;
                }
                write!(out, "], positions: &{positions:?} }}")?;
            }
            StrHashKind::Displace { asso, four_byte } => {
                write!(out, "Displace {{ asso: &")?;
                write_row(out, asso)?;
                write!(out, ", four_byte: {four_byte} }}")?;
            }
            StrHashKind::FullHash { asso, seed } => {
                write!(out, "FullHash {{ asso: &")?;
                write_row(out, asso)?;
                write!(out, ", seed: {seed} }}")?;
            }
            StrHashKind::Wide {
                muls,
                len_mul,
                len_in_lane,
                strong,
                pilots,
                bucket_shift,
                base_shift,
            } => {
                write!(
                    out,
                    "Wide {{ muls: &{muls:?}, len_mul: {len_mul}, len_in_lane: {len_in_lane}, \
                     strong: {strong}, pilots: &{pilots:?}, bucket_shift: {bucket_shift}, \
                     base_shift: {base_shift} }}"
                )?;
            }
        }
        write!(out, ", max_len: {}, slots: &[", self.max_len)?;
        for slot in self.slot_keys(keys) {
            let (lo, hi, len, idx) = slot.to_raw();
            write!(out, "{path}::SlotKey::raw({lo}, {hi}, {len}, {idx}),")?;
        }
        out.write_str("],")
    }

    /// [`write_fields`](Self::write_fields) into a new `String`.
    pub fn fields_source<K: AsRef<[u8]>>(&self, path: &str, keys: &[K]) -> String {
        let mut out = String::new();
        self.write_fields(&mut out, path, keys).unwrap();
        out
    }
}

fn write_row(out: &mut impl fmt::Write, row: &[u8; 256]) -> fmt::Result {
    out.write_str("[")?;
    for v in row {
        write!(out, "{v},")?;
    }
    out.write_str("]")
}

/// Why no hash could be generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrHashError {
    /// Key at this index repeats an earlier key.
    DuplicateKey(usize),
    /// No tier found a hash. Not expected in practice.
    Unsolvable,
}

impl fmt::Display for StrHashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrHashError::DuplicateKey(i) => write!(f, "duplicate key at index {i}"),
            StrHashError::Unsolvable => f.write_str("failed to solve string PHF"),
        }
    }
}

impl std::error::Error for StrHashError {}

/// Which generator to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tier {
    /// Pick automatically: the classic tiers for at most 255 keys shorter than
    /// 255 bytes, falling back to wide.
    Auto,
    /// Only the position hash.
    Positions,
    /// Only hash-and-displace.
    Displace,
    /// Only the seeded whole-key hash.
    FullHash,
    /// Only the wide pilot hash.
    Wide,
}

/// Generate a perfect hash for `keys`.
///
/// # Panics
///
/// Panics on duplicate keys, or if no hash is found.
pub fn generate_str_hash<K: AsRef<[u8]>>(keys: &[K]) -> StrHashState {
    try_generate_str_hash(keys, Tier::Auto).unwrap_or_else(|e| panic!("{e}"))
}

/// Generate a perfect hash for `keys` with the given tier. A specific
/// classic tier fails with [`StrHashError::Unsolvable`] if the set is too large
/// for it or the tier cannot separate the keys.
pub fn try_generate_str_hash<K: AsRef<[u8]>>(
    keys: &[K],
    tier: Tier,
) -> Result<StrHashState, StrHashError> {
    let keys: Vec<&[u8]> = keys.iter().map(AsRef::as_ref).collect();
    let mut seen = HashSet::with_capacity(keys.len());
    if let Some(i) = keys.iter().position(|k| !seen.insert(*k)) {
        return Err(StrHashError::DuplicateKey(i));
    }
    drop(seen);

    if keys.is_empty() {
        // One empty slot: every lookup lands there and fails, so the runtime
        // needs no emptiness check.
        return Ok(StrHashState {
            hash: StrHashKind::Positions {
                asso: Vec::new(),
                positions: Vec::new(),
            },
            max_len: 0,
            slots: vec![None],
        });
    }

    let fits = classic::fits(&keys);
    let state = match tier {
        Tier::Auto if fits => classic::generate(&keys).or_else(|| wide::generate(&keys)),
        Tier::Auto | Tier::Wide => wide::generate(&keys),
        _ if !fits => None,
        Tier::Positions => classic::try_positions(&keys),
        Tier::Displace => classic::try_displace(&keys),
        Tier::FullHash => classic::try_fullhash(&keys),
    }
    .ok_or(StrHashError::Unsolvable)?;

    verify(&state, &keys);
    Ok(state)
}

/// Check the state end to end with the runtime hash, as the C++ constructors
/// do at compile time.
fn verify(state: &StrHashState, keys: &[&[u8]]) {
    assert!(state.slots.len().is_power_of_two());
    let slot_keys = state.slot_keys(keys);
    for (i, key) in keys.iter().enumerate() {
        let slot = state.slot_of(key);
        assert_eq!(state.slots[slot], Some(i), "strhash: key {i} misplaced");
        assert!(slot_keys[slot].matches(key, |j| keys[j]));
    }
}

#[cfg(test)]
mod tests;
