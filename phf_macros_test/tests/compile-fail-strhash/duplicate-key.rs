use phf::{StrMap, phf_str_map};

static MAP: StrMap<u32> = phf_str_map! {
    "a" => 1,
    "b" => 2,
    "a" => 3,
};

fn main() {}
