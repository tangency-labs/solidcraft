//! Relations (parallel, perpendicular, angle, point on plane, point on line, line on plane): each
//! ties only part of a placement, several between two parts add up, they start from where the
//! parts are, and they keep holding as the parts change size.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use super::tests::{occ, place, run};
use crate::Session;

/// A 2 mm panel `W` x 300 (W = 400) in component Panel and a 20 x 10 x 6 bracket in component
/// Bracket off to the side, with named origins on both made where they are; then the bracket is
/// lifted, turned and tilted, as a part dropped in anywhere.
fn panel() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "400 mm"}));
    run(&mut s, "component.create", json!({"name": "Panel"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 300, "height": 2, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 6, "corner": [0, 500, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let panel = occ(&s, "Panel");
    run(&mut s, "occurrence.ground", json!({"occurrence": panel, "grounded": true}));
    for (name, o, k, at) in [
        ("Top", "Panel", "face", [100.0, 100.0, 2.0]),
        ("Front", "Panel", "face", [100.0, 0.0, 1.0]),
        ("Right", "Panel", "face", [400.0, 100.0, 1.0]),
        ("Bottom", "Bracket", "face", [10.0, 505.0, 0.0]),
        ("Face", "Bracket", "face", [10.0, 500.0, 3.0]),
        ("Corner", "Bracket", "point", [20.0, 500.0, 0.0]),
    ] {
        run(&mut s, "joint.origin", json!({"name": name, "occurrence": o, k: at}));
    }
    let b = occ(&s, "Bracket");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [30, -120, 50]}));
    run(&mut s, "occurrence.move", json!({"occurrence": b, "axis": [0, 0, 1], "angle": "25 deg", "origin": [40, 385, 50]}));
    run(&mut s, "occurrence.move", json!({"occurrence": b, "axis": [1, 0, 0], "angle": "10 deg", "origin": [40, 385, 50]}));
    s
}

fn relation(s: &mut Session, kind: &str, a: &str, b: &str, extra: Value) -> Value {
    let mut p = json!({"type": kind, "a": a, "b": b, "name": format!("{kind} {a}-{b}")});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    run(s, "joint.create", p)
}

fn dof(r: &Value, s: &Session) -> Option<u64> {
    let b = occ(s, "Bracket");
    r["dof"].as_array()?.iter().find(|d| d["occurrence"] == json!(b))?["dof"].as_u64()
}

fn near(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.dist(b) < tol
}

fn clean(r: &Value) -> bool {
    r["conflicts"] == json!([]) && r["errors"] == json!([])
}

#[test]
fn closing_planar_joint_starts_from_the_current_placement() {
    let mut s = Session::default();
    for name in ["Base", "Part"] {
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 6}));
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "planar", "name": "First", "flip": true, "angle": 0,
            "a": {"occurrence": "Base", "point": [0, 0, 0], "x": [1, 0, 0]},
            "b": {"occurrence": "Part", "point": [0, 0, 0], "x": [1, 0, 0]}, "values": [50, 30, 120]}),
    );
    assert!(clean(&r), "{r}");
    // Check a frame, so a turn as well as a translation is observable.
    let points = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
    let before = points.map(|p| place(&s, "Part", p));
    let at = before[0];
    let x = before[1] - at;
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "planar", "name": "Closing", "flip": true, "angle": 0,
            "a": {"occurrence": "Base", "point": [0, 0, 0], "x": [1, 0, 0]},
            "b": {"occurrence": "Part", "point": [at.x, at.y, at.z], "x": [x.x, x.y, x.z]}}),
    );
    assert!(clean(&r), "{r}");
    for (p, want) in points.into_iter().zip(before) {
        let got = place(&s, "Part", p);
        assert!(near(got, want, 1e-6), "{p:?}: {got:?}, expected {want:?}");
    }
}

#[test]
fn stacked_relations_place_a_part_and_follow_the_panel() {
    let mut s = panel();
    // Flat on the top (parallel, and a corner on it), its face along the panel's front edge, the
    // corner 40 mm in from the front and the right end.
    let r = relation(&mut s, "parallel", "Top", "Bottom", json!({}));
    assert_eq!(dof(&r, &s), Some(4), "{r}");
    let r = relation(&mut s, "point_on_plane", "Top", "Corner", json!({}));
    assert_eq!(dof(&r, &s), Some(3), "{r}");
    let r = relation(&mut s, "parallel", "Front", "Face", json!({"flip": true}));
    assert_eq!(dof(&r, &s), Some(2), "{r}");
    let r = relation(&mut s, "point_on_plane", "Front", "Corner", json!({"offset": -40}));
    assert_eq!(dof(&r, &s), Some(1), "{r}");
    let r = relation(&mut s, "point_on_plane", "Right", "Corner", json!({"offset": -40}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof(&r, &s), Some(0), "{r}");
    for w in [400.0, 650.0, 250.0, 1000.0] {
        run(&mut s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
        let r = run(&mut s, "joint.solve", json!({}));
        assert!(clean(&r), "W {w}: {r}");
        // The corner, the bracket's other bottom corner along its face, and its top: flat, square
        // and 40 mm in.
        assert!(near(place(&s, "Bracket", Vec3::new(20.0, 500.0, 0.0)), Vec3::new(w - 40.0, 40.0, 2.0), 1e-6), "W {w}");
        assert!(near(place(&s, "Bracket", Vec3::new(0.0, 500.0, 0.0)), Vec3::new(w - 60.0, 40.0, 2.0), 1e-6), "W {w}");
        assert!(near(place(&s, "Bracket", Vec3::new(20.0, 510.0, 6.0)), Vec3::new(w - 40.0, 50.0, 8.0), 1e-6), "W {w}");
    }
}

#[test]
fn a_relation_moves_the_part_no_further_than_it_must() {
    let mut s = panel();
    let corner = |s: &Session| place(s, "Bracket", Vec3::new(20.0, 500.0, 0.0));
    let along = |s: &Session| place(s, "Bracket", Vec3::new(0.0, 510.0, 6.0)) - corner(s);
    let (before, turn) = (corner(&s), along(&s));
    let r = relation(&mut s, "point_on_plane", "Top", "Corner", json!({}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof(&r, &s), Some(5), "{r}");
    // Straight down onto the top, turned as it was.
    assert!(near(corner(&s), Vec3::new(before.x, before.y, 2.0), 1e-6), "{:?} from {before:?}", corner(&s));
    assert!(near(along(&s), turn, 1e-6), "{:?} from {turn:?}", along(&s));
    // Parallel to the top: it only turns level, about the corner it keeps.
    let r = relation(&mut s, "parallel", "Top", "Bottom", json!({}));
    assert!(clean(&r), "{r}");
    let up = place(&s, "Bracket", Vec3::new(20.0, 500.0, 6.0)) - corner(&s);
    assert!(near(up, Vec3::new(0.0, 0.0, 6.0), 1e-6), "{up:?}");
    assert!((corner(&s).z - 2.0).abs() < 1e-6, "{:?}", corner(&s));
}

#[test]
fn an_angle_relation_holds_its_tilt_and_follows_an_edit() {
    let mut s = panel();
    relation(&mut s, "point_on_plane", "Top", "Corner", json!({}));
    let tilt = |s: &Session| {
        let n = place(s, "Bracket", Vec3::new(10.0, 505.0, 0.0)) - place(s, "Bracket", Vec3::new(10.0, 505.0, 1.0));
        n.normalized().map_or(f64::NAN, |n| n.dot(Vec3::Z).acos().to_degrees())
    };
    // 0 deg is mated: the bracket's bottom against the top, its outward normal straight down.
    let r = relation(&mut s, "angle", "Top", "Bottom", json!({"angle": "30 deg"}));
    assert!(clean(&r), "{r}");
    assert!((tilt(&s) - 150.0).abs() < 1e-6, "{}", tilt(&s));
    run(&mut s, "joint.edit", json!({"joint": "angle Top-Bottom", "angle": "60 deg"}));
    assert!((tilt(&s) - 120.0).abs() < 1e-6, "{}", tilt(&s));
    run(&mut s, "joint.edit", json!({"joint": "angle Top-Bottom", "type": "perpendicular"}));
    assert!((tilt(&s) - 90.0).abs() < 1e-6, "{}", tilt(&s));
    // The corner stayed on the top through every change.
    assert!((place(&s, "Bracket", Vec3::new(20.0, 500.0, 0.0)).z - 2.0).abs() < 1e-6);
}

#[test]
fn a_rod_lies_tangent_on_the_panel_and_rolls_along_it() {
    let mut s = panel();
    run(&mut s, "component.create", json!({"name": "Rod"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 700, 40], "axis": [1, 0, 0], "radius": 5, "height": 80, "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "joint.origin", json!({"name": "Axis", "occurrence": "Rod", "circle": [0, 700, 40]}));
    let r = relation(&mut s, "line_on_plane", "Top", "Axis", json!({"offset": 5}));
    assert!(clean(&r), "{r}");
    let rod = occ(&s, "Rod");
    let d = r["dof"].as_array().and_then(|a| a.iter().find(|d| d["occurrence"] == json!(rod))).and_then(|d| d["dof"].as_u64());
    assert_eq!(d, Some(4), "{r}");
    // Its axis level, 5 mm over the top: it touches it along a line.
    let (a, b) = (place(&s, "Rod", Vec3::new(0.0, 700.0, 40.0)), place(&s, "Rod", Vec3::new(80.0, 700.0, 40.0)));
    assert!((a.z - 7.0).abs() < 1e-6 && (b.z - 7.0).abs() < 1e-6, "{a:?} {b:?}");
    // Rolled along: still tangent.
    run(&mut s, "joint.drive", json!({"joint": "line_on_plane Top-Axis", "values": [100, 50, "30 deg", "90 deg"]}));
    let (a, b) = (place(&s, "Rod", Vec3::new(0.0, 700.0, 40.0)), place(&s, "Rod", Vec3::new(80.0, 700.0, 40.0)));
    assert!((a.z - 7.0).abs() < 1e-6 && (b.z - 7.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn relations_that_disagree_are_reported_not_forced() {
    let mut s = panel();
    relation(&mut s, "point_on_plane", "Right", "Corner", json!({"offset": -40}));
    let r = relation(&mut s, "point_on_plane", "Right", "Corner", json!({"offset": -60}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(1), "{r}");
    // A point on the panel's front plane's line: still placed, every number finite.
    let r = relation(&mut s, "point_on_line", "Front", "Corner", json!({}));
    assert!(r["dof"].is_array(), "{r}");
    let p = place(&s, "Bracket", Vec3::new(20.0, 500.0, 0.0));
    assert!(p.is_finite(), "{p:?}");
    // Saved and read back, the relations are the same.
    let saved = serde_json::to_string(&s.doc.assembly.joints).unwrap_or_default();
    let back: Vec<solidcraft_doc::joints::Joint> = serde_json::from_str(&saved).unwrap_or_default();
    assert_eq!(back, s.doc.assembly.joints);
    assert!(saved.contains("\"point_on_plane\"") && saved.contains("\"point_on_line\""), "{saved}");
}

/// More parts placed by relations than one group of loops may hold (each relation after a part's
/// first closes a loop): every group is small, so every part is placed.
#[test]
fn a_panel_full_of_parts_placed_by_relations_stays_solved() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "1500 mm"}));
    run(&mut s, "component.create", json!({"name": "Panel"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 300, "height": 2, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let panel = occ(&s, "Panel");
    run(&mut s, "occurrence.ground", json!({"occurrence": panel, "grounded": true}));
    for (name, at) in [("Top", [100.0, 100.0, 2.0]), ("Front", [100.0, 0.0, 1.0]), ("Right", [1500.0, 100.0, 1.0])] {
        run(&mut s, "joint.origin", json!({"name": name, "occurrence": "Panel", "face": at}));
    }
    // 30 brackets, 5 relations each: 120 loops, more than the 100 one group may hold. The joints
    // go straight into the design and are solved once (made one by one, each would solve all).
    let n = 30usize;
    for k in 0..n {
        let (c, y) = (format!("B{k}"), 500.0 + 30.0 * k as f64);
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": c}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 6, "corner": [0, y, 0]}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        for (o, k2, at) in [("b", "face", [10.0, y + 5.0, 0.0]), ("f", "face", [10.0, y, 3.0]), ("c", "point", [20.0, y, 0.0])] {
            run(&mut s, "joint.origin", json!({"name": format!("{c}{o}"), "occurrence": c, k2: at}));
        }
        let x = 40.0 * (k + 1) as f64;
        for (kind, a, b, flip, offset) in [
            ("parallel", "Top", "b", false, 0.0),
            ("point_on_plane", "Top", "c", false, 0.0),
            ("parallel", "Front", "f", true, 0.0),
            ("point_on_plane", "Front", "c", false, -40.0),
            ("point_on_plane", "Right", "c", false, -x),
        ] {
            let origin = |n: &str| s.doc.assembly.origins.iter().find(|o| o.name == n).map(|o| o.origin.clone());
            let (Some(a), Some(b)) = (origin(a), origin(&format!("{c}{b}"))) else { panic!("origins") };
            let id = s.doc.assembly.joints.iter().map(|j| j.id).max().unwrap_or(0) + 1;
            let kind = solidcraft_doc::joints::JointKind::parse(kind).unwrap_or(solidcraft_doc::joints::JointKind::Rigid);
            let mut j = solidcraft_doc::joints::Joint {
                id,
                name: format!("R{id}"),
                kind,
                a,
                b,
                values: vec![0.0; kind.dofs().len()],
                limits: vec![None; kind.dofs().len()],
                flip,
                offset,
                angle: 0.0,
                suppressed: false,
                exprs: Default::default(),
                captured: None,
                constraint: None,
            };
            if let Some(q) = solidcraft_doc::joints::relation_values(&s.doc, &s.model.state(), &j) {
                j.values = q;
            }
            s.doc_mut().assembly.joints.push(j);
        }
    }
    for w in [1500.0, 1800.0] {
        run(&mut s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
        let r = run(&mut s, "joint.solve", json!({}));
        assert!(clean(&r), "W {w}: {r}");
        for k in [0, n / 2, n - 1] {
            let (c, y) = (format!("B{k}"), 500.0 + 30.0 * k as f64);
            let at = place(&s, &c, Vec3::new(20.0, y, 0.0));
            assert!(near(at, Vec3::new(w - 40.0 * (k + 1) as f64, 40.0, 2.0), 1e-6), "W {w} {c}: {at:?}");
        }
    }
}

/// A leaf standing on a grounded base, on a hinge along x through its bottom edge, and an angle
/// relation between the base's top and the leaf's side (made at 120 degrees: the leaf turned 30
/// degrees on the hinge). The relation's angle is the leaf's turn plus 90 degrees.
fn hinged_leaf() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Leaf"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 10, "height": 60, "corner": [0, 45, 10]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    run(&mut s, "joint.origin", json!({"name": "Top", "occurrence": "Base", "face": [50.0, 20.0, 10.0]}));
    run(&mut s, "joint.origin", json!({"name": "Side", "occurrence": "Leaf", "face": [50.0, 55.0, 40.0]}));
    run(
        &mut s,
        "joint.as_built",
        json!({"type": "revolute", "name": "Hinge", "a": "Base", "b": "Leaf", "at": {"occurrence": 0, "point": [0, 50, 10], "z": [1, 0, 0]}}),
    );
    let r = relation(&mut s, "angle", "Top", "Side", json!({"angle": "120 deg"}));
    assert!(clean(&r), "{r}");
    s
}

/// How far the leaf is turned on the hinge, in degrees: its side's normal about x.
fn leaf_turn(s: &Session) -> f64 {
    let n = place(s, "Leaf", Vec3::new(50.0, 56.0, 40.0)) - place(s, "Leaf", Vec3::new(50.0, 55.0, 40.0));
    n.z.atan2(n.y).to_degrees()
}

/// An angle relation on a hinged part: as its angle changes the part keeps to its side of the
/// hinge, and past 180 degrees it carries on round instead of coming back the way it went. So it
/// does edit after edit, and from one start as the Edit Joint dialog previews each value, also
/// from straight up, where either side is as near (#12426).
#[test]
fn an_angle_relation_on_a_hinge_keeps_its_side_and_carries_on_past_180() {
    let mut s = hinged_leaf();
    assert!((leaf_turn(&s) - 30.0).abs() < 1e-4, "{}", leaf_turn(&s));
    let edit = |s: &mut Session, angle: i32| {
        let r = run(s, "joint.edit", json!({"joint": "angle Top-Side", "angle": format!("{angle} deg")}));
        assert!(clean(&r), "{angle} deg: {r}");
    };
    // From 120 degrees each time, as the dialog previews: no value comes back the other way.
    let start = s.scratch();
    for angle in [140, 170, 190, 200, 260] {
        let mut t = start.scratch();
        edit(&mut t, angle);
        let want = f64::from(angle - 90);
        assert!((leaf_turn(&t) - want).abs() < 1e-4, "from 120 to {angle} deg: turned {}, not {want}", leaf_turn(&t));
    }
    // Edit after edit, through straight up (180) and on.
    for angle in [130, 140, 150, 160, 170, 180, 190, 200, 260] {
        edit(&mut s, angle);
        let want = f64::from(angle - 90);
        assert!((leaf_turn(&s) - want).abs() < 1e-4, "{angle} deg: turned {}, not {want}", leaf_turn(&s));
    }
    // From straight up each time: every value on the same side.
    edit(&mut s, 180);
    let straight = s.scratch();
    let angles = [140, 150, 160, 170];
    let sides: Vec<f64> = angles
        .into_iter()
        .map(|angle| {
            let mut t = straight.scratch();
            edit(&mut t, angle);
            leaf_turn(&t) - 90.0
        })
        .collect();
    for (d, angle) in sides.iter().zip(angles) {
        assert!((d.abs() - f64::from(180 - angle)).abs() < 1e-4, "{angle} deg from straight up: {sides:?}");
    }
    assert!(sides.iter().all(|d| d.signum() == sides[0].signum()), "one side from straight up: {sides:?}");
}

/// A suppressed Angle relation whose part was moved meanwhile starts again from where the part is
/// when its angle is edited, so turning it back on doesn't carry the part back to where it was:
/// with the angle edited first and the joint unsuppressed after, or both in one edit.
#[test]
fn an_angle_edit_while_the_joint_is_suppressed_starts_from_where_the_part_is() {
    let side = |s: &Session| place(s, "Leaf", Vec3::new(50.0, 55.0, 40.0));
    for together in [false, true] {
        let mut s = hinged_leaf();
        run(&mut s, "joint.delete", json!({"joint": "Hinge"}));
        run(&mut s, "joint.edit", json!({"joint": "angle Top-Side", "suppressed": true}));
        let leaf = occ(&s, "Leaf");
        run(&mut s, "occurrence.move", json!({"occurrence": leaf, "translate": [0, 0, 50]}));
        let moved = side(&s);
        if together {
            run(&mut s, "joint.edit", json!({"joint": "angle Top-Side", "angle": "130 deg", "suppressed": false}));
        } else {
            run(&mut s, "joint.edit", json!({"joint": "angle Top-Side", "angle": "130 deg"}));
            run(&mut s, "joint.edit", json!({"joint": "angle Top-Side", "suppressed": false}));
        }
        assert!(near(side(&s), moved, 1e-6), "together {together}: {:?}, moved to {moved:?}", side(&s));
    }
}

// Assembly constraints (`assembly.constraint.create`): the rules in the Constrain dialog's words,
// kept as joints of the kinds they need and tagged with the rule.

fn constrain(s: &mut Session, kind: &str, a: Value, b: Value, extra: Value) -> Value {
    let mut p = json!({"type": kind, "a": a, "b": b});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    run(s, "assembly.constraint.create", p)
}

fn dof_of(r: &Value, s: &Session, component: &str) -> Option<u64> {
    let o = occ(s, component);
    r["dof"].as_array()?.iter().find(|d| d["occurrence"] == json!(o))?["dof"].as_u64()
}

/// Mate, flush and a distance place the bracket fully, in the panel's words, and it follows the
/// panel as it changes size: its bottom on the top, its front flush with the panel's front, its
/// right end 40 mm in from the panel's.
#[test]
fn mate_flush_and_distance_place_a_part_and_follow_the_panel() {
    let mut s = panel();
    let r = constrain(&mut s, "mate", json!("Top"), json!("Bottom"), json!({}));
    assert_eq!(dof(&r, &s), Some(3), "{r}");
    assert_eq!(r["name"], "Mate1");
    let r = constrain(&mut s, "flush", json!("Front"), json!("Face"), json!({}));
    assert_eq!(dof(&r, &s), Some(1), "{r}");
    let r = constrain(&mut s, "distance", json!("Right"), json!("Corner"), json!({"offset": -40}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof(&r, &s), Some(0), "{r}");
    for w in [400.0, 650.0, 250.0] {
        run(&mut s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
        let r = run(&mut s, "joint.solve", json!({}));
        assert!(clean(&r), "W {w}: {r}");
        assert!(near(place(&s, "Bracket", Vec3::new(20.0, 500.0, 0.0)), Vec3::new(w - 40.0, 0.0, 2.0), 1e-6), "W {w}");
        assert!(near(place(&s, "Bracket", Vec3::new(0.0, 500.0, 0.0)), Vec3::new(w - 60.0, 0.0, 2.0), 1e-6), "W {w}");
        assert!(near(place(&s, "Bracket", Vec3::new(20.0, 510.0, 6.0)), Vec3::new(w - 40.0, 10.0, 8.0), 1e-6), "W {w}");
    }
    // Each is a joint of the kind its rule needs, tagged with the rule.
    let list = run(&mut s, "joint.list", json!({}));
    let kinds: Vec<(String, String)> = list["joints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| (j["type"].as_str().unwrap().to_string(), j["constraint"].as_str().unwrap_or("").to_string()))
        .collect();
    assert_eq!(kinds, [("planar".into(), "mate".into()), ("planar".into(), "flush".into()), ("point_on_plane".into(), "distance".into())]);
    let inspect = run(&mut s, "joint.inspect", json!({"joint": "Flush1"}));
    assert_eq!(inspect["joints"][0]["constraint"], "flush", "{inspect}");
    // Saved and reopened, the rules stay; a joint made as a joint has none.
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let rules: Vec<_> = back.assembly.joints.iter().map(|j| j.constraint).collect();
    use solidcraft_doc::joints::Constraint;
    assert_eq!(rules, [Some(Constraint::Mate), Some(Constraint::Flush), Some(Constraint::Distance)]);
    relation(&mut s, "parallel", "Top", "Bottom", json!({}));
    assert!(!s.doc.to_json().contains("\"constraint\":null"));
    assert_eq!(s.doc.assembly.joints.last().unwrap().constraint, None);
}

/// A mate moves the part only across the gap: a bracket flat above the panel drops straight onto
/// it, not turned and not slid.
#[test]
fn a_mate_moves_the_part_only_across_the_gap() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [30, 40, 50]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let plate = occ(&s, "Plate");
    run(&mut s, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
    let corners = [Vec3::new(30.0, 40.0, 50.0), Vec3::new(50.0, 40.0, 50.0), Vec3::new(30.0, 60.0, 70.0)];
    let r = constrain(
        &mut s,
        "mate",
        json!({"occurrence": "Plate", "face": [5, 5, 10]}),
        json!({"occurrence": "Block", "face": [31, 41, 50]}),
        json!({}),
    );
    assert!(clean(&r), "{r}");
    assert_eq!(dof_of(&r, &s, "Block"), Some(3), "{r}");
    for c in corners {
        let got = place(&s, "Block", c);
        assert!(near(got, c - Vec3::new(0.0, 0.0, 40.0), 1e-6), "{c:?}: {got:?}");
    }
    // A gap: 5 mm above the plate.
    let r = run(&mut s, "joint.edit", json!({"joint": "Mate1", "offset": 5}));
    assert!(clean(&r), "{r}");
    assert!(near(place(&s, "Block", corners[0]), Vec3::new(30.0, 40.0, 15.0), 1e-6));
}

/// A part's own datum plane and axis are references too: a block mated onto the plate's XY plane
/// drops onto it, and a point on the plate's Z axis brings its corner onto that axis. Insert and
/// tangent need a rim or a radius, which an axis has not.
#[test]
fn datum_planes_and_axes_take_constraints() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [30, 40, 50]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let plate = occ(&s, "Plate");
    run(&mut s, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
    let xy = json!({"occurrence": "Plate", "plane": "XY"});
    let z = json!({"occurrence": "Plate", "axis": "Z"});
    let bottom = json!({"occurrence": "Block", "face": [31, 41, 50]});
    let corner = json!({"occurrence": "Block", "point": [30, 40, 50]});
    let r = constrain(&mut s, "mate", xy.clone(), bottom.clone(), json!({}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof_of(&r, &s, "Block"), Some(3), "{r}");
    assert!(near(place(&s, "Block", Vec3::new(30.0, 40.0, 50.0)), Vec3::new(30.0, 40.0, 0.0), 1e-6));
    assert!(near(place(&s, "Block", Vec3::new(50.0, 60.0, 70.0)), Vec3::new(50.0, 60.0, 20.0), 1e-6));
    let r = constrain(&mut s, "point_on_axis", z.clone(), corner.clone(), json!({}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof_of(&r, &s, "Block"), Some(1), "{r}");
    assert!(near(place(&s, "Block", Vec3::new(30.0, 40.0, 50.0)), Vec3::ZERO, 1e-6));
    for (kind, a, b, says) in [
        ("insert", z.clone(), z.clone(), "two round edges"),
        ("tangent", xy.clone(), z.clone(), "a planar face and a round face"),
        ("distance", xy.clone(), z.clone(), "a planar face and a point"),
    ] {
        let p = json!({"type": kind, "a": a, "b": b, "offset": 1});
        let e = s.execute("assembly.constraint.create", &p).expect_err(&p.to_string()).to_string();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(s.doc.assembly.joints.len(), 2);
}

/// A shaft and a collar: two round faces. Concentric leaves a slide and a turn; Insert only the
/// turn, with the collar's end on the shaft's.
#[test]
fn concentric_and_insert_leave_a_slide_and_a_turn_or_a_turn() {
    let shaft = || {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Shaft"}));
        run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "axis": [0, 0, 1], "radius": 5, "height": 20, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": "Collar"}));
        run(&mut s, "solid.cylinder", json!({"base": [50, 10, 30], "axis": [0, 0, 1], "radius": 5, "height": 10, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        let o = occ(&s, "Shaft");
        run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
        s
    };
    let top = json!({"occurrence": "Shaft", "circle": [5, 0, 20]});
    let bottom = json!({"occurrence": "Collar", "circle": [55, 10, 30]});
    let mut s = shaft();
    let r = constrain(&mut s, "concentric", top.clone(), bottom.clone(), json!({}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof_of(&r, &s, "Collar"), Some(2), "{r}");
    let c = place(&s, "Collar", Vec3::new(50.0, 10.0, 30.0));
    assert!(c.x.abs() < 1e-6 && c.y.abs() < 1e-6, "on the shaft's axis: {c:?}");
    let mut s = shaft();
    let r = constrain(&mut s, "insert", top, bottom, json!({}));
    assert!(clean(&r), "{r}");
    assert_eq!(dof_of(&r, &s, "Collar"), Some(1), "{r}");
    assert!(near(place(&s, "Collar", Vec3::new(50.0, 10.0, 30.0)), Vec3::new(0.0, 0.0, 20.0), 1e-6));
    assert!(near(place(&s, "Collar", Vec3::new(50.0, 10.0, 40.0)), Vec3::new(0.0, 0.0, 30.0), 1e-6), "standing on the shaft, not through it");
}

/// The rules' remaining freedoms: parallel 4, perpendicular and angle 5, point on axis 4, tangent
/// 4 (a roller on the panel: it rolls, slides and turns), each made where the parts are.
#[test]
fn each_rule_leaves_the_freedom_it_should() {
    let roller = |s: &mut Session| {
        run(s, "component.create", json!({"name": "Roller"}));
        run(s, "solid.cylinder", json!({"base": [0, 600, 50], "axis": [1, 0, 0], "radius": 4, "height": 30, "operation": "new"}));
        run(s, "component.activate", json!({"component": "root"}));
    };
    for (kind, a, b, extra, part, want) in [
        ("parallel", json!("Top"), json!("Bottom"), json!({}), "Bracket", 4),
        ("perpendicular", json!("Top"), json!("Face"), json!({}), "Bracket", 5),
        ("angle", json!("Top"), json!("Bottom"), json!({"angle": 30}), "Bracket", 5),
        ("distance", json!("Top"), json!("Bottom"), json!({"offset": 12}), "Bracket", 3),
        ("tangent", json!("Top"), json!({"occurrence": "Roller", "circle": [0, 604, 50]}), json!({}), "Roller", 4),
        ("point_on_axis", json!({"occurrence": "Roller", "circle": [0, 604, 50]}), json!("Corner"), json!({}), "Bracket", 4),
    ] {
        let mut s = panel();
        roller(&mut s);
        if kind == "point_on_axis" {
            let o = occ(&s, "Roller");
            run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
        }
        let r = constrain(&mut s, kind, a, b, extra);
        assert!(clean(&r), "{kind}: {r}");
        assert_eq!(dof_of(&r, &s, part), Some(want), "{kind}: {r}");
        if kind == "tangent" {
            // Its axis a radius above the panel's top.
            let p = place(&s, "Roller", Vec3::new(0.0, 600.0, 50.0));
            assert!((p.z - 6.0).abs() < 1e-6, "tangent: {p:?}");
        }
    }
}

/// A rule given the wrong kind of reference, or without the number it needs, is refused with what
/// it needs, and nothing is added.
#[test]
fn a_rule_with_the_wrong_references_is_refused() {
    let mut s = panel();
    let cases = [
        (json!({"type": "mate", "a": "Top", "b": "Corner"}), "two planar faces"),
        (json!({"type": "insert", "a": "Top", "b": "Bottom"}), "two round edges"),
        (json!({"type": "distance", "a": "Top", "b": "Bottom"}), "offset"),
        (json!({"type": "angle", "a": "Top", "b": "Bottom"}), "angle"),
        (json!({"type": "weld", "a": "Top", "b": "Bottom"}), "`type` must be"),
        (json!({"type": "mate", "a": "Top", "b": "Bottom", "offset": "1e999"}), ""),
    ];
    // An orientation rule needs a direction on both sides: a point has only a position (its z is a
    // default), with another point or with a face.
    let point = json!({"occurrence": "Panel", "point": [0, 0, 2]});
    let mut cases = cases.to_vec();
    for kind in ["parallel", "perpendicular", "angle"] {
        for b in [json!("Corner"), json!("Bottom")] {
            cases.push((json!({"type": kind, "a": point.clone(), "b": b, "angle": 30}), "a point has no direction"));
        }
    }
    for (p, says) in cases {
        let e = s.execute("assembly.constraint.create", &p).expect_err(&p.to_string()).to_string();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert!(s.doc.assembly.joints.is_empty());
    // Two edges have the directions of their faces: they make a parallel (a plate's top front edge
    // and a block's, both where the boxes were made).
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [30, 40, 50]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = constrain(
        &mut s,
        "parallel",
        json!({"occurrence": "Plate", "edge": [50, 0, 10]}),
        json!({"occurrence": "Block", "edge": [40, 40, 70]}),
        json!({}),
    );
    assert!(clean(&r), "{r}");
    assert_eq!(s.doc.assembly.joints.first().and_then(|j| j.constraint), Some(solidcraft_doc::joints::Constraint::Parallel));
}

/// A roller tangent on the panel keeps resting on it as its radius changes, and a gap edited in as
/// a number stays a gap above the face, not the axis's height.
/// A constraint given a blank or `null` name is named after its rule, as one given no name is. A
/// joint.edit to another kind of joint makes it a plain joint: the tag goes, and a former tangent's
/// offset no longer follows its face's radius.
#[test]
fn a_constraint_edited_to_another_joint_type_is_a_plain_joint() {
    let mut s = panel();
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "4 mm"}));
    run(&mut s, "component.create", json!({"name": "Roller"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 600, 50], "axis": [1, 0, 0], "radius": "R", "height": 30, "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = constrain(&mut s, "tangent", json!("Top"), json!({"occurrence": "Roller", "circle": [0, 604, 50]}), json!({"name": null}));
    assert!(clean(&r), "{r}");
    let r = constrain(&mut s, "parallel", json!("Top"), json!("Bottom"), json!({"name": "  "}));
    assert!(clean(&r), "{r}");
    let names: Vec<&str> = s.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["Tangent1", "Parallel1"]);
    run(&mut s, "joint.edit", json!({"joint": "Tangent1", "type": "revolute"}));
    let j = s.doc.assembly.joints.iter().find(|j| j.name == "Tangent1").cloned().unwrap();
    assert_eq!((j.kind, j.constraint, j.exprs.offset.clone()), (solidcraft_doc::joints::JointKind::Revolute, None, None));
    let list = run(&mut s, "joint.list", json!({}));
    let listed = list["joints"].as_array().and_then(|js| js.iter().find(|x| x["name"] == "Tangent1")).cloned().unwrap_or_default();
    assert!(listed.get("constraint").is_none(), "{listed}");
    // The radius no longer moves it.
    run(&mut s, "parameters.change", json!({"name": "R", "expression": "7 mm"}));
    run(&mut s, "joint.solve", json!({}));
    let after = s.doc.assembly.joints.iter().find(|j| j.name == "Tangent1").map(|j| j.offset);
    assert_eq!(after, Some(j.offset));
}

#[test]
fn a_tangent_constraint_follows_its_faces_radius() {
    let mut s = panel();
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "4 mm"}));
    run(&mut s, "component.create", json!({"name": "Roller"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 600, 50], "axis": [1, 0, 0], "radius": "R", "height": 30, "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = constrain(&mut s, "tangent", json!("Top"), json!({"occurrence": "Roller", "circle": [0, 604, 50]}), json!({}));
    assert!(clean(&r), "{r}");
    let axis = |s: &Session| place(s, "Roller", Vec3::new(0.0, 600.0, 50.0)).z;
    assert!((axis(&s) - 6.0).abs() < 1e-6, "{}", axis(&s));
    for (radius, gap, want) in [(7.0, None, 9.0), (2.5, None, 4.5), (2.5, Some(1.0), 5.5), (6.0, None, 9.0)] {
        run(&mut s, "parameters.change", json!({"name": "R", "expression": format!("{radius} mm")}));
        if let Some(g) = gap {
            run(&mut s, "joint.edit", json!({"joint": "Tangent1", "offset": g}));
        }
        let r = run(&mut s, "joint.solve", json!({}));
        assert!(clean(&r), "R {radius}: {r}");
        assert!((axis(&s) - want).abs() < 1e-6, "R {radius}, gap {gap:?}: axis at {}, expected {want}", axis(&s));
    }
}

// assembly.constraint.edit: a constraint's rule, gap, angle, facing or suppression changed in place.

/// A plate (100 x 100 x 10) and a 20 mm block above it at (30, 40, 50), the plate grounded; the
/// block's bottom mated to the plate's top as `Mate1`.
fn plate_and_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [30, 40, 50]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let plate = occ(&s, "Plate");
    run(&mut s, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
    let r = constrain(
        &mut s,
        "mate",
        json!({"occurrence": "Plate", "face": [5, 5, 10]}),
        json!({"occurrence": "Block", "face": [31, 41, 50]}),
        json!({}),
    );
    assert!(clean(&r), "{r}");
    s
}

/// The block's lowest and highest z, from its eight corners.
fn block_z(s: &Session) -> (f64, f64) {
    let mut zs = Vec::new();
    for x in [30.0, 50.0] {
        for y in [40.0, 60.0] {
            for z in [50.0, 70.0] {
                zs.push(place(s, "Block", Vec3::new(x, y, z)).z);
            }
        }
    }
    (zs.iter().copied().fold(f64::INFINITY, f64::min), zs.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

fn near_z(got: (f64, f64), want: (f64, f64)) -> bool {
    (got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6
}

fn edit_rule(s: &mut Session, p: Value) -> Value {
    run(s, "assembly.constraint.edit", p)
}

/// One constraint changed from Mate to Distance to Flush: each rule places the block where its
/// numbers say, the gap carries over to the next rule, and the joint is the kind its rule needs.
#[test]
fn a_constraints_rule_changes_in_place() {
    let mut s = plate_and_block();
    assert!(near_z(block_z(&s), (10.0, 30.0)), "mate: {:?}", block_z(&s));
    // Distance 15: the bottom 15 mm above the top, still facing it.
    let r = edit_rule(&mut s, json!({"constraint": "Mate1", "type": "distance", "offset": 15}));
    assert!(clean(&r) && dof_of(&r, &s, "Block") == Some(3), "{r}");
    assert!(near_z(block_z(&s), (25.0, 45.0)), "distance: {:?}", block_z(&s));
    // Flush keeps the 15 mm gap: the bottom turned to face up, 15 mm above the top.
    let r = edit_rule(&mut s, json!({"constraint": "Mate1", "type": "flush"}));
    assert!(clean(&r) && dof_of(&r, &s, "Block") == Some(3), "{r}");
    assert!(near_z(block_z(&s), (5.0, 25.0)), "flush, 15 mm: {:?}", block_z(&s));
    // Flush at 0: the bottom in the top's plane, the block below it.
    edit_rule(&mut s, json!({"constraint": "Mate1", "offset": 0}));
    assert!(near_z(block_z(&s), (-10.0, 10.0)), "flush, 0: {:?}", block_z(&s));
    let j = s.doc.assembly.joints.iter().find(|j| j.name == "Mate1").unwrap();
    use solidcraft_doc::joints::{Constraint, JointKind};
    assert_eq!((j.kind, j.flip, j.constraint), (JointKind::Planar, true, Some(Constraint::Flush)));
    // Saved and reopened, the new rule stays.
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.assembly.joints.iter().find(|j| j.name == "Mate1").and_then(|j| j.constraint), Some(Constraint::Flush));
    // Turned over again without a type: back to facing the top, 0 mm off it.
    edit_rule(&mut s, json!({"constraint": "Mate1", "flip": true}));
    assert!(near_z(block_z(&s), (10.0, 30.0)), "turned over: {:?}", block_z(&s));
}

/// A new gap moves the block straight across it: not turned, not slid.
#[test]
fn an_offset_edit_moves_the_part_only_across_the_gap() {
    let mut s = plate_and_block();
    let corners = [Vec3::new(30.0, 40.0, 50.0), Vec3::new(50.0, 40.0, 50.0), Vec3::new(30.0, 60.0, 70.0)];
    let before = corners.map(|c| place(&s, "Block", c));
    let r = edit_rule(&mut s, json!({"constraint": "Mate1", "offset": 5}));
    assert!(clean(&r), "{r}");
    for (c, was) in corners.into_iter().zip(before) {
        let got = place(&s, "Block", c);
        assert!(near(got, was + Vec3::new(0.0, 0.0, 5.0), 1e-6), "{c:?}: {got:?}, was {was:?}");
    }
}

/// A rule the references don't fit, a plain joint, a missing angle: refused with why, the joint
/// as it was.
#[test]
fn an_edit_that_does_not_fit_is_refused_and_changes_nothing() {
    let mut s = plate_and_block();
    run(
        &mut s,
        "joint.create",
        json!({"type": "parallel", "name": "parallel Top-Bottom", "a": {"occurrence": "Plate", "face": [5, 5, 10]}, "b": {"occurrence": "Block", "face": [31, 41, 10]}}),
    );
    let joints = s.doc.assembly.joints.clone();
    let z = block_z(&s);
    for (p, says) in [
        (json!({"constraint": "Mate1", "type": "insert"}), "insert needs two round edges"),
        (json!({"constraint": "Mate1", "type": "tangent"}), "tangent needs a planar face and a round face"),
        (json!({"constraint": "Mate1", "type": "angle"}), "angle needs `angle`"),
        (json!({"constraint": "Mate1", "type": "weld"}), "`type` must be"),
        (json!({"constraint": "parallel Top-Bottom", "offset": 1}), "was made as a joint"),
        (json!({"constraint": "Nothing"}), "no joint"),
    ] {
        let e = s.execute("assembly.constraint.edit", &p).expect_err(&p.to_string()).to_string();
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(s.doc.assembly.joints, joints, "{p}");
        assert!(near_z(block_z(&s), z), "{p}");
    }
}

/// Suppressed, the constraint places nothing and the block is free; brought back, it sits on the
/// plate again.
#[test]
fn a_constraint_suppressed_and_brought_back() {
    let mut s = plate_and_block();
    let block = occ(&s, "Block");
    run(&mut s, "occurrence.move", json!({"occurrence": block, "translate": [0, 0, 40]}));
    let r = edit_rule(&mut s, json!({"constraint": "Mate1", "suppressed": true}));
    assert!(s.doc.assembly.joints.iter().all(|j| j.suppressed), "{r}");
    assert_eq!(dof_of(&r, &s, "Block"), Some(6), "{r}");
    let r = edit_rule(&mut s, json!({"constraint": "Mate1", "suppressed": false}));
    assert!(clean(&r) && dof_of(&r, &s, "Block") == Some(3), "{r}");
    assert!(near_z(block_z(&s), (10.0, 30.0)), "back: {:?}", block_z(&s));
}

/// A tangent's gap carries over its radius: changed to 2 mm, then the rule mapped again, the
/// roller's axis stays a radius and 2 mm above the panel.
#[test]
fn a_tangent_keeps_its_gap_through_an_edit() {
    let mut s = panel();
    run(&mut s, "component.create", json!({"name": "Roller"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 600, 50], "axis": [1, 0, 0], "radius": 4, "height": 30, "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    constrain(&mut s, "tangent", json!("Top"), json!({"occurrence": "Roller", "circle": [0, 604, 50]}), json!({}));
    let axis = |s: &Session| place(s, "Roller", Vec3::new(0.0, 600.0, 50.0)).z;
    edit_rule(&mut s, json!({"constraint": "Tangent1", "offset": 2}));
    assert!((axis(&s) - 8.0).abs() < 1e-6, "{}", axis(&s));
    let r = edit_rule(&mut s, json!({"constraint": "Tangent1", "type": "tangent"}));
    assert!(clean(&r), "{r}");
    assert!((axis(&s) - 8.0).abs() < 1e-6, "{}", axis(&s));
}

/// A rule changed to one kept as another kind of joint (a mate is planar, a parallel rule is not)
/// keeps a tag, with the new rule; the same change through joint.edit makes it a plain joint.
#[test]
fn a_rule_change_keeps_the_tag_where_joint_edit_drops_it() {
    use solidcraft_doc::joints::{Constraint, JointKind};
    let mut s = plate_and_block();
    let r = edit_rule(&mut s, json!({"constraint": "Mate1", "type": "parallel"}));
    assert!(clean(&r), "{r}");
    let rule = |s: &Session| s.doc.assembly.joints.iter().find(|j| j.name == "Mate1").map(|j| (j.kind, j.constraint));
    assert_eq!(rule(&s), Some((JointKind::Parallel, Some(Constraint::Parallel))));
    // Started where its parts are: the block didn't move.
    assert!(near_z(block_z(&s), (10.0, 30.0)), "{:?}", block_z(&s));
    run(&mut s, "joint.edit", json!({"joint": "Mate1", "type": "planar"}));
    assert_eq!(rule(&s), Some((JointKind::Planar, None)));
}
