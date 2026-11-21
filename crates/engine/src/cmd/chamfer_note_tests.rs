//! Chamfer notes follow the model's Chamfer features (#13028). The parts are made with SolidCraft's
//! own commands and checked against hand-worked values: a 40 x 30 x 10 block whose top front edge
//! (y = 0, z = 10) is chamfered, equal, by an angle or by two distances, and as a part placed twice.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_kernel::PointClass;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn id_of(s: &Session, feature: &str) -> u64 {
    s.doc.find_feature(feature).map(|f| f.id).unwrap_or_else(|| panic!("no feature {feature}"))
}

/// The first edge the Chamfer feature `feature` cuts, by its persistent name.
fn edge_of(s: &Session, feature: &str) -> String {
    s.doc.find_feature(feature).and_then(|f| f.edge_names.first().cloned()).unwrap_or_else(|| panic!("{feature} names no edge"))
}

/// The note of `feature`'s first edge, with `extra` parameters.
fn note_with(s: &mut Session, feature: &str, extra: Value) -> Value {
    let mut p = json!({"feature": id_of(s, feature), "edge": edge_of(s, feature)});
    if let (Some(m), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    run(s, "drawing.chamfer_note", p)
}

/// Its text at 0 decimals, as the issue writes its examples.
fn text(s: &mut Session, feature: &str) -> String {
    let v = note_with(s, feature, json!({"decimals": 0}));
    v["text"].as_str().map(String::from).unwrap_or_else(|| panic!("no text: {v}"))
}

fn solid_at(s: &Session, x: f64, y: f64, z: f64) -> bool {
    s.model.state().bodies.iter().any(|b| matches!(b.body.classify_point(Vec3::new(x, y, z)), Ok(PointClass::Inside)))
}

/// The block, its top front edge chamfered with `chamfer`'s parameters.
fn block(chamfer: Value) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Block"}));
    let mut p = json!({"edges": [[20, 0, 10]], "name": "C"});
    if let (Some(m), Some(e)) = (p.as_object_mut(), chamfer.as_object()) {
        m.extend(e.clone());
    }
    run(&mut s, "solid.chamfer", p);
    s
}

#[test]
fn a_note_reads_the_chamfers_size() {
    let mut s = block(json!({"distance": 2}));
    let v = note_with(&mut s, "C", json!({}));
    assert_eq!(v["status"], "resolved", "{v}");
    assert_eq!(v["note"], json!({"count": 1, "size": {"type": "equal", "distance": 2.0}}), "{v}");
    assert_eq!(v["text"], "2.00 × 45.00°", "the shared default of 2 decimals: {v}");
    assert_eq!(text(&mut s, "C"), "2 × 45°");
    assert_eq!(text(&mut block(json!({"distance": 2, "angle": "30 deg"})), "C"), "2 × 30°");
    // Two distances: the setback on the face facing up most first. By hand: unflipped, the top
    // keeps y > 2 and the front z < 7, so (y 2.5, z 9.9) is still solid (the cut is z = 7 + 1.5 y);
    // flipped, the top keeps y > 3 and the front z < 8, so that point is cut (z = 8 + 2/3 y).
    let mut plain = block(json!({"distance": 2, "distance2": 3}));
    assert!(solid_at(&plain, 20.0, 2.5, 9.9));
    assert_eq!(text(&mut plain, "C"), "2 × 3");
    let mut flipped = block(json!({"distance": 2, "distance2": 3, "flip": true}));
    assert!(!solid_at(&flipped, 20.0, 2.5, 9.9));
    assert_eq!(text(&mut flipped, "C"), "3 × 2");
    let v = note_with(&mut flipped, "C", json!({}));
    assert_eq!(v["note"]["size"], json!({"type": "distances", "up": 3.0, "other": 2.0}), "{v}");
}

#[test]
fn a_note_follows_parameter_edits_suppression_and_deletion() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "d", "expression": "2 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 10]], "distance": "d", "name": "C"}));
    assert_eq!(text(&mut s, "C"), "2 × 45°");
    run(&mut s, "parameters.change", json!({"name": "d", "expression": "3 mm"}));
    assert_eq!(text(&mut s, "C"), "3 × 45°");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&mut s, "C"), "2 × 45°");
    run(&mut s, "timeline.suppress", json!({"feature": "C", "suppressed": true}));
    let v = note_with(&mut s, "C", json!({}));
    assert_eq!(v["status"], "suppressed", "{v}");
    assert!(v.get("text").is_none(), "{v}");
    run(&mut s, "timeline.suppress", json!({"feature": "C", "suppressed": false}));
    // Deleted: lost, keeping its ids; undone: resolved again.
    let (id, edge) = (id_of(&s, "C"), edge_of(&s, "C"));
    run(&mut s, "timeline.delete", json!({"feature": "C"}));
    let gone = run(&mut s, "drawing.chamfer_note", json!({"feature": id, "edge": edge}));
    assert_eq!(gone["status"], "lost", "{gone}");
    assert_eq!(gone["chamfer"], json!({"feature": id, "edge": edge}));
    assert!(gone.get("note").is_none(), "{gone}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&mut s, "C"), "2 × 45°");
}

#[test]
fn a_later_cut_that_takes_the_chamfer_away_reads_failed() {
    let mut s = block(json!({"distance": 2}));
    // A cut through the whole front strip, 5 mm deep into the block from y = -1.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Over"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1, -1], "p1": [41, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "direction": "negative", "operation": "cut", "name": "Strip"}));
    assert!(!solid_at(&s, 20.0, 4.0, 5.0), "the strip is gone");
    let v = note_with(&mut s, "C", json!({}));
    assert_eq!(v["status"], "failed", "{v}");
    assert_eq!(v["reason"], "a later feature removed the chamfer", "{v}");
}

#[test]
fn a_later_cut_that_splits_the_chamfer_face_reads_it_still() {
    let mut s = block(json!({"distance": 2}));
    // A slot 2 mm wide across the chamfer's middle (x = 19…21), 3 mm deep from the top and past the
    // chamfer's back (y = 2): the chamfer face is left in two pieces. By hand, (20, 1, 8.5) is under
    // the chamfer (its cut is z = 8 + y), so it goes with the slot, and (10, 1, 8.5) stays.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Across"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [19, -1], "p1": [21, 3]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 3, "direction": "negative", "operation": "cut", "name": "Slot"}));
    assert!(!solid_at(&s, 20.0, 1.0, 8.5) && solid_at(&s, 10.0, 1.0, 8.5), "the slot cuts the chamfer only there");
    let stem = format!("F{}:blend", id_of(&s, "C"));
    let pieces: usize =
        s.model.state().bodies.iter().map(|b| solidcraft_doc::naming::face_names(b).iter().filter(|n| n.starts_with(&stem)).count()).sum();
    assert_eq!(pieces, 2, "the chamfer face is in two pieces");
    let v = note_with(&mut s, "C", json!({}));
    assert_eq!(v["status"], "resolved", "{v}");
    assert_eq!(v["text"], "2.00 × 45.00°", "{v}");
}

#[test]
fn a_note_counts_the_edges_when_asked_and_reads_along_a_tangent_chain() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    let top = json!([[20, 0, 10], [20, 30, 10], [0, 15, 10], [40, 15, 10]]);
    run(&mut s, "solid.chamfer", json!({"edges": top, "distance": 2, "tangent_chain": false, "name": "C"}));
    assert_eq!(text(&mut s, "C"), "2 × 45°");
    let v = note_with(&mut s, "C", json!({"decimals": 0, "count": true}));
    assert_eq!((&v["note"]["count"], &v["text"]), (&json!(4), &json!("4 × (2 × 45°)")), "{v}");
    // Each of the four edges is its own reference to the same chamfer.
    let (id, edges) = (id_of(&s, "C"), s.doc.find_feature("C").map(|f| f.edge_names.clone()).unwrap_or_default());
    assert_eq!(edges.len(), 4);
    for edge in edges {
        assert_eq!(run(&mut s, "drawing.chamfer_note", json!({"feature": id, "edge": edge}))["status"], "resolved");
    }
    // A vertical edge rounded first: the top edge's chain runs on round it, one picked edge.
    let mut c = Session::default();
    run(&mut c, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    run(&mut c, "solid.fillet", json!({"edges": [[40, 30, 5]], "radius": 5}));
    run(&mut c, "solid.chamfer", json!({"edges": [[20, 30, 10]], "distance": 1, "name": "C"}));
    let v = note_with(&mut c, "C", json!({"decimals": 0, "count": true}));
    assert_eq!((&v["status"], &v["note"]["count"], &v["text"]), (&json!("resolved"), &json!(1), &json!("1 × 45°")), "{v}");
    assert!(!solid_at(&c, 40.0 - 0.3, 15.0, 10.0 - 0.3), "the chain ran on along the edge at x = 40");
}

/// A part placed twice is one definition: its Chamfer feature cuts one edge, so the note reads
/// once, in the part's own terms, with a count of 1 that `count` doesn't double. Both placements
/// show the chamfer in the world, so the second one is not a chamfer the note misses.
#[test]
fn a_chamfer_in_a_part_placed_twice_reads_once() {
    let mut a = Session::default();
    // `component.create` makes the new part the active one, so the block and its chamfer are its.
    let made = run(&mut a, "component.create", json!({"name": "Bracket"}));
    let (part, first) = (made["component"].as_u64().unwrap_or_default(), made["occurrence"].as_u64().unwrap_or_default());
    run(&mut a, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Block"}));
    run(&mut a, "solid.chamfer", json!({"edges": [[20, 0, 10]], "distance": 2, "name": "C"}));
    assert_eq!(a.doc.find_feature("C").map(|f| f.component), Some(part), "C is in Bracket");
    // Placed a second time 100 along y, and the first placement moved 50 up, so neither
    // placement's frame is the part's own.
    run(&mut a, "occurrence.copy", json!({"component": part, "translate": [0, 100, 0]}));
    run(&mut a, "occurrence.ground", json!({"occurrence": first, "grounded": false}));
    run(&mut a, "occurrence.move", json!({"occurrence": first, "translate": [0, 0, 50]}));
    // By hand, in the part's frame: (20, 0.5, 9.5) is in the 2 mm chamfer (0.5 + 0.5 < 2), so cut;
    // (20, 5, 5) is solid. In the world: the first placement at z + 50, the second at y + 100.
    let world = a.world_state().unwrap_or_else(|e| panic!("world: {e}"));
    let inside = |x: f64, y: f64, z: f64| world.bodies.iter().any(|b| matches!(b.body.classify_point(Vec3::new(x, y, z)), Ok(PointClass::Inside)));
    for (dy, dz) in [(0.0, 50.0), (100.0, 0.0)] {
        assert!(inside(20.0, 5.0 + dy, 5.0 + dz), "the block is placed at +({dy}, {dz})");
        assert!(!inside(20.0, 0.5 + dy, 9.5 + dz), "the chamfer is cut at +({dy}, {dz})");
    }
    let v = note_with(&mut a, "C", json!({"decimals": 0, "count": true}));
    assert_eq!(v["status"], "resolved", "{v}");
    assert_eq!(v["note"], json!({"count": 1, "size": {"type": "equal", "distance": 2.0}}), "one edge, not one per placement: {v}");
    assert_eq!(v["text"], "2 × 45°", "{v}");
}

#[test]
fn a_note_takes_the_drawings_unit_and_inches_show_three_decimals() {
    let mut s = block(json!({"distance": 2.54}));
    assert_eq!(note_with(&mut s, "C", json!({"units": "in"}))["text"], "0.100 × 45.00°");
    run(&mut s, "drawing.create", json!({"units": "in"}));
    assert_eq!(note_with(&mut s, "C", json!({}))["text"], "0.100 × 45.00°", "the open drawing's unit");
    run(&mut s, "drawing.close", json!({}));
    assert_eq!(note_with(&mut s, "C", json!({}))["text"], "2.54 × 45.00°", "closed: the design's unit");
}

#[test]
fn hostile_note_input_is_refused_without_a_panic() {
    let mut s = block(json!({"distance": 2}));
    let (c, edge) = (id_of(&s, "C"), edge_of(&s, "C"));
    let block = id_of(&s, "Block");
    let component = run(&mut s, "component.create", json!({"name": "Bought"}))["component"].as_u64().unwrap_or_default();
    let refused = |s: &mut Session, p: Value| {
        let r = s.execute("drawing.chamfer_note", &p);
        assert!(r.is_err(), "{p} gave {r:?}");
        r.err().map(|e| e.to_string()).unwrap_or_default()
    };
    for (p, why) in [
        (json!({"feature": 999_999, "edge": edge}), "feature"),
        (json!({"feature": u64::MAX, "edge": edge}), "feature"),
        (json!({"feature": 0, "edge": edge}), "feature"),
        (json!({"feature": -3, "edge": edge}), "feature"),
        (json!({"feature": 2.5, "edge": edge}), "feature"),
        (json!({"feature": "C", "edge": edge}), "feature"),
        (json!({"edge": edge}), "feature"),
        (json!({"feature": c}), "edge"),
        (json!({"feature": component, "edge": edge}), "is a component"),
        (json!({"feature": block, "edge": edge}), "not supported yet"),
        (json!({"feature": c, "edge": "F1:+x|F1:+y"}), "doesn't cut that edge"),
        (json!({"feature": c, "edge": ""}), "edge name"),
        (json!({"feature": c, "edge": "a\u{0}b"}), "edge name"),
        (json!({"feature": c, "edge": "e".repeat(10_000)}), "edge name"),
        (json!({"feature": c, "edge": 7}), "edge"),
        (json!({"feature": c, "edge": edge, "decimals": 7}), "decimals"),
        (json!({"feature": c, "edge": edge, "units": "furlong"}), "unit"),
        (json!({"feature": c, "edge": edge, "language": "-"}), "language"),
        (json!({"feature": c, "edge": edge, "count": "yes"}), "count"),
        (json!({"feature": c, "edge": edge, "decimal": 3}), "decimal"),
    ] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
        assert!(e.len() < 400, "{p}: the message echoes no long input: {e}");
    }
    // A value that isn't finite: the model can't evaluate it, so the note reads failed.
    if let Some(f) = std::sync::Arc::make_mut(&mut s.doc).feature_mut(c)
        && let solidcraft_doc::FeatureKind::Chamfer { distance, .. } = &mut f.kind
    {
        *distance = "1e999 mm".into();
    }
    s.refresh();
    assert_eq!(run(&mut s, "drawing.chamfer_note", json!({"feature": c, "edge": edge}))["status"], "failed");
    // A Chamfer saved before its edges had names is not guessed by position.
    if let Some(f) = std::sync::Arc::make_mut(&mut s.doc).feature_mut(c) {
        f.edge_names.clear();
    }
    assert!(refused(&mut s, json!({"feature": c, "edge": edge})).contains("not supported yet"));
    // Hostile values in each place: an error or an answer, never a panic.
    for value in crate::tests::hostile_values() {
        for p in [json!({"feature": value, "edge": edge}), json!({"feature": c, "edge": value}), json!({"feature": c, "edge": edge, "units": value})]
        {
            if let Err(crate::EngineError::Internal(_, msg)) = s.execute("drawing.chamfer_note", &p) {
                panic!("{p}: {msg}");
            }
        }
    }
}
