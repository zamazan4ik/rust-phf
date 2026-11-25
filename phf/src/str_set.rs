//! An immutable set of strings, constructed at compile time with a hash
//! specialised to its members.
use core::fmt;
use core::iter::FusedIterator;

use crate::StrMap;
use crate::str_map;

/// An immutable set of `&'static str`, constructed at compile time.
///
/// Requires the `strhash` feature. See [`StrMap`] for how it differs from
/// [`Set`](crate::Set). Iteration follows declaration order.
///
/// ## Note
///
/// The fields of this struct are public so that they may be initialized by the
/// `phf_str_set!` macro and code generation. They are subject to change at any
/// time and should never be accessed directly.
pub struct StrSet {
    #[doc(hidden)]
    pub map: StrMap<()>,
}

impl StrSet {
    /// Create a new, empty, immutable set.
    #[inline]
    pub const fn new() -> Self {
        Self { map: StrMap::new() }
    }

    /// Returns the number of elements in the `StrSet`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if the `StrSet` contains no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if `value` is in the `StrSet`.
    #[inline]
    pub fn contains(&self, value: &str) -> bool {
        self.map.contains_key(value)
    }

    /// Like [`contains`](Self::contains), for a value given as bytes.
    #[inline]
    pub fn contains_bytes(&self, value: &[u8]) -> bool {
        self.map.get_index_bytes(value).is_some()
    }

    /// Returns the set's internal static instance of the given value.
    ///
    /// This can be useful for interning schemes.
    #[inline]
    pub fn get_key(&self, value: &str) -> Option<&'static str> {
        self.map.get_key(value)
    }

    /// Returns the declaration index of `value`.
    #[inline]
    pub fn get_index(&self, value: &str) -> Option<usize> {
        self.map.get_index(value)
    }

    /// Returns the value at declaration index `index`.
    #[inline]
    pub fn index(&self, index: usize) -> Option<&'static str> {
        self.map.index(index).map(|e| e.0)
    }

    /// Returns an iterator over the values in declaration order.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            iter: self.map.keys(),
        }
    }
}

impl Default for StrSet {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StrSet {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_set().entries(self).finish()
    }
}

impl PartialEq for StrSet {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl Eq for StrSet {}

impl<'a> IntoIterator for &'a StrSet {
    type Item = &'static str;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// An iterator over the values in a `StrSet`.
#[derive(Clone, Debug)]
pub struct Iter<'a> {
    iter: str_map::Keys<'a, ()>,
}

impl Iterator for Iter<'_> {
    type Item = &'static str;

    #[inline]
    fn next(&mut self) -> Option<&'static str> {
        self.iter.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl DoubleEndedIterator for Iter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<&'static str> {
        self.iter.next_back()
    }
}

impl ExactSizeIterator for Iter<'_> {}

impl FusedIterator for Iter<'_> {}

#[cfg(feature = "serde")]
impl serde::Serialize for StrSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self)
    }
}
