//! Relations (parallel, perpendicular, angle, point on plane, point on line, line on plane): each
//! ties only part of a placement, several between two parts add up, they start from where the
//! parts are, and they keep holding as the parts change size.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

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
