//! The kernel rig's corpus (`crates/solidcore/tests/kernel_cases/`, exported from SolidCraft's
//! rig): every operand record an entry carries (in its `<case>.bodies.json.gz`) is this crate's
//! B-rep record, reads with its serde form (unknown fields refused) and passes its structural
//! validation, and every operation an entry lists takes records it gives or bodies an earlier
//! operation made. The runner that replays the operations lands with
//! `solidcore-ops`; until then this keeps the corpus readable as the record it is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use solidcore_record::brep::Brep;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

/// Every entry's file: `<area>/<case>.json`, a ladder tier's under `ladder/<tier>/`.
fn entries(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            entries(&p, out);
        } else if p.extension().is_some_and(|x| x == "json")
            && p.parent() != Some(corpus().as_path())
        {
            out.push(p);
        }
    }
}

fn validate_counts(root: &Path, files: &[PathBuf], manifest: &Value) {
    let listed: std::collections::BTreeMap<_, _> = manifest["areas"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(area, a)| (PathBuf::from(area), a["cases"].as_u64().unwrap()))
        .collect();
    let mut actual = std::collections::BTreeMap::new();
    for file in files {
        let area = file.strip_prefix(root).unwrap().parent().unwrap();
        *actual.entry(area.to_path_buf()).or_insert(0_u64) += 1;
    }
    assert_eq!(actual, listed, "the manifest counts entries per area");
}

fn validate_results(e: &Value, bodies: &serde_json::Map<String, Value>, made: &[&str], f: &Path) {
    // Each final body is an operation's output or a given operand.
    for r in e["result"].as_array().unwrap() {
        let object = r
            .as_object()
            .unwrap_or_else(|| panic!("{}: final body is not an object", f.display()));
        match (object.get("made"), object.get("operand")) {
            (Some(value), None) => {
                let b = value
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: made must be a string", f.display()));
                assert!(
                    made.contains(&b),
                    "{}: {b} is made by no operation",
                    f.display()
                );
            }
            (None, Some(value)) => {
                let b = value
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: operand must be a string", f.display()));
                assert!(
                    bodies.contains_key(b),
                    "{}: operand {b} has no record",
                    f.display()
                );
            }
            _ => panic!("{}: final body must name made or operand", f.display()),
        }
    }
}

#[test]
fn every_operand_is_a_valid_record_and_every_input_is_given() {
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("manifest.json")).unwrap())
            .unwrap();
    validate_counts(&corpus(), &files, &manifest);
    let mut records = 0;
    for f in &files {
        let e: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        assert_eq!(
            Path::new(e["area"].as_str().unwrap()),
            f.strip_prefix(corpus()).unwrap().parent().unwrap(),
            "{}: area does not match path",
            f.display()
        );
        let bodies = e["bodies"].as_object().unwrap();
        if !bodies.is_empty() {
            let gz = f.with_file_name(e["records"].as_str().unwrap());
            let mut text = String::new();
            flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap())
                .read_to_string(&mut text)
                .unwrap();
            let all: serde_json::Map<String, Value> = serde_json::from_str(&text).unwrap();
            assert_eq!(
                all.len(),
                bodies.len(),
                "{}: one record per body",
                gz.display()
            );
            for name in bodies.keys() {
                let r: Brep = serde_json::from_value(all[name].clone())
                    .unwrap_or_else(|err| panic!("{}: {name}: {err}", gz.display()));
                r.validate_built()
                    .unwrap_or_else(|err| panic!("{}: {name}: {err}", gz.display()));
                records += 1;
            }
        }
        // An operation's input is a record given here or an earlier operation's output.
        let mut made: Vec<&str> = Vec::new();
        for c in e["calls"].as_array().unwrap() {
            for i in c["inputs"].as_array().unwrap() {
                let i = i.as_str().unwrap();
                assert!(
                    bodies.contains_key(i) || made.contains(&i),
                    "{}: {i} is neither a record nor made before",
                    f.display()
                );
            }
            made.extend(
                c["outputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            );
        }
        validate_results(&e, bodies, &made, f);
        if e.get("records").is_some_and(|r| !r.is_null()) {
            assert!(
                !bodies.is_empty(),
                "{}: a records file with no body",
                f.display()
            );
        }
        if e["expressible"].as_bool() == Some(true) {
            assert_eq!(
                e["operation_expressible"].as_bool(),
                Some(true),
                "{}",
                f.display()
            );
        }
        if e["operation_expressible"].as_bool() == Some(true) {
            assert!(
                !e["calls"].as_array().unwrap().is_empty(),
                "{}: expressible with no operation",
                f.display()
            );
        } else {
            assert!(
                !e["why_not"].as_array().unwrap().is_empty(),
                "{}: not expressible, with no reason",
                f.display()
            );
        }
    }
    assert!(records > 0);
}

#[test]
fn regression_area_counts() {
    let root = Path::new("corpus");
    let files: Vec<PathBuf> = ["a/one.json", "a/two.json", "ladder/tier/one.json"]
        .iter()
        .map(|p| root.join(p))
        .collect();
    let valid = serde_json::json!({"areas": {"a": {"cases": 2}, "ladder/tier": {"cases": 1}}});
    validate_counts(root, &files, &valid);
    for invalid in [
        serde_json::json!({"areas": {"a": {"cases": 1}, "ladder/tier": {"cases": 2}}}),
        serde_json::json!({"areas": {"a": {"cases": 3}}}),
        serde_json::json!({"areas": {"a": {"cases": 2}, "ladder/other": {"cases": 1}}}),
    ] {
        assert!(
            std::panic::catch_unwind(|| validate_counts(root, &files, &invalid)).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn regression_final_references() {
    let bodies = serde_json::json!({"given": {}})
        .as_object()
        .unwrap()
        .clone();
    let made = ["made"];
    let path = Path::new("case.json");
    for result in [
        serde_json::json!({"made":"made"}),
        serde_json::json!({"operand":"given"}),
        serde_json::json!({"body":"b1", "made":"made"}),
        serde_json::json!({"body":"b2", "operand":"given"}),
    ] {
        validate_results(
            &serde_json::json!({"result":[result]}),
            &bodies,
            &made,
            path,
        );
    }
    for result in [
        serde_json::json!({"made": 2}),
        serde_json::json!({"operand": null}),
        serde_json::json!({}),
        serde_json::json!("made"),
        serde_json::json!({"made":"unknown"}),
        serde_json::json!({"operand":"unknown"}),
        serde_json::json!({"made":"made", "operand":"given"}),
    ] {
        let e = serde_json::json!({"result":[result]});
        assert!(
            std::panic::catch_unwind(|| validate_results(&e, &bodies, &made, path)).is_err(),
            "accepted {e}"
        );
    }
}
