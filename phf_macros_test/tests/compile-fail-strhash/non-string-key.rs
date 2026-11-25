use phf::{StrMap, StrSet, phf_str_map, phf_str_set};

static MAP: StrMap<u32> = phf_str_map! {
    "a" => 1,
    b"b" => 2,
};

static SET: StrSet = phf_str_set! { 1u32 };

fn main() {}
