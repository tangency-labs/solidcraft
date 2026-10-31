//! A design opened from the model its file keeps is the design evaluated again: the same
//! results, the same bodies (to the bit), the same face names and volumes after every feature,
//! and the same after an edit made to both.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{ModelBody, ModelState, SavedModel};

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

/// #9456: a design whose model was computed by another evaluation source (saved by another
/// release) evaluates its timeline again, but takes its STEP imports from the file when this
/// build's importer read them: the same model as one that reads the files again, bodies to the
/// bit, after an edit too. A save then keeps those imports without reading them.
#[test]
fn a_design_evaluated_again_takes_its_imports_from_the_file() {
    let mut source = Session::default();
    let hinge = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Hinge Assembly").unwrap();
    for c in (hinge.script)()["commands"].as_array().unwrap() {
        source.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
    }
    let step = std::env::temp_dir().join(format!("solidcraft-model-cache-{}-imports.step", std::process::id()));
    source.execute("file.export", &json!({"path": step.to_string_lossy()})).unwrap();
    let mut built = Session::default();
    built.execute("file.insert_step", &json!({"path": step.to_string_lossy()})).unwrap();
    built.execute("file.insert_step", &json!({"path": step.to_string_lossy(), "matrix": [1, 0, 0, 200, 0, 1, 0, 0, 0, 0, 1, 0]})).unwrap();
    let _ = std::fs::remove_file(&step);
    let design = built.design();
    // The file another release's save leaves to this build: the imports, a model it cannot use.
    let imports = SavedModel::imports_only(built.model.saved_imports(&design));
    assert_eq!(imports.imports.digests.len(), 1, "both features import the same text");
    assert_eq!(imports.imports.read.len(), 1, "read in this session");
    let bytes =
        solidcraft_engine::io::write_design_container_with_cache(&design, solidcraft_engine::io::DesignCompression::Fast, None, Some(&imports))
            .unwrap();
    assert!(solidcraft_engine::io::read_design_geometry(&bytes).is_none());
    let kept = solidcraft_engine::io::DesignContainerReader::new(std::io::Cursor::new(&bytes)).unwrap().geometry().unwrap();
    assert!(kept.imports.stored.as_ref().is_some_and(|s| s.has(&imports.imports.digests[0])), "the file keeps the import");
    let path = temp("imports");
    std::fs::write(&path, &bytes).unwrap();
    let mut cached = open(&path);
    assert!(cached.model.recomputed > 0, "evaluated again");
    assert_eq!(cached.model.imports.reused(), 1, "the import taken from the file");
    let mut rebuilt = Session::new(solidcraft_engine::io::read_design(&bytes).unwrap());
    assert_eq!(rebuilt.model.imports.reused(), 0);
    assert_eq!(model("imports", &cached), model("imports", &rebuilt), "evaluated with the file's import");
    edit(&mut cached);
    edit(&mut rebuilt);
    assert_eq!(model("imports", &cached), model("imports", &rebuilt), "after the same edit");
    // Saved now, it keeps the model and the import; opened again, nothing is evaluated.
    cached.execute("file.save", &json!({})).unwrap();
    let reopened = open(&path);
    assert_eq!(reopened.model.last_recomputed, 0);
    // A model taken from the file keeps the file's import for its next save, unread.
    let saved = reopened.model.saved(&reopened.design()).unwrap();
    assert_eq!(saved.imports.digests.len(), 1);
    assert!(saved.imports.read.is_empty());
    assert!(saved.imports.stored.as_ref().is_some_and(|s| s.has(&saved.imports.digests[0])));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.bak", path.display()));
}

/// `built` saved with `saved`, a model of it that keeps only some states (as a file without room
/// for all of them does, #3220), opens as the design evaluated again: evaluating just the
/// features whose state the file left out, then after suppressing a feature at, before or after
/// each state it kept, after undoing that, and with the marker moved there and back.
fn check_thinned(name: &str, built: &Session, saved: &SavedModel) {
    let design = built.design();
    let bytes =
        solidcraft_engine::io::write_design_container_with_cache(&design, solidcraft_engine::io::DesignCompression::Fast, None, Some(saved)).unwrap();
    assert!(solidcraft_engine::io::read_design_geometry(&bytes).is_some(), "{name}: the file keeps the final state");
    let path = temp(&name.replace(' ', "-"));
    std::fs::write(&path, &bytes).unwrap();
    let mut cached = open(&path);
    let mut rebuilt = Session::new(solidcraft_engine::io::read_design(&bytes).unwrap());
    let features = saved.records["features"].as_array().unwrap();
    let unkept = features.iter().filter(|f| f["skipped"] == false && f.get("state").is_none()).count();
    assert!(unkept > 0, "{name}: some states left out");
    assert_eq!(cached.model.recomputed, unkept, "{name}: evaluated just the features whose state the file left out");
    assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: opened from the file's model");
    let n = cached.doc.features.len();
    let mut at: Vec<usize> = features
        .iter()
        .enumerate()
        .filter(|(_, f)| f["skipped"] == false && f.get("state").is_some())
        .flat_map(|(k, _)| [k.saturating_sub(1), k, k + 1])
        .filter(|i| *i < n)
        .collect();
    at.dedup();
    for i in at {
        let edit = json!({"feature": cached.doc.features[i].id, "suppressed": true});
        let (a, b) = (cached.execute("timeline.suppress", &edit), rebuilt.execute("timeline.suppress", &edit));
        assert_eq!(a.is_ok(), b.is_ok(), "{name}: suppressing feature {i}");
        assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: feature {i} suppressed");
        if a.is_ok() {
            cached.execute("edit.undo", &json!({})).unwrap();
            rebuilt.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: suppressing feature {i} undone");
        }
        for marker in [json!({"position": i}), json!({})] {
            cached.execute("timeline.rollback", &marker).unwrap();
            rebuilt.execute("timeline.rollback", &marker).unwrap();
            assert_eq!(model(name, &cached), model(name, &rebuilt), "{name}: the marker at {marker}, from feature {i}");
        }
    }
    let _ = std::fs::remove_file(&path);
}

/// A model too big for its file keeps the final state, then the slowest features' states as fit
/// (#3220). Both the smallest such model (the final state, and states that add no body to it)
/// and one that keeps more open as the design evaluated again. The placed components' design has
/// a cut whose fingerprint reads where the bodies before it lie, which a state left out does not
/// say until it is evaluated.
#[test]
fn thinned_models_open_as_evaluated() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/components-arm.solidcraft");
    for sample in ["Sample Plate", "Plastic Enclosure", "components-arm"] {
        let built = match solidcraft_engine::sample::samples().into_iter().find(|s| s.name == sample) {
            Some(s) => {
                let mut built = Session::default();
                for c in (s.script)()["commands"].as_array().unwrap() {
                    built.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
                }
                built
            }
            None => open(&corpus),
        };
        let saved = built.model.saved(&built.design()).unwrap();
        // Every body a megabyte (the records fit in the room left over): each budget keeps one more
        // body's worth of states.
        let body = 1 << 20;
        let pieces: Vec<Vec<(usize, usize)>> = (0..saved.bodies.len()).map(|i| vec![(i, body)]).collect();
        let thinned: Vec<SavedModel> =
            (0..=saved.bodies.len()).filter_map(|n| saved.thinned(&pieces, n * body + body - 1).map(|(m, _, _)| m)).collect();
        let kept = |m: &SavedModel| m.records["features"].as_array().unwrap().iter().filter(|f| f.get("state").is_some()).count();
        let smallest = thinned.first().unwrap();
        assert!(smallest.records["states"].as_array().unwrap().len() < saved.records["states"].as_array().unwrap().len(), "{sample}");
        let more = thinned.iter().find(|m| kept(m) > kept(smallest) && kept(m) < built.doc.features.len()).unwrap();
        check_thinned(&format!("{sample} smallest"), &built, smallest);
        check_thinned(&format!("{sample} thinned"), &built, more);
    }
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

/// A design saved before body names were pinned (#6403) pins them as it opens, as its first
/// command would, and opening it is no change. That command pinned them as part of itself, so
/// undoing it went back to the unpinned design, which no evaluated result matched: the first
/// undo after opening from the command line evaluated every feature again (82-93 s in a
/// 5,484-feature design); `doc.open` pinned them after opening, which marked the design changed.
#[test]
fn a_design_saved_before_names_were_pinned_opens_pinned_and_unchanged() {
    let plate = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Sample Plate").unwrap();
    let mut built = Session::default();
    for c in (plate.script)()["commands"].as_array().unwrap() {
        built.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
    }
    let mut doc = built.design().into_owned();
    for f in &mut doc.features {
        f.body_names.clear();
    }
    doc.reserved_body_names.clear();
    let unpinned = Session::new(doc);
    let design = unpinned.design();
    let saved = unpinned.model.saved(&design).unwrap();
    let bytes = solidcraft_engine::io::write_design_container_with_cache(&design, solidcraft_engine::io::DesignCompression::Fast, None, Some(&saved))
        .unwrap();
    let path = temp("unpinned");
    std::fs::write(&path, &bytes).unwrap();
    let file = path.to_string_lossy().to_string();
    // As the command line opens a design (`load`), and as the app does (`doc.open`).
    let (doc, saved) = solidcraft_engine::io::read_design_file(&file, None::<fn(solidcraft_engine::io::DesignPreview)>).unwrap();
    for (how, mut s) in [("opened_at", Session::opened_at(doc, saved.as_ref(), Some(&file))), ("doc.open", open(&path))] {
        assert_eq!(s.model.last_recomputed, 0, "{how}: taken from the file");
        assert!(s.doc.features.iter().any(|f| !f.body_names.is_empty()), "{how}: the names are pinned");
        assert!(!s.is_dirty(), "{how}: opening is no change");
        let read: Vec<Arc<ModelState>> = s.model.results.iter().map(|r| r.state.clone()).collect();
        s.execute("solid.box", &json!({"length": 3, "width": 3, "height": 3, "operation": "new"})).unwrap();
        // As in a large design, whose evaluation cache keeps only a few results: undo finds the
        // results in the model itself or evaluates them again.
        *s.model.cache.lock().unwrap() = Default::default();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.model.last_recomputed, 0, "{how}: the first undo evaluates nothing");
        assert!(s.model.results.iter().zip(&read).all(|(r, st)| Arc::ptr_eq(&r.state, st)), "{how}: the states read from the file");
        assert!(!s.is_dirty(), "{how}: undone to the design as opened");
    }
    let _ = std::fs::remove_file(&path);
}
