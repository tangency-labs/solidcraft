//! Frozen B-rep records of the bodies the tests mesh (`tests/data`, made by
//! `tests/data/generate_fixtures.rs.txt` from SolidCraft's kernel) and the sizes they were made at.

use std::collections::HashMap;

use solidcore_record::brep;

/// The frozen record `name` (`tests/data/<name>.json.gz`).
pub(crate) fn fixture(name: &str) -> brep::Brep {
    use std::io::Read as _;
    let path = format!("{}/tests/data/{name}.json.gz", env!("CARGO_MANIFEST_DIR"));
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut json = String::new();
    flate2::read::GzDecoder::new(file)
        .read_to_string(&mut json)
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The size of the body `name` was frozen from: its box diagonal as SolidCraft measured it when
/// it was made, which its mesh tolerance rules ([`MeshTolerance::rules`]) take.
pub(crate) fn size_of(name: &str) -> f64 {
    let path = format!("{}/tests/data/sizes.json", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let sizes: HashMap<String, f64> =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{path}: {e}"));
    *sizes
        .get(name)
        .unwrap_or_else(|| panic!("no size for {name}"))
}
