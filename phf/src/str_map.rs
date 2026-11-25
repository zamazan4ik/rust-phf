//! An immutable map with string keys, constructed at compile time with a hash
//! specialised to its key set.
use core::fmt;
use core::iter::FusedIterator;
use core::ops::Index;
use core::slice;
use phf_shared::strhash::{SlotKey, StrHash};
#[cfg(feature = "serde")]
use serde::ser::{Serialize, SerializeMap, Serializer};

/// An immutable map from `&'static str` keys, constructed at compile time.
///
/// Requires the `strhash` feature. Unlike [`Map`](crate::Map), which hashes
/// every key type with one keyed hash, a `StrMap` uses a hash chosen for its
/// key set by [`phf_str_map!`](crate::phf_str_map) or `phf_codegen::StrMap`.
/// Small sets read only one or two bytes of the key; larger ones use a
/// multiplicative hash of the whole key. Lookups compare the key without
/// branching on its bytes for keys up to 16 bytes long.
///
/// Iteration follows the declaration order of the entries.
///
/// ## Note
///
/// The fields of this struct are public so that they may be initialized by the
/// `phf_str_map!` macro and code generation. They are subject to change at any
/// time and should never be accessed directly.
pub struct StrMap<V: 'static> {
    #[doc(hidden)]
    pub hash: StrHash<'static>,
    #[doc(hidden)]
    pub max_len: usize,
    // A power of two in length, and never empty.
    #[doc(hidden)]
    pub slots: &'static [SlotKey],
    #[doc(hidden)]
    pub entries: &'static [(&'static str, V)],
}

impl<V> StrMap<V> {
    /// Create a new, empty, immutable map.
    #[inline]
    pub const fn new() -> Self {
        Self {
            hash: StrHash::Positions {
                asso: &[],
                positions: &[],
            },
            max_len: 0,
            slots: &[SlotKey::EMPTY],
            entries: &[],
        }
    }

    /// Returns the number of entries in the `StrMap`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the `StrMap` is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Determines if `key` is in the `StrMap`.
    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        self.get_index_bytes(key.as_bytes()).is_some()
    }

    /// Returns a reference to the value that `key` maps to.
    #[inline]
    pub fn get(&self, key: &str) -> Option<&V> {
        self.get_entry(key).map(|e| e.1)
    }

    /// Like [`get`](Self::get), for a key given as bytes.
    #[inline]
    pub fn get_bytes(&self, key: &[u8]) -> Option<&V> {
        self.get_index_bytes(key).map(|i| &self.entries[i].1)
    }

    /// Returns the map's internal static instance of the given key.
    ///
    /// This can be useful for interning schemes.
    #[inline]
    pub fn get_key(&self, key: &str) -> Option<&'static str> {
        self.get_index_bytes(key.as_bytes())
            .map(|i| self.entries[i].0)
    }

    /// Like [`get`](Self::get), but returns both the key and the value.
    #[inline]
    pub fn get_entry(&self, key: &str) -> Option<(&'static str, &V)> {
        self.get_index_bytes(key.as_bytes()).map(|i| {
            let (k, v) = &self.entries[i];
            (*k, v)
        })
    }

    /// Returns the declaration index of `key`.
    #[inline]
    pub fn get_index(&self, key: &str) -> Option<usize> {
        self.get_index_bytes(key.as_bytes())
    }

    /// Like [`get_index`](Self::get_index), for a key given as bytes.
    #[inline]
    pub fn get_index_bytes(&self, key: &[u8]) -> Option<usize> {
        // Also keeps lengths within what a slot can record.
        if key.len() > self.max_len {
            return None;
        }
        let mask = self.slots.len() - 1;
        let slot = &self.slots[self.hash.slot(key, mask)];
        if slot.matches(key, |i| self.entries[i].0.as_bytes()) {
            slot.index()
        } else {
            None
        }
    }

    /// Returns the entry at declaration index `index`.
    #[inline]
    pub fn index(&self, index: usize) -> Option<(&'static str, &V)> {
        self.entries.get(index).map(|(k, v)| (*k, v))
    }

    /// Returns an iterator over the key/value pairs in declaration order.
    pub fn entries(&self) -> Entries<'_, V> {
        Entries {
            iter: self.entries.iter(),
        }
    }

    /// Returns an iterator over the keys in declaration order.
    pub fn keys(&self) -> Keys<'_, V> {
        Keys {
            iter: self.entries.iter(),
        }
    }

    /// Returns an iterator over the values in declaration order.
    pub fn values(&self) -> Values<'_, V> {
        Values {
            iter: self.entries.iter(),
        }
    }
}

impl<V> Default for StrMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: fmt::Debug> fmt::Debug for StrMap<V> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_map().entries(self.entries()).finish()
    }
}

impl<V: PartialEq> PartialEq for StrMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && self.max_len == other.max_len
            && self.slots == other.slots
            && self.entries == other.entries
    }
}

impl<V: Eq> Eq for StrMap<V> {}

impl<V> Index<&str> for StrMap<V> {
    type Output = V;

    fn index(&self, k: &str) -> &V {
        self.get(k).expect("invalid key")
    }
}

impl<'a, V> IntoIterator for &'a StrMap<V> {
    type Item = (&'static str, &'a V);
    type IntoIter = Entries<'a, V>;

    fn into_iter(self) -> Entries<'a, V> {
        self.entries()
    }
}

macro_rules! entry_iterator {
    ($(#[$doc:meta])* $name:ident, $item:ty, |$entry:ident| $map:expr) => {
        $(#[$doc])*
        pub struct $name<'a, V: 'static> {
            iter: slice::Iter<'a, (&'static str, V)>,
        }

        impl<V> Clone for $name<'_, V> {
            fn clone(&self) -> Self {
                Self {
                    iter: self.iter.clone(),
                }
            }
        }

        impl<V: fmt::Debug> fmt::Debug for $name<'_, V> {
            fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt.debug_list().entries(self.clone()).finish()
            }
        }

        impl<'a, V> Iterator for $name<'a, V> {
            type Item = $item;

            #[inline]
            fn next(&mut self) -> Option<$item> {
                self.iter.next().map(|$entry| $map)
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                self.iter.size_hint()
            }
        }

        impl<'a, V> DoubleEndedIterator for $name<'a, V> {
            #[inline]
            fn next_back(&mut self) -> Option<$item> {
                self.iter.next_back().map(|$entry| $map)
            }
        }

        impl<V> ExactSizeIterator for $name<'_, V> {}

        impl<V> FusedIterator for $name<'_, V> {}
    };
}

entry_iterator!(
    /// An iterator over the key/value pairs in a `StrMap`.
    Entries,
    (&'static str, &'a V),
    |e| (e.0, &e.1)
);
entry_iterator!(
    /// An iterator over the keys in a `StrMap`.
    Keys,
    &'static str,
    |e| e.0
);
entry_iterator!(
    /// An iterator over the values in a `StrMap`.
    Values,
    &'a V,
    |e| &e.1
);

#[cfg(feature = "serde")]
impl<V: Serialize> Serialize for StrMap<V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (k, v) in self.entries() {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}
