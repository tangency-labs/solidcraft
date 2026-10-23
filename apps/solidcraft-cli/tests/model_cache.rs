//! A design opened from the model its file keeps is the design evaluated again: the same
//! results, the same bodies (to the bit), the same face names and volumes after every feature,
//! and the same after an edit made to both.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{ModelBody, ModelState};

fn corpus() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "solidcraft")).collect();
    v.sort();
    v
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("solidcraft-model-cache-{}-{name}.solidcraft", std::process::id()))
}

fn open(path: &Path) -> Session {
    let mut s = Session::default();
    s.execute("doc.open", &json!({"path": path.to_string_lossy()})).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    s
}

fn body(name: &str, b: &ModelBody) -> Value {
    let measured = solidcraft_engine::kernel::measure(&b.body).ok();
    json!({
        "name": b.name, "feature": b.feature, "component": b.component, "source": b.source_name, "transferred": b.transferred,
        "brep": solidcraft_engine::kernel::encode_native_body(&b.body).map(|bytes| format!("{:x}", sha(&bytes))).unwrap_or_else(|e| format!("{name}: {e}")),
        "faces": *solidcraft_engine::doc::naming::face_names(b),
        "volume": measured.as_ref().map(|m| m.volume.to_bits()), "area": measured.as_ref().map(|m| m.area.to_bits()),
        "look": b.imported.as_ref().map(|l| format!("{l:?}")),
    })
}

fn sha(bytes: &[u8]) -> u128 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    let a = h.finish();
    bytes.len().hash(&mut h);
    (u128::from(a) << 64) | u128::from(h.finish())
}

fn state(name: &str, st: &Arc<ModelState>) -> Value {
    json!({
        "bodies": st.bodies.iter().map(|b| body(name, b)).collect::<Vec<_>>(),
        "sketches": st.sketches.iter().map(|s| json!([s.feature, s.name, s.plane, s.sketch, s.report, s.profiles.len()])).collect::<Vec<_>>(),
        "counter": st.body_counter, "sheets": st.sheets, "threads": st.threads, "construct": format!("{:?}", st.construct),
    })
}

/// Everything a model holds that the rest of the app reads.
fn model(name: &str, s: &Session) -> Value {
    let results: Vec<Value> = s
        .model
        .results
        .iter()
        .map(|r| json!({"id": r.id, "name": r.name, "error": r.error, "warning": r.warning, "skipped": r.skipped, "state": state(name, &r.state)}))
        .collect();
    json!({"results": results, "world": state(name, &s.world_state())})
}

/// The same edit on both: a parameter's value when the design has one, else a new body.
fn edit(s: &mut Session) {
    let param = s.doc.params.first().map(|p| (p.name.clone(), p.expr.clone()));
    match param {
        Some((name, expr)) => {
            s.execute("parameters.change", &json!({"name": name, "expression": format!("({expr}) * 1.05")})).unwrap();
        }
        None => {
            s.execute("solid.box", &json!({"length": 7, "width": 8, "height": 9, "operation": "new"})).unwrap();
        }
    }
}

fn check(name: &str, mut built: Session) {
    let path = temp(name);
    built.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(solidcraft_engine::io::read_design_geometry(&bytes).is_some(), "{name}: the save keeps the model");
    let mut cached = open(&path);
    assert_eq!(cached.model.last_recomputed, 0, "{name}: opened without evaluating");
    let mut rebuilt = Session::new(solidcraft_engine::io::read_design(&bytes).unwrap());
    assert!(rebuilt.model.last_recomputed > 0);
    assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: opened from the file's model");
    edit(&mut cached);
    edit(&mut rebuilt);
    assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: after the same edit");
    cached.execute("edit.undo", &json!({})).unwrap();
    rebuilt.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: after undoing it");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.bak", path.display()));
}

#[test]
fn corpus_designs_open_from_their_model_as_evaluated() {
    for p in corpus() {
        let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let s = open(&p);
        // Names saved before format 14 are found again through names worked out from geometry on
        // demand, which a saved model does not keep.
        let legacy = s.doc.to_json().contains(&format!("\"{}", solidcraft_engine::doc::naming::LEGACY_MARK));
        if s.model.results.iter().any(|r| r.error.is_some()) || legacy {
            // Not kept: a model with errors, or names saved before format 14.
            let path = temp(&name);
            let mut s = s;
            s.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
            assert!(solidcraft_engine::io::read_design_geometry(&std::fs::read(&path).unwrap()).is_none(), "{name}");
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(format!("{}.bak", path.display()));
            continue;
        }
        check(&name, s);
    }
}

#[test]
fn sample_designs_open_from_their_model_as_evaluated() {
    for sample in solidcraft_engine::sample::samples() {
        let mut s = Session::default();
        for c in (sample.script)()["commands"].as_array().unwrap() {
            s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap_or_else(|e| panic!("{}: {e}", sample.name));
        }
        check(sample.name, s);
    }
}

#[test]
fn imported_assemblies_open_from_their_model_as_evaluated() {
    // A STEP assembly of the hinge sample: parts placed several times.
    let mut source = Session::default();
    let hinge = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Hinge Assembly").unwrap();
    for c in (hinge.script)()["commands"].as_array().unwrap() {
        source.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
    }
    let step = std::env::temp_dir().join(format!("solidcraft-model-cache-{}-hinge.step", std::process::id()));
    source.execute("file.export", &json!({"path": step.to_string_lossy()})).unwrap();
    let mut s = Session::default();
    s.execute("file.insert_step", &json!({"path": step.to_string_lossy()})).unwrap();
    s.execute("file.insert_step", &json!({"path": step.to_string_lossy(), "matrix": [1, 0, 0, 200, 0, 1, 0, 0, 0, 0, 1, 0]})).unwrap();
    let _ = std::fs::remove_file(&step);
    check("imported hinge", s);
}

/// A design whose boolean needed a retry keeps that retry's diagnostics in its model, and opens
/// from it (#4443): an empty list of rejected candidates is left out of the file and read back
/// as empty.
#[test]
fn a_design_whose_boolean_needed_a_retry_opens_from_its_model() {
    // A ring joined to a sloped floor through its bore: the join needs a common-bore retry
    // (the geometry of boolean_retry_warning_and_details_survive_cache_undo_and_reopen).
    let slope = 1f64.to_radians().tan();
    let mut s = Session::default();
    for (command, params) in [
        ("sketch.create", json!({"plane": "XZ", "name": "Floor section"})),
        ("sketch.line", json!({"points": [[0, 0], [100, 0], [100, -10. - slope * 100.], [0, -10.]], "closed": true})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"sketch": "Floor section", "distance": 80, "body_name": "Floor"})),
        ("solid.cylinder", json!({"base": [50, 40, -5], "radius": 10, "height": 30, "operation": "cut"})),
        ("sketch.create", json!({"plane": "XY", "name": "Ring section"})),
        ("sketch.circle.center", json!({"center": [50, 40], "radius": 20})),
        ("sketch.circle.center", json!({"center": [50, 40], "radius": 10})),
        ("sketch.finish", json!({})),
        (
            "solid.extrude",
            json!({"sketch": "Ring section", "profiles": [{"point": [65, 40]}], "start_offset": -5, "distance": 7, "body_name": "Ring"}),
        ),
        ("solid.combine", json!({"target": "Floor", "tools": ["Ring"], "operation": "join"})),
    ] {
        s.execute(command, &params).unwrap();
    }
    let retried = s.model.results.last().unwrap();
    assert!(!retried.boolean_diagnostics.events.is_empty(), "the join needs a retry: {:?}", retried.boolean_diagnostics);
    assert!(retried.boolean_diagnostics.rejected.is_empty(), "no rejected candidates, so the file leaves the list out");
    // Read as Open reads it. The in-process evaluation cache would hand an evaluation the same
    // results, so the check is the model itself being taken.
    let path = temp("boolean-retry");
    s.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
    let (doc, saved) = solidcraft_engine::io::read_design_file(&path.to_string_lossy(), None::<fn(solidcraft_engine::io::DesignPreview)>).unwrap();
    let saved = saved.expect("the save keeps the model");
    if let Err(e) = solidcraft_engine::doc::Model::adopt(&doc, &saved) {
        panic!("the saved model is not used: {e}");
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.bak", path.display()));
    check("boolean retry", s);
}

/// Opened from its model, a design evaluates only from the first feature an edit changes: the
/// saved states before it are kept as they were read (the same states, not evaluated again).
#[test]
fn an_edit_after_opening_evaluates_from_the_feature_it_changes() {
    let plate = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Sample Plate").unwrap();
    let mut built = Session::default();
    for c in (plate.script)()["commands"].as_array().unwrap() {
        built.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
    }
    let path = temp("incremental");
    built.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
    let n = built.doc.features.len();
    assert!(n >= 4);
    let mut s = open(&path);
    assert_eq!(s.model.last_recomputed, 0);
    let read: Vec<Arc<ModelState>> = s.model.results.iter().map(|r| r.state.clone()).collect();
    // How many of the first features still have the states read from the file.
    let kept = |s: &Session| s.model.results.iter().zip(&read).take_while(|(r, st)| Arc::ptr_eq(&r.state, st)).count();
    // The marker moved back and forth: the features before it are as read.
    s.execute("timeline.rollback", &json!({"position": n - 2})).unwrap();
    assert_eq!(kept(&s), n - 2, "rolled back");
    s.execute("timeline.rollback", &json!({})).unwrap();
    assert_eq!(kept(&s), n, "rolled forward: the features after the marker take their saved results again");
    // Suppressing the second-to-last feature keeps every state before it.
    let k = s.doc.features[n - 2].id;
    s.execute("timeline.suppress", &json!({"feature": k, "suppressed": true})).unwrap();
    assert_eq!(kept(&s), n - 2, "evaluated from the suppressed feature on");
    s.execute("edit.undo", &json!({})).unwrap();
    // A new feature at the end starts from the final state read from the file.
    let mut s = open(&path);
    let read: Vec<Arc<ModelState>> = s.model.results.iter().map(|r| r.state.clone()).collect();
    s.execute("solid.box", &json!({"length": 3, "width": 3, "height": 3, "operation": "new"})).unwrap();
    assert_eq!(s.model.results.len(), n + 1);
    assert!(s.model.results.iter().zip(&read).all(|(r, st)| Arc::ptr_eq(&r.state, st)), "only the new feature evaluated");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.bak", path.display()));
}

/// An edit made after opening and undone finds the saved results again: undo evaluates nothing.
#[test]
fn undoing_an_edit_after_opening_reuses_the_saved_results() {
    let plate = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Sample Plate").unwrap();
    let mut built = Session::default();
    for c in (plate.script)()["commands"].as_array().unwrap() {
        built.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
    }
    let path = temp("undo");
    built.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
    let mut s = open(&path);
    let read: Vec<Arc<ModelState>> = s.model.results.iter().map(|r| r.state.clone()).collect();
    let first = s.doc.features[1].id;
    s.execute("timeline.suppress", &json!({"feature": first, "suppressed": true})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model.last_recomputed, 0, "taken from the saved results");
    assert!(s.model.results.iter().zip(&read).all(|(r, st)| Arc::ptr_eq(&r.state, st)), "the very states read from the file");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.bak", path.display()));
}
