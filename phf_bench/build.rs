//! Generates, for every data set in `data/`, a `phf::Map`, a `phf::StrMap`
//! and the hit and miss key lists.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const SETS: &[&str] = &[
    "protocols",
    "cpp_keywords",
    "mime_types",
    "http_headers",
    "letters",
    "counters",
    "java_fqcn",
    "js_reserved",
    "http_headers50",
    "sp100",
    "sp500",
    "nasdaq",
];

fn read_keys(path: &Path) -> Vec<String> {
    println!("cargo::rerun-if-changed={}", path.display());
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn main() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let mut out = String::new();
    let mut table = String::from("pub static DATA_SETS: &[DataSet] = &[\n");
    for name in SETS {
        let hits = read_keys(&data.join(format!("{name}.hits")));
        let misses = read_keys(&data.join(format!("{name}.misses")));
        let upper = name.to_uppercase();

        let mut map = phf_codegen::Map::new();
        let mut str_map = phf_codegen::StrMap::new();
        for (i, key) in hits.iter().enumerate() {
            map.entry(key.as_str(), i.to_string());
            str_map.entry(key.as_str(), i.to_string());
        }
        writeln!(
            out,
            "static {upper}_MAP: ::phf::Map<&'static str, u32> = {};",
            map.build()
        )
        .unwrap();
        writeln!(
            out,
            "static {upper}_STR_MAP: ::phf::StrMap<u32> = {};",
            str_map.build()
        )
        .unwrap();
        writeln!(out, "static {upper}_HITS: &[&str] = &{hits:?};").unwrap();
        writeln!(out, "static {upper}_MISSES: &[&str] = &{misses:?};").unwrap();
        writeln!(
            table,
            "    DataSet {{ name: {name:?}, map: &{upper}_MAP, str_map: &{upper}_STR_MAP, \
             hits: {upper}_HITS, misses: {upper}_MISSES }},"
        )
        .unwrap();
    }
    table.push_str("];\n");
    out.push_str(&table);
    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("data_sets.rs");
    fs::write(dest, out).unwrap();
}
