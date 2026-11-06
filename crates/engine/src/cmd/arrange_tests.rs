use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn block(s: &mut Session, name: &str, size: [f64; 3], corner: [f64; 3]) {
    run(s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner, "operation": "new", "body_name": name}));
}

/// An L of two 30 x 10 arms, 5 high, at `corner`.
fn ell(s: &mut Session, name: &str, corner: [f64; 3]) {
    block(s, name, [30.0, 10.0, 5.0], corner);
    let up = [corner[0], corner[1] + 10.0, corner[2]];
    run(s, "solid.box", json!({"length": 10, "width": 20, "height": 5, "corner": up, "operation": "join", "targets": [name]}));
}

fn names(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().or_else(|| x["body"].as_str()).map(String::from)).collect()
}

/// The box of a body as it stands: (min, max).
fn bounds(s: &Session, name: &str) -> ([f64; 3], [f64; 3]) {
    let st = s.world_state().unwrap_or_else(|e| panic!("{e}"));
    let b = st.body(name).unwrap_or_else(|| panic!("no body {name}"));
    let m = b.mesh().bounds();
    ([m.min.x, m.min.y, m.min.z], [m.max.x, m.max.y, m.max.z])
}

/// Every body in `bodies` lies in the rectangle (within the display mesh's chord), and no two
/// overlap or come closer than `spacing`, measured on the exact faces.
fn assert_laid_out(s: &mut Session, bodies: &[&str], rect: [f64; 4], spacing: f64) {
    for b in bodies {
        let (lo, hi) = bounds(s, b);
        let chord = 1e-2;
        assert!(
            lo[0] >= rect[0] - chord && lo[1] >= rect[1] - chord && hi[0] <= rect[2] + chord && hi[1] <= rect[3] + chord,
            "{b} at {lo:?}..{hi:?}"
        );
    }
    let r = run(s, "inspect.clearance", json!({"bodies": bodies, "clearance": (spacing - 1e-4).max(0.0)}));
    assert!(r["overlaps"].as_array().is_some_and(Vec::is_empty), "{r}");
    if spacing > 0.0 {
        assert!(r["near"].as_array().is_some_and(Vec::is_empty), "{r}");
    }
}

#[test]
fn blocks_fill_a_plate_exactly() {
    let mut s = Session::default();
    let all = ["A", "B", "C", "D"];
    for (i, n) in all.iter().enumerate() {
        block(&mut s, n, [20.0, 10.0, 5.0], [i as f64 * 7.0, i as f64 * 3.0, i as f64 * 10.0]);
    }
    let r = run(&mut s, "solid.arrange", json!({"bodies": all, "width": 40, "length": 20, "spacing": 0}));
    assert_eq!(names(&r["placed"]).len(), 4, "{r}");
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    assert!((r["fill"].as_f64().unwrap_or(0.0) - 1.0).abs() < 1e-3, "{r}");
    assert_laid_out(&mut s, &all, [0.0, 0.0, 40.0, 20.0], 0.0);
    // They come down onto the sheet.
    for n in all {
        assert!(bounds(&s, n).0[2].abs() < 1e-9, "{n}");
    }
}

#[test]
fn round_bodies_keep_the_spacing_and_margin_on_their_true_outline() {
    let mut s = Session::default();
    let all = ["P1", "P2", "P3", "P4", "Ball"];
    for (i, n) in all.iter().take(4).enumerate() {
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 8, "center": [i as f64 * 50.0, 0, 0], "operation": "new", "body_name": n}));
    }
    run(&mut s, "solid.sphere", json!({"radius": 6, "center": [0, 80, 20], "operation": "new", "body_name": "Ball"}));
    let r = run(&mut s, "solid.arrange", json!({"bodies": all, "width": 70, "length": 47, "origin": [100, -50], "spacing": 2, "margin": 1}));
    assert_eq!(names(&r["placed"]).len(), 5, "{r}");
    let err = r["footprint_error_mm"].as_f64().unwrap_or(1.0);
    assert!(err > 0.0 && err <= 0.02, "{r}");
    assert_laid_out(&mut s, &all, [101.0, -49.0, 169.0, -4.0], 2.0);
}

#[test]
fn ells_interlock_when_they_may_turn_and_not_otherwise() {
    let mut s = Session::default();
    ell(&mut s, "L1", [0.0, 0.0, 0.0]);
    ell(&mut s, "L2", [100.0, 0.0, 0.0]);
    // Two Ls turned half a turn against each other make a 30 x 40 block... less the corners: a
    // 40 x 30 rectangle holds them only interlocked.
    let snapshot = s.doc.clone();
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["L1", "L2"], "width": 40, "length": 30, "spacing": 0, "angle_step": 0}));
    assert_eq!(names(&r["unplaced"]).len(), 1, "{r}");
    s.doc = snapshot;
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["L1", "L2"], "width": 40, "length": 30, "spacing": 0, "angle_step": 90}));
    assert_eq!(names(&r["placed"]).len(), 2, "{r}");
    assert_laid_out(&mut s, &["L1", "L2"], [0.0, 0.0, 40.0, 30.0], 0.0);
}

#[test]
fn a_body_that_does_not_fit_stays_put_and_one_undo_puts_all_back() {
    let mut s = Session::default();
    block(&mut s, "Small", [10.0, 10.0, 10.0], [50.0, 50.0, 0.0]);
    block(&mut s, "Huge", [500.0, 10.0, 10.0], [0.0, 100.0, 0.0]);
    let before = (bounds(&s, "Small"), bounds(&s, "Huge"));
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["Small", "Huge"], "width": 100, "length": 100}));
    assert_eq!(names(&r["unplaced"]), ["Huge"], "{r}");
    assert_eq!(bounds(&s, "Huge"), before.1);
    assert_ne!(bounds(&s, "Small"), before.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((bounds(&s, "Small"), bounds(&s, "Huge")), before);
}

#[test]
fn other_bodies_and_ones_that_do_not_fit_stand_in_the_way() {
    let mut s = Session::default();
    // A post in the middle of a 60 x 60 sheet, a slab under it (the table), parts floating
    // above: they come down onto the sheet and keep clear of the post, not of the table.
    block(&mut s, "Table", [100.0, 100.0, 5.0], [-20.0, -20.0, -5.0]);
    block(&mut s, "Post", [20.0, 20.0, 30.0], [20.0, 20.0, 0.0]);
    let parts = ["P1", "P2", "P3", "P4"];
    for (i, n) in parts.iter().enumerate() {
        block(&mut s, n, [18.0, 18.0, 4.0], [200.0 + 30.0 * i as f64, 0.0, 40.0]);
    }
    // Too long for the sheet, lying across it: it stays, and the others keep clear of it too.
    block(&mut s, "Long", [90.0, 6.0, 4.0], [-15.0, 52.0, 0.0]);
    let all = ["P1", "P2", "P3", "P4", "Long"];
    let r = run(&mut s, "solid.arrange", json!({"bodies": all, "width": 60, "length": 60, "spacing": 1}));
    assert_eq!(names(&r["unplaced"]), ["Long"], "{r}");
    assert_eq!(names(&r["placed"]).len(), 4, "{r}");
    for n in parts {
        let (lo, hi) = bounds(&s, n);
        assert!(lo[2].abs() < 1e-6 && (hi[2] - 4.0).abs() < 1e-6, "{n} stands on the sheet: {lo:?}");
    }
    let mut every: Vec<&str> = parts.to_vec();
    every.extend(["Post", "Long"]);
    let c = run(&mut s, "inspect.clearance", json!({"bodies": every, "clearance": 0.0}));
    assert!(c["overlaps"].as_array().is_some_and(Vec::is_empty), "{c}");
}

#[test]
fn bad_calls_say_what_is_wrong_and_change_nothing() {
    let mut s = Session::default();
    block(&mut s, "A", [10.0, 10.0, 10.0], [0.0, 0.0, 0.0]);
    let undo = s.undo.len();
    for (p, want) in [
        (json!({"width": 10, "length": 10}), "bodies"),
        (json!({"bodies": ["Nope"], "width": 10, "length": 10}), "Nope"),
        (json!({"bodies": ["A"], "width": 0, "length": 10}), "width"),
        (json!({"bodies": ["A"], "width": 10, "length": -1}), "length"),
        (json!({"bodies": ["A"], "width": 10, "length": 10, "spacing": -1}), "spacing"),
        (json!({"bodies": ["A"], "width": 10, "length": 10, "angle_step": 0.01}), "angle_step"),
        (json!({"bodies": ["A"], "width": 10, "length": 10, "method": "best"}), "method"),
        (json!({"bodies": ["A"], "width": 10, "length": 10, "improve": 1.5}), "improve"),
        (json!({"bodies": ["A"], "width": 1e300, "length": 10}), "width"),
    ] {
        let e = s.execute("solid.arrange", &p).map(|v| v.to_string()).unwrap_err().to_string();
        assert!(e.contains(want), "{p}: {e}");
    }
    assert_eq!(s.undo.len(), undo);
}

/// A 60 x 40 x 5 plate with a 10 x 10 x 20 block standing on it.
fn plate_with_block(s: &mut Session, name: &str) {
    block(s, name, [60.0, 40.0, 5.0], [0.0, 0.0, 0.0]);
    run(s, "solid.box", json!({"length": 10, "width": 10, "height": 20, "corner": [5, 5, 5], "operation": "join", "targets": [name]}));
}

fn spans(s: &Session, name: &str) -> [f64; 3] {
    let (lo, hi) = bounds(s, name);
    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
}

#[test]
fn lay_flat_puts_a_plate_with_a_block_on_it_plate_down() {
    let mut s = Session::default();
    plate_with_block(&mut s, "P");
    // Stood on its edge and turned, lifted off the plane.
    run(&mut s, "solid.move", json!({"bodies": ["P"], "axis": [1, 0.3, 0.2], "angle": "100 deg", "translate": [0, 0, 30]}));
    // A round bar on its side: its ends are its only flat sides.
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 50, "center": [0, 80, 0], "operation": "new", "body_name": "Bar"}));
    run(&mut s, "solid.move", json!({"bodies": ["Bar"], "axis": [0, 1, 0], "angle": "90 deg"}));
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["P", "Bar"], "width": 100, "length": 100, "spacing": 2, "lay_flat": true}));
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    let [x, y, z] = spans(&s, "P");
    let mut flat = [x, y];
    flat.sort_by(f64::total_cmp);
    assert!((flat[0] - 40.0).abs() < 0.05 && (flat[1] - 60.0).abs() < 0.05 && (z - 25.0).abs() < 0.05, "{x} {y} {z}");
    let (lo, _) = bounds(&s, "P");
    assert!(lo[2].abs() < 1e-3, "stands on z = 0: {lo:?}");
    let clear = run(&mut s, "inspect.clearance", json!({"bodies": ["P", "Bar"], "clearance": 1.0}));
    assert!(clear["overlaps"].as_array().is_some_and(Vec::is_empty), "{clear}");
    // The plate is at the bottom: the centre of mass is low.
    let mass = run(&mut s, "inspect.measure", json!({"bodies": ["P"]}));
    let com = &mass["bodies"][0]["center_of_mass"];
    let cz = com[2].as_f64().or_else(|| com["z"].as_f64()).unwrap_or(f64::NAN);
    // Plate 12 000 mm³ about z = 2.5, block 2 000 mm³ about z = 15.
    assert!((cz - (12_000.0 * 2.5 + 2_000.0 * 15.0) / 14_000.0).abs() < 0.05, "{mass}");
    let [bx, by, bz] = spans(&s, "Bar");
    assert!((bz - 50.0).abs() < 0.05 && (bx - 12.0).abs() < 0.05 && (by - 12.0).abs() < 0.05, "the bar stands on an end: {bx} {by} {bz}");
    assert!(r["placed"].as_array().is_some_and(|p| p.iter().all(|b| b["bottom"].is_array())), "{r}");
}

#[test]
fn a_picked_bottom_face_is_the_one_laid_down() {
    let mut s = Session::default();
    plate_with_block(&mut s, "P");
    block(&mut s, "Q", [20.0, 10.0, 5.0], [100.0, 0.0, 0.0]);
    // The plate's long side at y = 0 goes down; Q is laid flat as it is.
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["P", "Q"], "width": 200, "length": 200, "lay_flat": true, "bottoms": [[30, 0, 2.5]]}));
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    let [_, _, z] = spans(&s, "P");
    assert!((z - 40.0).abs() < 0.05, "laid on its side: {z}");
    let bottom = r["placed"].as_array().and_then(|p| p.iter().find(|b| b["body"] == "P")).map(|b| b["bottom"].clone());
    assert_eq!(bottom, Some(json!([0.0, -1.0, 0.0])), "{r}");
    let [_, _, qz] = spans(&s, "Q");
    assert!((qz - 5.0).abs() < 0.05, "{qz}");
    // Without lay_flat, only the picked body is laid down.
    let mut s = Session::default();
    plate_with_block(&mut s, "P");
    block(&mut s, "Q", [20.0, 10.0, 5.0], [100.0, 0.0, 0.0]);
    run(&mut s, "solid.move", json!({"bodies": ["Q"], "axis": [1, 0, 0], "angle": "90 deg"}));
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["P", "Q"], "width": 200, "length": 200, "bottoms": [[30, 0, 2.5]]}));
    let [_, _, qz] = spans(&s, "Q");
    assert!((qz - 10.0).abs() < 0.05, "Q keeps its turn: {qz} {r}");
}

#[test]
fn bad_bottoms_say_what_is_wrong_and_change_nothing() {
    let mut s = Session::default();
    plate_with_block(&mut s, "P");
    block(&mut s, "Q", [20.0, 10.0, 5.0], [100.0, 0.0, 0.0]);
    let before = bounds(&s, "P");
    for (p, why) in [
        (json!({"bottoms": [[500, 500, 500]]}), "not on a flat face"),
        (json!({"bottoms": [[30, 0, 2.5], [30, 40, 2.5]]}), "two bottom faces"),
        (json!({"bottoms": "top"}), "`bottoms`"),
        (json!({"lay_flat": 1}), "`lay_flat`"),
    ] {
        let mut q = json!({"bodies": ["P", "Q"], "width": 200, "length": 200});
        if let (Some(q), Some(p)) = (q.as_object_mut(), p.as_object()) {
            q.extend(p.clone());
        }
        match s.execute("solid.arrange", &q) {
            Ok(v) => panic!("{q} gave {v}"),
            Err(e) => assert!(e.to_string().contains(why), "{q}: {e}"),
        }
    }
    assert_eq!(bounds(&s, "P"), before);
}

#[test]
fn a_pick_in_a_ring_is_the_part_inside_it_not_the_ring() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 6, "center": [0, 0, 0], "body_name": "Ring"}));
    run(&mut s, "solid.cylinder", json!({"radius": 12, "height": 6, "center": [0, 0, 0], "operation": "cut", "targets": ["Ring"]}));
    block(&mut s, "Pin", [8.0, 8.0, 6.0], [-4.0, -4.0, 0.0]);
    // The pin's top is in the plane of the ring's top, and inside the ring's box.
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["Ring", "Pin"], "width": 200, "length": 200, "bottoms": [[0, 0, 6]]}));
    let pin = r["placed"].as_array().and_then(|a| a.iter().find(|b| b["body"] == "Pin")).cloned().unwrap_or_default();
    assert_eq!(pin["bottom"], json!([0.0, 0.0, 1.0]), "{r}");
    assert!(r["placed"].as_array().and_then(|a| a.iter().find(|b| b["body"] == "Ring")).is_some_and(|b| b["bottom"].is_null()), "{r}");
}

#[test]
fn turns_compose_into_one_axis_and_angle() {
    use super::{IDENTITY, axis_angle, pointing_down};
    use solidcore_record::Vec3;
    use solidcraft_geom::prelude::*;
    let turn = |axis: Vec3, a: f64| {
        let axis = axis.normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        IDENTITY.map(|v| v * a.cos() + axis.cross(v) * a.sin() + axis * (axis.dot(v) * (1.0 - a.cos())))
    };
    for (axis, a) in [
        (Vec3::new(1.0, 0.0, 0.0), 0.3),
        (Vec3::new(1.0, 2.0, -0.5), 2.0),
        (Vec3::new(0.0, 1.0, 0.0), std::f64::consts::PI),
        (Vec3::new(1.0, 1.0, 1.0), std::f64::consts::PI - 1e-9),
        (Vec3::new(-0.2, 0.1, 1.0), 1e-13),
    ] {
        let r = turn(axis, a);
        let (got_axis, got) = axis_angle(r);
        let back = turn(got_axis, got);
        for k in 0..3 {
            assert!((back[k] - r[k]).len() < 1e-7, "{axis:?} {a}: {got_axis:?} {got}");
        }
    }
    for n in [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.6, 0.0, 0.8), Vec3::new(0.0, -1.0, 0.0)] {
        let r = pointing_down(n);
        let image = r[0] * n.x + r[1] * n.y + r[2] * n.z;
        assert!((image - Vec3::new(0.0, 0.0, -1.0)).len() < 1e-12, "{n:?} -> {image:?}");
    }
}

#[test]
fn quantity_makes_copies_and_more_sheets_take_what_does_not_fit() {
    let mut s = Session::default();
    block(&mut s, "A", [20.0, 20.0, 5.0], [0.0, 0.0, 30.0]);
    // Four fit on a 40 x 40 sheet; ten copies need three sheets, and two sheets leave two over.
    let r = run(
        &mut s,
        "solid.arrange",
        json!({"bodies": ["A"], "width": 40, "length": 40, "spacing": 0, "quantity": "10", "sheets": 2, "sheet_spacing": 5}),
    );
    let placed = r["placed"].as_array().cloned().unwrap_or_default();
    assert_eq!(placed.len(), 8, "{r}");
    assert_eq!((r["copies_left"].as_u64(), r["sheets_used"].as_u64()), (Some(2), Some(2)), "{r}");
    assert!((r["fill"].as_f64().unwrap_or(0.0) - 1.0).abs() < 1e-3, "{r}");
    let bodies: Vec<String> = placed.iter().filter_map(|b| b["body"].as_str().map(String::from)).collect();
    assert_eq!(s.world_state().map(|w| w.bodies.len()).unwrap_or(0), 8, "seven copies and A");
    assert_eq!(placed.iter().filter(|b| b["copy_of"] == "A").count(), 7, "{r}");
    for (k, x0) in [(0, 0.0), (1, 45.0)] {
        let on: Vec<&str> = placed.iter().filter(|b| b["sheet"] == k).filter_map(|b| b["body"].as_str()).collect();
        assert_eq!(on.len(), 4, "{r}");
        assert_laid_out(&mut s, &on, [x0, 0.0, x0 + 40.0, 40.0], 0.0);
    }
    let all: Vec<&str> = bodies.iter().map(String::as_str).collect();
    let r = run(&mut s, "inspect.clearance", json!({"bodies": all, "clearance": 0.0}));
    assert!(r["overlaps"].as_array().is_some_and(Vec::is_empty), "{r}");
    // One undo takes the copies away and puts A back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.world_state().map(|w| w.bodies.len()).unwrap_or(0), 1);
    assert_eq!(bounds(&s, "A").0, [0.0, 0.0, 30.0]);
}

#[test]
fn copy_leaves_the_bodies_and_lays_out_copies() {
    let mut s = Session::default();
    block(&mut s, "A", [20.0, 20.0, 5.0], [0.0, 0.0, 0.0]);
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["A"], "width": 40, "length": 40, "spacing": 0, "quantity": 2, "copy": true}));
    let placed = r["placed"].as_array().cloned().unwrap_or_default();
    assert_eq!(placed.len(), 2, "{r}");
    assert!(placed.iter().all(|b| b["copy_of"] == "A" && b["body"] != "A"), "{r}");
    assert_eq!(r["copies_left"].as_u64(), Some(0), "{r}");
    assert_eq!(bounds(&s, "A").0, [0.0, 0.0, 0.0], "A stays where it was");
    assert_eq!(s.world_state().map(|w| w.bodies.len()).unwrap_or(0), 3);
    // The copies keep clear of A, which stands on the sheet.
    let all: Vec<&str> = std::iter::once("A").chain(placed.iter().filter_map(|b| b["body"].as_str())).collect();
    let r2 = run(&mut s, "inspect.clearance", json!({"bodies": all, "clearance": 0.0}));
    assert!(r2["overlaps"].as_array().is_some_and(Vec::is_empty), "{r2}");
    for b in &placed {
        let (lo, _) = bounds(&s, b["body"].as_str().unwrap_or(""));
        assert!(lo[0] >= 20.0 - 1e-6 || lo[1] >= 20.0 - 1e-6, "a copy on A: {lo:?}");
    }
    run(&mut s, "edit.undo", json!({}));
    // Nothing fits: an error that says so, and A untouched.
    let none = s.execute("solid.arrange", &json!({"bodies": ["A"], "width": 10, "length": 10, "quantity": 2, "copy": true}));
    assert!(none.as_ref().is_err_and(|e| e.to_string().contains("nothing fits")), "{none:?}");
    assert_eq!(bounds(&s, "A").0, [0.0, 0.0, 0.0]);
    let bad = s.execute("solid.arrange", &json!({"bodies": ["A"], "width": 40, "length": 40, "copy": "yes"}));
    assert!(bad.is_err());
}

#[test]
fn a_level_face_is_a_sheet_with_its_holes() {
    let mut s = Session::default();
    // A 100 x 60 table top 10 thick with a 40 x 40 hole through the middle.
    block(&mut s, "Table", [100.0, 60.0, 10.0], [0.0, 0.0, -10.0]);
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [30, 10, -10], "operation": "cut", "targets": ["Table"]}));
    let parts = ["P1", "P2", "P3", "P4"];
    for (i, n) in parts.iter().enumerate() {
        block(&mut s, n, [25.0, 25.0, 4.0], [200.0 + 30.0 * i as f64, 0.0, 50.0]);
    }
    // The hole leaves room for one 25 mm square at each end, two by two: four fit.
    let r = run(&mut s, "solid.arrange", json!({"bodies": parts, "sheet": [5, 5, 0], "spacing": 1, "lay_flat": true}));
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    for n in parts {
        let (lo, hi) = bounds(&s, n);
        assert!(lo[2].abs() < 1e-6 && (hi[2] - 4.0).abs() < 1e-6, "{n} stands on the table: {lo:?}");
        let over_hole = lo[0] < 70.0 - 1e-6 && hi[0] > 30.0 + 1e-6 && lo[1] < 50.0 - 1e-6 && hi[1] > 10.0 + 1e-6;
        assert!(!over_hole && lo[0] >= -1e-3 && hi[0] <= 100.0 + 1e-3 && lo[1] >= -1e-3 && hi[1] <= 60.0 + 1e-3, "{n} at {lo:?}..{hi:?}");
    }
    // Not laid down, they still stand on the table, from wherever they were.
    for (i, n) in parts.iter().enumerate() {
        run(&mut s, "solid.move", json!({"bodies": [n], "translate": [200.0 + 30.0 * i as f64, 0, 50]}));
    }
    run(&mut s, "solid.arrange", json!({"bodies": parts, "sheet": [5, 5, 0], "spacing": 1}));
    for n in parts {
        let (lo, hi) = bounds(&s, n);
        assert!(lo[2].abs() < 1e-6 && (hi[2] - 4.0).abs() < 1e-6, "{n} stands on the table: {lo:?}");
    }
    let five = run(&mut s, "solid.arrange", json!({"bodies": parts, "sheet": [5, 5, 0], "spacing": 1, "quantity": 2}));
    assert!(five["copies_left"].as_u64().is_some_and(|n| n > 0), "the table is full: {five}");
    // Over the hole, with a rectangle too, on a side, on the underside, two sheets, an origin.
    for p in [
        json!({"sheet": [50, 30, 0]}),
        json!({"sheet": [5, 5, 0], "width": 10, "length": 10}),
        json!({"sheet": [0, 30, -5]}),
        json!({"sheet": [5, 5, -10]}),
        json!({"sheet": [5, 5, 0], "sheets": 2}),
        json!({"sheet": [5, 5, 0], "origin": [1, 1]}),
    ] {
        let mut q = json!({"bodies": parts});
        if let (Some(q), Some(p)) = (q.as_object_mut(), p.as_object()) {
            q.extend(p.clone());
        }
        assert!(s.execute("solid.arrange", &q).is_err(), "{q}");
    }
}

#[test]
fn a_ring_laid_flat_can_be_arranged_again() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 6, "center": [0, 0, 0], "body_name": "Ring"}));
    run(&mut s, "solid.cylinder", json!({"radius": 12, "height": 6, "center": [0, 0, 0], "operation": "cut", "targets": ["Ring"]}));
    run(
        &mut s,
        "solid.move",
        json!({"bodies": ["Ring"], "axis": [-0.9250086831160302, -0.13270863267522826, -0.8602891528507621], "angle": "51.8 deg", "translate": [0, 300, 60]}),
    );
    run(&mut s, "solid.arrange", json!({"bodies": ["Ring"], "width": 260, "length": 200, "lay_flat": true}));
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["Ring"], "width": 300, "length": 120, "origin": [0, -160], "quantity": 3, "copy": true}));
    assert_eq!(r["placed"].as_array().map(Vec::len), Some(3), "{r}");
}
