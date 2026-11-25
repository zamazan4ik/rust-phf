//! Key sets for the lookup benchmarks, taken from the benchmarks of
//! ConstexprCore/perfect_hash. Each set has a `phf::Map` and a `phf::StrMap`
//! mapping every hit key to its index.

/// One benchmark key set.
pub struct DataSet {
    /// Short name of the set.
    pub name: &'static str,
    /// The set as a `phf::Map`.
    pub map: &'static phf::Map<&'static str, u32>,
    /// The set as a `phf::StrMap`.
    pub str_map: &'static phf::StrMap<u32>,
    /// Keys in the set, in index order.
    pub hits: &'static [&'static str],
    /// Keys not in the set.
    pub misses: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/data_sets.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_agree() {
        for set in DATA_SETS {
            for (i, key) in set.hits.iter().enumerate() {
                assert_eq!(set.map.get(key), Some(&(i as u32)), "{}", set.name);
                assert_eq!(set.str_map.get(key), Some(&(i as u32)), "{}", set.name);
            }
            for key in set.misses {
                assert_eq!(set.map.get(key), None, "{}: {key}", set.name);
                assert_eq!(set.str_map.get(key), None, "{}: {key}", set.name);
            }
        }
    }
}
