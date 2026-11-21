//! Amalgam Tag (#18799): labeling a tool design's bodies, and what the tag refuses.
use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{AlignFrame, BodyName, FeatureKind};

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
        FeatureKind::AmalgamTag { remove, add, new, .. } => (remove.clone(), add.clone(), new.clone()),
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
    assert!(refused(&mut s, json!({"new": "Insert", "sea": 1})).contains("`sea`"));
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
    assert!(matches!(&f.kind, FeatureKind::AmalgamTag { remove, add, new, seat: None }
        if *remove == names(&["Pocket"]) && *add == names(&["Boss"]) && *new == names(&["Insert"])));
}

/// The seat is the tool's origin, a frame found on the tool design: a sketch point, or a joint
/// origin picked on a body (here the center of `Boss`'s top, at (45, 5, 12)). A seat that isn't
/// there is refused, naming it.
#[test]
fn a_tag_takes_a_seat() {
    let mut s = tool();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Seat"}));
    run(&mut s, "sketch.point", json!({"point": [10, 10], "id": "o"}));
    run(&mut s, "sketch.finish", json!({}));
    let id = run(&mut s, "solid.amalgam_tag", json!({"remove": "Pocket", "seat": {"sketch": "Seat", "point": "o"}}))["feature"].as_u64().unwrap();
    assert!(
        matches!(&s.doc.feature(id).unwrap().kind, FeatureKind::AmalgamTag { seat: Some(AlignFrame::SketchPoint { point, .. }), .. } if point == "o")
    );
    let on_face = run(&mut s, "solid.amalgam_tag", json!({"add": "Boss", "seat": {"face": [45, 5, 12]}}))["feature"].as_u64().unwrap();
    assert!(matches!(&s.doc.feature(on_face).unwrap().kind, FeatureKind::AmalgamTag { seat: Some(AlignFrame::Origin { .. }), .. }));
    assert!(s.model.result(on_face).unwrap().error.is_none());
    let missing = refused(&mut s, json!({"remove": "Pocket", "seat": {"sketch": "Seat", "point": "nowhere"}}));
    assert!(missing.contains("nowhere"), "{missing}");
}

// ---- Placing a tool (solid.amalgamate) ----

/// A tool design saved at `path`: `Pocket`, a cylinder r6 from z -4 to 0 (cut), and `Boss`, a
/// cylinder r3 from z -4 to 6 standing in it (joined), tagged `Cup`; and `Notch`, a 4 mm cube
/// at x 10..14, y -2..2, z -4..0, tagged `Notch` (cut only, off to one side, so turns show).
fn tool_design(path: &str) {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, -4], "radius": 6, "height": 4, "body_name": "Pocket"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, -4], "radius": 3, "height": 10, "body_name": "Boss", "operation": "new"}));
    run(&mut s, "solid.amalgam_tag", json!({"remove": "Pocket", "add": "Boss", "name": "Cup"}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [10, -2, -4], "body_name": "Notch", "operation": "new"}));
    run(&mut s, "solid.amalgam_tag", json!({"remove": "Notch", "name": "Notch"}));
    run(&mut s, "file.save", json!({"path": path}));
}

/// Sketch `Seeds` on the plane z = 10 with a point at each of `points` (id, x, y).
fn seeds(s: &mut Session, points: &[(&str, f64, f64)]) {
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Seeds"}));
    for (id, x, y) in points {
        run(s, "sketch.point", json!({"point": [x, y], "id": id}));
    }
    run(s, "sketch.finish", json!({}));
}

/// A 100 x 100 x 10 `Plate` and seed points `a` at (25, 25) and `b` at (75, 75) on its top.
fn target() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "body_name": "Plate"}));
    seeds(&mut s, &[("a", 25.0, 25.0), ("b", 75.0, 75.0)]);
    s
}

fn seed(point: &str) -> Value {
    json!({"sketch": "Seeds", "point": point})
}

fn bodies(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

/// At each seed the pocket (r6, 4 deep: 144π) is cut first, then the boss (r3, 10 tall: 90π)
/// joined standing on the pocket's floor: the plate loses 54π a seed and stays one body. Joined
/// first, the boss's foot would be cut away with the pocket and the rest float free.
#[test]
fn amalgamate_cuts_then_joins_at_every_seed() {
    let dir = crate::tests::Folder::new("amalgamate-cup");
    let file = dir.path("tool.sc");
    tool_design(&file);
    let mut s = target();
    let p = json!({"path": file, "tag": "Cup", "seeds": [seed("a"), seed("b")], "remove_from": "Plate", "add_to": "Plate"});
    let id = run(&mut s, "solid.amalgamate", p)["feature"].as_u64().unwrap();
    assert!(s.model.result(id).unwrap().error.is_none());
    assert_eq!(bodies(&s), ["Plate"]);
    let want = 100_000.0 - 2.0 * 54.0 * std::f64::consts::PI;
    assert!((volume(&s, "Plate") - want).abs() < 1e-3, "{} != {want}", volume(&s, "Plate"));
    // The tool is kept in the design: it builds the same without the tool's file.
    drop(dir);
    let doc = solidcraft_io::read_design(&solidcraft_io::write_design(&s.doc).unwrap()).unwrap();
    let opened = Session::opened(doc, None);
    assert!((volume(&opened, "Plate") - want).abs() < 1e-3);
}

/// Quarter turns and the flip apply to every copy alike. The notch sits 10..14 mm along the
/// tool's x: unturned it cuts `East` (x 56..70), a quarter turn puts it along y into `North`
/// (y 56..70), and turned over it is above both (64 mm³ each time it cuts).
#[test]
fn turns_and_flip_turn_every_copy() {
    let dir = crate::tests::Folder::new("amalgamate-turns");
    let file = dir.path("tool.sc");
    tool_design(&file);
    let place = |turns: u32, flip: bool| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 14, "width": 10, "height": 10, "corner": [56, 44, 0], "body_name": "East"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 14, "height": 10, "corner": [44, 56, 0], "body_name": "North", "operation": "new"}));
        seeds(&mut s, &[("c", 50.0, 50.0)]);
        let p = json!({"path": file, "tag": "Notch", "seeds": [seed("c")], "turns": turns, "flip": flip, "remove_from": ["East", "North"]});
        run(&mut s, "solid.amalgamate", p);
        ((volume(&s, "East") - 1400.0).round(), (volume(&s, "North") - 1400.0).round())
    };
    assert_eq!(place(0, false), (-64.0, 0.0));
    assert_eq!(place(1, false), (0.0, -64.0));
    assert_eq!(place(0, true), (0.0, 0.0));
}

/// Each copy's faces are named after its seed: the names at `b` are the same whether or not
/// there is a copy at `a`, and no name is shared between the two copies.
#[test]
fn copies_are_named_after_their_seeds() {
    let dir = crate::tests::Folder::new("amalgamate-names");
    let file = dir.path("tool.sc");
    tool_design(&file);
    let names = |at: Vec<Value>| {
        let mut s = target();
        run(&mut s, "solid.amalgamate", json!({"path": file, "tag": "Cup", "seeds": at, "remove_from": "Plate", "add_to": "Plate"}));
        let st = s.model.state();
        let plate = st.body("Plate").unwrap();
        let mut placed: Vec<String> = solidcraft_doc::naming::face_names(plate).iter().filter(|n| n.contains(":at:")).cloned().collect();
        placed.sort();
        placed
    };
    let both = names(vec![seed("a"), seed("b")]);
    let only_b = names(vec![seed("b")]);
    assert!(!only_b.is_empty());
    assert!(only_b.iter().all(|n| both.contains(n)), "{only_b:?} not all in {both:?}");
    let mut unique = both.clone();
    unique.dedup();
    assert_eq!(unique.len(), both.len(), "{both:?}");
    assert_eq!(both.len(), 2 * only_b.len());
}

/// What Amalgamate can't place is refused, naming it: a role without its targets (never
/// skipped), targets for a role the tool hasn't, a seed found by nothing named, the same seed
/// twice, a tag that isn't there and a design with no tag.
#[test]
fn amalgamate_refuses_what_it_cannot_place() {
    let dir = crate::tests::Folder::new("amalgamate-refusals");
    let file = dir.path("tool.sc");
    tool_design(&file);
    let plain = dir.path("plain.sc");
    let mut p = Session::default();
    run(&mut p, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    run(&mut p, "file.save", json!({"path": plain}));
    let mut s = target();
    let mut refused = |p: Value| {
        let features = s.doc.features.len();
        let e = s.execute("solid.amalgamate", &p).err().unwrap_or_else(|| panic!("{p} was accepted")).to_string();
        assert_eq!(s.doc.features.len(), features, "{p}");
        e
    };
    let skip = refused(json!({"path": file, "tag": "Cup", "seeds": [seed("a")], "add_to": "Plate"}));
    assert!(skip.contains("the tool has Remove bodies: name the bodies they are cut from in `remove_from`"), "{skip}");
    let extra = refused(json!({"path": file, "tag": "Notch", "seeds": [seed("a")], "remove_from": "Plate", "add_to": "Plate"}));
    assert!(extra.contains("the tool has no Add bodies: leave `add_to` out"), "{extra}");
    let bare = refused(json!({"path": file, "seeds": [{"point": [50, 50, 10]}], "remove_from": "Plate"}));
    assert!(bare.contains("not supported yet: a seed at a point or frame"), "{bare}");
    let twice = refused(json!({"path": file, "seeds": [seed("a"), seed("a")], "remove_from": "Plate"}));
    assert!(twice.contains("`seeds[1]` is at the same place as an earlier seed"), "{twice}");
    let missing = refused(json!({"path": file, "tag": "Hinge", "seeds": [seed("a")], "remove_from": "Plate"}));
    assert!(missing.contains("no Amalgam Tag named `Hinge` (it has `Cup`, `Notch`)"), "{missing}");
    assert!(refused(json!({"path": plain, "seeds": [seed("a")], "remove_from": "Plate"})).contains("has no Amalgam Tag"));
    assert!(refused(json!({"path": file, "seeds": [seed("a")], "remove_from": "Plate", "turns": 4})).contains("turns"));
}
