//! The kernel rig's corpus (`crates/solidcore/tests/kernel_cases/`, exported from SolidCraft's
//! rig): every operand record an entry carries (in its `<case>.bodies.json.gz`) is this crate's
//! B-rep record, reads with its serde form (unknown fields refused) and passes its structural
//! validation, and every operation an entry lists takes records it gives or bodies an earlier
//! operation made. Nonempty replay inputs are unique across the corpus: descriptions and
//! expected answers cannot disguise duplicate calls on the same operand records.
//! Recorded volumes of passing cases without known-failure markers must also agree with
//! independent analytic expectations.

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

/// Exact replay input, independent of the entry's description, expected values and gzip
/// filename. Do not round numbers: even a tiny parameter change can select another branch.
/// Empty traces describe missing/application-only operations, not duplicate kernel tests.
fn replay_input(e: &Value, records: &Value) -> Option<Vec<u8>> {
    if e["calls"].as_array().unwrap().is_empty() {
        return None;
    }
    let mut input = serde_json::json!({
        "calls": e["calls"],
        "bodies": e["bodies"],
        "records": records,
    });
    input.sort_all_objects();
    Some(serde_json::to_vec(&input).unwrap())
}

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

#[test]
fn no_two_entries_replay_the_same_calls_and_bodies() {
    let root = corpus();
    let mut files = Vec::new();
    entries(&root, &mut files);
    files.sort();
    let mut seen = std::collections::BTreeMap::new();
    let mut duplicates = Vec::new();
    for f in files {
        let e: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
        let records = match e["records"].as_str() {
            Some(name) => {
                let file = std::fs::File::open(f.with_file_name(name)).unwrap();
                let mut text = String::new();
                flate2::read::GzDecoder::new(file)
                    .read_to_string(&mut text)
                    .unwrap();
                serde_json::from_str(&text).unwrap()
            }
            None => serde_json::json!({}),
        };
        if let Some(input) = replay_input(&e, &records) {
            let name = f.strip_prefix(&root).unwrap().display().to_string();
            match seen.entry(input) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(name);
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    duplicates.push(format!("{} = {name}", slot.get()));
                }
            }
        }
    }
    assert!(
        duplicates.is_empty(),
        "duplicate kernel replay inputs (fix lost trace arguments before deduplicating):\n{}",
        duplicates.join("\n")
    );
}

#[test]
fn replay_identity_uses_arguments_and_records_not_case_metadata() {
    let entry = serde_json::json!({
        "case": "one", "records": "one.bodies.json.gz", "expected": {"volume": 1},
        "bodies": {"b1": {"trusted": true, "keeping_splines": false}},
        "calls": [{"op": "blend_dropping", "inputs": ["b1"], "outputs": ["b2"],
            "params": {"turn": "mitre", "sizes": [1.0]}}],
    });
    let records = serde_json::json!({"b1": {"vertices": [[0.0, 0.0, 0.0]]}});
    let key = replay_input(&entry, &records);
    let mut renamed = entry.clone();
    renamed["case"] = serde_json::json!("two");
    renamed["records"] = serde_json::json!("two.bodies.json.gz");
    renamed["expected"]["volume"] = serde_json::json!(2);
    assert_eq!(key, replay_input(&renamed, &records));

    for (pointer, value) in [
        ("/calls/0/params/turn", serde_json::json!("swept")),
        ("/calls/0/params/sizes/0", serde_json::json!(1.0 + 1e-12)),
        ("/bodies/b1/trusted", serde_json::json!(false)),
        ("/bodies/b1/keeping_splines", serde_json::json!(true)),
    ] {
        let mut changed = entry.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert_ne!(key, replay_input(&changed, &records), "ignored {pointer}");
    }
    let mut changed_records = records.clone();
    changed_records["b1"]["vertices"][0][0] = serde_json::json!(1e-12);
    assert_ne!(key, replay_input(&entry, &changed_records));
    let mut extra_call = entry.clone();
    extra_call["calls"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "op": "copy", "inputs": ["b2"], "outputs": ["b3"], "params": {},
        }));
    assert_ne!(key, replay_input(&extra_call, &records));
    assert_eq!(
        None,
        replay_input(&serde_json::json!({"calls": []}), &records)
    );
}

/// The manifest is the export's record: the SolidCraft and solidcore commits it was made at
/// (`solidcraft`, `solidcore`) and every case that isn't expressible with its reasons
/// (`not_expressible`). It stores no per-area totals: those are counts of the case files, and a
/// stored copy only made every pull request that adds a case edit the same lines. Every case it
/// names must be a case file of the corpus, and one the kernel alone does not account for.
fn validate_manifest(root: &Path, files: &[PathBuf], manifest: &Value) {
    let m = manifest.as_object().expect("manifest.json: an object");
    for k in m.keys() {
        assert!(
            ["solidcraft", "solidcore", "not_expressible"].contains(&k.as_str()),
            "manifest.json: `{k}` is not a manifest field (per-area totals are counted from the \
             case files, never stored)"
        );
    }
    for k in ["solidcraft", "solidcore"] {
        assert!(
            m.get(k).is_some_and(|v| v.is_string() || v.is_null()),
            "manifest.json: `{k}` must be the commit the cases were exported at"
        );
    }
    let cases: std::collections::BTreeMap<String, &PathBuf> = files
        .iter()
        .map(|f| {
            let name = f
                .strip_prefix(root)
                .unwrap()
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            (name, f)
        })
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for n in m["not_expressible"]
        .as_array()
        .expect("manifest.json: `not_expressible` is a list")
    {
        let case = n["case"]
            .as_str()
            .unwrap_or_else(|| panic!("manifest.json: not_expressible entry {n} has no `case`"));
        assert!(
            seen.insert(case),
            "manifest.json: not_expressible lists {case} twice"
        );
        let file = cases.get(case).unwrap_or_else(|| {
            panic!(
                "manifest.json: not_expressible lists {case}, which has no file \
                 ({case}.json under kernel_cases/): remove it from the manifest"
            )
        });
        if let Ok(text) = std::fs::read_to_string(file) {
            let e: Value = serde_json::from_str(&text).unwrap();
            assert_ne!(
                e["operation_expressible"].as_bool(),
                Some(true),
                "manifest.json: not_expressible lists {case}, which {} marks operation_expressible",
                file.display()
            );
        }
    }
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

fn validate_continuations(e: &Value, f: &Path) {
    let calls = e["calls"].as_array().unwrap();
    let mut terminal = false;
    for (i, c) in calls.iter().enumerate() {
        if let Some(marker) = c.get("continued") {
            assert!(
                marker.is_boolean(),
                "{}: call {i}: continued must be a boolean",
                f.display()
            );
            assert!(
                c["error"].is_string(),
                "{}: call {i}: continued requires an expected error",
                f.display()
            );
            assert!(
                !terminal,
                "{}: call {i}: continued follows a terminal refusal",
                f.display()
            );
        }
        terminal |= c["error"].is_string() && c["continued"].as_bool() != Some(true);
    }
}

fn validate_analytic_volume(e: &Value, f: &Path) {
    if e["expected"]["source"].as_str() != Some("analytic") {
        return;
    }
    // Failing cases intentionally preserve the rig's incorrect output.
    if !e["rig"]["known_failing"].is_null() || e["rig"]["status"].as_str() != Some("pass") {
        return;
    }
    let Some(got) = e["rig"]["measured"]["volume_mm3"].as_f64() else {
        return;
    };
    let expected = &e["expected"]["volume_mm3"];
    if let Some(want) = expected.as_f64() {
        // Match the replay runner: abs_mm takes precedence over relative tolerance.
        let tolerance = &e["expected"]["tolerance"];
        let limit = tolerance["abs_mm"]
            .as_f64()
            .unwrap_or_else(|| tolerance["rel"].as_f64().unwrap_or(1e-3) * want.abs());
        assert!(
            (got - want).abs() <= limit,
            "{}: recorded volume {got} mm³ differs from analytic {want} mm³ (allowed difference {limit} mm³)",
            f.display()
        );
    } else if let Some(bounds) = expected.as_array() {
        assert_eq!(
            bounds.len(),
            2,
            "{}: volume interval needs two bounds",
            f.display()
        );
        let lo = bounds[0]
            .as_f64()
            .unwrap_or_else(|| panic!("{}: volume interval needs numeric bounds", f.display()));
        let hi = bounds[1]
            .as_f64()
            .unwrap_or_else(|| panic!("{}: volume interval needs numeric bounds", f.display()));
        assert!(
            lo <= got && got <= hi,
            "{}: recorded volume {got} mm³ is outside analytic interval [{lo}, {hi}] mm³",
            f.display()
        );
    }
}

#[test]
fn every_operand_is_a_valid_record_and_every_input_is_given() {
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("manifest.json")).unwrap())
            .unwrap();
    validate_manifest(&corpus(), &files, &manifest);
    let mut records = 0;
    for f in &files {
        let e: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        validate_analytic_volume(&e, f);
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
        validate_continuations(&e, f);
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
fn regression_analytic_scalar_volumes() {
    for (want, got, tolerance, accepted) in [
        (1000.0, 1001.0, serde_json::json!(null), true),
        (1000.0, 999.0, serde_json::json!(null), true),
        (1000.0, 2000.0, serde_json::json!(null), false),
        (1000.0, 998.0, serde_json::json!(null), false),
        (0.001, 0.0015, serde_json::json!(null), false),
        (0.0, 0.0, serde_json::json!(null), true),
        (0.0, 1e-12, serde_json::json!(null), false),
        (1000.0, 1010.0, serde_json::json!({"rel": 0.01}), true),
        (1000.0, 1011.0, serde_json::json!({"rel": 0.01}), false),
        (10.0, 10.5, serde_json::json!({"abs_mm": 0.5}), true),
        (
            10.0,
            10.6,
            serde_json::json!({"abs_mm": 0.5, "rel": 1.0}),
            false,
        ),
        (
            10.0,
            10.5,
            serde_json::json!({"abs_mm": 0.5, "rel": 0.0}),
            true,
        ),
    ] {
        let e = serde_json::json!({
            "expected": {"source": "analytic", "volume_mm3": want, "tolerance": tolerance},
            "rig": {"status": "pass", "measured": {"volume_mm3": got}},
        });
        assert_eq!(
            std::panic::catch_unwind(|| validate_analytic_volume(&e, Path::new("case.json")))
                .is_ok(),
            accepted,
            "wrong verdict for {e}"
        );
    }
}

#[test]
fn regression_analytic_volume_intervals() {
    for (got, accepted) in [
        (9.0, false),
        (10.0, true),
        (15.0, true),
        (20.0, true),
        (21.0, false),
    ] {
        let e = serde_json::json!({
            "expected": {"source": "analytic", "volume_mm3": [10.0, 20.0],
                "tolerance": {"abs_mm": 100.0, "rel": 100.0}},
            "rig": {"status": "pass", "measured": {"volume_mm3": got}},
        });
        assert_eq!(
            std::panic::catch_unwind(|| validate_analytic_volume(&e, Path::new("case.json")))
                .is_ok(),
            accepted,
            "wrong verdict for {e}"
        );
    }
}

#[test]
fn regression_analytic_volume_check_respects_rig_status() {
    for (rig, accepted) in [
        (serde_json::json!({"status": "pass"}), false),
        (
            serde_json::json!({"status": "pass", "known_failing": null}),
            false,
        ),
        (serde_json::json!({"status": "fail"}), true),
        (serde_json::json!({"status": "error"}), true),
        (serde_json::json!({}), true),
        (
            serde_json::json!({"status": "fail", "known_failing": "wrong volume"}),
            true,
        ),
        (
            serde_json::json!({"status": "pass", "known_failing": "wrong volume"}),
            true,
        ),
    ] {
        for expected in [serde_json::json!(1.0), serde_json::json!([1.0, 2.0])] {
            let mut e = serde_json::json!({
                "expected": {"source": "analytic", "volume_mm3": expected},
                "rig": rig,
            });
            e["rig"]["measured"] = serde_json::json!({"volume_mm3": 3.0});
            assert_eq!(
                std::panic::catch_unwind(|| validate_analytic_volume(&e, Path::new("case.json")))
                    .is_ok(),
                accepted,
                "wrong verdict for {e}"
            );
        }
    }
}

#[test]
fn analytic_volume_check_skips_unavailable_comparisons() {
    for e in [
        serde_json::json!({"expected": {"source": "analytic", "volume_mm3": 1.0}}),
        serde_json::json!({"expected": {"source": "analytic", "volume_mm3": 1.0},
            "rig": {"status": "pass"}}),
        serde_json::json!({"expected": {"source": "analytic", "volume_mm3": 1.0},
            "rig": {"status": "pass", "measured": {"volume_mm3": null}}}),
        serde_json::json!({"expected": {"source": "analytic"},
            "rig": {"status": "pass", "measured": {"volume_mm3": 2.0}}}),
        serde_json::json!({"expected": {"source": "measured", "volume_mm3": 1.0},
            "rig": {"status": "pass", "measured": {"volume_mm3": 2.0}}}),
        serde_json::json!({"expected": {"volume_mm3": 1.0},
            "rig": {"status": "pass", "measured": {"volume_mm3": 2.0}}}),
    ] {
        validate_analytic_volume(&e, Path::new("case.json"));
    }
}

#[test]
fn regression_manifest_names_only_case_files() {
    let root = Path::new("corpus");
    let files: Vec<PathBuf> = ["a/one.json", "a/two.json", "ladder/tier/one.json"]
        .iter()
        .map(|p| root.join(p))
        .collect();
    let pins = |not: Value| serde_json::json!({"solidcraft": "c1", "solidcore": "c2", "not_expressible": not});
    let valid = pins(
        serde_json::json!([{"case": "a/two", "why": ["x"]}, {"case": "ladder/tier/one", "why": []}]),
    );
    validate_manifest(root, &files, &valid);
    validate_manifest(root, &files, &pins(serde_json::json!([])));
    let mut totals = pins(serde_json::json!([]));
    totals["areas"] = serde_json::json!({"a": {"cases": 2}});
    for invalid in [
        // a case with no file
        pins(serde_json::json!([{"case": "a/three", "why": []}])),
        pins(serde_json::json!([{"case": "ladder/other/one", "why": []}])),
        // listed twice
        pins(serde_json::json!([{"case": "a/one", "why": []}, {"case": "a/one", "why": []}])),
        // no case name
        pins(serde_json::json!([{"why": []}])),
        // stored totals
        totals,
        // no pins
        serde_json::json!({"not_expressible": []}),
    ] {
        assert!(
            std::panic::catch_unwind(|| validate_manifest(root, &files, &invalid)).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn regression_handled_refusal_markers() {
    let path = Path::new("case.json");
    for calls in [
        serde_json::json!([{"error": "refused"}]),
        serde_json::json!([{"error": "refused", "continued": false}]),
        serde_json::json!([{"error": "refused", "continued": true}]),
        serde_json::json!([{"error": "refused", "continued": true}, {"outputs": ["b1"]}]),
    ] {
        validate_continuations(&serde_json::json!({"calls": calls}), path);
    }
    for calls in [
        serde_json::json!([{"error": "refused", "continued": "true"}, {}]),
        serde_json::json!([{"outputs": ["b1"], "continued": true}, {}]),
        serde_json::json!([{"outputs": ["b1"], "continued": false}]),
        serde_json::json!([{"error": "terminal"}, {"error": "later", "continued": true}]),
        serde_json::json!([{"error": "terminal", "continued": false}, {"error": "later", "continued": true}]),
    ] {
        let e = serde_json::json!({"calls": calls});
        assert!(
            std::panic::catch_unwind(|| validate_continuations(&e, path)).is_err(),
            "accepted {e}"
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
