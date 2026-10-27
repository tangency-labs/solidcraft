//! FreeCAD and Rhino files (read through cadmpeg) go the same way as STEP files: a new rigid
//! component named after the file (#1426), with the `up` and `names` options (#1312). A FreeCAD
//! file's feature history is rebuilt as features in that component unless `history: solids`.

use serde_json::{Value, json};

use super::run;
use crate::*;

const BLOCK_FCSTD: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/block.FCStd");
const BLOCK_3DM: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/block.3dm");
const BRACKET_FCSTD: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/bracket.FCStd");

/// `bytes` saved as `name` in a fresh temp directory.
fn file(tag: &str, name: &str, bytes: &[u8]) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-native-cad-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    (dir, path.to_string_lossy().to_string())
}

fn extent(s: &Session) -> ([f64; 3], [f64; 3]) {
    let b = crate::view::bounds(s);
    ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z])
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6)
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn body_names(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

#[test]
fn freecad_and_rhino_files_come_in_as_a_component_named_after_the_file() {
    for (tag, name, bytes) in [("fc", "block.FCStd", BLOCK_FCSTD), ("rh", "block.3dm", BLOCK_3DM)] {
        let (dir, path) = file(tag, name, bytes);
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0]}));
        let r = run(&mut s, "file.insert_step", json!({"path": path, "history": "solids"}));
        assert_eq!(r["component_name"], "block", "{name}: {r}");
        let feature = r["feature"].as_u64().unwrap();
        assert_eq!(s.doc.feature(feature).unwrap().kind.type_name(), "BaseFeature", "{name}");
        assert_eq!(s.doc.feature(feature).unwrap().component, r["component"].as_u64().unwrap(), "{name}");
        assert!((volume(&mut s) - (6000.0 + 125.0)).abs() < 1e-6, "{name}");
        // Rhino keeps the object's name; cadmpeg leaves FreeCAD's label out, so it is the file's.
        assert!(body_names(&s).iter().any(|n| n == "Block" || n == "block"), "{name}: {:?}", body_names(&s));
        assert_eq!(s.active_component, 0, "{name}: the active component is unchanged");

        // As written the block spans 10 × 20 × 30; up: y stands it up, (x, y, z) → (x, -z, y).
        let mut d = Session::default();
        let r = run(&mut d, "doc.open", json!({"path": path}));
        assert_eq!((r["features"].clone(), r["component_name"].clone()), (json!(1), json!("block")), "{name}: {r}");
        assert!(near(extent(&d).1, [10.0, 20.0, 30.0]), "{name}: {:?}", extent(&d));
        let mut d = Session::default();
        let r = run(&mut d, "file.insert_step", json!({"path": path, "up": "y"}));
        assert_eq!(r["stand_up"], r["occurrence"], "{name}: {r}");
        let (lo, hi) = extent(&d);
        assert!(near(lo, [0.0, -30.0, 0.0]) && near(hi, [10.0, 0.0, 20.0]), "{name}: stood up: {lo:?} {hi:?}");

        // Every names choice is taken, and the body is named something.
        for names in ["solid", "product", "instance"] {
            let mut d = Session::default();
            run(&mut d, "file.insert_step", json!({"path": path, "names": names}));
            let got = body_names(&d);
            assert!(got.len() == 1 && !got[0].is_empty(), "{name} names={names}: {got:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A broken file is refused with a message naming it, and the design is left as it was.
#[test]
fn a_broken_freecad_or_rhino_file_is_refused_cleanly() {
    let mut cut = BLOCK_FCSTD[..BLOCK_FCSTD.len() / 2].to_vec();
    cut.extend_from_slice(b"garbage");
    for (tag, name, bytes) in
        [("bad-fc", "broken.FCStd", cut.as_slice()), ("bad-rh", "broken.3dm", &BLOCK_3DM[..200]), ("acis", "cube.sat", b"700 0 1 0\n")]
    {
        let (dir, path) = file(tag, name, bytes);
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
        let before = s.doc.clone();
        let e: Result<Value> = s.execute("file.insert_step", &json!({"path": path}));
        let e = e.expect_err(name).to_string();
        assert!(!e.contains("internal"), "{name}: a clean error, not a caught panic: {e}");
        assert_eq!(s.doc, before, "{name}: the design is unchanged");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A FreeCAD file's history comes in as features in a component named after the file, and stays
/// parametric: a sketch dimension drives the part. A second copy's body is numbered and its
/// features act on it. With `history: solids` the file's solids come in as one Import feature,
/// without the shapes FreeCAD keeps hidden.
#[test]
fn a_freecad_history_comes_in_as_editable_features() {
    let (dir, path) = file("hist", "bracket.FCStd", BRACKET_FCSTD);
    let expected = 7840.0 - 50.0 * std::f64::consts::PI;
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!((r["history"].as_str(), r["features"].as_u64(), r["component_name"].as_str()), (Some("features"), Some(5), Some("bracket")), "{r}");
    let component = r["component"].as_u64().unwrap();
    assert!(s.doc.features.iter().all(|f| f.component == component), "every feature is in the file's component");
    assert_eq!(body_names(&s), ["Bracket"]);
    let v = volume(&mut s);
    assert!((v - expected).abs() < 0.05, "volume {v}");
    assert!(s.model.results.iter().all(|r| r.error.is_none()));

    // The rectangle's 40 mm dimension is a parameter: 50 mm adds 10 × 20 × 10 mm³.
    let param = s
        .doc
        .features
        .iter()
        .find_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Sketch { sketch, .. } if f.name == "Profile" => {
                sketch.constraints.iter().find(|c| c.kind.value().is_some_and(|v| (v - 40.0).abs() < 1e-9)).and_then(|c| c.param.clone())
            }
            _ => None,
        })
        .expect("the 40 mm dimension");
    s.doc_mut().set_param(&param, "50 mm", None, None).unwrap();
    s.refresh();
    let wider = volume(&mut s);
    assert!((wider - (v + 2000.0)).abs() < 0.05, "volume {wider}");

    let mut d = Session::default();
    run(&mut d, "file.insert_step", json!({"path": path}));
    run(&mut d, "file.insert_step", json!({"path": path}));
    assert_eq!(body_names(&d), ["Bracket", "Bracket (2)"]);
    assert!((volume(&mut d) - 2.0 * expected).abs() < 0.1, "each copy's pocket and fillet act on its own body");

    let mut d = Session::default();
    let r = run(&mut d, "file.insert_step", json!({"path": path, "history": "solids"}));
    assert_eq!(d.doc.feature(r["feature"].as_u64().unwrap()).unwrap().kind.type_name(), "BaseFeature");
    assert_eq!(body_names(&d).len(), 1, "the pad's, pocket's and tools' hidden shapes are not imported");
    assert!((volume(&mut d) - expected).abs() < 0.05);
    let e = d.execute("file.insert_step", &json!({"path": path, "history": "sketches"})).expect_err("an unknown history choice");
    assert!(e.to_string().contains("history"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
