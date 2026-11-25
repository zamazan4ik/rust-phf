//! Builders for `phf::StrMap` and `phf::StrSet` (the `strhash` feature).

use std::borrow::Cow;
use std::fmt;

use phf_generator::strhash::{StrHashState, generate_str_hash};

/// A builder for the `phf::StrMap` type.
///
/// Keys are strings, written as string literals in the generated source.
/// Requires the `strhash` feature here and on the runtime `phf` dependency.
pub struct StrMap<'a> {
    keys: Vec<Cow<'a, str>>,
    values: Vec<Cow<'a, str>>,
    path: Cow<'a, str>,
}

impl<'a> StrMap<'a> {
    /// Creates a new `phf::StrMap` builder.
    pub fn new() -> Self {
        StrMap {
            keys: vec![],
            values: vec![],
            path: Cow::Borrowed("::phf"),
        }
    }

    /// Set the path to the `phf` crate from the global namespace
    pub fn phf_path(&mut self, path: impl Into<Cow<'a, str>>) -> &mut Self {
        self.path = path.into();
        self
    }

    /// Adds an entry to the builder.
    ///
    /// `value` will be written exactly as provided in the constructed source.
    pub fn entry(
        &mut self,
        key: impl Into<Cow<'a, str>>,
        value: impl Into<Cow<'a, str>>,
    ) -> &mut Self {
        self.keys.push(key.into());
        self.values.push(value.into());
        self
    }

    /// Calculate the hash parameters and return a struct implementing
    /// [`Display`](::std::fmt::Display) for the constructed `phf::StrMap`.
    ///
    /// With the `quote` feature enabled, the returned value also implements
    /// `quote::ToTokens`.
    ///
    /// # Panics
    ///
    /// Panics if there are any duplicate keys.
    pub fn build(&self) -> DisplayStrMap<'_> {
        if let Some(i) = first_duplicate(&self.keys) {
            panic!("duplicate key `{}`", self.keys[i].escape_debug());
        }
        DisplayStrMap {
            state: generate_str_hash(&key_bytes(&self.keys)),
            path: &self.path,
            keys: &self.keys,
            values: &self.values,
        }
    }
}

fn key_bytes<'k>(keys: &'k [Cow<'_, str>]) -> Vec<&'k [u8]> {
    keys.iter().map(|k| k.as_bytes()).collect()
}

fn first_duplicate(keys: &[Cow<'_, str>]) -> Option<usize> {
    let mut seen = std::collections::HashSet::with_capacity(keys.len());
    keys.iter().position(|k| !seen.insert(k))
}

impl<'a, K: Into<Cow<'a, str>>, V: Into<Cow<'a, str>>> FromIterator<(K, V)> for StrMap<'a> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut map = StrMap::new();
        for (key, value) in iter {
            map.entry(key, value);
        }
        map
    }
}

/// An adapter for printing a [`StrMap`].
pub struct DisplayStrMap<'a> {
    state: StrHashState,
    path: &'a str,
    keys: &'a [Cow<'a, str>],
    values: &'a [Cow<'a, str>],
}

impl fmt::Display for DisplayStrMap<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::StrMap {{ ", self.path)?;
        self.state
            .write_fields(f, self.path, &key_bytes(self.keys))?;
        write!(f, " entries: &[")?;
        for (key, value) in self.keys.iter().zip(self.values) {
            write!(f, "\n        ({key:?}, {value}),")?;
        }
        write!(f, "\n    ],\n}}")
    }
}

#[cfg(feature = "quote")]
impl quote::ToTokens for DisplayStrMap<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        crate::write_tokens(tokens, self);
    }
}

/// A builder for the `phf::StrSet` type.
pub struct StrSet<'a> {
    map: StrMap<'a>,
}

impl<'a> StrSet<'a> {
    /// Constructs a new `phf::StrSet` builder.
    pub fn new() -> Self {
        StrSet { map: StrMap::new() }
    }

    /// Set the path to the `phf` crate from the global namespace
    pub fn phf_path(&mut self, path: impl Into<Cow<'a, str>>) -> &mut Self {
        self.map.phf_path(path);
        self
    }

    /// Adds an entry to the builder.
    pub fn entry(&mut self, entry: impl Into<Cow<'a, str>>) -> &mut Self {
        self.map.entry(entry, "()");
        self
    }

    /// Calculate the hash parameters and return a struct implementing
    /// [`Display`](::std::fmt::Display) for the constructed `phf::StrSet`.
    ///
    /// With the `quote` feature enabled, the returned value also implements
    /// `quote::ToTokens`.
    ///
    /// # Panics
    ///
    /// Panics if there are any duplicate entries.
    pub fn build(&self) -> DisplayStrSet<'_> {
        DisplayStrSet {
            inner: self.map.build(),
        }
    }
}

impl<'a, T: Into<Cow<'a, str>>> FromIterator<T> for StrSet<'a> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut set = StrSet::new();
        for entry in iter {
            set.entry(entry);
        }
        set
    }
}

/// An adapter for printing a [`StrSet`].
pub struct DisplayStrSet<'a> {
    inner: DisplayStrMap<'a>,
}

impl fmt::Display for DisplayStrSet<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::StrSet {{ map: {} }}", self.inner.path, self.inner)
    }
}

#[cfg(feature = "quote")]
impl quote::ToTokens for DisplayStrSet<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        crate::write_tokens(tokens, self);
    }
}
