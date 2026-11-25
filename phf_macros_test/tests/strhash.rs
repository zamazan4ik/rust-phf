//! Tests for `phf_str_map!` and `phf_str_set!`, including cases ported from
//! the C++ test suite of ConstexprCore/perfect_hash.

use phf::{StrMap, StrSet, phf_str_map, phf_str_set};

/// Inputs near `key` that must not match unless they are themselves keys.
fn probes(key: &str) -> Vec<Vec<u8>> {
    let k = key.as_bytes();
    let mut out = vec![
        [k, b"\0"].concat(),
        [k, &[0u8; 16]].concat(),
        [k, &[0u8; 32]].concat(),
        [k, b"x"].concat(),
        [b"x", k].concat(),
    ];
    if !k.is_empty() {
        out.push(k[..k.len() - 1].to_vec());
        out.push(k[1..].to_vec());
        for i in 0..k.len() {
            let mut flipped = k.to_vec();
            flipped[i] ^= 0x40;
            out.push(flipped);
            let mut bumped = k.to_vec();
            bumped[i] = bumped[i].wrapping_add(1);
            out.push(bumped);
        }
    }
    out
}

fn check_set(set: &StrSet, keys: &[&str], absent: &[&str]) {
    assert_eq!(set.len(), keys.len());
    for (i, key) in keys.iter().enumerate() {
        assert!(set.contains(key), "{key:?}");
        assert_eq!(set.get_index(key), Some(i));
        assert_eq!(set.index(i), Some(*key));
        assert_eq!(set.get_key(key), Some(*key));
    }
    for key in absent {
        assert!(!set.contains(key), "{key:?}");
    }
    check_probes(|k| set.contains_bytes(k), keys);
    assert_eq!(set.iter().collect::<Vec<_>>(), keys);
}

fn check_probes(contains: impl Fn(&[u8]) -> bool, keys: &[&str]) {
    for key in keys {
        for probe in probes(key) {
            let expected = keys.iter().any(|k| k.as_bytes() == probe);
            assert_eq!(contains(&probe), expected, "probe {probe:?}");
        }
    }
    // Long inputs, including the 255-byte length that once marked empty slots.
    for len in [254, 255, 256, 4096] {
        for b in 0..=255u8 {
            let mut input = vec![0u8; len];
            input[0] = b;
            assert!(!contains(&input));
        }
    }
}

#[test]
fn map_basics() {
    #[derive(Debug, PartialEq)]
    enum Method {
        Get,
        Post,
        Put,
        Delete,
        Patch,
    }
    static METHODS: StrMap<Method> = phf_str_map! {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "PATCH" => Method::Patch,
    };
    assert_eq!(METHODS.len(), 5);
    assert_eq!(METHODS.get("GET"), Some(&Method::Get));
    assert_eq!(METHODS["DELETE"], Method::Delete);
    assert_eq!(METHODS.get_entry("PATCH"), Some(("PATCH", &Method::Patch)));
    assert_eq!(METHODS.get_bytes(b"PUT"), Some(&Method::Put));
    assert_eq!(METHODS.get("get"), None);
    assert_eq!(METHODS.get("GETS"), None);
    assert_eq!(METHODS.get(""), None);
    assert!(METHODS.contains_key("POST"));
    assert_eq!(
        METHODS.keys().collect::<Vec<_>>(),
        ["GET", "POST", "PUT", "DELETE", "PATCH"]
    );
    assert_eq!(METHODS.values().next(), Some(&Method::Get));
    assert_eq!(METHODS.entries().len(), 5);
    assert_eq!(
        (&METHODS).into_iter().next_back(),
        Some(("PATCH", &Method::Patch))
    );
    check_probes(
        |k| METHODS.get_bytes(k).is_some(),
        &["GET", "POST", "PUT", "DELETE", "PATCH"],
    );
}

#[test]
fn empty() {
    static MAP: StrMap<u32> = phf_str_map! {};
    static SET: StrSet = phf_str_set! {};
    assert!(MAP.is_empty());
    assert_eq!(MAP.get(""), None);
    assert_eq!(MAP.get("a"), None);
    assert!(SET.is_empty());
    assert!(!SET.contains(""));
    assert_eq!(StrMap::<u32>::new(), MAP);
    assert_eq!(StrSet::default(), SET);
}

#[test]
fn or_patterns_and_cfg() {
    static MAP: StrMap<u8> = phf_str_map! {
        "+" | "add" | "plus" => 1,
        #[cfg(feature = "enabled_feature")]
        "enabled" => 2,
        #[cfg(feature = "disabled_feature")]
        "disabled" => 3,
        #[cfg(all())]
        #[cfg(not(any()))]
        "both" => 4,
    };
    assert_eq!(MAP.get("+"), Some(&1));
    assert_eq!(MAP.get("plus"), Some(&1));
    assert_eq!(MAP.get("enabled"), Some(&2));
    assert_eq!(MAP.get("disabled"), None);
    assert_eq!(MAP.get("both"), Some(&4));
    assert_eq!(MAP.len(), 5);
}

#[test]
fn edge_cases() {
    macro_rules! set_case {
        ([$($key:literal),*], [$($absent:literal),*]) => {{
            static SET: StrSet = phf_str_set! { $($key),* };
            check_set(&SET, &[$($key),*], &[$($absent),*]);
        }};
    }
    // prefix strings
    set_case!(["a", "ab", "abc", "abcd"], ["abcde", "", "b"]);
    // similar strings
    set_case!(["foo", "fop", "foq", "goo", "boo"], ["fo", "foox", "zoo"]);
    // 26 single-char strings
    set_case!(
        [
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
            "r", "s", "t", "u", "v", "w", "x", "y", "z"
        ],
        ["A", "aa", "", "0"]
    );
    // two element set
    set_case!(["yes", "no"], ["maybe", "ye"]);
    // all same length
    set_case!(["cat", "dog", "bat", "rat", "hat"], ["car", "mat"]);
    // differ only at the last char
    set_case!(["abp", "abq", "abr", "abs", "abt"], ["abx", "ab"]);
    // differ only at the first char
    set_case!(["a_end", "b_end", "c_end", "d_end", "e_end"], ["z_end"]);
    // interior position only differs
    set_case!(["xax", "xbx", "xcx", "xdx"], ["xzx", "xxx"]);
    // needs two positions
    set_case!(["ax", "ay", "bx", "by"], ["cx", "az"]);
    // forces backtracking
    set_case!(["cat", "cot", "bat", "bot"], ["cut", "bit"]);
    // mixed lengths with shared characters
    set_case!(["a", "aa", "aaa", "ab", "ba", "abc"], ["b", "aaaa"]);
    // 15 C++ keywords
    set_case!(
        [
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
            "default"
        ],
        ["delete", "do", "align"]
    );
    // long keys
    set_case!(
        [
            "this_is_a_very_long_key_one",
            "this_is_a_very_long_key_two",
            "this_is_a_very_long_key_three",
            "short"
        ],
        ["this_is_a_very_long_key_four", "this_is_a_very_long_key_"]
    );
    // empty string as a key
    set_case!(["", "a", "bc"], ["b", "ab"]);
    // middle bytes differ
    set_case!(["abcXdef", "abcYdef", "abcZdef"], ["abcWdef"]);
    // stock tickers
    set_case!(
        [
            "AAPL", "AMD", "AMZN", "BA", "C", "COST", "CVX", "DIS", "F", "GE", "GOOG", "GS", "HD",
            "IBM", "JPM", "MA", "META", "MSFT", "NVDA", "TSLA"
        ],
        ["PLTR", "UBER", "G"]
    );
    // similar 2-char keys
    set_case!(
        [
            "AA", "AB", "AC", "AD", "AE", "AF", "AG", "AH", "AI", "AJ", "BA", "BB", "BC", "BD",
            "BE", "BF", "BG", "BH", "BI", "BJ", "CA", "CB", "CC", "CD", "CE"
        ],
        ["DA", "AK"]
    );
    // MIME types
    set_case!(
        [
            "application/json",
            "text/html",
            "image/png",
            "text/css",
            "text/plain"
        ],
        ["text/htm", "application/jso"]
    );
}

#[test]
fn key_length_layouts() {
    // One set per comparison layout, each mixing in NUL bytes, as in the C++
    // key-length tests.
    macro_rules! set_case {
        ($($key:literal),*) => {{
            static SET: StrSet = phf_str_set! { $($key),* };
            check_set(&SET, &[$($key),*], &[]);
        }};
    }
    set_case!("", "a", "\0");
    set_case!("a", "ab", "abc", "abcd", "\0", "a\0", "a\0c", "\0\0\0\0");
    set_case!("http", "https", "ftp", "ws", "wss", "a\0b\0c");
    set_case!(
        "abcdefg",
        "abcdef",
        "a",
        "\0\0\0\0\0\0\0",
        "ab\0\0\0\0g",
        "\x07"
    );
    set_case!(
        "abcdefgh",
        "abcdefg",
        "a",
        "\0\0\0\0\0\0\0\0",
        "abc\0\0\0\0h"
    );
    set_case!(
        "Accept-Encoding",
        "Accept",
        "Accept-Encodin\0",
        "\0",
        "Content-Type",
        "\x0f",
        "\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"
    );
    set_case!("application/json", "text/html", "a\0", "application/jso\0");
    set_case!(
        "application/json5",
        "text/html",
        "a\0",
        "application/json\0"
    );
    set_case!("", "ab", "ac");
    set_case!("", "abcdefg", "\0");
}

#[test]
fn http_headers_map() {
    static HEADERS: StrMap<u32> = phf_str_map! {
        "Accept" => 0, "Accept-Encoding" => 1, "Authorization" => 2, "Cache-Control" => 3,
        "Connection" => 4, "Content-Length" => 5, "Content-Type" => 6, "Cookie" => 7,
        "Date" => 8, "Host" => 9, "If-None-Match" => 10, "Location" => 11, "Origin" => 12,
        "Referer" => 13, "Server" => 14, "Set-Cookie" => 15, "User-Agent" => 16, "Vary" => 17,
        "Via" => 18, "X-Forwarded-For" => 19,
    };
    for (i, (key, value)) in HEADERS.entries().enumerate() {
        assert_eq!(*value, i as u32);
        assert_eq!(HEADERS.get(key), Some(&(i as u32)));
    }
    let keys: Vec<&str> = HEADERS.keys().collect();
    check_probes(|k| HEADERS.get_bytes(k).is_some(), &keys);
}

static WIDE_KEYS: &[&str] = &[
    "AAA", "AAB", "AAC", "AAD", "AAE", "AAF", "AAG", "AAH", "AAI", "AAJ", "AAK", "AAL", "AAM",
    "AAN", "AAO", "AAP", "AAQ", "AAR", "AAS", "AAT", "AAU", "AAV", "AAW", "AAX", "AAY", "AAZ",
    "ABA", "ABB", "ABC", "ABD", "ABE", "ABF", "ABG", "ABH", "ABI", "ABJ", "ABK", "ABL", "ABM",
    "ABN", "ABO", "ABP", "ABQ", "ABR", "ABS", "ABT", "ABU", "ABV", "ABW", "ABX", "ABY", "ABZ",
    "ACA", "ACB", "ACC", "ACD", "ACE", "ACF", "ACG", "ACH", "ACI", "ACJ", "ACK", "ACL", "ACM",
    "ACN", "ACO", "ACP", "ACQ", "ACR", "ACS", "ACT", "ACU", "ACV", "ACW", "ACX", "ACY", "ACZ",
    "ADA", "ADB", "ADC", "ADD", "ADE", "ADF", "ADG", "ADH", "ADI", "ADJ", "ADK", "ADL", "ADM",
    "ADN", "ADO", "ADP", "ADQ", "ADR", "ADS", "ADT", "ADU", "ADV", "ADW", "ADX", "ADY", "ADZ",
    "AEA", "AEB", "AEC", "AED", "AEE", "AEF", "AEG", "AEH", "AEI", "AEJ", "AEK", "AEL", "AEM",
    "AEN", "AEO", "AEP", "AEQ", "AER", "AES", "AET", "AEU", "AEV", "AEW", "AEX", "AEY", "AEZ",
    "AFA", "AFB", "AFC", "AFD", "AFE", "AFF", "AFG", "AFH", "AFI", "AFJ", "AFK", "AFL", "AFM",
    "AFN", "AFO", "AFP", "AFQ", "AFR", "AFS", "AFT", "AFU", "AFV", "AFW", "AFX", "AFY", "AFZ",
    "AGA", "AGB", "AGC", "AGD", "AGE", "AGF", "AGG", "AGH", "AGI", "AGJ", "AGK", "AGL", "AGM",
    "AGN", "AGO", "AGP", "AGQ", "AGR", "AGS", "AGT", "AGU", "AGV", "AGW", "AGX", "AGY", "AGZ",
    "AHA", "AHB", "AHC", "AHD", "AHE", "AHF", "AHG", "AHH", "AHI", "AHJ", "AHK", "AHL", "AHM",
    "AHN", "AHO", "AHP", "AHQ", "AHR", "AHS", "AHT", "AHU", "AHV", "AHW", "AHX", "AHY", "AHZ",
    "AIA", "AIB", "AIC", "AID", "AIE", "AIF", "AIG", "AIH", "AII", "AIJ", "AIK", "AIL", "AIM",
    "AIN", "AIO", "AIP", "AIQ", "AIR", "AIS", "AIT", "AIU", "AIV", "AIW", "AIX", "AIY", "AIZ",
    "AJA", "AJB", "AJC", "AJD", "AJE", "AJF", "AJG", "AJH", "AJI", "AJJ", "AJK", "AJL", "AJM",
    "AJN", "AJO", "AJP", "AJQ", "AJR", "AJS", "AJT", "AJU", "AJV", "AJW", "AJX", "AJY", "AJZ",
    "AKA", "AKB", "AKC", "AKD", "AKE", "AKF", "AKG", "AKH", "AKI", "AKJ", "AKK", "AKL", "AKM",
    "AKN", "AKO", "AKP", "AKQ", "AKR", "AKS", "AKT", "AKU", "AKV", "AKW", "AKX", "AKY", "AKZ",
    "ALA", "ALB", "ALC", "ALD", "ALE", "ALF", "ALG", "ALH", "ALI", "ALJ", "ALK", "ALL", "ALM",
    "ALN",
];

#[test]
fn wide_set() {
    static WIDE: StrSet = phf_str_set! {
    "AAA", "AAB", "AAC", "AAD", "AAE", "AAF", "AAG", "AAH", "AAI", "AAJ", "AAK", "AAL",
    "AAM", "AAN", "AAO", "AAP", "AAQ", "AAR", "AAS", "AAT", "AAU", "AAV", "AAW", "AAX",
    "AAY", "AAZ", "ABA", "ABB", "ABC", "ABD", "ABE", "ABF", "ABG", "ABH", "ABI", "ABJ",
    "ABK", "ABL", "ABM", "ABN", "ABO", "ABP", "ABQ", "ABR", "ABS", "ABT", "ABU", "ABV",
    "ABW", "ABX", "ABY", "ABZ", "ACA", "ACB", "ACC", "ACD", "ACE", "ACF", "ACG", "ACH",
    "ACI", "ACJ", "ACK", "ACL", "ACM", "ACN", "ACO", "ACP", "ACQ", "ACR", "ACS", "ACT",
    "ACU", "ACV", "ACW", "ACX", "ACY", "ACZ", "ADA", "ADB", "ADC", "ADD", "ADE", "ADF",
    "ADG", "ADH", "ADI", "ADJ", "ADK", "ADL", "ADM", "ADN", "ADO", "ADP", "ADQ", "ADR",
    "ADS", "ADT", "ADU", "ADV", "ADW", "ADX", "ADY", "ADZ", "AEA", "AEB", "AEC", "AED",
    "AEE", "AEF", "AEG", "AEH", "AEI", "AEJ", "AEK", "AEL", "AEM", "AEN", "AEO", "AEP",
    "AEQ", "AER", "AES", "AET", "AEU", "AEV", "AEW", "AEX", "AEY", "AEZ", "AFA", "AFB",
    "AFC", "AFD", "AFE", "AFF", "AFG", "AFH", "AFI", "AFJ", "AFK", "AFL", "AFM", "AFN",
    "AFO", "AFP", "AFQ", "AFR", "AFS", "AFT", "AFU", "AFV", "AFW", "AFX", "AFY", "AFZ",
    "AGA", "AGB", "AGC", "AGD", "AGE", "AGF", "AGG", "AGH", "AGI", "AGJ", "AGK", "AGL",
    "AGM", "AGN", "AGO", "AGP", "AGQ", "AGR", "AGS", "AGT", "AGU", "AGV", "AGW", "AGX",
    "AGY", "AGZ", "AHA", "AHB", "AHC", "AHD", "AHE", "AHF", "AHG", "AHH", "AHI", "AHJ",
    "AHK", "AHL", "AHM", "AHN", "AHO", "AHP", "AHQ", "AHR", "AHS", "AHT", "AHU", "AHV",
    "AHW", "AHX", "AHY", "AHZ", "AIA", "AIB", "AIC", "AID", "AIE", "AIF", "AIG", "AIH",
    "AII", "AIJ", "AIK", "AIL", "AIM", "AIN", "AIO", "AIP", "AIQ", "AIR", "AIS", "AIT",
    "AIU", "AIV", "AIW", "AIX", "AIY", "AIZ", "AJA", "AJB", "AJC", "AJD", "AJE", "AJF",
    "AJG", "AJH", "AJI", "AJJ", "AJK", "AJL", "AJM", "AJN", "AJO", "AJP", "AJQ", "AJR",
    "AJS", "AJT", "AJU", "AJV", "AJW", "AJX", "AJY", "AJZ", "AKA", "AKB", "AKC", "AKD",
    "AKE", "AKF", "AKG", "AKH", "AKI", "AKJ", "AKK", "AKL", "AKM", "AKN", "AKO", "AKP",
    "AKQ", "AKR", "AKS", "AKT", "AKU", "AKV", "AKW", "AKX", "AKY", "AKZ", "ALA", "ALB",
    "ALC", "ALD", "ALE", "ALF", "ALG", "ALH", "ALI", "ALJ", "ALK", "ALL", "ALM", "ALN"
    };
    check_set(&WIDE, WIDE_KEYS, &["ZZZ", "AAAA", "", "A"]);
}

#[test]
fn long_keys() {
    static FQCN: StrSet = phf_str_set! {
        "java.util.concurrent.ConcurrentHashMap",
        "java.util.concurrent.ConcurrentLinkedQueue",
        "java.util.concurrent.atomic.AtomicInteger",
        "java.util.concurrent.atomic.AtomicLong",
        "java.lang.String",
        "java.lang.StringBuilder",
        "java.io.BufferedReader",
        "java.io.BufferedWriter",
    };
    let keys: Vec<&str> = FQCN.iter().collect();
    check_set(
        &FQCN,
        &keys,
        &["java.util.concurrent.ConcurrentHashMaP", "java.lang"],
    );
}

#[test]
fn serde() {
    static MAP: StrMap<u32> = phf_str_map! { "b" => 2, "a" => 1 };
    static SET: StrSet = phf_str_set! { "x", "y" };
    assert_eq!(serde_json::to_string(&MAP).unwrap(), r#"{"b":2,"a":1}"#);
    assert_eq!(serde_json::to_string(&SET).unwrap(), r#"["x","y"]"#);
}

#[test]
fn debug() {
    static MAP: StrMap<u32> = phf_str_map! { "b" => 2, "a" => 1 };
    assert_eq!(format!("{MAP:?}"), r#"{"b": 2, "a": 1}"#);
}
