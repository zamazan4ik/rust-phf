//! String-specialised perfect hashing (the `strhash` feature).
//!
//! The functions here are the single definition of every hash used by
//! `phf::StrMap` / `phf::StrSet`. `phf_generator` calls them to verify the
//! tables it builds, and the runtime calls them for lookups, so both sides
//! agree by construction.
//!
//! The algorithms are ported from [ConstexprCore/perfect_hash] (MIT License,
//! Copyright (c) 2026 ConstexprCore). Small sets use one of three hashes over
//! a 256-slot (at most) power-of-two table:
//!
//! * [`StrHash::Positions`]: `len + sum(asso[i][key[pos_i]])`, gperf style;
//! * [`StrHash::Displace`]: hash-and-displace over a few fixed bytes;
//! * [`StrHash::FullHash`]: a seeded hash of the whole key plus displacement.
//!
//! Larger sets and long keys use [`StrHash::Wide`], a multiplicative hash of
//! the key's little-endian 64-bit lanes with a pilot per bucket.
//!
//! Unlike the C++ original, nothing here reads past the end of the key:
//! reading out of bounds is undefined behaviour in Rust even within a page.
//! Keys are compared through [`SlotKey`], which uses overlapping in-bounds
//! loads instead.
//!
//! [ConstexprCore/perfect_hash]: https://github.com/ConstexprCore/perfect_hash

/// Position value that selects the last byte of the key.
pub const POS_LAST_CHAR: u8 = 255;

/// The hash function of a `StrMap`, with the tables it reads.
///
/// Every variant addresses a power-of-two table; [`StrHash::slot`] takes the
/// table's mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrHash<'a> {
    /// `h = len + sum(asso[i][key[positions[i]]])`. Positions past the end of
    /// the key contribute nothing; [`POS_LAST_CHAR`] reads the last byte.
    Positions {
        /// One association table per position.
        asso: &'a [[u8; 256]],
        /// The byte positions read, one per table.
        positions: &'a [u8],
    },
    /// `h = asso[hd_bucket(key)] + hd_key_hash(key)`.
    Displace {
        /// Displacement per bucket.
        asso: &'a [u8; 256],
        /// Whether the key hash reads four bytes rather than two.
        four_byte: bool,
    },
    /// `h = fh_hash(key, seed); h = asso[(h >> 48) & 0xFF] + h`.
    FullHash {
        /// Displacement per bucket.
        asso: &'a [u8; 256],
        /// Seed of the whole-key hash.
        seed: u64,
    },
    /// Whole-key lane hash with a pilot per bucket; see [`wide_hash`].
    Wide {
        /// One odd multiplier per 64-bit lane.
        muls: &'a [u64],
        /// Multiplier for the key length when it is not folded into a lane.
        len_mul: u64,
        /// Whether the length rides in the top byte of the last lane.
        len_in_lane: bool,
        /// Whether lanes are avalanched between and after the products.
        strong: bool,
        /// Pilot per bucket. The length is a power of two.
        pilots: &'a [u32],
        /// Shift that leaves the bucket bits of the hash.
        bucket_shift: u32,
        /// Shift that leaves the base-slot bits of the hash.
        base_shift: u32,
    },
}

impl StrHash<'_> {
    /// Return the slot of `key` in a table of `mask + 1` slots.
    #[inline]
    pub fn slot(&self, key: &[u8], mask: usize) -> usize {
        match *self {
            StrHash::Positions { asso, positions } => positions_hash(key, asso, positions) & mask,
            StrHash::Displace { asso, four_byte } => {
                let bucket = hd_bucket(key);
                (usize::from(asso[bucket])).wrapping_add(hd_key_hash(key, four_byte)) & mask
            }
            StrHash::FullHash { asso, seed } => {
                let h = fh_hash(key, seed);
                let bucket = ((h >> 48) & 0xFF) as usize;
                (u64::from(asso[bucket]).wrapping_add(h) as usize) & mask
            }
            StrHash::Wide {
                muls,
                len_mul,
                len_in_lane,
                strong,
                pilots,
                bucket_shift,
                base_shift,
            } => {
                let h = wide_hash(key, muls, len_mul, len_in_lane, strong);
                wide_slot(h, pilots, bucket_shift, base_shift, mask)
            }
        }
    }
}

#[inline(always)]
fn byte_or_zero(key: &[u8], idx: usize) -> usize {
    key.get(idx).map_or(0, |&b| usize::from(b))
}

/// The gperf-style hash: `len + sum(asso[i][key[positions[i]]])`.
#[inline]
pub fn positions_hash(key: &[u8], asso: &[[u8; 256]], positions: &[u8]) -> usize {
    let mut h = key.len();
    for (table, &pos) in asso.iter().zip(positions) {
        let idx = if pos == POS_LAST_CHAR {
            key.len().wrapping_sub(1)
        } else {
            usize::from(pos)
        };
        if let Some(&b) = key.get(idx) {
            h = h.wrapping_add(usize::from(table[usize::from(b)]));
        }
    }
    h
}

/// Hash-and-displace bucket: first byte, last byte and length.
#[inline]
pub fn hd_bucket(key: &[u8]) -> usize {
    let first = key.first().map_or(0, |&b| usize::from(b));
    let last = key.last().map_or(0, |&b| usize::from(b));
    first
        .wrapping_add(last.wrapping_mul(3))
        .wrapping_add(key.len().wrapping_mul(17))
        & 0xFF
}

/// Hash-and-displace key hash: a polynomial over the length and the first
/// two or four bytes.
#[inline]
pub fn hd_key_hash(key: &[u8], four_byte: bool) -> usize {
    let mut h = key.len();
    h = h.wrapping_mul(31).wrapping_add(byte_or_zero(key, 0));
    h = h.wrapping_mul(31).wrapping_add(byte_or_zero(key, 1));
    if four_byte {
        h = h.wrapping_mul(31).wrapping_add(byte_or_zero(key, 2));
        h = h.wrapping_mul(31).wrapping_add(byte_or_zero(key, 3));
    }
    h
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// Seeded 64-bit hash of the whole key, read as little-endian 8-byte chunks.
#[inline]
pub fn fh_hash(key: &[u8], seed: u64) -> u64 {
    let mut h = seed ^ GOLDEN.wrapping_mul((key.len() as u64).wrapping_add(1));
    let mut chunks = key.chunks_exact(8);
    for chunk in &mut chunks {
        h = (h ^ u64::from_le_bytes(chunk.try_into().unwrap())).wrapping_mul(GOLDEN);
        h ^= h >> 32;
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        h = (h ^ load_partial(rest)).wrapping_mul(GOLDEN);
        h ^= h >> 32;
    }
    // murmur3 fmix64
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    h
}

/// Load a key of fewer than 8 bytes as a zero-padded little-endian `u64`,
/// with in-bounds loads only.
#[inline(always)]
fn load_partial(k: &[u8]) -> u64 {
    let n = k.len();
    debug_assert!(n < 8);
    if n >= 4 {
        // Two overlapping 4-byte loads; the shared bytes are equal.
        let lo = u64::from(u32::from_le_bytes(k[..4].try_into().unwrap()));
        let hi = u64::from(u32::from_le_bytes(k[n - 4..].try_into().unwrap()));
        lo | (hi << (8 * (n - 4)))
    } else if n > 0 {
        u64::from(k[0])
            | (u64::from(k[n / 2]) << (8 * (n / 2)))
            | (u64::from(k[n - 1]) << (8 * (n - 1)))
    } else {
        0
    }
}

/// Load lane `i` (bytes `8i..8i+8`) of `key`, zero-padded.
#[inline(always)]
fn load_lane(key: &[u8], i: usize) -> u64 {
    let off = 8 * i;
    match key.get(off..off + 8) {
        Some(lane) => u64::from_le_bytes(lane.try_into().unwrap()),
        None => load_partial(key.get(off..).unwrap_or(&[])),
    }
}

const STRONG_MUL: u64 = 0xBF58_476D_1CE4_E5B9;

/// The wide hash: the XOR of each 64-bit lane times its multiplier.
///
/// With `len_in_lane` the key length is OR-ed into the top byte of the last
/// lane (keys are then at most 15 bytes, so that byte holds no key data).
/// Otherwise `len * len_mul` is XOR-ed in, which also separates keys that
/// differ only by trailing NUL bytes. `strong` avalanches between lanes and
/// at the end, for key sets the plain form cannot separate.
#[inline]
pub fn wide_hash(key: &[u8], muls: &[u64], len_mul: u64, len_in_lane: bool, strong: bool) -> u64 {
    let n = key.len();
    let lanes = muls.len();
    let mut h;
    if len_in_lane {
        let len_bits = (n as u64) << 56;
        if lanes == 1 {
            h = (load_partial_or_lane(key) | len_bits).wrapping_mul(muls[0]);
        } else {
            h = load_lane(key, 0).wrapping_mul(muls[0]);
            if strong {
                h ^= h >> 29;
                h = h.wrapping_mul(STRONG_MUL);
            }
            h ^= (load_lane(key, 1) | len_bits).wrapping_mul(muls[1]);
        }
    } else {
        h = load_lane(key, 0).wrapping_mul(muls[0]);
        // Zero lanes add nothing to the plain form, so stop after the key.
        let used = if strong {
            lanes
        } else {
            n.div_ceil(8).min(lanes)
        };
        for (i, &mul) in muls.iter().enumerate().take(used).skip(1) {
            if strong {
                h ^= h >> 29;
                h = h.wrapping_mul(STRONG_MUL);
            }
            h ^= load_lane(key, i).wrapping_mul(mul);
        }
        h ^= (n as u64).wrapping_mul(len_mul);
    }
    if strong {
        h ^= h >> 29;
        h = h.wrapping_mul(STRONG_MUL);
        h ^= h >> 32;
    }
    h
}

#[inline(always)]
fn load_partial_or_lane(key: &[u8]) -> u64 {
    if key.len() >= 8 {
        load_lane(key, 0)
    } else {
        load_partial(key)
    }
}

/// Bucket of a wide hash.
#[inline(always)]
pub fn wide_bucket(h: u64, bucket_shift: u32, buckets: usize) -> usize {
    (h >> bucket_shift) as usize & (buckets - 1)
}

/// Base slot of a wide hash, before the pilot is added.
#[inline(always)]
pub fn wide_base(h: u64, base_shift: u32, mask: usize) -> usize {
    (h >> base_shift) as usize & mask
}

/// Slot of a wide hash: `(base + pilot[bucket]) & mask`.
#[inline]
pub fn wide_slot(h: u64, pilots: &[u32], bucket_shift: u32, base_shift: u32, mask: usize) -> usize {
    let bucket = wide_bucket(h, bucket_shift, pilots.len());
    wide_base(h, base_shift, mask).wrapping_add(pilots[bucket] as usize) & mask
}

/// The stored form of the key in one slot, for comparison without reading the
/// key itself.
///
/// For keys of at most 16 bytes, `(len, lo, hi)` determines the key, so
/// matching them is the whole comparison. Longer keys also compare the bytes
/// between the two 8-byte windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotKey {
    lo: u64,
    hi: u64,
    len: u32,
    idx: u32,
}

impl SlotKey {
    /// An unoccupied slot; matches no key.
    pub const EMPTY: SlotKey = SlotKey {
        lo: 0,
        hi: 0,
        len: u32::MAX,
        idx: u32::MAX,
    };

    /// Build a slot from its fields, as printed by code generation.
    #[inline]
    pub const fn raw(lo: u64, hi: u64, len: u32, idx: u32) -> SlotKey {
        SlotKey { lo, hi, len, idx }
    }

    /// Build the slot for `key`, which is entry `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `key` is `u32::MAX` bytes or longer, or `idx` is `u32::MAX`.
    pub fn new(key: &[u8], idx: usize) -> SlotKey {
        let len = u32::try_from(key.len())
            .ok()
            .filter(|&len| len != u32::MAX)
            .expect("strhash key too long");
        let idx = u32::try_from(idx)
            .ok()
            .filter(|&idx| idx != u32::MAX)
            .expect("too many strhash keys");
        let (lo, hi) = windows(key);
        SlotKey { lo, hi, len, idx }
    }

    /// The fields in the order [`SlotKey::raw`] takes them.
    pub fn to_raw(&self) -> (u64, u64, u32, u32) {
        (self.lo, self.hi, self.len, self.idx)
    }

    /// The entry index, or `None` for an empty slot.
    #[inline]
    pub fn index(&self) -> Option<usize> {
        (self.idx != u32::MAX).then_some(self.idx as usize)
    }

    /// Whether `key` is the key stored in this slot. `stored` returns the
    /// stored key for an entry index; it is called only for keys longer than
    /// 16 bytes whose windows match.
    ///
    /// The caller must reject keys of `u32::MAX` bytes or more first, which
    /// the maps do by rejecting keys longer than their longest key.
    #[inline]
    pub fn matches<'a>(&self, key: &[u8], stored: impl FnOnce(usize) -> &'a [u8]) -> bool {
        let (lo, hi) = windows(key);
        let diff = (lo ^ self.lo) | (hi ^ self.hi) | ((key.len() as u64) ^ u64::from(self.len));
        if diff != 0 {
            return false;
        }
        let n = key.len();
        n <= 16 || key[8..n - 8] == stored(self.idx as usize)[8..n - 8]
    }
}

/// The first and last 8 bytes of a key, overlapping for short keys, packed so
/// that equal-length keys of at most 16 bytes have equal windows only when
/// they are equal.
#[inline(always)]
fn windows(k: &[u8]) -> (u64, u64) {
    let n = k.len();
    if n >= 8 {
        (
            u64::from_le_bytes(k[..8].try_into().unwrap()),
            u64::from_le_bytes(k[n - 8..].try_into().unwrap()),
        )
    } else {
        (load_partial(k), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_lane(key: &[u8], i: usize) -> u64 {
        let mut bytes = [0u8; 8];
        for (b, slot) in bytes.iter_mut().enumerate() {
            *slot = key.get(8 * i + b).copied().unwrap_or(0);
        }
        u64::from_le_bytes(bytes)
    }

    #[test]
    fn partial_loads_match_reference() {
        let data: [u8; 20] = core::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(1));
        for len in 0..=data.len() {
            let key = &data[..len];
            for lane in 0..4 {
                assert_eq!(
                    load_lane(key, lane),
                    reference_lane(key, lane),
                    "len {len} lane {lane}"
                );
            }
        }
    }

    #[test]
    fn windows_separate_equal_length_keys() {
        // Every pair of distinct keys over a two-letter alphabet, lengths 0..=10.
        for len in 0..=10usize {
            let keys: [u16; 1024] = core::array::from_fn(|i| i as u16);
            let mut seen = [(0u64, 0u64); 1024];
            let count = 1usize << len;
            for (i, &bits) in keys.iter().take(count).enumerate() {
                let mut key = [0u8; 10];
                for (b, byte) in key.iter_mut().take(len).enumerate() {
                    *byte = if bits >> b & 1 == 1 { b'a' } else { 0 };
                }
                let w = windows(&key[..len]);
                assert!(!seen[..i].contains(&w), "collision at len {len}");
                seen[i] = w;
            }
        }
    }

    #[test]
    fn slot_key_compares_long_keys() {
        let a = b"0123456789abcdefXYZ0123456789abcdef";
        let mut b = *a;
        b[17] = b'!';
        let slot = SlotKey::new(a, 0);
        assert!(slot.matches(a, |_| a));
        assert!(!slot.matches(&b, |_| a));
        assert!(!SlotKey::EMPTY.matches(b"", |_| unreachable!()));
        assert!(SlotKey::new(b"", 3).matches(b"", |_| unreachable!()));
    }
}
