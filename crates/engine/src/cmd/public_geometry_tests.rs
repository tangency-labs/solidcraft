use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn folder(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-geometry-copy-{case}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A skeleton design saved at `path`: the plane `Deck` at z = `deck`, the plane `Wall` at
/// y = 10, both in the public geometry `Layout`.
fn skeleton(path: &Path, deck: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create_skeleton", json!({}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": deck, "name": "Deck"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XZ", "offset": 10, "name": "Wall"}));
    run(&mut s, "geometry.publish", json!({"name": "Layout", "items": ["Deck", "Wall"]}));
    run(&mut s, "file.save", json!({"path": path}));
    s
}

/// The height (z of the origin) of the construction plane `name`.
fn height(s: &Session, name: &str) -> f64 {
    match s.model.state().construct.iter().find(|c| c.name == name).map(|c| c.geom.clone()) {
        Some(solidcraft_doc::construct::ConstructGeom::Plane(p)) => p.origin.z,
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn a_part_copies_a_skeleton_files_public_geometry_and_updates_on_demand() {
    let dir = folder("update");
    let source = dir.join("skeleton.sc");
    let mut sk = skeleton(&source, 40.0);

    let mut part = Session::default();
    let made = run(&mut part, "geometry.import", json!({"path": source, "publication": "layout"}));
    assert_eq!(made["name"], "Layout");
    assert_eq!(made["items"].as_array().unwrap().len(), 2, "{made}");
    assert_eq!(height(&part, "Deck"), 40.0);
    // The copied plane is an ordinary construction plane: other features build on it.
    run(&mut part, "construct.plane.offset", json!({"base": "Deck", "offset": 5, "name": "Shelf"}));
    assert_eq!(height(&part, "Shelf"), 45.0);
    assert_eq!(run(&mut part, "geometry.copies", json!({}))["copies"][0]["state"], "up_to_date");

    // The skeleton moves its plane, drops `Wall` and shares a new one: nothing changes here
    // until the copy is updated.
    run(&mut sk, "timeline.delete", json!({"feature": "Deck"}));
    run(&mut sk, "construct.plane.offset", json!({"base": "XY", "offset": 60, "name": "Deck"}));
    run(&mut sk, "construct.plane.offset", json!({"base": "YZ", "offset": 7, "name": "Rib"}));
    run(&mut sk, "geometry.unpublish", json!({"name": "Layout"}));
    run(&mut sk, "geometry.publish", json!({"name": "Layout", "items": ["Deck", "Rib"]}));
    run(&mut sk, "file.save", json!({"path": source}));
    assert_eq!(height(&part, "Deck"), 40.0);
    assert_eq!(run(&mut part, "geometry.copies", json!({}))["copies"][0]["state"], "out_of_date");

    let updated = run(&mut part, "geometry.update", json!({}));
    let copy = &updated["copies"][0];
    assert_eq!(copy["state"], "updated", "{updated}");
    assert_eq!(copy["changed"], json!(["Deck"]));
    assert_eq!(copy["added"], json!(["Rib"]));
    assert_eq!(copy["lost"], json!(["Wall"]), "a lost element keeps what it had");
    assert_eq!(height(&part, "Deck"), 60.0);
    assert_eq!(height(&part, "Shelf"), 65.0, "what was built on the copy follows it");
    assert!(part.doc.features.iter().any(|f| f.name == "Wall"));
    assert_eq!(run(&mut part, "geometry.update", json!({}))["copies"][0]["state"], "unchanged");

    // A file that is gone leaves the copy as it is, and says so.
    std::fs::remove_file(&source).unwrap();
    let gone = run(&mut part, "geometry.update", json!({"copies": ["Layout"]}));
    assert_eq!(gone["copies"][0]["state"], "missing", "{gone}");
    assert_eq!(height(&part, "Deck"), 60.0);
    assert_eq!(run(&mut part, "geometry.copies", json!({}))["copies"][0]["state"], "missing");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A skeleton design saved at `path` sharing the sketch `Outline`, a rectangle `width` × 10 with
/// its width dimensioned, in the public geometry `Shape`.
fn outline(path: &Path, width: f64) {
    let mut s = Session::default();
    run(&mut s, "component.create_skeleton", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [width, 10]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": width}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "geometry.publish", json!({"name": "Shape", "items": ["Outline"]}));
    run(&mut s, "file.save", json!({"path": path}));
}

#[test]
fn a_copied_sketch_is_extruded_here_and_follows_an_update() {
    let dir = folder("sketch");
    let source = dir.join("skeleton.sc");
    outline(&source, 20.0);
    let volume = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN);

    let mut part = Session::default();
    let made = run(&mut part, "geometry.import", json!({"path": source, "publication": "Shape"}));
    assert_eq!(made["items"][0]["type"], "Sketch", "{made}");
    assert!(made.get("skipped").is_none(), "{made}");
    // The copy holds the curves where they were and none of the source design's dimensions.
    let copied = part.doc.sketch(made["items"][0]["feature"].as_u64().unwrap()).unwrap();
    assert!(copied.constraints.is_empty() && copied.curves.len() == 4 && copied.curves.iter().all(|c| c.fixed));
    assert!(part.doc.params.is_empty());
    run(&mut part, "solid.extrude", json!({"distance": 5}));
    assert!((volume(&mut part) - 1000.0).abs() < 1e-6, "{}", volume(&mut part));

    outline(&source, 30.0);
    let updated = run(&mut part, "geometry.update", json!({}));
    assert_eq!(updated["copies"][0]["changed"], json!(["Outline"]), "{updated}");
    assert!((volume(&mut part) - 1500.0).abs() < 1e-6, "the extrude follows the copy: {}", volume(&mut part));
    assert_eq!(run(&mut part, "geometry.update", json!({}))["copies"][0]["state"], "unchanged");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_geometry_copy_refuses_what_it_cannot_take_and_follows_its_features() {
    let dir = folder("refusals");
    let source = dir.join("skeleton.sc");
    skeleton(&source, 40.0);

    let mut part = Session::default();
    let refused = |s: &mut Session, p: Value| s.execute("geometry.import", &p).unwrap_err().to_string();
    assert!(refused(&mut part, json!({"path": source, "publication": "Nothing"})).contains("no public geometry"));
    assert!(refused(&mut part, json!({"path": dir.join("absent.sc"), "publication": "Layout"})).contains("not found"));
    assert!(refused(&mut part, json!({"publication": "Layout"})).contains("path"));
    assert!(part.doc.geometry_copies.is_empty() && part.doc.features.is_empty(), "a refused copy leaves nothing behind");
    // What it shares is listed without copying it.
    let shared = run(&mut part, "geometry.publications", json!({"path": source}));
    assert_eq!(shared["publications"][0]["name"], "Layout", "{shared}");
    assert_eq!(shared["publications"][0]["items"].as_array().unwrap().len(), 2);

    run(&mut part, "geometry.import", json!({"path": source, "publication": "Layout", "name": "Frame"}));
    assert!(refused(&mut part, json!({"path": source, "publication": "Layout", "name": "frame"})).contains("already exists"));
    assert!(part.execute("geometry.update", &json!({"copies": ["Nobody"]})).is_err());

    // The copy is saved and read back, and it lets go of a deleted feature; its last takes it away.
    let reopened = solidcraft_io::read_design(&solidcraft_io::write_design(&part.doc)).unwrap();
    assert_eq!(reopened.geometry_copies, part.doc.geometry_copies);
    run(&mut part, "timeline.delete", json!({"feature": "Deck"}));
    assert_eq!(part.doc.geometry_copies[0].items.len(), 1);
    run(&mut part, "timeline.delete", json!({"feature": "Wall"}));
    assert!(part.doc.geometry_copies.is_empty());

    // A name taken here is no obstacle: the element comes in under its name and the copy's.
    let mut busy = Session::default();
    run(&mut busy, "construct.plane.offset", json!({"base": "XY", "offset": 1, "name": "Deck"}));
    let made = run(&mut busy, "geometry.import", json!({"path": source, "publication": "Layout"}));
    assert_eq!(made["items"][0]["feature"], busy.doc.features.iter().find(|f| f.name == "Deck (Layout)").unwrap().id, "{made}");
    assert_eq!(height(&busy, "Deck (Layout)"), 40.0);

    // A design can't copy from itself.
    let mut own = Session::default();
    run(&mut own, "file.save", json!({"path": dir.join("own.sc")}));
    let e = own.execute("geometry.import", &json!({"path": dir.join("own.sc"), "publication": "Layout"})).unwrap_err().to_string();
    assert!(e.contains("from itself"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
