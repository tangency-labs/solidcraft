//! Amalgam Tag (#18799): labelling a tool design's bodies, and what the tag refuses.
use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{BodyName, FeatureKind};

fn run(s: &mut Session, command: &str, p: Value) -> Value {
    s.execute(command, &p).unwrap_or_else(|e| panic!("{command} {p}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    let features = s.doc.features.len();
    let e = match s.execute("solid.amalgam_tag", &p) {
        Ok(v) => panic!("{p} was accepted: {v}"),
        Err(e) => e.to_string(),
    };
    assert_eq!(s.doc.features.len(), features, "a refused tag leaves the timeline as it was ({p})");
    e
}

fn volume(s: &Session, body: &str) -> f64 {
    let b = s.model.state().body(body).map(|b| b.body.clone()).unwrap_or_else(|| panic!("no body {body}"));
    solidcraft_kernel::measure(&b).unwrap().volume
}

fn names(list: &[&str]) -> Vec<BodyName> {
    list.iter().map(|n| BodyName::from(*n)).collect()
}

/// A tool design: `Pocket` (20 x 20 x 6) to cut, `Boss` (10 x 10 x 12) to join, `Insert`
/// (8 x 8 x 8) to come in on its own, apart from each other.
fn tool() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 6, "body_name": "Pocket"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 12, "corner": [40, 0, 0], "body_name": "Boss", "operation": "new"}));
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [80, 0, 0], "body_name": "Insert", "operation": "new"}));
    s
}

fn tag_of(s: &Session, id: u64) -> (Vec<BodyName>, Vec<BodyName>, Vec<BodyName>) {
    match &s.doc.feature(id).unwrap().kind {
        FeatureKind::AmalgamTag { remove, add, new } => (remove.clone(), add.clone(), new.clone()),
        k => panic!("not a tag: {k:?}"),
    }
}

/// The tag records each body under its role, takes the feature's name as its own, and leaves
/// every body as it was: the same bodies, the same volumes (2,400, 1,200 and 512 mm³).
#[test]
fn a_tag_labels_bodies_and_changes_no_geometry() {
    let mut s = tool();
    let r = run(&mut s, "solid.amalgam_tag", json!({"remove": "Pocket", "add": ["Boss"], "new": ["Insert"], "name": "Hinge cup"}));
    let id = r["feature"].as_u64().unwrap();
    assert_eq!(tag_of(&s, id), (names(&["Pocket"]), names(&["Boss"]), names(&["Insert"])));
    assert_eq!(s.doc.feature(id).unwrap().name, "Hinge cup");
    assert!(s.model.result(id).unwrap().error.is_none());
    assert_eq!(s.model.state().bodies.len(), 3);
    for (body, v) in [("Pocket", 2400.0), ("Boss", 1200.0), ("Insert", 512.0)] {
        assert!((volume(&s, body) - v).abs() < 1e-6, "{body}: {} != {v}", volume(&s, body));
    }
    // A tag may use any of the roles alone, and a design may hold several tags.
    run(&mut s, "solid.amalgam_tag", json!({"new": "Insert", "name": "Insert only"}));
    assert_eq!(s.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::AmalgamTag { .. })).count(), 2);
}

/// What a tag can't use is refused, naming it, and the timeline is left as it was.
#[test]
fn a_tag_refuses_what_it_cannot_use() {
    let mut s = tool();
    assert!(refused(&mut s, json!({})).contains("tag at least one body"));
    assert!(refused(&mut s, json!({"remove": []})).contains("tag at least one body"));
    assert!(refused(&mut s, json!({"remove": "Ghost"})).contains("`Ghost`"));
    let both = refused(&mut s, json!({"remove": "Pocket", "add": "Pocket"}));
    assert!(both.contains("`Pocket` is tagged both Remove and Add"), "{both}");
    let twice = refused(&mut s, json!({"new": ["Insert", "Insert"]}));
    assert!(twice.contains("`Insert` is tagged New twice"), "{twice}");
    assert!(refused(&mut s, json!({"new": "Insert", "seat": "Origin"})).contains("unknown parameter `seat`"));
    // A surface body (an open tube) can't be cut or joined, but it can come in on its own.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Square"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [120, 0], "p1": [130, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Square", "distance": "10 mm", "name": "Tube", "body_name": "Tube"}));
    let open = refused(&mut s, json!({"remove": "Tube"}));
    assert!(open.contains("`Tube` does not enclose a volume: Remove bodies are cut"), "{open}");
    assert!(refused(&mut s, json!({"add": "Tube"})).contains("Add bodies are joined"));
    run(&mut s, "solid.amalgam_tag", json!({"new": "Tube"}));
}

/// A tagged body that is renamed keeps its role under its new name.
#[test]
fn a_tag_follows_a_renamed_body() {
    let mut s = tool();
    let id = run(&mut s, "solid.amalgam_tag", json!({"remove": "Pocket", "add": "Boss"}))["feature"].as_u64().unwrap();
    run(&mut s, "body.rename", json!({"body": "Pocket", "name": "Cup bore"}));
    assert_eq!(tag_of(&s, id).0, names(&["Cup bore"]));
    assert!(s.model.result(id).unwrap().error.is_none());
}

/// A tag is saved with the design and comes back as it was.
#[test]
fn a_tag_is_saved_and_read_back() {
    let mut s = tool();
    let id = run(&mut s, "solid.amalgam_tag", json!({"remove": "Pocket", "add": "Boss", "new": "Insert", "name": "Hinge cup"}))["feature"]
        .as_u64()
        .unwrap();
    let bytes = solidcraft_io::write_design(&s.doc).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("\"type\":\"amalgam_tag\"") || text.contains("\"type\": \"amalgam_tag\""), "{text}");
    let back = solidcraft_io::read_design(&bytes).unwrap();
    let f = back.feature(id).unwrap();
    assert_eq!(f.name, "Hinge cup");
    assert!(matches!(&f.kind, FeatureKind::AmalgamTag { remove, add, new }
        if *remove == names(&["Pocket"]) && *add == names(&["Boss"]) && *new == names(&["Insert"])));
}
