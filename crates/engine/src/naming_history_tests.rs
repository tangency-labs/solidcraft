//! Persistent names follow the kernel's exact face history: a reference to a face or edge finds
//! the face it was made on after upstream edits that split, merge, mirror, shrink or move what it
//! names, or says it could not. It never quietly attaches to another face, which is what finding
//! names again by sampling geometry could do.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn id(s: &Session, name: &str) -> u64 {
    s.doc.features.iter().rev().find(|f| f.name == name).map(|f| f.id).unwrap_or_else(|| panic!("feature {name}"))
}

fn warning(s: &Session, name: &str) -> Option<String> {
    s.model.result(id(s, name)).and_then(|r| r.warning.clone())
}

fn error(s: &Session, name: &str) -> Option<String> {
    s.model.result(id(s, name)).and_then(|r| r.error.clone())
}

/// Whether any body holds `p`.
fn solid_at(s: &Session, p: [f64; 3]) -> bool {
    s.model.state().bodies.iter().any(|b| b.body.contains(Vec3::new(p[0], p[1], p[2])))
}

/// Which body holds `p`.
fn body_at(s: &Session, p: [f64; 3]) -> Option<String> {
    s.model.state().bodies.iter().find(|b| b.body.contains(Vec3::new(p[0], p[1], p[2]))).map(|b| b.name.clone())
}

fn names(s: &Session, body: &str) -> Vec<(String, bool)> {
    let state = s.model.state();
    let b = state.body(body).unwrap_or_else(|| panic!("body {body}"));
    let names = solidcraft_doc::naming::face_names(b);
    names.iter().enumerate().map(|(i, n)| (n.clone(), solidcraft_doc::naming::face_name_exact(b, i))).collect()
}

/// A 40 x 30 x 10 block ("Block": sketch lines l1 bottom, l2 right, l3 top, l4 left).
fn block(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10, "name": "Block"}));
}

/// A slot across the block from x = `slot_x` to `slot_x + slot_w`, from z = `floor` up through
/// the top: it splits the top (and, cut to the floor, the block) in two.
fn slot(s: &mut Session, floor: f64) {
    run(s, "parameters.add", json!({"name": "slot_x", "expression": "15 mm"}));
    run(s, "parameters.add", json!({"name": "slot_w", "expression": "10 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "SlotSketch"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [15, -5], "p1": [25, 35]}));
    run(s, "sketch.dimension", json!({"entities": ["origin", "p1"], "type": "horizontal", "value": "slot_x"}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": "slot_w"}));
    run(s, "sketch.finish", json!({}));
    let mut p = json!({"sketch": "SlotSketch", "distance": 12, "operation": "cut", "name": "Slot"});
    if floor > 0.0 {
        p["start_offset"] = json!(floor);
    }
    run(s, "solid.extrude", p);
}

fn change(s: &mut Session, name: &str, expression: &str) {
    run(s, "parameters.change", json!({"name": name, "expression": expression}));
}

#[test]
fn faces_built_from_a_profile_are_named_exactly() {
    let mut s = Session::default();
    block(&mut s);
    slot(&mut s, 5.0);
    let block = id(&s, "Block");
    let cut = id(&s, "Slot");
    let n = names(&s, "Body1");
    assert!(n.iter().all(|(_, exact)| *exact), "{n:?}");
    let has = |want: &str| n.iter().any(|(x, _)| x == want);
    // The top is in two pieces, each told apart by the walls only it touches.
    assert!(has(&format!("F{block}:top#[F{block}:side:l4,F{cut}:side:l4]")), "{n:?}");
    assert!(has(&format!("F{block}:top#[F{block}:side:l2,F{cut}:side:l2]")), "{n:?}");
    // The slot's floor is its start cap (above the sketch plane), its walls its sketch curves.
    for want in [format!("F{cut}:top"), format!("F{cut}:side:l2"), format!("F{cut}:side:l4"), format!("F{block}:start")] {
        assert!(has(&want), "{want} in {n:?}");
    }
}

#[test]
fn an_upstream_cut_that_splits_a_face_keeps_the_edge_that_bounded_it() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 10]], "radius": 2, "name": "Round"}));
    let block = id(&s, "Block");
    let round = s.doc.find_feature("Round").cloned().unwrap();
    assert_eq!(round.edge_names, vec![format!("F{block}:side:l2|F{block}:top")]);
    // Upstream of the fillet, a slot splits the top it bounds.
    run(&mut s, "timeline.roll_to", json!({"feature": "Block"}));
    slot(&mut s, 5.0);
    run(&mut s, "timeline.roll_to", json!({}));
    assert_eq!(error(&s, "Round"), None);
    // Only one edge runs between the right wall and a piece of the top: the same edge.
    assert_eq!(warning(&s, "Round"), None);
    let strip = (4.0 - std::f64::consts::PI) * 30.0;
    let v = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - (12000.0 - 10.0 * 30.0 * 5.0 - strip)).abs() < 0.05, "{v}");
    assert!(!solid_at(&s, [39.9, 15.0, 9.9]), "the right top edge is rounded");
    assert!(solid_at(&s, [0.1, 15.0, 9.9]), "the left one is not");
}

#[test]
fn a_blend_follows_its_named_edge_to_a_moved_lid_instead_of_the_body_below() {
    for (command, size, removed) in [("solid.fillet", "radius", (4.0 - std::f64::consts::PI) * 5.0), ("solid.chamfer", "distance", 2.0 * 5.0)] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "height", "expression": "10 mm"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": "height", "corner": [0, 0, 0]}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "LidSketch"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "start_offset": "height", "name": "Lid"}));
        run(&mut s, command, json!({"edges": [[40, 30, 12.5]], size: 2, "name": "Blend"}));
        let blend = s.doc.find_feature("Blend").unwrap();
        assert!(blend.edge_names.iter().all(|n| !n.is_empty()));
        // Exercise the saved reference, including files with no explicit target body.
        let mut s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        s.refresh();

        // The old pick lies on the lower body's edge after this edit. The saved name
        // still identifies the lid's edge, now at z = 20..25.
        change(&mut s, "height", "20 mm");
        assert_eq!(error(&s, "Blend"), None, "{command}");
        assert_eq!(warning(&s, "Blend"), None, "{command}");
        let state = s.model.state();
        let base = state.body("Body1").unwrap();
        let lid = state.body("Body2").unwrap();
        let base_volume = solidcraft_kernel::measure(&base.body).unwrap().volume;
        let lid_volume = solidcraft_kernel::measure(&lid.body).unwrap().volume;
        assert!((base_volume - 24000.0).abs() < 0.01, "{command}: the base stays sharp: {base_volume}");
        assert!((lid_volume - (6000.0 - removed)).abs() < 0.01, "{command}: the lid keeps its blend: {lid_volume}");
        assert!(base.body.contains(Vec3::new(39.9, 29.9, 12.5)));
        assert!(!lid.body.contains(Vec3::new(39.9, 29.9, 22.5)));
    }
}

#[test]
fn a_piece_keeps_its_identity_when_the_cut_that_made_it_moves_or_shrinks() {
    let mut s = Session::default();
    block(&mut s);
    slot(&mut s, 5.0);
    let block = id(&s, "Block");
    let cut = id(&s, "Slot");
    // Raise the piece right of the slot, picked near the slot.
    run(&mut s, "solid.offset_face", json!({"faces": [[27, 15, 10]], "distance": 2, "name": "Raise"}));
    let raise = s.doc.find_feature("Raise").cloned().unwrap();
    assert_eq!(raise.face_names, vec![format!("F{block}:top#[F{block}:side:l2,F{cut}:side:l2]")]);
    assert!(solid_at(&s, [27.0, 15.0, 11.0]) && solid_at(&s, [39.0, 15.0, 11.0]) && !solid_at(&s, [10.0, 15.0, 11.0]));
    // The slot moves right past the picked point, which now lies on the other piece: the name,
    // not the point, says which piece was raised.
    change(&mut s, "slot_x", "28 mm");
    assert_eq!(error(&s, "Raise"), None);
    assert_eq!(warning(&s, "Raise"), None);
    assert!(solid_at(&s, [39.0, 15.0, 11.0]), "the right piece is still the raised one");
    assert!(!solid_at(&s, [27.0, 15.0, 11.0]) && !solid_at(&s, [10.0, 15.0, 11.0]), "the left piece is not raised");
    // Shrunk to a sliver, the slot still splits the top: the same piece, exactly.
    change(&mut s, "slot_w", "0.05 mm");
    assert_eq!(warning(&s, "Raise"), None);
    assert!(solid_at(&s, [39.0, 15.0, 11.0]) && !solid_at(&s, [27.0, 15.0, 11.0]));
    // Gone altogether (suppressed), the piece is the whole top again: said, not guessed.
    run(&mut s, "timeline.suppress", json!({"feature": "Slot", "suppressed": true}));
    let w = warning(&s, "Raise").unwrap_or_default();
    assert!(w.contains("split") || w.contains("no longer exists"), "{w}");
}

#[test]
fn a_face_merged_into_another_answers_to_its_name_and_says_so() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "boss_h", "expression": "5 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "B"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 0], "p1": [60, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "B", "distance": "boss_h", "operation": "join", "name": "Boss"}));
    let boss = id(&s, "Boss");
    run(&mut s, "solid.offset_face", json!({"faces": [[50, 15, 5]], "distance": 1, "name": "Raise"}));
    let raise = s.doc.find_feature("Raise").cloned().unwrap();
    assert_eq!(raise.face_names, vec![format!("F{boss}:top")]);
    assert!(solid_at(&s, [50.0, 15.0, 5.5]) && !solid_at(&s, [50.0, 15.0, 6.5]));
    // As tall as the block, the boss's top and the block's are one face: the reference finds
    // it and says it was merged.
    change(&mut s, "boss_h", "10 mm");
    assert_eq!(error(&s, "Raise"), None);
    let w = warning(&s, "Raise").unwrap_or_default();
    assert!(w.contains("merged"), "{w}");
    assert!(solid_at(&s, [50.0, 15.0, 10.5]) && solid_at(&s, [20.0, 15.0, 10.5]), "the merged top is raised as one");
}

#[test]
fn mirrored_twins_keep_their_own_edges() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "hole_x", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "H"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 15], "radius": 3, "id": "c"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "c.center"], "type": "horizontal", "value": "hole_x"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "c.center"], "type": "vertical", "value": "15 mm"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "H", "distance": 10, "operation": "cut", "name": "Hole"}));
    run(&mut s, "solid.mirror", json!({"features": ["Hole"], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}, "name": "Twin"}));
    let twin = id(&s, "Twin");
    // Chamfer the twin's rim (at x = 30) only.
    run(&mut s, "solid.chamfer", json!({"edges": [[33, 15, 10]], "distance": 1, "name": "Bevel"}));
    let bevel = s.doc.find_feature("Bevel").cloned().unwrap();
    assert!(bevel.edge_names.iter().all(|n| n.contains(&format!("@F{twin}.1"))), "{:?}", bevel.edge_names);
    let rim = |c: f64| [c + 3.5, 15.0, 9.8];
    assert!(!solid_at(&s, rim(30.0)) && solid_at(&s, rim(10.0)));
    // The holes move toward each other: the chamfer's picked point is now off both rims, and
    // the twin is the hole nearer it than its source is. Its name says which.
    change(&mut s, "hole_x", "14 mm");
    assert_eq!(error(&s, "Bevel"), None);
    assert_eq!(warning(&s, "Bevel"), None);
    assert!(!solid_at(&s, rim(26.0)), "the twin's rim is chamfered");
    assert!(solid_at(&s, rim(14.0)), "its source's is not");
}

#[test]
fn pieces_of_a_cut_body_keep_their_body_names_when_the_cut_moves() {
    let mut s = Session::default();
    block(&mut s);
    slot(&mut s, 0.0);
    let right = body_at(&s, [39.0, 15.0, 5.0]).unwrap();
    let left = body_at(&s, [1.0, 15.0, 5.0]).unwrap();
    assert_ne!(right, left);
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 10]], "radius": 2, "name": "Round"}));
    for x in ["5 mm", "22 mm", "15 mm", "2 mm"] {
        change(&mut s, "slot_x", x);
        assert_eq!(body_at(&s, [39.0, 15.0, 5.0]), Some(right.clone()), "slot at {x}");
        assert_eq!(body_at(&s, [1.0, 15.0, 5.0]), Some(left.clone()), "slot at {x}");
        assert_eq!(error(&s, "Round"), None, "slot at {x}");
        assert_eq!(warning(&s, "Round"), None, "slot at {x}");
        assert!(!solid_at(&s, [39.9, 15.0, 9.9]), "slot at {x}");
        // Faces of the pieces, of one face cut in two, are told apart across the bodies.
        let all: Vec<String> = [&right, &left].iter().flat_map(|b| names(&s, b)).map(|(n, _)| n).collect();
        let unique: std::collections::BTreeSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "{all:?}");
    }
}

#[test]
fn a_name_of_a_body_without_history_is_found_by_geometry_and_says_so_once_it_moves() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "len"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "name": "Block"}));
    // Lifting it 2 mm with Align: an operation that reports no history (Replace Face, used here
    // before, keeps it now). The top ends at z = 12 as it did.
    run(&mut s, "solid.align", json!({"bodies": ["Body1"], "from": [0, 0, 0], "to": [0, 0, 2], "name": "Lift"}));
    let state = s.model.state();
    let body = state.bodies.first().unwrap();
    assert!((0..solidcraft_doc::naming::face_names(body).len()).all(|i| !solidcraft_doc::naming::face_name_exact(body, i)));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 12]], "radius": 2, "name": "Round"}));
    // Where it was picked, the pick and the name agree: nothing to say.
    assert_eq!(warning(&s, "Round"), None);
    // Moved away from the pick, only geometry found it: the fillet says so.
    change(&mut s, "len", "50 mm");
    assert_eq!(error(&s, "Round"), None);
    let w = warning(&s, "Round").unwrap_or_default();
    assert!(w.contains("found by geometry, not exact history"), "{w}");
    assert!(!solid_at(&s, [49.9, 15.0, 11.9]));
}

/// Replace Face keeps the body's history, as Offset Face does: every face of the result is named
/// exactly, and a sketch takes an attachment on the replaced face (before, no face could: "this
/// face can't keep an attachment") and follows it when the target moves.
#[test]
fn a_sketch_on_a_replaced_face_follows_it() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "top", "expression": "15 mm"}));
    block(&mut s);
    run(&mut s, "solid.replace_face", json!({"faces": [[20, 15, 10]], "target": {"origin": [0, 0, "top"], "normal": [0, 0, 1]}, "name": "Lift"}));
    assert!(names(&s, "Body1").iter().all(|(_, exact)| *exact), "{:?}", names(&s, "Body1"));
    // A side the replacement only stretched takes an attachment too.
    run(&mut s, "sketch.create", json!({"plane": {"face": [0, 15, 5]}, "name": "Side"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 15]}, "name": "OnTop"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "OnTop", "distance": 5, "operation": "join", "name": "Boss"}));
    // The boss stands on the replaced top wherever the circle's centre lands on it.
    let (lo, hi) = {
        let state = s.model.state();
        let b = state.body("Body1").unwrap().mesh().bounds();
        (b.min.z, b.max.z)
    };
    assert!((lo - 0.0).abs() < 1e-6 && (hi - 20.0).abs() < 1e-6, "{lo} {hi}");
    change(&mut s, "top", "18 mm");
    assert_eq!(error(&s, "Boss"), None);
    let hi = s.model.state().body("Body1").unwrap().mesh().bounds().max.z;
    assert!((hi - 23.0).abs() < 1e-6, "the boss follows the top to z = 18: {hi}");
}

#[test]
fn a_sketch_on_a_certified_face_survives_a_fillet_and_a_cut_upstream_of_it() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "h", "name": "Block"}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 5]], "radius": 3, "name": "Corner"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "H"}));
    run(&mut s, "sketch.circle.center", json!({"center": [8, 8], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "H", "distance": 20, "operation": "cut", "name": "Hole"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [25, 18, 10]}, "name": "Top"}));
    run(&mut s, "sketch.circle.center", json!({"center": [25, 18], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Top", "distance": 4, "operation": "join", "name": "Peg"}));
    let top = |s: &Session| s.model.state().bodies.iter().map(|b| b.mesh().bounds().max.z).fold(f64::MIN, f64::max);
    assert!((top(&s) - 14.0).abs() < 1e-6);
    let state = s.model.state();
    let body = state.bodies.first().unwrap();
    assert!((0..solidcraft_doc::naming::face_names(body).len()).all(|i| solidcraft_doc::naming::face_name_exact(body, i)));
    change(&mut s, "h", "16 mm");
    for f in ["Corner", "Hole", "Top", "Peg"] {
        assert_eq!(error(&s, f), None, "{f}");
        assert_eq!(warning(&s, f), None, "{f}");
    }
    assert!((top(&s) - 20.0).abs() < 1e-6, "{}", top(&s));
}

/// A fillet of a shell's inner corners names those edges. Thinning the wall past the outer
/// round removes them. The fillet must say the names are gone, not blend the seam nearest
/// the old pick, which now sits on the axis of the inner round (#775).
#[test]
fn a_shell_fillet_does_not_follow_a_gone_edge_onto_a_smooth_seam() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-27.5, -40], "p1": [27.5, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 25, "name": "Extrude1"}));
    run(
        &mut s,
        "solid.fillet",
        json!({"radius": 5, "name": "Fillet1", "edges": [[-27.5, -40, 12.5], [27.5, -40, 12.5], [27.5, 40, 12.5], [-27.5, 40, 12.5]]}),
    );
    run(&mut s, "solid.shell", json!({"faces": [[0, 0, 25]], "thickness": 5, "direction": "inside", "name": "Shell1"}));
    run(
        &mut s,
        "solid.fillet",
        json!({"radius": 5, "name": "Fillet2", "edges": [[-22.5, 35, 15], [22.5, 35, 15], [22.5, -35, 15], [-22.5, -35, 15]]}),
    );
    assert_eq!(error(&s, "Fillet2"), None);
    let edited = s.execute("timeline.edit", &json!({"feature": "Shell1", "set": {"thickness": "2"}}));
    let message = error(&s, "Fillet2").or_else(|| edited.err().map(|e| e.to_string())).unwrap_or_default();
    assert!(message.contains("no longer exists"), "{message}");
    assert!(!message.contains("smooth edge"), "{message}");
}

/// The faces `model.faces` lists for `body`: name, whether it is exact, and centroid.
fn listed(s: &mut Session, body: &str) -> Vec<(String, bool, [f64; 3])> {
    let v = run(s, "model.faces", json!({"body": body}));
    let faces = v["faces"].as_array().unwrap_or_else(|| panic!("faces of {body}: {v}"));
    faces
        .iter()
        .map(|f| {
            let c = |k: usize| f["centroid"][k].as_f64().unwrap_or(f64::NAN);
            (f["name"].as_str().unwrap_or_default().to_string(), f["exact"].as_bool() == Some(true), [c(0), c(1), c(2)])
        })
        .collect()
}

/// The face of `faces` whose centroid is within 0.05 of `at`.
fn named_at(faces: &[(String, bool, [f64; 3])], at: [f64; 3]) -> String {
    let near = |c: &[f64; 3]| (0..3).all(|k| (c[k] - at[k]).abs() < 0.05);
    let found: Vec<&String> = faces.iter().filter(|(_, _, c)| near(c)).map(|(n, _, _)| n).collect();
    match found.as_slice() {
        [n] => (*n).clone(),
        _ => panic!("faces at {at:?}: {found:?} in {faces:?}"),
    }
}

/// #1209: an arm with a boss at each end (each boss the radius of the arm's round end under it)
/// and a square cut through one. The second boss and the cut are prism booleans sewn slab by
/// slab, where the arm's round end and the boss over it are two pieces of one cylinder: every
/// face keeps exact history, so the cut's walls are its own four faces and the boss's outer
/// cylinder keeps its name, through the fillets on the bore's corners too.
#[test]
fn a_cut_through_one_of_two_bosses_names_only_its_own_walls() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let (lo0, lo1) = ([16.54527384497186, -18.318724018148217], [159.66891048662592, -12.425397803491872]);
    let (hi0, hi1) = ([159.66891048662592, 12.425397803491872], [16.54527384497186, 18.318724018148217]);
    let (j0, j1) = ([162.79697152270225, -13.15736982664994], [162.79697152270225, 13.15736982664994]);
    let (k0, k1) = ([11.818299035314524, 19.73139143375053], [11.818299035314524, -19.73139143375053]);
    run(&mut s, "sketch.line", json!({"points": [lo0, lo1]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [159.91576013178314, -18.420317757309917], "start": j0, "end": lo1}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [170.0, 0], "start": j0, "end": j1}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [159.91576013178314, 18.420317757309917], "start": hi0, "end": j1}));
    run(&mut s, "sketch.line", json!({"points": [hi0, hi1]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [16.95668992023388, 28.310257274511628], "start": k0, "end": hi1}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": k0, "end": k1}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [16.95668992023388, -28.310257274511628], "start": lo0, "end": k1}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 16, "body_name": "Arm"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 46.0, "height": 22, "base": [0, 0, 0], "operation": "join", "targets": ["Arm"]}));
    let boss = named_at(&listed(&mut s, "Arm"), [19.98, -4.24, 19.0]);
    assert!(boss.starts_with("F3:side"), "{boss}");
    run(&mut s, "solid.cylinder", json!({"diameter": 30.0, "height": 20, "base": [170.0, 0, 0], "operation": "join", "targets": ["Arm"]}));
    let joined = listed(&mut s, "Arm");
    assert!(joined.iter().all(|(_, exact, _)| *exact), "{joined:?}");
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [6.35, 6.35]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"through_all": true, "operation": "cut", "targets": ["Arm"]}));
    let cut = listed(&mut s, "Arm");
    assert!(cut.iter().all(|(_, exact, _)| *exact), "{cut:?}");
    let walls: Vec<&String> = cut.iter().map(|(n, _, _)| n).filter(|n| n.starts_with("F6:")).collect();
    assert_eq!(walls.len(), 4, "{cut:?}");
    for (at, line) in [([0.0, -6.35, 11.0], "l1"), ([6.35, 0.0, 11.0], "l2"), ([0.0, 6.35, 11.0], "l3"), ([-6.35, 0.0, 11.0], "l4")] {
        assert_eq!(named_at(&cut, at), format!("F6:side:{line}"));
    }
    assert_eq!(named_at(&cut, [19.98, -4.24, 19.0]), boss);
    run(&mut s, "solid.fillet", json!({"edges": [[6.35, 6.35, 10], [-6.35, 6.35, 10], [-6.35, -6.35, 10], [6.35, -6.35, 10]], "radius": 1.5}));
    let filleted = listed(&mut s, "Arm");
    let blends = filleted.iter().filter(|(n, _, _)| n.starts_with("F7:blend:")).count();
    assert_eq!(blends, 4, "{filleted:?}");
    assert_eq!(named_at(&filleted, [19.98, -4.24, 19.0]), boss);
}

/// #3902: a torus with a closed, twisted square sweep cut inside it splits the sweep's walls into
/// pieces the kernel returns in a different order on each evaluation. The pieces are numbered by
/// where they lie, so every fresh evaluation gives each piece the same name.
#[test]
fn split_pieces_keep_their_numbers_across_evaluations() {
    let script: Value = serde_json::from_str(include_str!("../../../examples/regressions/closed_sweep_torus_cut.json")).unwrap();
    let mut s = Session::default();
    s.run_script(&script).unwrap();
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).expect("a body");
    let named = |s: &mut Session| -> Vec<(String, [i64; 3])> {
        let mut faces: Vec<(String, [i64; 3])> = listed(s, &body).into_iter().map(|(n, _, c)| (n, c.map(|x| x.round() as i64))).collect();
        faces.sort();
        faces
    };
    let first = named(&mut s);
    assert!(first.iter().any(|(n, _)| n.contains('#')), "no split pieces: {first:?}");
    for run in 0..10 {
        let mut fresh = Session::new(solidcraft_doc::Document::clone(&s.doc));
        fresh.refresh();
        assert_eq!(named(&mut fresh), first, "evaluation {run}");
    }
}

/// #3902: a design saved before the pieces of a face were numbered by place opens with the
/// numbers it was saved with, so a reference to `…#1` stays on the piece it was made on. The
/// block cut by the second block here has its side split in two, numbered the other way round
/// by place.
#[test]
fn a_design_saved_before_pieces_were_numbered_by_place_keeps_their_numbers() {
    let script: Value = serde_json::from_str(include_str!("../../../tests/action_matrix/designs/multi-body-join-cut.json")).unwrap();
    let mut s = Session::default();
    s.run_script(&script).unwrap();
    let by_place = listed(&mut s, "Block");
    // As a build before the change saved it: its features carry no mark, and the file does not
    // list the change among those it has.
    let mut file: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    for f in file["features"].as_array_mut().unwrap() {
        f.as_object_mut().unwrap().remove("kernel_piece_order");
    }
    if let Some(Value::Array(pending)) = file.get_mut("format_pending") {
        pending.retain(|c| c != "face-pieces-by-place");
    }
    // 0.18.3 folded the change into format 29, so a build before it wrote 28.
    file["format"] = Value::String(solidcraft_doc::format::format_string(28));
    let old = solidcraft_doc::Document::from_json(&file.to_string()).unwrap();
    assert!(old.features.iter().all(|f| f.kernel_piece_order), "the features of an older design keep the old numbering");
    let mut reopened = Session::new(old);
    reopened.refresh();
    let kept = listed(&mut reopened, "Block");
    let at = |faces: &[(String, bool, [f64; 3])], name: &str| faces.iter().find(|(n, _, _)| n == name).map(|(_, _, c)| *c);
    // The pieces are numbered differently by place, and the older design keeps its own numbers.
    let (old1, new1) = (at(&kept, "F2:side#1").unwrap(), at(&by_place, "F2:side#1").unwrap());
    assert!((0..3).any(|k| (old1[k] - new1[k]).abs() > 1.0), "{kept:?} {by_place:?}");
    let mut legacy = solidcraft_doc::Document::clone(&s.doc);
    legacy.features.iter_mut().for_each(|f| f.kernel_piece_order = true);
    let mut saved_with = Session::new(legacy);
    saved_with.refresh();
    assert_eq!(listed(&mut saved_with, "Block"), kept);
}

/// The volumes of the design's bodies, by name.
fn volumes(s: &Session) -> Vec<(String, f64)> {
    let st = s.model.state();
    st.bodies.iter().map(|b| (b.name.clone(), solidcraft_kernel::measure(&b.body).unwrap().volume)).collect()
}

/// Whether the four vertical corners of the box `lo`..`hi` are rounded halfway up `z`.
fn corners_rounded(s: &Session, lo: [f64; 2], hi: [f64; 2], z: f64) -> bool {
    let e = 0.2;
    [[lo[0] + e, lo[1] + e], [hi[0] - e, lo[1] + e], [hi[0] - e, hi[1] - e], [lo[0] + e, hi[1] - e]].iter().all(|[x, y]| !solid_at(s, [*x, *y, z]))
}

/// A 50 mm square box of parametric `height`, and a 10 mm box sketched on its top: two bodies.
fn stacked_boxes(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "height", "expression": "40 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Low"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-25, -25], "p1": [25, 25]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": "height", "name": "Lower"}));
    run(s, "sketch.create", json!({"plane": {"face": [0, 0, 40]}, "name": "High"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-25, -25], "p1": [25, 25]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10, "operation": "new", "name": "Upper"}));
    assert_eq!(s.model.state().bodies.len(), 2);
}

/// Every feature evaluated without an error or a warning.
fn all_clean(s: &Session, when: &str) {
    for f in &s.doc.features {
        let r = s.model.result(f.id);
        assert_eq!(r.and_then(|r| r.error.clone()), None, "{} {when}", f.name);
        assert_eq!(r.and_then(|r| r.warning.clone()), None, "{} {when}", f.name);
    }
}

/// Two stacked boxes, the lower one of parametric height, the vertical edges of each rounded by
/// its own fillet (or by one fillet for all eight). Raising the lower box puts the upper box's
/// picks (z = 45) on the lower box's rounded corners: the upper box's rounds stay on the upper
/// box, found by their names, not moved onto the lower box's already rounded edges.
#[test]
fn rounds_on_stacked_boxes_stay_on_their_own_box_when_the_lower_one_grows() {
    for one_feature in [false, true] {
        let mut s = Session::default();
        stacked_boxes(&mut s);
        let low = json!([[-25, -25, 20], [25, -25, 20], [25, 25, 20], [-25, 25, 20]]);
        let high = json!([[-25, -25, 45], [25, -25, 45], [-25, 25, 45], [25, 25, 45]]);
        if one_feature {
            let all: Vec<Value> = low.as_array().unwrap().iter().chain(high.as_array().unwrap()).cloned().collect();
            run(&mut s, "solid.fillet", json!({"edges": all, "radius": 2, "name": "Rounds"}));
        } else {
            run(&mut s, "solid.fillet", json!({"edges": low, "radius": 2, "name": "RoundLow"}));
            run(&mut s, "solid.fillet", json!({"edges": high, "radius": 2, "name": "RoundHigh"}));
        }
        for height in [40.0, 50.0, 47.0, 45.0, 30.0] {
            change(&mut s, "height", &format!("{height} mm"));
            let when = format!("at height {height} ({one_feature})");
            assert_eq!(s.model.state().bodies.len(), 2, "{when}");
            assert!(corners_rounded(&s, [-25.0, -25.0], [25.0, 25.0], height / 2.0), "lower box {when}: {:?}", volumes(&s));
            assert!(corners_rounded(&s, [-25.0, -25.0], [25.0, 25.0], height + 5.0), "upper box {when}: {:?}", volumes(&s));
            // Nothing else rounded: the edges where the boxes meet stay sharp.
            assert!(solid_at(&s, [0.0, -24.9, height - 0.1]), "a top edge of the lower box was rounded {when}");
            assert!(solid_at(&s, [0.0, -24.9, height + 0.1]), "a bottom edge of the upper box was rounded {when}");
            all_clean(&s, &when);
        }
    }
}

/// Two bodies from one extrude (two profiles), all eight vertical edges rounded in one fillet:
/// each edge is rounded on its own body, never moved onto the first body.
#[test]
fn one_fillet_over_two_bodies_of_one_extrude_rounds_each_body_s_own_edges() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "20 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [45, 0], "p1": [85, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "h", "profiles": [0, 1], "operation": "new", "name": "Pair"}));
    assert_eq!(s.model.state().bodies.len(), 2);
    let edges = json!([[0, 0, 10], [40, 0, 10], [40, 30, 10], [0, 30, 10], [45, 0, 10], [85, 0, 10], [85, 30, 10], [45, 30, 10]]);
    run(&mut s, "solid.fillet", json!({"edges": edges, "radius": 2, "name": "Rounds"}));
    let check = |s: &Session, z: f64, when: &str| {
        assert!(corners_rounded(s, [0.0, 0.0], [40.0, 30.0], z), "first body {when}: {:?}", volumes(s));
        assert!(corners_rounded(s, [45.0, 0.0], [85.0, 30.0], z), "second body {when}: {:?}", volumes(s));
        // Nothing else rounded: the top edges stay sharp.
        assert!(solid_at(s, [20.0, 0.1, 2.0 * z - 0.1]) && solid_at(s, [65.0, 0.1, 2.0 * z - 0.1]), "a top edge was rounded {when}");
        assert_eq!(error(s, "Rounds"), None, "{when}");
        assert_eq!(warning(s, "Rounds"), None, "{when}");
    };
    check(&s, 10.0, "when made");
    change(&mut s, "h", "30 mm");
    check(&s, 15.0, "after h changed");
}

/// One fillet over a vertical edge of each of two stacked boxes. A build before blends were split
/// by body named the upper box's edge after the lower box's nearest edge (its top front edge),
/// which was rounded instead. The pick (outside the box the name was recorded on) shows that name
/// is not the upper box's: the edge is found at its point, on the upper box.
#[test]
fn one_fillet_over_stacked_boxes_rounds_the_upper_box_s_edge_even_when_saved_named_on_the_lower() {
    let mut s = Session::default();
    stacked_boxes(&mut s);
    run(&mut s, "solid.fillet", json!({"edges": [[25, -25, 20], [-25, -25, 45]], "radius": 2, "name": "Rounds"}));
    let check = |s: &Session, when: &str| {
        assert!(!solid_at(s, [24.8, -24.8, 20.0]), "lower box's edge not rounded {when}");
        assert!(!solid_at(s, [-24.8, -24.8, 45.0]), "upper box's edge not rounded {when}");
        assert!(solid_at(s, [0.0, -24.9, 39.9]), "the lower box's top front edge was rounded {when}");
        all_clean(s, when);
    };
    check(&s, "when made");
    let f = id(&s, "Rounds");
    let names = s.doc.feature(f).unwrap().edge_names.clone();
    let upper = format!("F{}:", id(&s, "Upper"));
    assert!(names[1].starts_with(&upper), "the upper box's edge is named on the upper box: {names:?}");
    // As an older build saved it: both edges named and referenced on the lower box.
    let before = s.model.state_before(f);
    let lower = before.bodies.iter().find(|b| b.feature == id(&s, "Lower")).unwrap().clone();
    let old_names = solidcraft_doc::naming::edge_names_at(&lower, &[Vec3::new(25.0, -25.0, 20.0), Vec3::new(-25.0, -25.0, 45.0)]);
    assert!(old_names[1].contains("top"), "{old_names:?}");
    let old_refs =
        solidcraft_doc::edge_refs(&before, &Some(lower.name.clone().into()), &[Vec3::new(25.0, -25.0, 20.0), Vec3::new(-25.0, -25.0, 45.0)]);
    let feature = s.doc_mut().feature_mut(f).unwrap();
    feature.edge_names = old_names;
    feature.edge_refs = old_refs;
    s.refresh();
    check(&s, "saved by an older build");
}

/// A shell opened by a point pick on a bar's end face keeps opening that face when a parameter
/// edit moves the bar so the stored point no longer lies on it (it now lies on the bottom face).
/// The point is moved onto the named face clear of its edges: the face's nearest point to the old
/// pick is on the edge it shares with the bottom face, where a pick finds both faces (#2531).
#[test]
fn a_shell_point_pick_moved_off_its_face_reopens_only_that_face() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "100 mm", "unit": "mm"}));
    run(&mut s, "parameters.add", json!({"name": "Y0", "expression": "5 mm", "unit": "mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 5], "p1": [100, 25]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["origin", "l1.start"], "mode": "vertical"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l1.start"], "type": "vertical", "value": "Y0"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "type": "length", "value": "L"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l4"], "type": "length", "value": "20 mm"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": "20 mm", "operation": "new", "body_name": "Bar"}));
    run(&mut s, "solid.shell", json!({"faces": [[100, 15, 10]], "thickness": "2 mm", "direction": "inside", "body": "Bar", "name": "Shell"}));
    change(&mut s, "L", "150 mm");
    change(&mut s, "Y0", "15 mm");
    assert_eq!(error(&s, "Shell"), None);
    let state = s.model.state();
    let bar = state.body("Bar").expect("the shelled bar");
    let volume = solidcraft_kernel::measure(&bar.body).expect("measure").volume;
    // 150 x 20 x 20 open at x = 150 only: 60000 - 148 * 16 * 16.
    assert!((volume - 22112.0).abs() < 1e-3, "{volume}");
    assert_eq!(bar.body.face_count(), 11);
    assert!(solid_at(&s, [100.0, 16.0, 10.0]), "the bottom face stays closed");
    assert!(!solid_at(&s, [149.0, 25.0, 10.0]), "the end at x = 150 is open");
}
