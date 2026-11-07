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

/// Stretches the time budget for this thread's layouts: a test of a layout's quality must not
/// be hurried by a loaded machine.
fn unhurried() {
    super::BUDGET_SCALE.with(|c| c.set(100.0));
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
    let r =
        run(&mut s, "solid.arrange", json!({"bodies": ["L1", "L2"], "width": 40, "length": 30, "spacing": 0, "angle_step": 0, "lay_flat": false}));
    assert_eq!(names(&r["unplaced"]).len(), 1, "{r}");
    s.doc = snapshot;
    let r =
        run(&mut s, "solid.arrange", json!({"bodies": ["L1", "L2"], "width": 40, "length": 30, "spacing": 0, "angle_step": 90, "lay_flat": false}));
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
    // With lay_flat off, only the picked body is laid down.
    let mut s = Session::default();
    plate_with_block(&mut s, "P");
    block(&mut s, "Q", [20.0, 10.0, 5.0], [100.0, 0.0, 0.0]);
    run(&mut s, "solid.move", json!({"bodies": ["Q"], "axis": [1, 0, 0], "angle": "90 deg"}));
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["P", "Q"], "width": 200, "length": 200, "lay_flat": false, "bottoms": [[30, 0, 2.5]]}));
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
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["Ring", "Pin"], "width": 200, "length": 200, "lay_flat": false, "bottoms": [[0, 0, 6]]}));
    let pin = r["placed"].as_array().and_then(|a| a.iter().find(|b| b["body"] == "Pin")).cloned().unwrap_or_default();
    assert_eq!(pin["bottom"], json!([0.0, 0.0, 1.0]), "{r}");
    assert!(r["placed"].as_array().and_then(|a| a.iter().find(|b| b["body"] == "Ring")).is_some_and(|b| b["bottom"].is_null()), "{r}");
}

/// The middle of a body's box.
fn middle(s: &Session, name: &str) -> [f64; 3] {
    let (lo, hi) = bounds(s, name);
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]
}

#[test]
fn parts_lie_on_a_tilted_face_in_its_plane() {
    let mut s = Session::default();
    block(&mut s, "Ramp", [200.0, 200.0, 10.0], [0.0, 0.0, 0.0]);
    run(&mut s, "solid.move", json!({"bodies": ["Ramp"], "axis": [1, 0, 0], "angle": "30 deg"}));
    let (c, sn) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    // The ramp's top, turned: its normal and a point in its middle.
    let n = [0.0, -sn, c];
    let top = [100.0, 100.0 * c - 10.0 * sn, 100.0 * sn + 10.0 * c];
    block(&mut s, "A", [20.0, 10.0, 5.0], [300.0, 0.0, 0.0]);
    block(&mut s, "B", [30.0, 20.0, 5.0], [300.0, 50.0, 0.0]);
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["A", "B"], "sheet": top}));
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    assert!(r["sheet_frame"].is_array(), "{r}");
    let height = n[1] * top[1] + n[2] * top[2];
    for b in ["A", "B"] {
        // Lying flat on the ramp: its middle half its thickness out of the face.
        let m = middle(&s, b);
        let out = n[1] * m[1] + n[2] * m[2] - height;
        assert!((out - 2.5).abs() < 0.05, "{b} is {out} off the ramp: {r}");
    }
    let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Ramp", "A", "B"], "clearance": 0.5}));
    assert!(r["overlaps"].as_array().is_some_and(Vec::is_empty), "{r}");
}

#[test]
fn a_trays_floor_is_a_sheet_inside_its_walls() {
    let mut s = Session::default();
    block(&mut s, "Tray", [100.0, 80.0, 30.0], [0.0, 0.0, 0.0]);
    run(&mut s, "solid.box", json!({"length": 90, "width": 70, "height": 30, "corner": [5, 5, 5], "operation": "cut", "targets": ["Tray"]}));
    for (k, n) in ["A", "B", "C"].iter().enumerate() {
        block(&mut s, n, [25.0, 20.0, 8.0], [200.0, 40.0 * k as f64, 0.0]);
    }
    let r = run(&mut s, "solid.arrange", json!({"bodies": ["A", "B", "C"], "sheet": [50, 40, 5]}));
    assert!(names(&r["unplaced"]).is_empty(), "the walls are not in the way of the floor: {r}");
    assert_laid_out(&mut s, &["A", "B", "C"], [5.0, 5.0, 95.0, 75.0], 1.0);
    for n in ["A", "B", "C"] {
        assert!((bounds(&s, n).0[2] - 5.0).abs() < 1e-3, "{n} on the floor");
    }
}

/// A tray, level and tilted: its walls keep the parts in, and hollow parts (a tube, a cup) fit
/// like solid ones.
#[test]
fn hollow_parts_and_walls_on_a_floor_level_and_tilted() {
    for deg in [0.0_f64, 5.0] {
        let mut s = Session::default();
        block(&mut s, "Floor", [200.0, 120.0, 5.0], [0.0, 0.0, 0.0]);
        block(&mut s, "Walls", [200.0, 120.0, 25.0], [0.0, 0.0, 5.0]);
        run(&mut s, "solid.box", json!({"length": 190, "width": 110, "height": 25, "corner": [5, 5, 5], "operation": "cut", "targets": ["Walls"]}));
        run(&mut s, "solid.combine", json!({"target": "Floor", "tools": ["Walls"], "operation": "join"}));
        run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 25, "base": [300, 0, 0], "operation": "new", "body_name": "Tube"}));
        run(&mut s, "solid.cylinder", json!({"radius": 12, "height": 25, "base": [300, 0, 0], "operation": "cut", "targets": ["Tube"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 25, "height": 60, "base": [400, 0, 0], "operation": "new", "body_name": "Cup"}));
        run(&mut s, "solid.cylinder", json!({"radius": 21, "height": 56, "base": [400, 0, 4], "operation": "cut", "targets": ["Cup"]}));
        block(&mut s, "Block", [40.0, 30.0, 10.0], [300.0, 100.0, 0.0]);
        let (c, sn) = (deg.to_radians().cos(), deg.to_radians().sin());
        if deg != 0.0 {
            run(&mut s, "solid.move", json!({"bodies": ["Floor"], "axis": [0, 1, 0], "angle": format!("{deg} deg")}));
        }
        // The floor's top in its middle, turned about Y with it.
        let (x, z) = (100.0, 5.0);
        let at = [x * c + z * sn, 60.0, -x * sn + z * c];
        let r = run(&mut s, "solid.arrange", json!({"bodies": ["Tube", "Cup", "Block"], "sheet": at, "spacing": 2}));
        assert!(names(&r["unplaced"]).is_empty(), "at {deg} deg: {r}");
        let c2 = run(&mut s, "inspect.clearance", json!({"bodies": ["Tube", "Cup", "Block", "Floor"], "clearance": 1.9}));
        assert!(c2["overlaps"].as_array().is_some_and(Vec::is_empty), "at {deg} deg: {c2}");
    }
}

#[test]
fn a_rectangle_on_a_plane_starts_at_its_origin() {
    let mut s = Session::default();
    block(&mut s, "A", [20.0, 10.0, 5.0], [0.0, 0.0, 0.0]);
    block(&mut s, "B", [20.0, 10.0, 5.0], [0.0, 30.0, 0.0]);
    let r = run(
        &mut s,
        "solid.arrange",
        json!({"bodies": ["A", "B"], "width": 100, "length": 100, "plane": {"origin": [0, 0, 50], "normal": [1, 0, 0]}}),
    );
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    for n in ["A", "B"] {
        let (lo, hi) = bounds(&s, n);
        // On the plane x = 0, lying flat on it, inside the 100 x 100 square from (0, 0, 50).
        assert!(lo[0].abs() < 1e-3 && (hi[0] - 5.0).abs() < 1e-3, "{n}: {lo:?} {hi:?}");
        assert!(lo[1].min(lo[2] - 50.0) > -100.0 - 1e-3 && hi[1].max(hi[2] - 50.0) < 100.0 + 1e-3, "{n}: {lo:?} {hi:?}");
    }
    let r = run(&mut s, "inspect.clearance", json!({"bodies": ["A", "B"], "clearance": 0.5}));
    assert!(r["overlaps"].as_array().is_some_and(Vec::is_empty), "{r}");
}

#[test]
fn avoid_off_lays_out_as_if_the_sheet_were_empty() {
    let mut s = Session::default();
    block(&mut s, "Fixture", [50.0, 50.0, 10.0], [0.0, 0.0, 0.0]);
    block(&mut s, "A", [20.0, 10.0, 5.0], [100.0, 0.0, 0.0]);
    let p = json!({"bodies": ["A"], "width": 50, "length": 50});
    let e = s.execute("solid.arrange", &p).map(|v| v.to_string()).unwrap_or_else(|e| e.to_string());
    assert!(e.contains("fit"), "the fixture fills the sheet: {e}");
    let mut q = p.clone();
    q["avoid"] = json!(false);
    let r = run(&mut s, "solid.arrange", q);
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
    let (lo, hi) = bounds(&s, "A");
    assert!(lo[0] >= -1e-3 && hi[0] <= 50.0 + 1e-3 && lo[1] >= -1e-3 && hi[1] <= 50.0 + 1e-3, "{lo:?} {hi:?}");
    // A hidden body is not in the way either.
    let mut s = Session::default();
    block(&mut s, "Fixture", [50.0, 50.0, 10.0], [0.0, 0.0, 0.0]);
    block(&mut s, "A", [20.0, 10.0, 5.0], [100.0, 0.0, 0.0]);
    run(&mut s, "browser.visibility", json!({"items": ["Fixture"], "visible": false}));
    let r = run(&mut s, "solid.arrange", p);
    assert!(names(&r["unplaced"]).is_empty(), "{r}");
}

fn hole(s: &mut Session, target: &str, center: [f64; 3], radius: f64, depth: f64) {
    run(s, "solid.cylinder", json!({"radius": radius, "height": depth, "center": center, "operation": "cut", "targets": [target]}));
}

/// A sketch on the XY plane, drawn by `curves`, extruded `height` up as a new body `name`.
fn sketched(s: &mut Session, name: &str, curves: &[(&str, Value)], height: f64) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    for (id, p) in curves {
        run(s, id, p.clone());
    }
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": height, "operation": "new", "body_name": name}));
}

/// The parts of the mixed job, built apart on the XY plane.
fn mixed_parts(s: &mut Session) -> [&'static str; 9] {
    block(s, "Cover", [70.0, 45.0, 4.0], [0.0, 0.0, 0.0]);
    let corners: Vec<Value> = [[0.0, 0.0], [70.0, 0.0], [0.0, 45.0], [70.0, 45.0]].iter().map(|[x, y]| json!([x, y, 2.0])).collect();
    run(s, "solid.fillet", json!({"body": "Cover", "radius": 8, "edges": corners}));
    for (x, y) in [(9.0, 9.0), (61.0, 9.0), (9.0, 36.0), (61.0, 36.0)] {
        hole(s, "Cover", [x, y, 0.0], 2.5, 4.0);
    }
    run(s, "solid.box", json!({"length": 30, "width": 15, "height": 2, "corner": [20, 15, 2], "operation": "cut", "targets": ["Cover"]}));
    sketched(s, "Nut", &[("sketch.polygon.inscribed", json!({"center": [120, 0], "radius": 9, "sides": 6}))], 6.0);
    hole(s, "Nut", [120.0, 0.0, 0.0], 4.5, 6.0);
    sketched(s, "Link", &[("sketch.slot.center_to_center", json!({"p0": [200, 0], "p1": [240, 0], "width": 14}))], 5.0);
    hole(s, "Link", [200.0, 0.0, 0.0], 3.0, 5.0);
    hole(s, "Link", [240.0, 0.0, 0.0], 3.0, 5.0);
    let cam = [
        ("sketch.line", json!({"points": [[0, 100], [30, 100]]})),
        ("sketch.arc.three_point", json!({"start": [30, 100], "through": [15, 118], "end": [0, 100]})),
    ];
    sketched(s, "Cam", &cam, 8.0);
    hole(s, "Cam", [15.0, 106.0, 0.0], 3.0, 8.0);
    let wing = [
        ("sketch.line", json!({"points": [[60, 100], [110, 100]]})),
        ("sketch.spline.fit_point", json!({"points": [[110, 100], [95, 112], [75, 116], [60, 100]]})),
    ];
    sketched(s, "Wing", &wing, 3.0);
    block(s, "Block", [30.0, 20.0, 12.0], [150.0, 100.0, 0.0]);
    let top: Vec<Value> = [[165.0, 100.0], [165.0, 120.0], [150.0, 110.0], [180.0, 110.0]].iter().map(|[x, y]| json!([x, y, 12.0])).collect();
    run(s, "solid.chamfer", json!({"body": "Block", "distance": 1.5, "edges": top}));
    // The bracket stands on the end of its leg, its foot out at the top.
    block(s, "Bracket", [5.0, 30.0, 40.0], [220.0, 100.0, 0.0]);
    run(s, "solid.box", json!({"length": 30, "width": 30, "height": 5, "corner": [225, 100, 35], "operation": "join", "targets": ["Bracket"]}));
    run(s, "solid.fillet", json!({"body": "Bracket", "radius": 4, "edges": [[225, 115, 35]]}));
    hole(s, "Bracket", [242.0, 115.0, 35.0], 3.0, 5.0);
    run(s, "solid.cylinder", json!({"radius": 25, "height": 4, "center": [0, 200, 0], "body_name": "Ring"}));
    hole(s, "Ring", [0.0, 200.0, 0.0], 18.0, 4.0);
    run(s, "solid.cylinder", json!({"radius": 6, "height": 10, "center": [100, 200, 0], "body_name": "Pin"}));
    ["Cover", "Nut", "Link", "Cam", "Wing", "Block", "Bracket", "Ring", "Pin"]
}

/// A run of shop parts as a laser or router job would bring them: a cover with rounded corners,
/// bolt holes and a pocket, a hex nut, a slotted link, a cam of a line and an arc, a wing with
/// a spline edge, a chamfered block, a bracket standing on its leg with a filleted inside
/// corner, a ring and a pin. Five of each, 45 parts, lie flat inside a 400 x 300 sheet,
/// clear of each other on their exact outlines, nothing placed by its box, in reasonable time.
/// With `SOLIDCRAFT_ARRANGE_SHOWCASE` set to a path, the arranged design is saved there.
#[test]
fn a_mixed_job_of_curved_filleted_and_chamfered_parts_lies_flat_and_clear() {
    let mut s = Session::default();
    let parts = mixed_parts(&mut s);
    unhurried();
    let started = std::time::Instant::now();
    let r = run(&mut s, "solid.arrange", json!({"bodies": parts, "width": 400, "length": 300, "spacing": 3, "margin": 5, "quantity": 5}));
    let took = started.elapsed();
    if let Some(path) = std::env::var_os("SOLIDCRAFT_ARRANGE_SHOWCASE") {
        run(&mut s, "file.save", json!({"path": path.to_string_lossy()}));
    }
    assert!(names(&r["unplaced"]).is_empty() && r["copies_left"] == 0, "{r}");
    assert!(r.get("boxed").is_none(), "every part has its outline: {r}");
    let placed = names(&r["placed"]);
    assert_eq!(placed.len(), 45, "{r}");
    let all: Vec<&str> = placed.iter().map(String::as_str).collect();
    assert_laid_out(&mut s, &all, [5.0, 5.0, 395.0, 295.0], 3.0);
    for b in &all {
        let (lo, _) = bounds(&s, b);
        assert!(lo[2].abs() < 1e-3, "{b} stands on the sheet");
        let tall = match b.trim_end_matches(|c: char| !c.is_ascii_alphabetic()).split_whitespace().next().unwrap_or("") {
            "Block" => 12.0,
            "Bracket" => 35.0,
            "Pin" => 10.0,
            "Cam" => 8.0,
            "Nut" => 6.0,
            "Link" => 5.0,
            _ => 4.0,
        };
        let [_, _, z] = spans(&s, b);
        assert!(z <= tall + 0.05, "{b} is {z} high");
    }
    assert!(took.as_secs_f64() < 60.0, "arranging 45 parts took {took:?}");
    eprintln!("45 parts in {took:?}, fill {}", r["fill"]);
}

/// A closed outline through `points`, moved by `at`, as a sketch line.
fn outline(points: &[[f64; 2]], at: [f64; 2]) -> (&'static str, Value) {
    let pts: Vec<Value> = points.iter().map(|[x, y]| json!([x + at[0], y + at[1]])).collect();
    ("sketch.line", json!({"points": pts, "closed": true}))
}

/// Profiles drawn on the XY plane cut through `target`, `depth` up.
fn cut_through(s: &mut Session, target: &str, curves: &[(&str, Value)], depth: f64) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    for (id, p) in curves {
        run(s, id, p.clone());
    }
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": depth, "operation": "cut", "targets": [target]}));
}

/// A laser job of 3 mm sheet-metal parts, the kind a real cut sheet carries: a frame with tabs
/// and a notch, a big window, a ring of bolt holes and two slots; a gusset with a concave arc
/// edge and lightening holes; a comb with five fingers; an arch of two arcs; small tabs and
/// lugs. Four of each, 28 parts, lie flat inside a 400 x 300 sheet, 2 mm apart on their exact
/// outlines and nothing placed by its box.
/// With `SOLIDCRAFT_ARRANGE_SHOWCASE` set to a path, the arranged job is saved there too (as
/// `<path>.laser.sc`).
#[test]
fn a_laser_job_of_sheet_metal_parts_with_windows_tabs_and_slots_nests_clear() {
    let mut s = Session::default();
    let t = 3.0;
    let frame = [
        [0.0, 0.0],
        [25.0, 0.0],
        [25.0, -6.0],
        [40.0, -6.0],
        [40.0, 0.0],
        [80.0, 0.0],
        [80.0, -6.0],
        [95.0, -6.0],
        [95.0, 0.0],
        [120.0, 0.0],
        [120.0, 80.0],
        [75.0, 80.0],
        [75.0, 72.0],
        [45.0, 72.0],
        [45.0, 80.0],
        [0.0, 80.0],
    ];
    sketched(&mut s, "Frame", &[outline(&frame, [0.0, 0.0])], t);
    run(&mut s, "solid.box", json!({"length": 66, "width": 36, "height": t, "corner": [27, 20, 0], "operation": "cut", "targets": ["Frame"]}));
    for y in [8.0, 24.0, 40.0, 56.0, 72.0] {
        for x in [8.0, 112.0] {
            hole(&mut s, "Frame", [x, y, 0.0], 2.0, t);
        }
    }
    let slots = [
        ("sketch.slot.center_to_center", json!({"p0": [17, 30], "p1": [17, 50], "width": 5})),
        ("sketch.slot.center_to_center", json!({"p0": [103, 30], "p1": [103, 50], "width": 5})),
    ];
    cut_through(&mut s, "Frame", &slots, t);
    let gusset = [
        ("sketch.line", json!({"points": [[0, 200], [0, 130], [70, 130]]})),
        ("sketch.arc.three_point", json!({"start": [70, 130], "through": [25, 155], "end": [0, 200]})),
    ];
    sketched(&mut s, "Gusset", &gusset, t);
    hole(&mut s, "Gusset", [14.0, 144.0, 0.0], 5.0, t);
    for (x, y) in [(6.0, 170.0), (40.0, 136.0), (6.0, 155.0)] {
        hole(&mut s, "Gusset", [x, y, 0.0], 2.0, t);
    }
    let mut comb = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 30.0]];
    for x in [84.0, 66.0, 48.0, 30.0, 12.0] {
        comb.extend([[x + 8.0, 30.0], [x + 8.0, 15.0], [x, 15.0], [x, 30.0]]);
    }
    comb.push([0.0, 30.0]);
    sketched(&mut s, "Comb", &[outline(&comb, [150.0, 0.0])], t);
    for x in [160.0, 180.0, 200.0, 220.0, 240.0] {
        hole(&mut s, "Comb", [x, 7.0, 0.0], 2.0, t);
    }
    let arch = [
        ("sketch.line", json!({"points": [[160, 130], [160, 100], [145, 100], [145, 130]]})),
        ("sketch.arc.three_point", json!({"start": [145, 130], "through": [130, 145], "end": [115, 130]})),
        ("sketch.line", json!({"points": [[115, 130], [115, 100], [100, 100], [100, 130]]})),
        ("sketch.arc.three_point", json!({"start": [100, 130], "through": [130, 160], "end": [160, 130]})),
    ];
    sketched(&mut s, "Arch", &arch, t);
    hole(&mut s, "Arch", [107.5, 110.0, 0.0], 3.0, t);
    hole(&mut s, "Arch", [152.5, 110.0, 0.0], 3.0, t);
    block(&mut s, "Tab", [22.0, 12.0, t], [200.0, 100.0, 0.0]);
    hole(&mut s, "Tab", [211.0, 106.0, 0.0], 2.5, t);
    let lug = [
        ("sketch.line", json!({"points": [[262, 100], [262, 115], [250, 115], [250, 100]]})),
        ("sketch.arc.three_point", json!({"start": [250, 100], "through": [256, 94], "end": [262, 100]})),
    ];
    sketched(&mut s, "Lug", &lug, t);
    hole(&mut s, "Lug", [256.0, 100.0, 0.0], 2.0, t);
    run(&mut s, "solid.cylinder", json!({"radius": 9, "height": t, "center": [300, 100, 0], "body_name": "Washer"}));
    hole(&mut s, "Washer", [300.0, 100.0, 0.0], 4.5, t);
    let parts = ["Frame", "Gusset", "Comb", "Arch", "Tab", "Lug", "Washer"];
    unhurried();
    let started = std::time::Instant::now();
    let r = run(&mut s, "solid.arrange", json!({"bodies": parts, "width": 400, "length": 300, "spacing": 2, "margin": 5, "quantity": 4}));
    let took = started.elapsed();
    if let Some(path) = std::env::var_os("SOLIDCRAFT_ARRANGE_SHOWCASE") {
        run(&mut s, "file.save", json!({"path": format!("{}.laser.sc", path.to_string_lossy())}));
    }
    assert!(names(&r["unplaced"]).is_empty() && r["copies_left"] == 0, "{r}");
    assert!(r.get("boxed").is_none(), "every part has its outline: {r}");
    let placed = names(&r["placed"]);
    assert_eq!(placed.len(), 28, "{r}");
    let all: Vec<&str> = placed.iter().map(String::as_str).collect();
    assert_laid_out(&mut s, &all, [5.0, 5.0, 395.0, 295.0], 2.0);
    for b in &all {
        let (lo, _) = bounds(&s, b);
        assert!(lo[2].abs() < 1e-3, "{b} stands on the sheet");
        let [_, _, z] = spans(&s, b);
        assert!(z <= t + 0.05, "{b} is {z} high");
    }
    assert!(took.as_secs_f64() < 60.0, "arranging 28 parts took {took:?}");
    eprintln!("laser job: 28 parts in {took:?}, fill {}", r["fill"]);
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
    // Over the hole, with a rectangle too, two sheets, an origin. (A side or the underside is a
    // sheet too, at its tilt.)
    for p in [
        json!({"sheet": [50, 30, 0]}),
        json!({"sheet": [5, 5, 0], "width": 10, "length": 10}),
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
fn a_round_face_and_a_hexagon_face_hold_their_parts_inside() {
    // Pucks of radius 8 on a disc's top (radius 60) and a hexagon plate's (apothem 52), 2 apart:
    // each puck's centre is at least 8 inside every edge.
    let pucks: Vec<String> = (0..14).map(|i| format!("P{i}")).collect();
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 60, "height": 10, "center": [0, 0, -10], "body_name": "Disc"}));
    sketched(&mut s, "Hex", &[("sketch.polygon.inscribed", json!({"center": [300, 0], "radius": 60, "sides": 6}))], 5.0);
    run(&mut s, "solid.move", json!({"bodies": ["Hex"], "translate": [0, 0, -5]}));
    for (i, n) in pucks.iter().enumerate() {
        run(&mut s, "solid.cylinder", json!({"radius": 8, "height": 4, "center": [600.0 + 20.0 * i as f64, 0, 30], "body_name": n}));
    }
    let apothem = 60.0 * (std::f64::consts::PI / 6.0).cos();
    for (plate, centre) in [("Disc", [0.0, 0.0]), ("Hex", [300.0, 0.0])] {
        let top = bounds(&s, plate).1[2];
        let sheet = [centre[0] + 5.0, centre[1] + 5.0, top];
        let r = run(&mut s, "solid.arrange", json!({"bodies": pucks, "sheet": sheet, "spacing": 2}));
        assert!(names(&r["placed"]).len() >= 10, "{r}");
        for n in names(&r["placed"]) {
            let (lo, hi) = bounds(&s, &n);
            let c = [(lo[0] + hi[0]) / 2.0 - centre[0], (lo[1] + hi[1]) / 2.0 - centre[1]];
            let inside = if plate == "Disc" {
                60.0 - c[0].hypot(c[1])
            } else {
                (0..6)
                    .map(|k| {
                        apothem
                            - (c[0] * (f64::from(k) * std::f64::consts::FRAC_PI_3).cos() + c[1] * (f64::from(k) * std::f64::consts::FRAC_PI_3).sin())
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            assert!(inside >= 8.0 - 0.05 && (lo[2] - top).abs() < 1e-6, "{n} at {c:?}, {inside} inside, {} over the face", lo[2] - top);
        }
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

/// The point `r` from `c` at angle `a` (radians) from +X.
fn gears_at(c: [f64; 2], r: f64, a: f64) -> [f64; 2] {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

/// A spur gear `t` thick standing on z = 0 at `c`: `teeth` (how many, the root circle's radius,
/// the tip circle's radius) trapezoid teeth, a bore of radius `bore` with a 3 mm keyway, and `holes`
/// lightening holes (how many, the radius of the ring they lie on, their radius).
fn gears_spur(s: &mut Session, name: &str, c: [f64; 2], teeth: (usize, f64, f64), t: f64, bore: f64, holes: (usize, f64, f64)) {
    let (teeth, root, tip) = teeth;
    let p = std::f64::consts::TAU / teeth as f64;
    let mut pts = Vec::with_capacity(4 * teeth);
    for k in 0..teeth {
        let a = k as f64 * p;
        pts.push(gears_at([0.0, 0.0], root, a));
        pts.push(gears_at([0.0, 0.0], tip, a + 0.25 * p));
        pts.push(gears_at([0.0, 0.0], tip, a + 0.5 * p));
        pts.push(gears_at([0.0, 0.0], root, a + 0.75 * p));
    }
    sketched(s, name, &[outline(&pts, c)], t);
    let mut cuts = vec![("sketch.circle.center", json!({"center": c, "radius": bore}))];
    let (count, ring, r) = holes;
    for k in 0..count {
        // Clear of the keyway, which points along +Y.
        let a = std::f64::consts::FRAC_PI_2 + (k as f64 + 0.5) * std::f64::consts::TAU / count as f64;
        cuts.push(("sketch.circle.center", json!({"center": gears_at(c, ring, a), "radius": r})));
    }
    cut_through(s, name, &cuts, t);
    run(
        s,
        "solid.box",
        json!({"length": 3, "width": bore + 2.0, "height": t, "corner": [c[0] - 1.5, c[1], 0], "operation": "cut", "targets": [name]}),
    );
}

/// A chain sprocket `t` thick at `c`: `teeth` roller seats (arcs dipping 2.5 mm under the seat
/// circle `seat`) between pointed teeth out to `tip`, a bore and four lightening holes.
fn gears_sprocket(s: &mut Session, name: &str, c: [f64; 2], teeth: usize, seat: f64, tip: f64, t: f64) {
    let p = std::f64::consts::TAU / teeth as f64;
    let h = 0.2 * p;
    // The angle of tooth `k`'s seat start, the last one ending exactly where the first begins.
    let start = |k: usize| (k % teeth) as f64 * p;
    let mut curves = Vec::with_capacity(2 * teeth);
    for k in 0..teeth {
        let a = gears_at(c, seat, start(k));
        let end = gears_at(c, seat, start(k) + 2.0 * h);
        let next = gears_at(c, seat, start(k + 1));
        curves.push(("sketch.arc.three_point", json!({"start": a, "through": gears_at(c, seat - 2.5, start(k) + h), "end": end})));
        curves.push(("sketch.line", json!({"points": [end, gears_at(c, tip, start(k) + h + 0.5 * p), next]})));
    }
    sketched(s, name, &curves, t);
    let mut cuts = vec![("sketch.circle.center", json!({"center": c, "radius": 10}))];
    for k in 0..4 {
        let a = std::f64::consts::FRAC_PI_4 + k as f64 * std::f64::consts::FRAC_PI_2;
        cuts.push(("sketch.circle.center", json!({"center": gears_at(c, 24.0, a), "radius": 5})));
    }
    cut_through(s, name, &cuts, t);
}

/// The three gears of the train: 36 teeth with six lightening holes, 24 teeth with five, and a
/// 20-tooth pinion with only its bore, each at its own spot on z = 0.
fn gears_train(s: &mut Session) -> [&'static str; 3] {
    gears_spur(s, "Gear36", [150.0, 0.0], (36, 45.0, 49.0), 6.0, 8.0, (6, 28.0, 10.0));
    gears_spur(s, "Gear24", [0.0, 0.0], (24, 30.0, 34.0), 5.0, 6.0, (5, 18.0, 7.0));
    gears_spur(s, "Pinion", [260.0, 0.0], (20, 18.0, 21.0), 4.0, 4.0, (0, 0.0, 0.0));
    ["Gear36", "Gear24", "Pinion"]
}

/// A drive job of many-toothed, many-holed flat parts: a 36-tooth and a 24-tooth spur gear with
/// bores, keyways and lightening holes, a 20-tooth pinion, an 18-tooth sprocket with arc roller
/// seats, a plate perforated by a grid of 40 holes, a six-pointed star knob, a cam with a spline
/// edge, and washers and spacers small enough to sit in the gears' lightening holes. Three of
/// each, 27 parts, lie flat inside a 500 x 400 sheet, 2 mm apart on their exact outlines, nothing
/// placed by its box, in under a minute.
/// With `SOLIDCRAFT_ARRANGE_SHOWCASE` set to a path, the arranged job is saved there too (as
/// `<path>.gears.sc`).
#[test]
fn a_job_of_gears_sprockets_and_perforated_plates_nests_clear() {
    let mut s = Session::default();
    let mut parts: Vec<&str> = gears_train(&mut s).to_vec();
    gears_sprocket(&mut s, "Sprocket", [0.0, 150.0], 18, 40.0, 46.0, 4.0);
    block(&mut s, "Perforated", [100.0, 70.0, 4.0], [120.0, 120.0, 0.0]);
    let mut grid = Vec::new();
    for i in 0..8 {
        for j in 0..5 {
            grid.push(("sketch.circle.center", json!({"center": [131.5 + 11.0 * i as f64, 133.0 + 11.0 * j as f64], "radius": 2.5})));
        }
    }
    cut_through(&mut s, "Perforated", &grid, 4.0);
    let star: Vec<[f64; 2]> = (0..12)
        .map(|k| gears_at([0.0, 0.0], if k % 2 == 0 { 20.0 } else { 12.0 }, std::f64::consts::FRAC_PI_2 + k as f64 * std::f64::consts::PI / 6.0))
        .collect();
    sketched(&mut s, "Star", &[outline(&star, [0.0, 280.0])], 6.0);
    hole(&mut s, "Star", [0.0, 280.0, 0.0], 3.0, 6.0);
    let cam = [
        ("sketch.line", json!({"points": [[85, 280], [115, 280]]})),
        ("sketch.spline.fit_point", json!({"points": [[115, 280], [118, 295], [105, 310], [88, 300], [85, 280]]})),
    ];
    sketched(&mut s, "Cam", &cam, 5.0);
    hole(&mut s, "Cam", [100.0, 290.0, 0.0], 4.0, 5.0);
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 3, "center": [200, 280, 0], "body_name": "Washer"}));
    hole(&mut s, "Washer", [200.0, 280.0, 0.0], 3.0, 3.0);
    run(&mut s, "solid.cylinder", json!({"radius": 4.5, "height": 5, "center": [240, 280, 0], "body_name": "Spacer"}));
    hole(&mut s, "Spacer", [240.0, 280.0, 0.0], 2.5, 5.0);
    parts.extend(["Sprocket", "Perforated", "Star", "Cam", "Washer", "Spacer"]);
    unhurried();
    let started = std::time::Instant::now();
    let r = run(&mut s, "solid.arrange", json!({"bodies": parts, "width": 500, "length": 400, "spacing": 2, "margin": 5, "quantity": 3}));
    let took = started.elapsed();
    if let Some(path) = std::env::var_os("SOLIDCRAFT_ARRANGE_SHOWCASE") {
        run(&mut s, "file.save", json!({"path": format!("{}.gears.sc", path.to_string_lossy())}));
    }
    assert!(names(&r["unplaced"]).is_empty() && r["copies_left"] == 0, "{r}");
    assert!(r.get("boxed").is_none(), "every part has its outline: {r}");
    let placed = names(&r["placed"]);
    assert_eq!(placed.len(), 27, "{r}");
    let all: Vec<&str> = placed.iter().map(String::as_str).collect();
    assert_laid_out(&mut s, &all, [5.0, 5.0, 495.0, 395.0], 2.0);
    for b in &all {
        let (lo, _) = bounds(&s, b);
        assert!(lo[2].abs() < 1e-3, "{b} stands on the sheet");
        let [_, _, z] = spans(&s, b);
        assert!(z <= 6.0 + 0.05, "{b} is {z} high");
    }
    assert!(took.as_secs_f64() < 60.0, "arranging 27 parts took {took:?}");
    eprintln!("gear job: 27 parts in {took:?}, fill {}", r["fill"]);
}

/// Plywood thickness for the router parts, in mm.
const ROUTER_PLY: f64 = 18.0;

/// A point of a router part's sketch, `at` moved.
fn router_p(at: [f64; 2], x: f64, y: f64) -> Value {
    json!([x + at[0], y + at[1]])
}

/// A chair side, 220 wide and 400 tall from `at`: a straight front leg and seat, a back that
/// curves up as an arc, a spline down the back edge, and two mortise slots cut through.
fn router_chair_side(s: &mut Session, name: &str, at: [f64; 2]) {
    let p = |x: f64, y: f64| router_p(at, x, y);
    let side = [
        ("sketch.line", json!({"points": [p(0.0, 0.0), p(220.0, 0.0), p(220.0, 200.0), p(120.0, 200.0)]})),
        ("sketch.arc.three_point", json!({"start": p(120.0, 200.0), "through": p(95.0, 300.0), "end": p(60.0, 400.0)})),
        ("sketch.line", json!({"points": [p(60.0, 400.0), p(20.0, 400.0)]})),
        ("sketch.spline.fit_point", json!({"points": [p(20.0, 400.0), p(22.0, 300.0), p(6.0, 150.0), p(0.0, 0.0)]})),
    ];
    sketched(s, name, &side, ROUTER_PLY);
    let mortises = [
        ("sketch.slot.center_to_center", json!({"p0": p(40.0, 100.0), "p1": p(40.0, 160.0), "width": 18})),
        ("sketch.slot.center_to_center", json!({"p0": p(90.0, 180.0), "p1": p(170.0, 180.0), "width": 12})),
    ];
    cut_through(s, name, &mortises, ROUTER_PLY);
}

/// A 400 x 160 shelf from `at` with two dados 18 wide and 6 deep across its top.
fn router_shelf(s: &mut Session, name: &str, at: [f64; 2]) {
    block(s, name, [400.0, 160.0, ROUTER_PLY], [at[0], at[1], 0.0]);
    for x in [90.0, 292.0] {
        let corner = [at[0] + x, at[1], ROUTER_PLY - 6.0];
        run(s, "solid.box", json!({"length": 18, "width": 160, "height": 6, "corner": corner, "operation": "cut", "targets": [name]}));
    }
}

/// A 220 x 160 panel from `at` with rounded corners, a cut-out through it and a pocket 8 deep
/// in its top, both with rounded inside corners.
fn router_panel(s: &mut Session, name: &str, at: [f64; 2]) {
    let [ax, ay] = at;
    let mid = ROUTER_PLY / 2.0;
    block(s, name, [220.0, 160.0, ROUTER_PLY], [ax, ay, 0.0]);
    let corners: Vec<Value> = [[0.0, 0.0], [220.0, 0.0], [0.0, 160.0], [220.0, 160.0]].iter().map(|[x, y]| json!([ax + x, ay + y, mid])).collect();
    run(s, "solid.fillet", json!({"body": name, "radius": 10, "edges": corners}));
    run(
        s,
        "solid.box",
        json!({"length": 90, "width": 60, "height": ROUTER_PLY, "corner": [ax + 20.0, ay + 50.0, 0], "operation": "cut", "targets": [name]}),
    );
    let inside: Vec<Value> = [[20.0, 50.0], [110.0, 50.0], [20.0, 110.0], [110.0, 110.0]].iter().map(|[x, y]| json!([ax + x, ay + y, mid])).collect();
    run(s, "solid.fillet", json!({"body": name, "radius": 6, "edges": inside}));
    run(
        s,
        "solid.box",
        json!({"length": 70, "width": 60, "height": 8, "corner": [ax + 130.0, ay + 50.0, ROUTER_PLY - 8.0], "operation": "cut", "targets": [name]}),
    );
    let pocket: Vec<Value> =
        [[130.0, 50.0], [200.0, 50.0], [130.0, 110.0], [200.0, 110.0]].iter().map(|[x, y]| json!([ax + x, ay + y, ROUTER_PLY - 4.0])).collect();
    run(s, "solid.fillet", json!({"body": name, "radius": 5, "edges": pocket}));
}

/// No two of `bodies` overlap or come closer than `spacing`, on their exact faces.
fn router_assert_clear(s: &mut Session, bodies: &[&str], spacing: f64) {
    let r = run(s, "inspect.clearance", json!({"bodies": bodies, "clearance": (spacing - 1e-4).max(0.0)}));
    assert!(r["overlaps"].as_array().is_some_and(Vec::is_empty), "{r}");
    assert!(r["near"].as_array().is_some_and(Vec::is_empty), "{r}");
}

/// The same kind of plywood parts laid out on the top of a 1000 x 700 board tilted 25 degrees
/// about X, used as the sheet: the chair side stood up and the tilted panel are laid down on the
/// board's face, every part lies flat in its plane inside it, and no two parts overlap or come
/// closer than the spacing.
/// With `SOLIDCRAFT_ARRANGE_SHOWCASE` set to a path, the arranged job is saved there too (as
/// `<path>.router-tilted.sc`).
#[test]
fn router_parts_lie_flat_on_a_tilted_board() {
    let mut s = Session::default();
    let (w, l) = (1000.0, 700.0);
    block(&mut s, "Board", [w, l, ROUTER_PLY], [0.0, 0.0, 0.0]);
    run(&mut s, "solid.move", json!({"bodies": ["Board"], "axis": [1, 0, 0], "angle": "25 deg"}));
    let (c, sn) = (25f64.to_radians().cos(), 25f64.to_radians().sin());
    // The board's top, turned: its normal and a point in its middle.
    let n = [0.0, -sn, c];
    let top = [w / 2.0, l / 2.0 * c - ROUTER_PLY * sn, l / 2.0 * sn + ROUTER_PLY * c];
    router_chair_side(&mut s, "ChairSide", [1500.0, 0.0]);
    run(&mut s, "solid.move", json!({"bodies": ["ChairSide"], "axis": [1, 0, 0], "angle": "90 deg"}));
    router_shelf(&mut s, "Shelf", [1800.0, 0.0]);
    router_panel(&mut s, "Panel", [1500.0, 500.0]);
    run(&mut s, "solid.move", json!({"bodies": ["Panel"], "axis": [0.3, 1, 0], "angle": "40 deg", "translate": [0, 0, 150]}));
    let parts = ["ChairSide", "Shelf", "Panel"];
    let r = run(&mut s, "solid.arrange", json!({"bodies": parts, "sheet": top, "spacing": 6, "quantity": 2}));
    if let Some(path) = std::env::var_os("SOLIDCRAFT_ARRANGE_SHOWCASE") {
        run(&mut s, "file.save", json!({"path": format!("{}.router-tilted.sc", path.to_string_lossy())}));
    }
    assert!(names(&r["unplaced"]).is_empty() && r["copies_left"] == 0, "{r}");
    assert!(r["sheet_frame"].is_array(), "{r}");
    let placed = names(&r["placed"]);
    assert_eq!(placed.len(), 6, "{r}");
    let all: Vec<&str> = placed.iter().map(String::as_str).collect();
    let height = n[1] * top[1] + n[2] * top[2];
    for b in &all {
        // Lying flat on the board: its middle half its thickness out of the face, and over it.
        let m = middle(&s, b);
        let out = n[1] * m[1] + n[2] * m[2] - height;
        assert!((out - ROUTER_PLY / 2.0).abs() < 0.05, "{b} is {out} off the board: {r}");
        let along = c * m[1] + sn * m[2];
        assert!(m[0] > 0.0 && m[0] < w && along > 0.0 && along < l, "{b} is not over the board: {m:?}");
    }
    let mut with_board = all.clone();
    with_board.push("Board");
    let c2 = run(&mut s, "inspect.clearance", json!({"bodies": with_board, "clearance": 0.5}));
    assert!(c2["overlaps"].as_array().is_some_and(Vec::is_empty), "{c2}");
    router_assert_clear(&mut s, &all, 6.0);
}

/// However many parts it is given, a layout is done in about six seconds: past its budget it
/// hurries, packs what is left by its box, then leaves the rest unplaced and says so. What it
/// placed is still on the sheets and clear of each other.
#[test]
fn a_huge_job_keeps_to_the_time_budget() {
    let mut s = Session::default();
    let parts = mixed_parts(&mut s);
    // The budget run five times faster, so the test takes seconds, not half a minute.
    let scale = 0.2;
    super::BUDGET_SCALE.with(|c| c.set(scale));
    let limit = (super::BUDGET.stop + super::BUDGET.repair) * scale;
    let started = std::time::Instant::now();
    let r =
        run(&mut s, "solid.arrange", json!({"bodies": parts, "width": 400, "length": 300, "spacing": 3, "margin": 5, "quantity": 60, "sheets": 4}));
    let took = started.elapsed();
    assert!(took.as_secs_f64() < limit + 1.5, "arranging 540 parts took {took:?}, over the budget's {limit} s");
    // A layout that ran past the point of hurrying says so.
    if took.as_secs_f64() > super::BUDGET.stop * scale {
        assert_eq!(r["hurried"], true, "{r}");
    }
    let placed = names(&r["placed"]);
    assert!(!placed.is_empty(), "{r}");
    assert!(r["placed"].as_array().into_iter().flatten().all(|p| p["sheet"].as_u64().is_some_and(|k| k < 4)), "{r}");
    eprintln!("huge job: {} of 540 parts in {took:?}", placed.len());
}

/// A plate in a component, placed in the design, and a block of the design's own.
fn plate_in_a_component(s: &mut Session) -> String {
    run(s, "component.create", json!({"name": "Plate"}));
    run(s, "solid.box", json!({"length": 40, "width": 20, "height": 3, "corner": [0, -60, 0]}));
    run(s, "component.activate", json!({"component": 0}));
    block(s, "Block", [10.0, 10.0, 10.0], [0.0, 60.0, 0.0]);
    let st = s.world_state().unwrap_or_else(|e| panic!("{e}"));
    st.bodies.iter().map(|b| b.name.clone()).find(|n| n != "Block").unwrap_or_else(|| panic!("no plate"))
}

#[test]
fn a_body_in_a_component_is_refused_without_keep_originals() {
    let mut s = Session::default();
    let plate = plate_in_a_component(&mut s);
    let before = bounds(&s, &plate);
    let e = s.execute("solid.arrange", &json!({"bodies": ["Block", plate], "width": 200, "length": 200})).err().map(|e| e.to_string());
    let e = e.unwrap_or_else(|| panic!("arranged a body of a component"));
    assert!(e.contains(&plate) && e.contains("Keep Originals") && !e.contains("`Block`"), "{e}");
    // Nothing was done: the block did not move either.
    assert_eq!(bounds(&s, &plate), before);
    assert_eq!(bounds(&s, "Block").0, [0.0, 60.0, 0.0]);
}

#[test]
fn keep_originals_copies_a_component_body_into_an_arrangement_group() {
    let mut s = Session::default();
    let plate = plate_in_a_component(&mut s);
    let before = bounds(&s, &plate);
    let r =
        run(&mut s, "solid.arrange", json!({"bodies": ["Block", plate.clone()], "width": 200, "length": 200, "origin": [100, 100], "copy": true}));
    let placed: Vec<&Value> = r["placed"].as_array().into_iter().flatten().collect();
    assert_eq!(placed.len(), 2, "{r}");
    assert!(placed.iter().all(|p| p["copy_of"].is_string()), "{r}");
    // The originals stay; the copies are plain bodies of the design, laid out and clear.
    assert_eq!(bounds(&s, &plate), before);
    assert_eq!(bounds(&s, "Block").0, [0.0, 60.0, 0.0]);
    let copies: Vec<&str> = placed.iter().filter_map(|p| p["body"].as_str()).collect();
    assert_laid_out(&mut s, &copies, [100.0, 100.0, 300.0, 300.0], 1.0);
    let st = s.model.state();
    assert!(copies.iter().all(|c| st.body(c).is_some()), "the copies are the design's own bodies");
    drop(st);
    assert_eq!(r["copy_groups"], json!(["Arrangement 1"]), "{r}");
    let group = s.doc.browser_groups.iter().find(|g| g.name == "Arrangement 1");
    assert!(group.is_some_and(|g| g.items.len() == 2), "{:?}", s.doc.browser_groups.iter().map(|g| &g.name).collect::<Vec<_>>());
    // One step: one undo takes it all back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.world_state().map(|w| w.bodies.len()).unwrap_or(0), 2);
}
