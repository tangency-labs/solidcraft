//! Hole callouts follow the model's Hole features (#7605). The parts are made with SolidCraft's
//! own commands and checked against hand-worked values: a 100 x 60 x 40 bearing block with a
//! 50 x 40 x 5 pocket, a Ø35 bore through from the pocket floor and Ø6.6 counterbored holes; a
//! flange with 6 Ø9 holes at sketch points; an M8 tapped blind hole.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

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

/// The callout of the hole `feature` drills (at `point`), with `extra` parameters.
fn callout_with(s: &mut Session, feature: &str, point: Option<&str>, extra: Value) -> Value {
    let mut p = json!({"feature": id_of(s, feature)});
    if let Some(point) = point {
        p["point"] = json!(point);
    }
    if let (Some(m), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    run(s, "drawing.hole_callout", p)
}

fn callout(s: &mut Session, feature: &str) -> Value {
    callout_with(s, feature, None, json!({}))
}

fn lines(v: &Value) -> Vec<String> {
    v["lines"].as_array().map(|a| a.iter().filter_map(|l| l.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn near(v: &Value, expected: f64) -> bool {
    v.as_f64().is_some_and(|x| (x - expected).abs() < 1e-9)
}

fn solid_at(s: &Session, x: f64, y: f64, z: f64) -> bool {
    s.model.state().bodies.iter().any(|b| b.body.contains(Vec3::new(x, y, z)))
}

/// The bearing block: 100 x 60 x 40 with a 50 x 40 x 5 pocket in its top (floor at z = 35).
fn bearing_block(s: &mut Session) {
    run(s, "solid.box", json!({"length": 100, "width": 60, "height": 40, "name": "Block"}));
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 40], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "PocketSketch"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [25, 10], "p1": [75, 50]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 5, "direction": "negative", "operation": "cut", "name": "Pocket"}));
}

#[test]
fn through_all_never_reads_as_blind_and_blind_never_as_through() {
    let mut s = Session::default();
    bearing_block(&mut s);
    // Through all from the pocket floor: 35 mm of material, but no depth.
    run(&mut s, "solid.hole", json!({"position": [50, 30, 35], "diameter": 35, "name": "Bore"}));
    // Blind in the top face, 12 deep.
    run(&mut s, "solid.hole", json!({"position": [90, 30, 40], "diameter": 8, "depth": 12, "name": "Blind"}));
    // A depth past the material (50 in 40): the bore keeps no floor, so it is through.
    run(&mut s, "solid.hole", json!({"position": [10, 50, 40], "diameter": 5, "depth": 50, "name": "PastTheBottom"}));
    let bore = callout(&mut s, "Bore");
    assert_eq!(bore["status"], "resolved", "{bore}");
    assert_eq!(bore["callout"]["extent"], json!({"type": "through"}), "{bore}");
    assert_eq!(lines(&bore), ["Ø35.00 THRU"]);
    let blind = callout(&mut s, "Blind");
    assert_eq!(blind["callout"]["extent"]["type"], "blind", "{blind}");
    assert!(near(&blind["callout"]["extent"]["depth"], 12.0), "{blind}");
    assert_eq!(lines(&blind), ["Ø8.00 ↧ 12.00"]);
    let past = callout(&mut s, "PastTheBottom");
    assert_eq!(past["callout"]["extent"], json!({"type": "through"}), "{past}");
    assert_eq!(lines(&past), ["Ø5.00 THRU"]);
    // A drilled hole's floor is its drill point: blind to the shoulder, 15 deep.
    run(
        &mut s,
        "solid.hole",
        json!({"position": [90, 10, 40], "diameter": 6, "depth": 15, "type": "drilled", "tip_angle": "118 deg", "name": "Drilled"}),
    );
    let drilled = callout(&mut s, "Drilled");
    assert_eq!(drilled["callout"]["form"]["type"], "drilled", "{drilled}");
    assert!(near(&drilled["callout"]["extent"]["depth"], 15.0), "{drilled}");
    assert_eq!(lines(&drilled), ["Ø6.00 ↧ 15.00"]);
    // An 8 mm plate's Ø6.6 through hole reads as through, not as 8 deep.
    let mut plate = Session::default();
    run(&mut plate, "solid.box", json!({"length": 60, "width": 30, "height": 8, "name": "Bracket"}));
    run(&mut plate, "solid.hole", json!({"position": [15, 15, 8], "diameter": 6.6, "name": "H"}));
    assert_eq!(lines(&callout(&mut plate, "H")), ["Ø6.60 THRU"]);
}

#[test]
fn a_hole_on_a_pocket_floor_takes_its_depth_from_the_floor() {
    let mut s = Session::default();
    bearing_block(&mut s);
    run(&mut s, "solid.hole", json!({"position": [50, 30, 35], "diameter": 35, "name": "Bore"}));
    run(&mut s, "solid.hole", json!({"position": [35, 20, 35], "diameter": 10, "depth": 10, "name": "OnFloor"}));
    let pocket = id_of(&s, "Pocket");
    for name in ["Bore", "OnFloor"] {
        let c = callout(&mut s, name);
        assert!(near(&c["callout"]["start"][2], 35.0), "{name} starts on the pocket floor: {c}");
        let face = c["callout"]["start_face"].as_str().unwrap_or_default().to_string();
        assert!(face.starts_with(&format!("F{pocket}:")), "{name} starts on a face of the pocket: {face}");
    }
    // 10 deep from the floor (z 35 → 25), not from the block's top (z 40 → 30).
    let c = callout(&mut s, "OnFloor");
    assert!(near(&c["callout"]["extent"]["depth"], 10.0), "{c}");
    assert_eq!(lines(&c), ["Ø10.00 ↧ 10.00"]);
    assert!(!solid_at(&s, 35.0, 20.0, 25.5) && solid_at(&s, 35.0, 20.0, 24.5), "the model's floor is at z = 25");
    // The Ø35 bore: through from the floor, never `x 35 deep`.
    assert_eq!(lines(&callout(&mut s, "Bore")), ["Ø35.00 THRU"]);
    // Drilled from a sketch on the block's top plane (z = 40) over the pocket: the hole starts on
    // the floor below the point, and its 10 mm go from there (z 35 → 25), not from the sketch.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 40], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Top"}));
    run(&mut s, "sketch.point", json!({"point": [65, 40], "id": "q"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Top", "points": ["q"], "diameter": 10, "depth": 10, "name": "FromTop"}));
    let c = callout_with(&mut s, "FromTop", Some("q"), json!({}));
    assert!(near(&c["callout"]["start"][2], 35.0), "{c}");
    let face = c["callout"]["start_face"].as_str().unwrap_or_default().to_string();
    assert!(face.starts_with(&format!("F{pocket}:")), "{face}");
    assert!(near(&c["callout"]["extent"]["depth"], 10.0), "{c}");
    assert_eq!(lines(&c), ["Ø10.00 ↧ 10.00"]);
    assert!(!solid_at(&s, 65.0, 40.0, 25.5) && solid_at(&s, 65.0, 40.0, 24.5), "the model's floor is at z = 25");
    // To Object: from the top (z = 40) to a point on the pocket floor (z = 35), 5 deep.
    run(&mut s, "solid.hole", json!({"position": [10, 30, 40], "diameter": 6, "to": [35, 20, 35], "name": "ToFloor"}));
    let c = callout(&mut s, "ToFloor");
    assert!(near(&c["callout"]["extent"]["depth"], 5.0), "{c}");
    assert_eq!(lines(&c), ["Ø6.00 ↧ 5.00"]);
}

#[test]
fn counterbore_and_countersink_callouts_keep_their_hole() {
    let mut s = Session::default();
    bearing_block(&mut s);
    run(
        &mut s,
        "solid.hole",
        json!({"position": [10, 30, 40], "diameter": 6.6, "type": "counterbore", "cb_diameter": 11, "cb_depth": 6.8, "name": "CB"}),
    );
    run(
        &mut s,
        "solid.hole",
        json!({"position": [90, 30, 40], "diameter": 6.6, "depth": 15, "type": "countersink", "cs_diameter": 12, "cs_angle": "90 deg", "name": "CS"}),
    );
    let cb = callout(&mut s, "CB");
    assert!(near(&cb["callout"]["diameter"], 6.6), "{cb}");
    assert_eq!(cb["callout"]["extent"], json!({"type": "through"}), "{cb}");
    assert_eq!(cb["callout"]["form"]["type"], "counterbore", "{cb}");
    assert!(near(&cb["callout"]["form"]["diameter"], 11.0) && near(&cb["callout"]["form"]["depth"], 6.8), "{cb}");
    assert_eq!(lines(&cb), ["Ø6.60 THRU", "⌴ Ø11.00 ↧ 6.80"]);
    let cs = callout(&mut s, "CS");
    assert!(near(&cs["callout"]["diameter"], 6.6), "{cs}");
    assert!(near(&cs["callout"]["extent"]["depth"], 15.0), "{cs}");
    assert_eq!(lines(&cs), ["Ø6.60 ↧ 15.00", "⌵ Ø12.00 × 90.00°"]);
    // Words in the drawing's content language, numbers in its unit.
    assert_eq!(lines(&callout_with(&mut s, "CB", None, json!({"language": "de-DE"}))), ["Ø6.60 DURCH", "⌴ Ø11.00 ↧ 6.80"]);
    assert_eq!(lines(&callout_with(&mut s, "CB", None, json!({"units": "in", "decimals": 2}))), ["Ø0.2598 THRU", "⌴ Ø0.4331 ↧ 0.2677"]);
}

/// A flange plate 120 x 120 x 15 with 6 points on a Ø95 circle (sketch `Pts` on its top face).
fn flange_with_points(s: &mut Session) -> Vec<String> {
    run(s, "solid.box", json!({"corner": [-60, -60, 0], "length": 120, "width": 120, "height": 15, "name": "Flange"}));
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 15], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Pts"}));
    let ids: Vec<String> = (0..6).map(|i| format!("p{i}")).collect();
    for (i, id) in ids.iter().enumerate() {
        let a = std::f64::consts::TAU * i as f64 / 6.0;
        run(s, "sketch.point", json!({"point": [47.5 * a.cos(), 47.5 * a.sin()], "id": id}));
    }
    run(s, "sketch.finish", json!({}));
    run(s, "solid.hole", json!({"sketch": "Pts", "points": ids, "diameter": 9, "name": "Holes"}));
    ids
}

#[test]
fn holes_at_sketch_points_count_and_a_deleted_point_is_lost() {
    let mut s = Session::default();
    let ids = flange_with_points(&mut s);
    for (i, id) in ids.iter().enumerate() {
        let c = callout_with(&mut s, "Holes", Some(id), json!({}));
        assert_eq!(c["status"], "resolved", "{c}");
        assert_eq!(c["callout"]["count"], 6, "{c}");
        assert_eq!(lines(&c), ["6× Ø9.00 THRU"]);
        // Each callout is its own hole, at its own point.
        let a = std::f64::consts::TAU * i as f64 / 6.0;
        assert!(near(&c["callout"]["start"][0], 47.5 * a.cos()) && near(&c["callout"]["start"][1], 47.5 * a.sin()), "{id}: {c}");
    }
    // A point the feature doesn't drill is lost, keeping its ids.
    let other = callout_with(&mut s, "Holes", Some("nowhere"), json!({}));
    assert_eq!(other["status"], "lost", "{other}");
    assert_eq!(other["hole"]["point"], "nowhere");
    // Delete p3: its callout is lost and never moves to a neighbour; the others keep their places.
    run(&mut s, "sketch.edit", json!({"sketch": "Pts"}));
    run(&mut s, "sketch.delete", json!({"entities": ["p3"]}));
    run(&mut s, "sketch.finish", json!({}));
    let lost = callout_with(&mut s, "Holes", Some("p3"), json!({}));
    assert_eq!(lost["status"], "lost", "{lost}");
    assert_eq!(lost["hole"], json!({"feature": id_of(&s, "Holes"), "point": "p3"}));
    assert!(lost.get("callout").is_none(), "{lost}");
    // Today the whole Hole feature fails when one of its points is deleted, so the others read
    // failed, naming the point. When the feature drills the points that are left, this becomes
    // `5× Ø9.00 THRU` with p2 at its own point.
    let p2 = callout_with(&mut s, "Holes", Some("p2"), json!({}));
    assert_eq!(p2["status"], "failed", "{p2}");
    assert!(p2["reason"].as_str().is_some_and(|r| r.contains("p3")), "{p2}");
    // Undo brings p3's hole back, at p3.
    run(&mut s, "edit.undo", json!({}));
    let back = callout_with(&mut s, "Holes", Some("p3"), json!({}));
    assert_eq!(back["status"], "resolved", "{back}");
    assert!(near(&back["callout"]["start"][0], -47.5), "{back}");
}

#[test]
fn suppress_undo_and_parameter_edits_change_the_callout() {
    let mut s = Session::default();
    bearing_block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "d", "expression": "8 mm"}));
    run(&mut s, "parameters.add", json!({"name": "deep", "expression": "12 mm"}));
    run(&mut s, "parameters.add", json!({"name": "cbd", "expression": "4 mm"}));
    run(
        &mut s,
        "solid.hole",
        json!({"position": [90, 30, 40], "diameter": "d", "depth": "deep", "type": "counterbore", "cb_diameter": 14, "cb_depth": "cbd", "name": "H"}),
    );
    assert_eq!(lines(&callout(&mut s, "H")), ["Ø8.00 ↧ 12.00", "⌴ Ø14.00 ↧ 4.00"]);
    run(&mut s, "parameters.change", json!({"name": "d", "expression": "10 mm"}));
    assert_eq!(lines(&callout(&mut s, "H")), ["Ø10.00 ↧ 12.00", "⌴ Ø14.00 ↧ 4.00"]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(lines(&callout(&mut s, "H")), ["Ø8.00 ↧ 12.00", "⌴ Ø14.00 ↧ 4.00"]);
    // Deeper than the block: through.
    run(&mut s, "parameters.change", json!({"name": "deep", "expression": "45 mm"}));
    assert_eq!(lines(&callout(&mut s, "H")), ["Ø8.00 THRU", "⌴ Ø14.00 ↧ 4.00"]);
    run(&mut s, "edit.undo", json!({}));
    // Suppressed, and back.
    run(&mut s, "timeline.suppress", json!({"feature": "H", "suppressed": true}));
    let c = callout(&mut s, "H");
    assert_eq!(c["status"], "suppressed", "{c}");
    assert!(c.get("text").is_none(), "{c}");
    run(&mut s, "timeline.suppress", json!({"feature": "H", "suppressed": false}));
    assert_eq!(callout(&mut s, "H")["status"], "resolved");
    // A counterbore deeper than the hole: the feature fails, and so does its callout.
    run(&mut s, "parameters.change", json!({"name": "cbd", "expression": "13 mm"}));
    let c = callout(&mut s, "H");
    assert_eq!(c["status"], "failed", "{c}");
    assert!(c["reason"].as_str().is_some_and(|r| r.contains("counterbore")), "{c}");
    run(&mut s, "edit.undo", json!({}));
    // Deleted: lost, keeping its id; undone: resolved again.
    let id = id_of(&s, "H");
    run(&mut s, "timeline.delete", json!({"feature": "H"}));
    let gone = run(&mut s, "drawing.hole_callout", json!({"feature": id}));
    assert_eq!(gone["status"], "lost", "{gone}");
    assert_eq!(gone["hole"], json!({"feature": id}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(lines(&callout(&mut s, "H")), ["Ø8.00 ↧ 12.00", "⌴ Ø14.00 ↧ 4.00"]);
    // A later pocket, 15 deep around it, cuts the whole hole away (12 deep from z = 40): failed, saying so.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 40], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Over"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [80, 20], "p1": [100, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 15, "direction": "negative", "operation": "cut", "name": "Over"}));
    let c = callout(&mut s, "H");
    assert_eq!(c["status"], "failed", "{c}");
    assert!(c["reason"].as_str().is_some_and(|r| r.contains("later feature")), "{c}");
}

#[test]
fn a_single_hole_at_a_sketch_point_needs_no_point_and_is_lost_with_it() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 30, "height": 10, "name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "S"}));
    run(&mut s, "sketch.point", json!({"point": [15, 15], "id": "c"}));
    run(&mut s, "sketch.point", json!({"point": [45, 15], "id": "d"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "S", "points": ["c"], "single": true, "diameter": 6, "depth": 4, "name": "One"}));
    // No point, or its own point: the same hole, at c.
    for point in [None, Some("c")] {
        let c = callout_with(&mut s, "One", point, json!({}));
        assert_eq!(c["status"], "resolved", "{c}");
        assert!(near(&c["callout"]["start"][0], 15.0) && near(&c["callout"]["start"][1], 15.0), "{c}");
        assert_eq!(c["callout"]["count"], 1, "{c}");
        assert_eq!(lines(&c), ["Ø6.00 ↧ 4.00"]);
    }
    // Another point of the sketch: the hole isn't drilled there.
    assert_eq!(callout_with(&mut s, "One", Some("d"), json!({}))["status"], "lost");
    // A hand-edited Single hole listing a second point drills only at its first; the second
    // point's callout is lost, never moved to the hole at c.
    let one = id_of(&s, "One");
    if let Some(f) = std::sync::Arc::make_mut(&mut s.doc).feature_mut(one)
        && let solidcraft_doc::FeatureKind::Hole { points: Some(sp), .. } = &mut f.kind
    {
        sp.ids.push("d".into());
    }
    s.refresh();
    let d = callout_with(&mut s, "One", Some("d"), json!({}));
    assert_eq!(d["status"], "lost", "{d}");
    assert!(d["reason"].as_str().is_some_and(|r| r.contains("only at point `c`")), "{d}");
    // Deleting c loses the callout, with or without its point.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.delete", json!({"entities": ["c"]}));
    run(&mut s, "sketch.finish", json!({}));
    for point in [None, Some("c")] {
        let c = callout_with(&mut s, "One", point, json!({}));
        assert_eq!(c["status"], "lost", "{c}");
    }
}

#[test]
fn a_tapped_hole_shows_its_thread_not_the_tap_drill() {
    let mut s = Session::default();
    bearing_block(&mut s);
    run(&mut s, "solid.hole", json!({"position": [90, 30, 40], "thread": "M8", "depth": 12, "name": "Tapped"}));
    let c = callout(&mut s, "Tapped");
    // The tap drill: major − pitch = 8 − 1.25.
    assert!(near(&c["callout"]["diameter"], 6.75), "{c}");
    assert_eq!(c["callout"]["thread"], "M8", "{c}");
    assert_eq!(lines(&c), ["M8 ↧ 12.00"]);
    run(&mut s, "solid.hole", json!({"position": [10, 30, 40], "thread": "M6", "name": "TappedThrough"}));
    assert_eq!(lines(&callout(&mut s, "TappedThrough")), ["M6 THRU"]);
}

#[test]
fn hostile_callout_input_is_refused_without_a_panic() {
    let mut s = Session::default();
    let ids = flange_with_points(&mut s);
    let holes = id_of(&s, "Holes");
    run(&mut s, "solid.hole", json!({"position": [0, 0, 15], "diameter": 20, "name": "Centre"}));
    let refused = |s: &mut Session, p: Value| {
        let r = s.execute("drawing.hole_callout", &p);
        assert!(r.is_err(), "{p} gave {r:?}");
        r.err().map(|e| e.to_string()).unwrap_or_default()
    };
    let component = run(&mut s, "component.create", json!({"name": "Bought"}))["component"].as_u64().unwrap_or_default();
    // Ids: unknown, malformed, of another kind of feature or of an item that is no feature; each
    // refused for its own reason.
    for (p, why) in [
        (json!({"feature": 999_999}), "feature"),
        (json!({"feature": u64::MAX}), "feature"),
        (json!({"feature": 0}), "feature"),
        (json!({"feature": -3}), "feature"),
        (json!({"feature": 2.5}), "feature"),
        (json!({"feature": "Holes"}), "feature"),
        (json!({}), "feature"),
        (json!({"feature": component}), "is a component"),
        (json!({"feature": holes, "point": 7}), "point"),
        (json!({"feature": holes, "point": ""}), "point"),
        (json!({"feature": holes, "point": "p\u{0}1"}), "point"),
        (json!({"feature": holes, "point": "p".repeat(10_000)}), "point"),
        (json!({"feature": id_of(&s, "Centre"), "point": "p1"}), "position"),
        (json!({"feature": holes, "point": ids[0], "decimals": 7}), "decimals"),
        (json!({"feature": holes, "point": ids[0], "decimals": 1e300}), "decimals"),
        (json!({"feature": holes, "point": ids[0], "units": "furlong"}), "unit"),
        (json!({"feature": holes, "point": ids[0], "units": "x".repeat(10_000)}), "unit"),
        (json!({"feature": holes, "point": ids[0], "language": "-"}), "language"),
    ] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
        assert!(e.len() < 400, "{p}: the message echoes no long input: {e}");
    }
    // A hole at sketch points needs its point.
    assert!(refused(&mut s, json!({"feature": holes})).contains("point"));
    let flange = id_of(&s, "Flange");
    assert!(refused(&mut s, json!({"feature": flange})).contains("not supported yet"));
    // Values that aren't finite.
    let centre = id_of(&s, "Centre");
    if let Some(f) = std::sync::Arc::make_mut(&mut s.doc).feature_mut(centre)
        && let solidcraft_doc::FeatureKind::Hole { diameter, .. } = &mut f.kind
    {
        *diameter = "1e999 mm".into();
    }
    s.refresh();
    // The model can't evaluate it, so the callout reads failed before its own range checks
    // (which stay as a guard).
    let c = run(&mut s, "drawing.hole_callout", json!({"feature": centre}));
    assert_eq!(c["status"], "failed", "{c}");
    // Too many points.
    if let Some(f) = std::sync::Arc::make_mut(&mut s.doc).feature_mut(holes)
        && let solidcraft_doc::FeatureKind::Hole { points: Some(sp), .. } = &mut f.kind
    {
        sp.ids = (0..10_001).map(|i| format!("p{i}")).collect();
    }
    s.refresh();
    assert!(refused(&mut s, json!({"feature": holes, "point": "p1"})).contains("10000"));
}
