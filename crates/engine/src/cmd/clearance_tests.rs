use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn cube(s: &mut Session, name: &str, size: f64, corner: [f64; 3]) {
    run(s, "solid.box", json!({"length": size, "width": size, "height": size, "corner": corner, "operation": "new", "body_name": name}));
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// The pair `{a, b}` of a list entry, sorted.
fn pair(v: &Value) -> [String; 2] {
    let mut p = [v["a"].as_str().unwrap_or("").to_string(), v["b"].as_str().unwrap_or("").to_string()];
    p.sort();
    p
}

/// Two 10 mm cubes sharing a 1 mm corner cube overlap by 1 mm³; a third, far off, is pruned.
#[test]
fn an_overlap_of_one_cubic_millimetre_is_reported() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "B", 10.0, [9.0, 9.0, 9.0]);
    cube(&mut s, "Far", 10.0, [100.0, 0.0, 0.0]);
    let r = run(&mut s, "inspect.clearance", json!({}));
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    assert_eq!(pair(&overlaps[0]), ["A", "B"]);
    assert!((f(&overlaps[0]["volume_mm3"]) - 1.0).abs() < 1e-6, "{r}");
    // Three faces of 1 x 1 from each cube: 6 mm².
    assert!((f(&overlaps[0]["area_mm2"]) - 6.0).abs() < 1e-6, "{r}");
    assert!((f(&overlaps[0]["center"][0]) - 9.5).abs() < 1e-6, "{r}");
    assert_eq!(r["pairs_checked"], 1, "{r}");
    assert_eq!(r["pairs_pruned"], 2, "{r}");
    assert_eq!(r["unchecked"].as_array().map(Vec::len), Some(0), "{r}");
    // An overlap under min_volume is contact, not an overlap: with a clearance it is near, at 0.
    let r = run(&mut s, "inspect.clearance", json!({"min_volume": 2, "clearance": 0.1}));
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
    assert_eq!(f(&r["near"][0]["distance_mm"]), 0.0, "{r}");
    assert!((f(&r["near"][0]["overlap_mm3"]) - 1.0).abs() < 1e-6, "{r}");
}

/// Two cubes 0.5 mm apart are near with a clearance of 1 mm; 2 mm apart they are not.
#[test]
fn pairs_closer_than_the_clearance_are_near() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "B", 10.0, [10.5, 2.0, 3.0]);
    cube(&mut s, "C", 10.0, [-12.0, 0.0, 0.0]);
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 1, "{r}");
    assert_eq!(pair(&near[0]), ["A", "B"]);
    assert!((f(&near[0]["distance_mm"]) - 0.5).abs() < 1e-9, "{r}");
    assert!(near[0].get("exact").is_none(), "measured on the exact faces: {r}");
    // The points lie on the facing faces x = 10 and x = 10.5.
    let xs = [f(&near[0]["from"][0]), f(&near[0]["to"][0])];
    assert!(xs.iter().any(|x| (x - 10.0).abs() < 1e-9) && xs.iter().any(|x| (x - 10.5).abs() < 1e-9), "{r}");
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
    // With no clearance nothing is near; with 2.5 mm C (2 mm from A) is near too.
    let r = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(r["near"].as_array().map(Vec::len), Some(0), "{r}");
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 2.5}));
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 2, "{r}");
    assert_eq!(pair(&near[1]), ["A", "C"]);
    assert!((f(&near[1]["distance_mm"]) - 2.0).abs() < 1e-9, "{r}");
}

/// Distances where the nearest points are on edges, and on curved faces whose meshes are far
/// coarser than the answer: cube edge to cube edge, a cylinder's side to a cube's face, and a
/// large sphere (mesh chord about 1 mm) to a cube.
#[test]
fn distances_are_exact_on_edges_and_curved_faces() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    // Its vertical edge at (12, 13) faces A's at (10, 10): √(2² + 3²).
    cube(&mut s, "B", 10.0, [12.0, 13.0, 0.0]);
    run(&mut s, "solid.cylinder", json!({"base": [-30, 5, 0], "radius": 5, "height": 10, "operation": "new", "body_name": "Pin"}));
    run(&mut s, "solid.sphere", json!({"radius": 500, "center": [5, 5, 510.7], "operation": "new", "body_name": "Ball"}));
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 50, "against": ["A"], "bodies": ["B", "Pin", "Ball"]}));
    let near = r["near"].as_array().cloned().unwrap_or_default();
    let d = |other: &str| near.iter().find(|n| pair(n).contains(&other.to_string())).map(|n| (f(&n["distance_mm"]), n.get("exact").is_none()));
    assert_eq!(near.len(), 3, "{r}");
    let (edge, exact) = d("B").unwrap_or((f64::NAN, false));
    assert!((edge - 13f64.sqrt()).abs() < 1e-6 && exact, "{r}");
    // The pin's side (x = -25) is 25 from A's face x = 0.
    let (side, exact) = d("Pin").unwrap_or((f64::NAN, false));
    assert!((side - 25.0).abs() < 1e-6 && exact, "{r}");
    // The ball's lowest point, (5, 5, 10.7), is 0.7 above A's top.
    let (ball, exact) = d("Ball").unwrap_or((f64::NAN, false));
    assert!((ball - 0.7).abs() < 1e-6 && exact, "{r}");
    // `against` leaves out the pairs among `bodies`, which are by default every other body.
    assert!(near.iter().all(|n| pair(n).contains(&"A".to_string())), "{r}");
    let all = run(&mut s, "inspect.clearance", json!({"clearance": 50, "against": ["A"]}));
    assert_eq!(all["near"].as_array().map(Vec::len), Some(3), "{all}");
}

/// A pin in a hole 0.2 mm wider all round, in a plate whose mesh is coarser than that gap: the
/// plate's flat faces reach over the hole on their surfaces but not on the faces, so the
/// distance is the gap to the hole's wall.
#[test]
fn a_pin_in_a_hole_is_as_far_as_the_holes_wall() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 2, "operation": "new", "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3.2, "height": 4, "base": [10, 10, -1], "operation": "cut", "targets": ["Plate"]}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [10, 10, -5], "operation": "new", "body_name": "Pin"}));
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
    assert!((f(&r["near"][0]["distance_mm"]) - 0.2).abs() < 1e-6, "{r}");
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 0.15}));
    assert_eq!(r["near"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Three spheres' nearest neighbours against their distances worked out by hand.
#[test]
fn nearest_distances_of_three_spheres() {
    let mut s = Session::default();
    for (name, r, c) in [("S1", 5.0, [0.0, 0.0, 0.0]), ("S2", 3.0, [12.0, 0.0, 0.0]), ("S3", 2.0, [3.0, 20.0, 4.0])] {
        run(&mut s, "solid.sphere", json!({"radius": r, "center": c, "operation": "new", "body_name": name}));
    }
    let r = run(&mut s, "inspect.clearance", json!({"nearest": 2}));
    let d12 = 12.0 - 8.0;
    let d13 = (9.0f64 + 400.0 + 16.0).sqrt() - 7.0;
    let d23 = (81.0f64 + 400.0 + 16.0).sqrt() - 5.0;
    let want = [("S1", [("S2", d12), ("S3", d13)]), ("S2", [("S1", d12), ("S3", d23)]), ("S3", [("S1", d13), ("S2", d23)])];
    for (body, list) in want {
        let got = r["nearest"][body].as_array().cloned().unwrap_or_default();
        assert_eq!(got.len(), 2, "{body}: {r}");
        for (g, (other, d)) in got.iter().zip(list) {
            assert_eq!(g["body"], other, "{body}: {r}");
            assert!((f(&g["distance_mm"]) - d).abs() < 1e-6, "{body} to {other}: {} against {d}", g["distance_mm"]);
        }
    }
    // One neighbour each.
    let r = run(&mut s, "inspect.clearance", json!({"nearest": 1, "bodies": ["S3"]}));
    assert_eq!(r["nearest"]["S3"].as_array().map(Vec::len), Some(0), "only S3 is in `bodies`, so it has no partner: {r}");
    let r = run(&mut s, "inspect.clearance", json!({"nearest": 1, "bodies": ["S3"], "against": ["S1", "S2"]}));
    assert_eq!(r["nearest"]["S3"][0]["body"], "S1", "{r}");
}

/// `skip_within` leaves out the pairs inside one component (two bodies made to touch, as the
/// parts of a bought component do) and keeps those between components.
#[test]
fn skip_within_leaves_out_a_components_own_pairs() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Bought"}));
    cube(&mut s, "Housing", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "Insert", 4.0, [8.0, 3.0, 3.0]);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    cube(&mut s, "Plate", 10.0, [-9.5, 0.0, 0.0]);
    let all = run(&mut s, "inspect.clearance", json!({}));
    let pairs: Vec<[String; 2]> = all["overlaps"].as_array().into_iter().flatten().map(pair).collect();
    assert!(pairs.contains(&["Housing".into(), "Insert".into()]) && pairs.contains(&["Housing".into(), "Plate".into()]), "{all}");
    for mode in ["component", "occurrence"] {
        let r = run(&mut s, "inspect.clearance", json!({"skip_within": mode}));
        let pairs: Vec<[String; 2]> = r["overlaps"].as_array().into_iter().flatten().map(pair).collect();
        assert_eq!(pairs, vec![["Housing".to_string(), "Plate".to_string()]], "{mode}: {r}");
        assert!((f(&r["overlaps"][0]["volume_mm3"]) - 50.0).abs() < 1e-6, "{r}");
    }
}

/// A bought assembly with its envelope, and a part beside it: a 10 mm housing with a 4 mm insert
/// in a component, the envelope around both, and a bracket 1.5 mm clear of the housing.
fn bought_with_envelope(s: &mut Session) {
    run(s, "component.create", json!({"name": "Bought"}));
    cube(s, "Housing", 10.0, [0.0, 0.0, 0.0]);
    run(s, "component.create", json!({"name": "Insert part"}));
    cube(s, "Insert", 4.0, [8.0, 3.0, 3.0]);
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "solid.bounding_solid", json!({"component": "Bought", "body_name": "Envelope"}));
    cube(s, "Bracket", 10.0, [13.5, 0.0, 0.0]);
}

/// A component's envelope is left out unless named; with `envelopes` it is checked instead of
/// the bodies inside it, and it is never paired with a body inside it.
#[test]
fn an_envelope_stands_in_for_its_components_bodies_when_asked() {
    let mut s = Session::default();
    bought_with_envelope(&mut s);
    // By default: the bodies themselves. The insert sits in the housing; the bracket is 1.5 mm
    // past the insert (which reaches x = 12).
    let all = run(&mut s, "inspect.clearance", json!({"clearance": 2}));
    let overlaps: Vec<[String; 2]> = all["overlaps"].as_array().into_iter().flatten().map(pair).collect();
    assert_eq!(overlaps, vec![["Housing".to_string(), "Insert".to_string()]], "{all}");
    let near: Vec<[String; 2]> = all["near"].as_array().into_iter().flatten().map(pair).collect();
    assert_eq!(near, vec![["Bracket".to_string(), "Insert".to_string()]], "{all}");
    assert_eq!(all["bodies"], 3, "the envelope is left out: {all}");
    // With envelopes: the envelope (0..12 along X) and the bracket, 1.5 mm apart, and nothing else.
    let env = run(&mut s, "inspect.clearance", json!({"clearance": 2, "envelopes": true}));
    assert_eq!(env["overlaps"].as_array().map(Vec::len), Some(0), "{env}");
    let near: Vec<[String; 2]> = env["near"].as_array().into_iter().flatten().map(pair).collect();
    assert_eq!(near, vec![["Bracket".to_string(), "Envelope".to_string()]], "{env}");
    assert!((f(&env["near"][0]["distance_mm"]) - 1.5).abs() < 1e-6, "{env}");
    assert_eq!(env["bodies"], 2, "{env}");
    // A body named inside an envelope means the envelope; one named against it too.
    let named = run(&mut s, "inspect.clearance", json!({"bodies": ["Bracket"], "against": ["Insert"], "clearance": 2, "envelopes": true}));
    assert_eq!(pair(&named["near"][0]), ["Bracket", "Envelope"], "{named}");
    // Named without `envelopes`, the envelope is checked, but never against what it encloses.
    let named = run(&mut s, "inspect.clearance", json!({"bodies": ["Envelope", "Housing", "Bracket"], "clearance": 2}));
    assert_eq!(named["overlaps"].as_array().map(Vec::len), Some(0), "{named}");
    let near: Vec<[String; 2]> = named["near"].as_array().into_iter().flatten().map(pair).collect();
    assert_eq!(near, vec![["Bracket".to_string(), "Envelope".to_string()]], "{named}");
    assert!(s.execute("inspect.clearance", &json!({"envelopes": "yes"})).is_err());
}

/// Parameters out of range are refused, not guessed at.
#[test]
fn bad_parameters_are_refused() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    for p in [
        json!({"clearance": -1}),
        json!({"clearance": "wide"}),
        json!({"min_volume": -0.1}),
        json!({"nearest": 1e9}),
        json!({"tolerance": 5}),
        json!({"skip_within": "body"}),
        json!({"bodies": ["Nope"]}),
        json!({"against": ["Nope"]}),
    ] {
        assert!(s.execute("inspect.clearance", &p).is_err(), "{p}");
    }
    let r = run(&mut s, "inspect.clearance", json!({"nearest": 3, "skip_within": null}));
    assert_eq!(r["pairs_checked"], 0, "{r}");
    assert_eq!(r["nearest"]["A"].as_array().map(Vec::len), Some(0), "{r}");
}

/// A pin sunk 0.01 mm into a plate 1.7 m long: the shared ring holds π (4.43² − 2.5²) × 0.01 mm³
/// wherever the plate stands, as on a plate of 100 mm (the boolean of the whole plate gave 0.31).
#[test]
fn a_thin_overlap_with_a_large_plate_is_measured_in_full() {
    let ring = std::f64::consts::PI * (4.43f64.powi(2) - 2.5f64.powi(2)) * 0.01;
    for (length, far) in [(100.0, false), (1700.0, false), (1700.0, true)] {
        let mut s = Session::default();
        let corner = [-length / 2.0, -length / 4.0, 0.0];
        run(&mut s, "solid.box", json!({"length": length, "width": length / 2.0, "height": 5, "corner": corner, "body_name": "Plate"}));
        run(&mut s, "solid.cylinder", json!({"radius": 2.5, "height": 9, "base": [0, 0, -2], "operation": "cut", "targets": ["Plate"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 2.4, "height": 7, "base": [0, 0, -1.99], "operation": "new", "body_name": "Pin"}));
        run(&mut s, "solid.cylinder", json!({"radius": 4.43, "height": 2, "base": [0, 0, 4.99], "operation": "join", "targets": ["Pin"]}));
        let shift = if far { [-326.3, 13.5, 1278.3] } else { [0.0; 3] };
        if far {
            run(&mut s, "solid.move", json!({"bodies": ["Plate", "Pin"], "translate": shift}));
        }
        let r = run(&mut s, "inspect.clearance", json!({"min_volume": 1e-4}));
        let o = &r["overlaps"][0];
        assert!((f(&o["volume_mm3"]) - ring).abs() < 1e-3 * ring, "{length} {far}: {r}");
        // The ring's centre is on the axis, 0.005 below the plate's face.
        assert!((f(&o["center"][2]) - (4.995 + shift[2])).abs() < 1e-4, "{length} {far}: {r}");
        assert!((f(&o["center"][0]) - shift[0]).abs() < 1e-4, "{length} {far}: {r}");
    }
}

/// Two plates 0.2 mm apart, the upper one set 10 mm in from the lower one's corner, and two set
/// side by side with a 0.2 mm gap: 0.2, not 0 (a point past a corner, on the line of one of its
/// edges, was taken for a point of the face). Gaps under twice the meshes' chord went wrong.
#[test]
fn a_small_gap_past_a_corner_is_not_contact() {
    let mut s = Session::default();
    let plate = |s: &mut Session, name: &str, size: [f64; 3], corner: [f64; 3]| {
        run(s, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner, "operation": "new", "body_name": name}));
    };
    plate(&mut s, "A", [100.0, 100.0, 2.0], [0.0, 0.0, 0.0]);
    plate(&mut s, "B", [100.0, 100.0, 2.0], [10.0, 10.0, 2.2]);
    plate(&mut s, "C", [100.0, 100.0, 2.0], [0.0, 0.0, 10.0]);
    plate(&mut s, "D", [100.0, 100.0, 2.0], [100.2, 0.0, 10.0]);
    // 3 mm apart in x and 0.2 in z, along their long sides: √(3² + 0.2²).
    plate(&mut s, "E", [1000.0, 100.0, 2.0], [0.0, 300.0, 0.0]);
    plate(&mut s, "F", [1000.0, 100.0, 2.0], [1003.0, 300.0, 2.2]);
    for (a, b, want) in [("A", "B", 0.2), ("C", "D", 0.2), ("E", "F", (9.0f64 + 0.04).sqrt())] {
        let r = run(&mut s, "inspect.clearance", json!({"bodies": [a], "against": [b], "clearance": 5, "nearest": 4}));
        assert!((f(&r["near"][0]["distance_mm"]) - want).abs() < 1e-9, "{a} {b}: {r}");
        assert!((f(&r["nearest"][a][0]["distance_mm"]) - want).abs() < 1e-9, "{a} {b}: {r}");
    }
}

/// A part in the missing quarter of an L-shaped block is 0.3 mm from both inside walls.
/// Its top corners are on the top's support plane, outside the trimmed face (#16393).
#[test]
fn a_small_gap_in_an_inside_corner_is_not_contact() {
    for gap in [0.3, 0.0] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 200, "width": 200, "height": 10, "corner": [-100, -100, 0], "body_name": "Block"}));
        run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 12, "corner": [0, 0, -1], "operation": "new", "body_name": "Cut"}));
        run(&mut s, "solid.combine", json!({"operation": "cut", "target": "Block", "tools": ["Cut"]}));
        run(&mut s, "solid.box", json!({"length": 90, "width": 90, "height": 10, "corner": [gap, gap, 0], "operation": "new", "body_name": "Part"}));
        for params in [json!({"clearance": 1, "nearest": 1}), json!({"nearest": 1})] {
            let r = run(&mut s, "inspect.clearance", params.clone());
            assert_eq!(r["unchecked"], json!([]), "{gap}: {r}");
            assert_eq!(r["overlaps"], json!([]), "{gap}: {r}");
            for body in ["Block", "Part"] {
                assert!((f(&r["nearest"][body][0]["distance_mm"]) - gap).abs() < 1e-9, "{gap}: {r}");
            }
            if params.get("clearance").is_some() {
                assert_eq!(r["near"].as_array().map(Vec::len), Some(1), "{gap}: {r}");
                assert!((f(&r["near"][0]["distance_mm"]) - gap).abs() < 1e-9, "{gap}: {r}");
                assert!(r["near"][0].get("exact").is_none(), "{gap}: {r}");
            } else {
                assert_eq!(r["near"], json!([]), "{gap}: {r}");
            }
        }
    }
}

/// #2283: a knuckle (Ø6, z 3.7 to 8.3, with a 150 mm arm) turning between two frame bands (Ø6,
/// z -1.5 to 3.5 and 8.5 to 13.5) on a shared axis is 0.2 mm from them at every angle. Its axis
/// tilted 1.5e-6 rad (as a STEP round trip leaves it), the band's rim lies on the knuckle's
/// cylinder extended past its end, and a point there was taken for a point of the knuckle at
/// some angles: contact (0) at a point off the body.
#[test]
fn a_hinge_gap_is_measured_on_the_trimmed_faces_at_every_angle() {
    let mut s = Session::default();
    let part = |s: &mut Session, cmd: &str, mut p: Value, name: &str, join: bool| {
        p["body_name"] = json!(name);
        p["operation"] = json!(if join { "join" } else { "new" });
        if join {
            p["targets"] = json!([name]);
        }
        run(s, cmd, p);
    };
    part(&mut s, "solid.cylinder", json!({"radius": 3, "height": 5, "base": [0, 0, -1.5]}), "Frame", false);
    part(&mut s, "solid.cylinder", json!({"radius": 3, "height": 5, "base": [0, 0, 8.5]}), "Frame", true);
    for z in [-1.5, 8.5] {
        part(&mut s, "solid.box", json!({"length": 137.5, "width": 4, "height": 5, "corner": [-140, -2, z]}), "Frame", true);
    }
    part(&mut s, "solid.box", json!({"length": 5, "width": 4, "height": 15, "corner": [-145, -2, -1.5]}), "Frame", true);
    part(&mut s, "solid.cylinder", json!({"radius": 3, "height": 4.6, "base": [0, 0, 3.7]}), "Temple", false);
    part(&mut s, "solid.box", json!({"length": 150, "width": 2, "height": 4.6, "corner": [0, -1, 3.7]}), "Temple", true);
    run(&mut s, "solid.move", json!({"bodies": ["Temple"], "axis": [1, 0, 0], "angle": "1.5e-6 rad"}));
    run(&mut s, "solid.move", json!({"bodies": ["Temple"], "axis": [0, 0, 1], "angle": "29 deg"}));
    for deg in 30..=80 {
        run(&mut s, "solid.move", json!({"bodies": ["Temple"], "axis": [0, 0, 1], "angle": "1 deg"}));
        let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Temple"], "against": ["Frame"], "clearance": 0.5, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "{deg}°: {r}");
        let near = &r["near"][0];
        assert!(near.get("exact").is_none(), "{deg}°: measured on the exact faces: {r}");
        assert!((f(&near["distance_mm"]) - 0.2).abs() < 1e-4, "{deg}°: {r}");
        assert!((f(&r["nearest"]["Temple"][0]["distance_mm"]) - 0.2).abs() < 1e-4, "{deg}°: {r}");
        // The witness points lie on the bodies: the frame's at a band end, the knuckle's at one
        // of its ends.
        let (from, to) = (f(&near["from"][2]), f(&near["to"][2]));
        assert!([3.5, 8.5].iter().any(|z| (from - z).abs() < 1e-4), "{deg}°: the frame's point {from} is at a band end: {r}");
        assert!([3.7, 8.3].iter().any(|z| (to - z).abs() < 1e-4), "{deg}°: the knuckle's point {to} is at a knuckle end: {r}");
    }
}

/// A second call measures only the pairs with a body that changed or moved since the first.
#[test]
fn pairs_whose_bodies_did_not_change_or_move_are_not_measured_again() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Bought"}));
    cube(&mut s, "Housing", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "Insert", 4.0, [8.0, 3.0, 3.0]);
    run(&mut s, "component.activate", json!({"component": "root"}));
    let bracket = run(&mut s, "component.create", json!({"name": "Bracket"}))["component"].as_u64().unwrap();
    cube(&mut s, "Plate", 10.0, [-9.5, 0.0, 0.0]);
    let settings = json!({"clearance": 1});
    let first = run(&mut s, "inspect.clearance", settings.clone());
    assert_eq!((first["pairs_checked"].as_u64(), first["pairs_reused"].as_u64()), (Some(2), Some(0)), "{first}");
    let again = run(&mut s, "inspect.clearance", settings.clone());
    assert_eq!(again["pairs_reused"], 2, "{again}");
    for k in ["overlaps", "near", "unchecked"] {
        assert_eq!(again[k], first[k], "{k}");
    }
    // Other settings are measured afresh.
    assert_eq!(run(&mut s, "inspect.clearance", json!({"clearance": 2}))["pairs_reused"], 0);
    // The bracket moves 1 mm away: only its pair is measured again, and it is now near, not over.
    let o = s.doc.occurrence_of(bracket).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": o, "translate": [-1, 0, 0], "capture": true}));
    let moved = run(&mut s, "inspect.clearance", settings.clone());
    assert_eq!((moved["pairs_checked"].as_u64(), moved["pairs_reused"].as_u64()), (Some(2), Some(1)), "{moved}");
    let overlaps: Vec<[String; 2]> = moved["overlaps"].as_array().into_iter().flatten().map(pair).collect();
    assert_eq!(overlaps, vec![["Housing".to_string(), "Insert".to_string()]], "{moved}");
    assert!((f(&moved["near"][0]["distance_mm"]) - 0.5).abs() < 1e-6, "{moved}");
    let still = run(&mut s, "inspect.clearance", settings);
    assert_eq!(still["pairs_reused"], 2, "nothing changed since: {still}");
    assert_eq!(still["near"], moved["near"]);
}

/// A body rebuilt with other geometry is measured again, not taken from an earlier call.
#[test]
fn a_body_that_changed_shape_is_measured_again() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "B", 10.0, [10.5, 0.0, 0.0]);
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    assert!((f(&r["near"][0]["distance_mm"]) - 0.5).abs() < 1e-6, "{r}");
    let list = run(&mut s, "parameters.list", json!({"filter": "feature"}));
    let length = list["parameters"][0]["name"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": length, "expression": "10.2 mm"}));
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    assert_eq!(r["pairs_reused"], 0, "{r}");
    assert!((f(&r["near"][0]["distance_mm"]) - 0.3).abs() < 1e-6, "{r}");
    // Set back, the first shape's result is the right one again.
    run(&mut s, "parameters.change", json!({"name": length, "expression": "10 mm"}));
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    assert!((f(&r["near"][0]["distance_mm"]) - 0.5).abs() < 1e-6, "{r}");
}

/// A flanged bushing and a headed pin seated in it line to line, on its very axis, the head on
/// the flange. Plain, they touch and share nothing; sunk 0.2 into the flange, the head shares the
/// ring under it. With a fillet under the head against the bore's sharp edge, the fillet's ring
/// is in the flange (about 0.49 mm³), next to walls that lie on one another: an overlap. The clock
/// is not read: what is under test is the result (#2999 dropped it from the sibling tests).
#[test]
fn a_pin_seated_line_to_line_in_its_bushing_is_measured() {
    let ring = std::f64::consts::PI * (7.0f64.powi(2) - 4.0f64.powi(2)) * 0.2;
    for (sunk, fillet) in [(0.0, false), (0.2, false), (0.0, true)] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 12, "base": [100, 0, 0], "operation": "new", "body_name": "Bush"}));
        run(&mut s, "solid.cylinder", json!({"radius": 7.5, "height": 1, "base": [100, 0, 12], "operation": "join", "targets": ["Bush"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "base": [100, 0, -3], "operation": "cut", "targets": ["Bush"]}));
        let z = 13.0 - sunk;
        run(&mut s, "solid.cylinder", json!({"radius": 4, "height": z + 2.0, "base": [100, 0, -2], "operation": "new", "body_name": "Pin"}));
        run(&mut s, "solid.cylinder", json!({"radius": 7, "height": 1.5, "base": [100, 0, z], "operation": "join", "targets": ["Pin"]}));
        if fillet {
            run(&mut s, "solid.fillet", json!({"edges": [[104, 0, z]], "radius": 0.3, "body": "Pin"}));
            run(&mut s, "solid.fillet", json!({"edges": [[107, 0, z + 1.5]], "radius": 0.5, "body": "Pin"}));
        }
        let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Pin"], "against": ["Bush"], "min_volume": 0.01, "clearance": 1e-6}));
        assert_eq!(r["unchecked"], json!([]), "sunk {sunk}: {r}");
        if fillet {
            assert!(f(&r["overlaps"][0]["volume_mm3"]) > 0.01, "{r}");
            continue;
        }
        if sunk == 0.0 {
            assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
            assert_eq!(f(&r["near"][0]["distance_mm"]), 0.0, "{r}");
        } else {
            let v = f(&r["overlaps"][0]["volume_mm3"]);
            assert!((v - ring).abs() <= 1e-3 * ring, "{v} {ring}: {r}");
        }
    }
}

/// A chamfered pin through a hole of its own radius, its axis a hair off the hole's, the walls
/// all but tangent along two lines: the crescent the pin shares with the plate is reported,
/// against the lens of two circles.
#[test]
fn a_pin_a_hair_off_its_hole_shares_its_crescent() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    for offset in [5.9e-4, 1e-2] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [-10, -10, 0], "body_name": "Plate"}));
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [0, 0, -2], "operation": "cut", "targets": ["Plate"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [offset, 0, -2], "operation": "new", "body_name": "Pin"}));
        run(&mut s, "solid.chamfer", json!({"edges": [[offset + 3.0, 0, 7]], "distance": 0.5, "body": "Pin"}));
        let r = run(&mut s, "inspect.clearance", json!({"min_volume": 1e-9}));
        assert_eq!(r["unchecked"], json!([]), "{offset}: {r}");
        let want = 5.0 * crescent(3.0, offset);
        let got = f(&r["overlaps"][0]["volume_mm3"]);
        assert!((got - want).abs() < 1e-6 * want, "{offset}: {got} against {want}: {r}");
    }
}

/// A small box pressed 0.6 mm into a 2000 × 1000 × 1 mm plate shares 3 × 3 × 0.6 = 5.4 mm³: an
/// overlap, not an unchecked pair.
#[test]
fn a_small_overlap_on_a_large_plate_is_reported() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2000, "width": 1000, "height": 1, "corner": [0, 0, 0], "operation": "new", "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 3, "width": 3, "height": 3, "corner": [700, 400, 0.4], "operation": "new", "body_name": "Box"}));
    let r = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(r["unchecked"].as_array().map(Vec::len), Some(0), "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    assert_eq!(pair(&overlaps[0]), ["Box", "Plate"]);
    assert!((f(&overlaps[0]["volume_mm3"]) - 5.4).abs() < 1e-6, "{r}");
}

/// A 40 mm roller lying on a 2000 × 1000 × 3 mm plate, pressed 0.2 mm into it, shares the
/// circular segment along its 10 mm (7.53 mm³). Its curved wall is faceted differently on its
/// own and in the union, so vol(a) + vol(b) − vol(a ∪ b) comes out 0.64 mm³ over the shared
/// volume (2e-5 of the three volumes, past the 1 % of the shared part): the measuring allowance
/// keeps it an overlap, not an unchecked pair.
#[test]
fn a_roller_pressed_into_a_large_plate_is_reported() {
    let (r, depth, length) = (20.0_f64, 0.2_f64, 10.0);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2000, "width": 1000, "height": 3, "corner": [0, 0, 0], "operation": "new", "body_name": "Plate"}));
    run(
        &mut s,
        "solid.cylinder",
        json!({"radius": r, "height": length, "base": [100, 50, 3.0 + r - depth], "axis": [1, 0, 0], "operation": "new", "body_name": "Roller"}),
    );
    let res = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(res["unchecked"].as_array().map(Vec::len), Some(0), "{res}");
    let overlaps = res["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{res}");
    assert_eq!(pair(&overlaps[0]), ["Plate", "Roller"]);
    let want = (r * r * ((r - depth) / r).acos() - (r - depth) * (r * r - (r - depth) * (r - depth)).sqrt()) * length;
    assert!((f(&overlaps[0]["volume_mm3"]) - want).abs() < 0.01 * want, "{want} {res}");
}

/// Two STL copies of a cube inserted into a design. Returns the names of the two mesh bodies.
fn insert_two_mesh_cubes(s: &mut Session, tag: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("solidcraft-mesh-unchecked-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("cube.stl");
    let mut cube = Session::default();
    run(&mut cube, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [0, 0, 0], "body_name": "Cube"}));
    run(&mut cube, "file.export", json!({"path": stl.to_string_lossy()}));
    for _ in 0..2 {
        run(s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
    }
    let _ = std::fs::remove_dir_all(&dir);
    s.world_state().unwrap().bodies.iter().filter(|b| b.body.is_mesh()).map(|b| b.name.clone()).collect()
}

/// #3249: mesh bodies are not measured, and used to vanish from the answer (`bodies: 0`, nothing
/// unchecked). They are listed in `unchecked` with a reason, and naming one in `bodies` or
/// `against` is not "no solid body".
#[test]
fn mesh_bodies_are_listed_as_unchecked_and_can_be_named() {
    let mut s = Session::default();
    let meshes = insert_two_mesh_cubes(&mut s, "clearance");
    assert_eq!(meshes.len(), 2, "{meshes:?}");
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    assert_eq!((r["bodies"].as_u64(), r["pairs_checked"].as_u64()), (Some(0), Some(0)), "{r}");
    let unchecked = r["unchecked"].as_array().cloned().unwrap_or_default();
    assert_eq!(unchecked.len(), 2, "{r}");
    for (u, name) in unchecked.iter().zip(&meshes) {
        assert_eq!(u["a"].as_str(), Some(name.as_str()), "{r}");
        assert!(u["b"].is_null(), "{r}");
        assert_eq!(u["reason"], "mesh body: not checked", "{r}");
    }
    // Beside solid bodies: those are measured, the meshes are still listed.
    cube(&mut s, "A", 10.0, [100.0, 0.0, 0.0]);
    cube(&mut s, "B", 10.0, [105.0, 0.0, 0.0]);
    let r = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(r["bodies"], 2, "{r}");
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(r["unchecked"].as_array().map(Vec::len), Some(2), "{r}");
    // A mesh body named in `bodies` or `against` is listed, not refused; an unknown name still is.
    for params in [json!({"bodies": ["A", meshes[0]]}), json!({"bodies": ["A"], "against": [meshes[0]]})] {
        let r = run(&mut s, "inspect.clearance", params.clone());
        let unchecked = r["unchecked"].as_array().cloned().unwrap_or_default();
        assert_eq!(unchecked.len(), 1, "{params}: {r}");
        assert_eq!(unchecked[0]["a"].as_str(), Some(meshes[0].as_str()), "{params}: {r}");
    }
    let e = s.execute("inspect.clearance", &json!({"bodies": ["Nope"]})).unwrap_err().to_string();
    assert!(e.contains("`Nope`"), "{e}");
}

/// A pin turned from a profile (its wall in three faces) in the bore of a tube of its own size
/// (in two faces), its end inside the bore, a hair off the axis, and a slit tube with the pin
/// through it: touching, not overlapping. Their facets cross where one body's faces end on the
/// shared cylinder, and the pairs were left unchecked ("could not be intersected there").
#[test]
fn a_turned_pin_in_a_bore_of_its_size_is_contact() {
    let mut s = Session::default();
    let pin = |s: &mut Session, name: &str, off: [f64; 3], z0: f64, z1: f64| {
        run(s, "sketch.create", json!({"plane": {"origin": off, "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, z0], "p1": [3, z1]}));
        run(s, "solid.revolve", json!({"axis": "y", "body_name": name}));
    };
    let tube = |s: &mut Session, name: &str, x: f64| {
        run(s, "solid.cylinder", json!({"radius": 5.25, "height": 19.55, "base": [x, 0, 0], "body_name": name}));
        run(s, "solid.cylinder", json!({"radius": 3, "height": 19.55, "base": [x, 0, 0], "operation": "cut", "targets": [name]}));
    };
    pin(&mut s, "Pin", [2.2e-8, 4.7e-7, 0.0], -2.35, 9.65);
    tube(&mut s, "Tube", 0.0);
    pin(&mut s, "Through", [30.0, 0.0, 0.0], -3.0, 22.0);
    tube(&mut s, "Slit", 30.0);
    run(&mut s, "solid.box", json!({"length": 8, "width": 0.1, "height": 21, "corner": [22, -0.05, -1], "operation": "cut", "targets": ["Slit"]}));
    let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Pin", "Through"], "against": ["Tube", "Slit"], "clearance": 1e-6}));
    assert_eq!(r["unchecked"].as_array().map(Vec::len), Some(0), "{r}");
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.iter().map(pair).collect::<Vec<_>>(), [["Pin", "Tube"], ["Slit", "Through"]], "{r}");
    assert!(near.iter().all(|n| f(&n["distance_mm"]) == 0.0 && n.get("overlap_mm3").is_none()), "{r}");
}

/// A pin of a bore's size through a D-bore whose flat is 0.022 mm inside the pin shares a
/// sliver with the tube along the whole bore (the circular segment beyond the flat, 0.21 mm³):
/// an overlap, not contact. Only the facetings of the shared cylinder may cross unseen; the flat
/// reaches into it.
#[test]
fn a_pin_through_a_d_bore_of_its_size_overlaps_the_flat() {
    let (r, flat, length) = (3.0_f64, 2.978_f64, 19.55);
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": length, "base": [0, 0, 0], "body_name": "Tube"}));
    run(&mut s, "solid.cylinder", json!({"radius": r, "height": length, "base": [0, 0, 0], "operation": "cut", "targets": ["Tube"]}));
    run(&mut s, "solid.box", json!({"length": 1, "width": 2, "height": length, "corner": [flat, -1, 0], "operation": "join", "targets": ["Tube"]}));
    run(&mut s, "solid.cylinder", json!({"radius": r, "height": length + 6.0, "base": [0, 0, -3], "operation": "new", "body_name": "Pin"}));
    let res = run(&mut s, "inspect.clearance", json!({"bodies": ["Pin"], "against": ["Tube"], "clearance": 1e-6}));
    let want = (r * r * (flat / r).acos() - flat * (r * r - flat * flat).sqrt()) * length;
    let overlaps = res["overlaps"].as_array().cloned().unwrap_or_default();
    assert!(res["near"].as_array().is_none_or(|n| n.is_empty()), "{res}");
    assert_eq!(res["unchecked"].as_array().map(Vec::len), Some(0), "{res}");
    assert_eq!(overlaps.len(), 1, "{res}");
    assert!((f(&overlaps[0]["volume_mm3"]) - want).abs() < 0.05 * want, "{want} {res}");
}

/// #21554: a pair whose overlap boolean failed (`unchecked`) must not fall through to
/// `nearest_lists`, where `body_distance` would report the colliding pair as `0.0` mm contact.
#[test]
fn unchecked_pairs_are_excluded_from_nearest() {
    use std::cell::RefCell;

    use solidcore_record::Vec3;
    use solidcraft_geom::Aabb3;

    use super::{PairContext, PairResult, collect_pair_results};

    let bx = |x0: f64, x1: f64| Aabb3 { min: Vec3::new(x0, 0.0, 0.0), max: Vec3::new(x1, 10.0, 10.0) };
    // A overlaps B and D (both unchecked) and sits 5 mm clear of C; B touches C (0 mm) and D.
    let names = ["A", "B", "C", "D"];
    let subjects = [0, 1, 2, 3];
    let boxes = [bx(0.0, 10.0), bx(5.0, 15.0), bx(15.0, 25.0), bx(2.0, 12.0)];
    let allowed = |i: usize, j: usize| i != j;
    let ctx = PairContext { names: &names, subjects: &subjects, boxes: &boxes, allowed: &allowed, gap: 0.0, min_volume: 0.01, nearest: 2 };

    // (0, 1) failed in measure_pair (`r.unchecked`); (3, 0) failed as `Err` (reversed indices).
    let close = [(0, 1), (3, 0)];
    let results = [
        Ok(PairResult { unchecked: Some("an intersection was missed".into()), ..PairResult::default() }),
        Err(solidcraft_kernel::KernelError::Failed("boolean failed".into())),
    ];
    let mut unchecked = Vec::new();
    let asked = RefCell::new(Vec::new());
    let summary = collect_pair_results(&ctx, &close, results, &mut unchecked, |pairs| {
        asked.borrow_mut().extend_from_slice(pairs);
        pairs
            .iter()
            .map(|&(i, j)| {
                let d = match (i.min(j), i.max(j)) {
                    (0, 1) | (0, 3) | (1, 2) | (1, 3) => 0.0,
                    (0, 2) => 5.0,
                    (2, 3) => 3.0,
                    _ => 100.0,
                };
                Ok(Some(d))
            })
            .collect()
    });

    assert!(summary.overlaps.is_empty());
    assert!(summary.near.is_empty());
    assert_eq!(unchecked.len(), 2);
    assert!(asked.borrow().iter().all(|&p| p != (0, 1) && p != (0, 3)), "unchecked pairs must never be sent to body_distance: {:?}", asked.borrow());

    let a_near: Vec<(&str, f64)> = summary.nearest["A"].iter().map(|n| (n.body.as_str(), n.distance_mm)).collect();
    assert_eq!(a_near, vec![("C", 5.0)], "A must only report checked neighbour C: {a_near:?}");
    assert!(summary.nearest["B"].iter().all(|n| n.body != "A"), "B must not report unchecked A: {:?}", summary.nearest["B"]);
    assert!(summary.nearest["D"].iter().all(|n| n.body != "A"), "D must not report unchecked A: {:?}", summary.nearest["D"]);

    // Checked overlaps keep appearing in `nearest` at 0.0 mm as documented.
    let mut unchecked_ok = Vec::new();
    let overlap_results = [
        Ok(PairResult { overlap: Some((5.0, 20.0, bx(5.0, 10.0), Vec3::new(7.5, 5.0, 5.0))), ..PairResult::default() }),
        Ok(PairResult { unchecked: Some("an intersection was missed".into()), ..PairResult::default() }),
    ];
    let summary_ok = collect_pair_results(&ctx, &close, overlap_results, &mut unchecked_ok, |pairs| {
        pairs
            .iter()
            .map(|&(i, j)| {
                let d = match (i.min(j), i.max(j)) {
                    (0, 2) => 5.0,
                    (1, 2) | (1, 3) => 0.0,
                    (2, 3) => 3.0,
                    _ => 100.0,
                };
                Ok(Some(d))
            })
            .collect()
    });
    assert_eq!(summary_ok.overlaps.len(), 1);
    assert_eq!(unchecked_ok.len(), 1);
    let a_near_ok: Vec<(&str, f64)> = summary_ok.nearest["A"].iter().map(|n| (n.body.as_str(), n.distance_mm)).collect();
    assert_eq!(a_near_ok, vec![("B", 0.0), ("C", 5.0)]);
}

/// Reference overlap volume (mm³) for the fixed fixture below: a flat-faced rib penetrating
/// 1.35 mm (`d = 48.65 mm`) into an `R = 50 mm` cylinder over height `h = 13 mm`:
/// `(R² · arccos(d / R) − d · √(R² − d²)) · h ≈ 270.7796 mm³`.
const RIB_CYLINDER_OVERLAP_MM3: f64 = 270.7796;

/// Saves a hollow cylinder (`CylBody`, outer radius 50, inner radius 40, z 0..40), a part with a
/// mounting plate touching z = 40 and a rib penetrating 1.35 mm into the cylinder wall over z
/// 27..40 (~271 mm³, with no vertex of either solid strictly inside the other), and a touching-only
/// part without the penetrating rib.
fn save_linked_cylinder_and_rib_parts(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let cyl_path = dir.join("cyl.sc");
    let rib_path = dir.join("rib.sc");
    let touch_path = dir.join("touch.sc");

    let mut cyl = Session::default();
    run(&mut cyl, "solid.cylinder", json!({"radius": 50, "height": 40, "base": [0, 0, 0], "operation": "new", "body_name": "CylBody"}));
    run(&mut cyl, "solid.cylinder", json!({"radius": 40, "height": 42, "base": [0, 0, -1], "operation": "cut", "targets": ["CylBody"]}));
    run(&mut cyl, "file.save", json!({"path": cyl_path.to_string_lossy()}));

    // Under the 90° Z rotation matrix [0, -1, 0, tx,  1, 0, 0, ty,  0, 0, 1, tz]:
    // local (lx, ly, lz) maps to world (-ly + tx, lx + ty, lz + tz).
    // Plate: lx -10..10, ly -65..-42, lz 40..46 -> world relative to origin: x 42..65, y -10..10, z 40..46 (touches z = 40 at 0 mm gap).
    // Rib:   lx -20..20, ly -65..-48.65, lz 27..40 -> world relative: x 48.65..65, y -20..20, z 27..40 (1.35 mm penetration, all corners at r >= 52.6 > 50).
    let mut rib = Session::default();
    run(
        &mut rib,
        "solid.box",
        json!({"length": 20, "width": 23, "height": 6, "corner": [-10, -65, 40], "operation": "new", "body_name": "PartBody"}),
    );
    run(
        &mut rib,
        "solid.box",
        json!({"length": 40, "width": 16.35, "height": 13, "corner": [-20, -65, 27], "operation": "join", "targets": ["PartBody"]}),
    );
    run(&mut rib, "file.save", json!({"path": rib_path.to_string_lossy()}));

    let mut touch = Session::default();
    run(
        &mut touch,
        "solid.box",
        json!({"length": 20, "width": 23, "height": 6, "corner": [-10, -65, 40], "operation": "new", "body_name": "TouchBody"}),
    );
    run(&mut touch, "file.save", json!({"path": touch_path.to_string_lossy()}));

    (cyl_path, rib_path, touch_path)
}

/// 1-level linked sub-assembly: a cylinder body in a linked component checked against a part
/// inserted via `component.insert_design` with a non-identity rotation+translation matrix far from
/// the origin, followed by `timeline.compute_all`. Verifies both the ~271 mm³ rib overlap (where
/// no vertex of either solid is strictly inside the other) and the 0 mm touching-face contact.
#[test]
fn linked_subassembly_with_rotated_part_detects_rib_overlap_and_touching_face() {
    let dir = std::env::temp_dir().join(format!("solidcraft-clearance-linked-1level-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (cyl_path, rib_path, touch_path) = save_linked_cylinder_and_rib_parts(&dir);

    let tx = -430.5;
    let ty = -1100.0;
    let tz = -693.8051;
    let matrix = json!([0, -1, 0, tx, 1, 0, 0, ty, 0, 0, 1, tz]);
    let want_vol = RIB_CYLINDER_OVERLAP_MM3;

    // Part A: overlapping rib (1.35 mm penetration, ~271 mm³) + touching mounting face (0 mm gap).
    let mut sub = Session::default();
    let cyl_ins = run(&mut sub, "component.insert_design", json!({"path": cyl_path.to_string_lossy(), "name": "CylLink", "translate": [tx, ty, tz]}));
    let part_ins = run(&mut sub, "component.insert_design", json!({"path": rib_path.to_string_lossy(), "name": "RibLink", "matrix": matrix}));
    let cyl_body = cyl_ins["bodies"][0].as_str().unwrap().to_string();
    let part_body = part_ins["bodies"][0].as_str().unwrap().to_string();
    let rebuilt = run(&mut sub, "timeline.compute_all", json!({}));
    assert_eq!(rebuilt["errors"], 0, "{rebuilt}");

    // Confirm the test point lies strictly inside both world bodies.
    let probe = [tx + 49.3, ty, tz + 33.5];
    for name in [&cyl_body, &part_body] {
        let cls = run(&mut sub, "inspect.classify_points", json!({"body": name, "points": [probe]}));
        assert_eq!(cls["results"][0]["class"], "inside", "{name}: {cls}");
    }

    let r = run(&mut sub, "inspect.clearance", json!({"bodies": [&part_body], "against": [&cyl_body], "clearance": 0.5}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    assert_eq!(pair(&overlaps[0]), {
        let mut expected = [cyl_body.clone(), part_body.clone()];
        expected.sort();
        expected
    });
    let got_vol = f(&overlaps[0]["volume_mm3"]);
    assert!((got_vol - want_vol).abs() < 0.02 * want_vol, "got {got_vol} vs expected {want_vol}: {r}");
    assert!((f(&overlaps[0]["center"][0]) - (tx + 49.2)).abs() < 1.0, "{r}");
    assert!((f(&overlaps[0]["center"][1]) - ty).abs() < 1.0, "{r}");
    assert!((f(&overlaps[0]["center"][2]) - (tz + 33.5)).abs() < 1.0, "{r}");

    // Part B: touching-only mounting face (0 mm gap, no rib overlap) must report in `near` at 0 mm.
    let mut sub_touch = Session::default();
    let cyl_ins =
        run(&mut sub_touch, "component.insert_design", json!({"path": cyl_path.to_string_lossy(), "name": "CylLink", "translate": [tx, ty, tz]}));
    let touch_ins =
        run(&mut sub_touch, "component.insert_design", json!({"path": touch_path.to_string_lossy(), "name": "TouchLink", "matrix": matrix}));
    let cyl_body = cyl_ins["bodies"][0].as_str().unwrap().to_string();
    let touch_body = touch_ins["bodies"][0].as_str().unwrap().to_string();
    run(&mut sub_touch, "timeline.compute_all", json!({}));

    let r = run(&mut sub_touch, "inspect.clearance", json!({"bodies": [&touch_body], "against": [&cyl_body], "clearance": 0.5}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    assert_eq!(r["overlaps"], json!([]), "{r}");
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 1, "{r}");
    assert_eq!(f(&near[0]["distance_mm"]), 0.0, "{r}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 2-level nested linked sub-assembly: tests both inserting the saved sub-assembly into a parent
/// scene with a second rotation+translation matrix, and placing linked components under a nested
/// sub-assembly component (2-level occurrence path) with non-identity matrices at both levels.
#[test]
fn nested_two_level_linked_subassembly_detects_overlap() {
    let dir = std::env::temp_dir().join(format!("solidcraft-clearance-linked-2level-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (cyl_path, rib_path, _) = save_linked_cylinder_and_rib_parts(&dir);
    let sub_path = dir.join("subasm.sc");

    let tx = -430.5;
    let ty = -1100.0;
    let tz = -693.8051;
    let inner_matrix = json!([0, -1, 0, tx, 1, 0, 0, ty, 0, 0, 1, tz]);
    // Second rigid rotation+translation (+90° about X, plus translation) for the parent scene.
    let outer_matrix = json!([1, 0, 0, 250.0, 0, 0, -1, 120.0, 0, 1, 0, -80.0]);
    let want_vol = RIB_CYLINDER_OVERLAP_MM3;

    // Build and save the sub-assembly containing both linked components.
    let mut sub = Session::default();
    run(&mut sub, "component.insert_design", json!({"path": cyl_path.to_string_lossy(), "name": "CylLink", "translate": [tx, ty, tz]}));
    run(&mut sub, "component.insert_design", json!({"path": rib_path.to_string_lossy(), "name": "RibLink", "matrix": inner_matrix}));
    run(&mut sub, "file.save", json!({"path": sub_path.to_string_lossy()}));

    // Case 2a: saved sub-assembly inserted into a parent scene with a non-identity matrix.
    let mut scene_linked = Session::default();
    let ins =
        run(&mut scene_linked, "component.insert_design", json!({"path": sub_path.to_string_lossy(), "name": "SubAsm", "matrix": outer_matrix}));
    let bodies: Vec<String> = ins["bodies"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    assert_eq!(bodies.len(), 2, "{ins}");
    run(&mut scene_linked, "timeline.compute_all", json!({}));

    let r = run(&mut scene_linked, "inspect.clearance", json!({"bodies": [&bodies[1]], "against": [&bodies[0]], "clearance": 0.5}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    let got_vol = f(&overlaps[0]["volume_mm3"]);
    assert!((got_vol - want_vol).abs() < 0.02 * want_vol, "2a got {got_vol} vs expected {want_vol}: {r}");

    // Case 2b: true 2-level occurrence hierarchy (Scene -> SubAsm -> CylLink / RibLink), each with
    // non-identity rotation+translation matrices.
    let mut scene_nested = Session::default();
    let sub_comp = run(&mut scene_nested, "component.create", json!({"name": "SubAsm", "activate": false}));
    let sub_id = sub_comp["component"].as_u64().unwrap();
    let sub_occ = sub_comp["occurrence"].as_u64().unwrap();
    run(&mut scene_nested, "occurrence.move", json!({"occurrence": sub_occ, "matrix": outer_matrix, "capture": true, "move_grounded": true}));
    let cyl_ins = run(
        &mut scene_nested,
        "component.insert_design",
        json!({"path": cyl_path.to_string_lossy(), "name": "CylLink", "parent": sub_id, "translate": [tx, ty, tz]}),
    );
    let part_ins = run(
        &mut scene_nested,
        "component.insert_design",
        json!({"path": rib_path.to_string_lossy(), "name": "RibLink", "parent": sub_id, "matrix": inner_matrix}),
    );
    let cyl_body = cyl_ins["bodies"][0].as_str().unwrap().to_string();
    let part_body = part_ins["bodies"][0].as_str().unwrap().to_string();
    run(&mut scene_nested, "timeline.compute_all", json!({}));

    let r = run(&mut scene_nested, "inspect.clearance", json!({"bodies": [&part_body], "against": [&cyl_body], "clearance": 0.5}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    let got_vol = f(&overlaps[0]["volume_mm3"]);
    assert!((got_vol - want_vol).abs() < 0.02 * want_vol, "2b got {got_vol} vs expected {want_vol}: {r}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `solidcraft_kernel::overlap_witness` detects interior overlap when a rib pierces a cylinder
/// wall without any vertex of either solid lying inside the other, and returns `None` when two
/// solids only touch along a planar face (0 mm gap).
#[test]
fn overlap_witness_catches_rib_penetrating_cylinder_without_interior_vertices() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 50, "height": 40, "base": [0, 0, 0], "operation": "new", "body_name": "Cyl"}));
    run(&mut s, "solid.box", json!({"length": 16.35, "width": 40, "height": 13, "corner": [48.65, -20, 27], "operation": "new", "body_name": "Rib"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 6, "corner": [35, -10, 40], "operation": "new", "body_name": "Touch"}));
    let st = s.world_state().unwrap();
    let cyl = st.body("Cyl").unwrap();
    let rib = st.body("Rib").unwrap();
    let touch = st.body("Touch").unwrap();
    let (mc, mr, mt) = (cyl.mesh(), rib.mesh(), touch.mesh());
    let sc = solidcraft_kernel::DistanceShape::new(&cyl.body, &mc, cyl.display_tol());
    let sr = solidcraft_kernel::DistanceShape::new(&rib.body, &mr, rib.display_tol());
    let stouch = solidcraft_kernel::DistanceShape::new(&touch.body, &mt, touch.display_tol());

    let witness = solidcraft_kernel::overlap_witness(&sc, &sr).unwrap();
    assert!(witness.is_some(), "rib penetrating cylinder wall must yield an exact overlap witness");
    let no_witness = solidcraft_kernel::overlap_witness(&sc, &stouch).unwrap();
    assert!(no_witness.is_none(), "touching-only solids must not produce an overlap witness: {no_witness:?}");
}

/// `timeline.compute_all` clears `clearance_cache` so a full recompute measures pairs afresh.
#[test]
fn timeline_compute_all_clears_clearance_cache() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "B", 10.0, [9.0, 0.0, 0.0]);
    let first = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(first["pairs_reused"], 0, "{first}");
    let second = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(second["pairs_reused"], 1, "{second}");
    run(&mut s, "timeline.compute_all", json!({}));
    let after = run(&mut s, "inspect.clearance", json!({}));
    assert_eq!(after["pairs_reused"], 0, "{after}");
    assert_eq!(after["overlaps"].as_array().map(Vec::len), Some(1), "{after}");
}

/// Socket Reach (#21884), on the issue's example: a 19 mm socket over a bolt head with a wall
/// 12 mm from the axis has 2.5 mm to spare; a rib 3 mm out of the wall into its path blocks it.
/// The socket comes from the side the edge has no material on; flipped, it runs into the plate.
#[test]
fn a_socket_is_blocked_by_a_rib_in_its_path_and_not_by_a_wall_beside_it() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 100, "width": 60, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"base": [30, 30, 5], "radius": 6.5, "height": 5, "operation": "new", "body_name": "Head"}));
    run(&mut s, "solid.box", json!({"corner": [42, 0, 5], "length": 5, "width": 60, "height": 40, "operation": "new", "body_name": "Wall"}));
    // The head's top edge: a circle of radius 6.5 about (30, 30) at z = 10.
    let reach = |s: &mut Session, extra: Value| {
        let mut p = json!({"edge": [36.5, 30, 10], "diameter": 19, "length": 30});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        run(s, "inspect.socket_reach", p)
    };
    let clear = reach(&mut s, json!({}));
    assert_eq!(clear["clear"], json!(true), "{clear}");
    assert_eq!(clear["origin"], json!([30.0, 30.0, 10.0]), "{clear}");
    assert!((f(&clear["direction"][2]) - 1.0).abs() < 1e-9, "away from the head, up: {clear}");
    assert_eq!(clear["blocked"], json!([]), "the wall is 2.5 mm clear");
    // A rib 3 mm out of the wall reaches 0.5 mm into the socket's path.
    run(&mut s, "solid.box", json!({"corner": [39, 26, 5], "length": 3, "width": 8, "height": 40, "operation": "new", "body_name": "Rib"}));
    let bodies_before = s.model.state().bodies.len();
    let blocked = reach(&mut s, json!({}));
    assert_eq!(blocked["clear"], json!(false), "{blocked}");
    let rows = blocked["blocked"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.iter().map(|r| r["body"].as_str().unwrap_or("")).collect::<Vec<_>>(), ["Rib"], "{blocked}");
    assert!(f(&rows[0]["volume_mm3"]) > 0.0 && f(&rows[0]["from_mm"]).abs() < 1e-6, "{blocked}");
    // Ignored, the rib leaves the socket clear.
    assert_eq!(reach(&mut s, json!({"ignore": ["Rib"]}))["clear"], json!(true));
    // Flipped, the socket's space runs down through the head and the plate.
    let down = reach(&mut s, json!({"flip": true}));
    assert!((f(&down["direction"][2]) + 1.0).abs() < 1e-9, "{down}");
    let bodies: Vec<&str> = down["blocked"].as_array().into_iter().flatten().filter_map(|r| r["body"].as_str()).collect();
    assert!(bodies.contains(&"Plate") && bodies.contains(&"Head"), "{down}");
    // Nothing is added to the design.
    assert_eq!(s.model.state().bodies.len(), bodies_before);
    // Refused: a straight edge, a socket of no size, an unknown body to ignore.
    for (bad, why) in [
        (json!({"edge": [50, 0, 0], "diameter": 19, "length": 30}), "a straight edge"),
        (json!({"edge": [36.5, 30, 10], "diameter": 0, "length": 30}), "no diameter"),
        (json!({"edge": [36.5, 30, 10], "diameter": 19, "length": -1}), "a negative length"),
        (json!({"edge": [36.5, 30, 10], "diameter": 19, "length": 30, "ignore": ["Nope"]}), "an unknown body"),
    ] {
        assert!(s.execute("inspect.socket_reach", &bad).is_err(), "{why}");
    }
}

/// #22050: a tenon seated flush inside a matching slot/window (nominally coincident faces,
/// vertices within ~3e-5 mm) is flush contact (0 mm), not left in `unchecked`, while a tenon
/// that actually penetrates the slot wall is reported as an overlap (never falsely cleared),
/// and a small clearance gap reports its exact gap.
#[test]
fn flush_tenon_in_slot_is_contact_and_penetration_is_overlap() {
    let make_slotted_plate = |s: &mut Session| {
        run(s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-20, -20, 0], "body_name": "Plate"}));
        run(s, "solid.box", json!({"length": 10, "width": 10, "height": 14, "corner": [0, 0, -2], "operation": "new", "body_name": "Cut"}));
        run(s, "solid.combine", json!({"operation": "cut", "target": "Plate", "tools": ["Cut"]}));
    };

    // Flush contact cases: exact flush, shifted 3e-5 mm along the slot axis, and stepped tenon
    // with a shoulder sitting flush on top of the plate while its bottom vertices are 3e-5 mm
    // from the plate's bottom vertices.
    for (label, z_shift, shoulder) in [("exact_flush", 0.0, false), ("shifted_3e5", 3e-5, false), ("flush_shoulder_3e5", 3e-5, true)] {
        let mut s = Session::default();
        make_slotted_plate(&mut s);
        let tenon_height = if shoulder { 10.0 - z_shift } else { 10.0 };
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 10, "height": tenon_height, "corner": [0, 0, z_shift], "operation": "new", "body_name": "Tenon"}),
        );
        if shoulder {
            run(
                &mut s,
                "solid.box",
                json!({"length": 16, "width": 16, "height": 4, "corner": [-3, -3, 10.0], "operation": "join", "targets": ["Tenon"]}),
            );
        }
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "{label}: {r}");
        assert_eq!(r["overlaps"], json!([]), "{label}: {r}");
        let near = r["near"].as_array().cloned().unwrap_or_default();
        assert_eq!(near.len(), 1, "{label}: {r}");
        assert_eq!(f(&near[0]["distance_mm"]), 0.0, "{label}: {r}");
        assert_eq!(f(&r["nearest"]["Tenon"][0]["distance_mm"]), 0.0, "{label}: {r}");
    }

    // Genuine interference: tenon penetrating one slot wall by 0.2 mm (20 mm³) and 0.01 mm (1 mm³).
    for depth in [0.2, 0.01] {
        let mut s = Session::default();
        make_slotted_plate(&mut s);
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 10, "height": 10, "corner": [-depth, 0, 3e-5], "operation": "new", "body_name": "Tenon"}),
        );
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "depth {depth}: {r}");
        let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
        assert_eq!(overlaps.len(), 1, "depth {depth}: {r}");
        let want = depth * 10.0 * (10.0 - 3e-5);
        assert!((f(&overlaps[0]["volume_mm3"]) - want).abs() < 1e-2 * want, "depth {depth}: {r}");
    }

    // Tiny penetration (3e-5 mm over a 50 × 10 mm wall = 0.015 mm³ >= min_volume) must never be
    // falsely cleared as flush contact.
    {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 80, "width": 80, "height": 10, "corner": [-15, -15, 0], "body_name": "Plate"}));
        run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 14, "corner": [0, 0, -2], "operation": "new", "body_name": "Cut"}));
        run(&mut s, "solid.combine", json!({"operation": "cut", "target": "Plate", "tools": ["Cut"]}));
        run(
            &mut s,
            "solid.box",
            json!({"length": 50, "width": 50, "height": 10, "corner": [-3e-5, 0, 3e-5], "operation": "new", "body_name": "Tenon"}),
        );
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
        let unchecked = r["unchecked"].as_array().cloned().unwrap_or_default();
        assert!(!overlaps.is_empty() || !unchecked.is_empty(), "3e-5 penetration must not be cleared: {r}");
    }

    // Small-gap clearance: tenon shrunk inside the slot by 0.05 mm and 3e-5 mm all round.
    for gap in [0.05, 3e-5] {
        let mut s = Session::default();
        make_slotted_plate(&mut s);
        let size = 10.0 - 2.0 * gap;
        run(
            &mut s,
            "solid.box",
            json!({"length": size, "width": size, "height": 10, "corner": [gap, gap, 3e-5], "operation": "new", "body_name": "Tenon"}),
        );
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "gap {gap}: {r}");
        assert_eq!(r["overlaps"], json!([]), "gap {gap}: {r}");
        let near = r["near"].as_array().cloned().unwrap_or_default();
        assert_eq!(near.len(), 1, "gap {gap}: {r}");
        assert!((f(&near[0]["distance_mm"]) - gap).abs() < 1e-6, "gap {gap}: {r}");
    }
}

/// #22050: when an overlap check still fails and lands in `unchecked`, a computable non-boolean
/// minimum distance is reported on the `unchecked` entry while keeping the pair omitted from
/// `overlaps`, `near`, and `nearest` (#21554).
#[test]
fn unchecked_pair_carries_non_boolean_distance_while_staying_excluded_from_nearest() {
    use solidcore_record::Vec3;
    use solidcraft_geom::Aabb3;
    use solidcraft_kernel::BodyDistance;

    use super::{PairContext, PairResult, collect_pair_results};

    let bx = |x0: f64, x1: f64| Aabb3 { min: Vec3::new(x0, 0.0, 0.0), max: Vec3::new(x1, 10.0, 10.0) };
    let names = ["A", "B", "C", "D"];
    let subjects = [0, 1, 2, 3];
    let boxes = [bx(0.0, 10.0), bx(10.0, 20.0), bx(25.0, 35.0), bx(10.0, 20.0)];
    let allowed = |i: usize, j: usize| i != j;
    let ctx = PairContext { names: &names, subjects: &subjects, boxes: &boxes, allowed: &allowed, gap: 1.0, min_volume: 0.01, nearest: 2 };

    let close = [(0, 1), (0, 3)];
    let results = [
        Ok(PairResult {
            unchecked: Some("relate conflict".into()),
            distance: Some(BodyDistance { distance: 0.0, from: Vec3::new(10.0, 5.0, 5.0), to: Vec3::new(10.0, 5.0, 5.0), exact: true }),
            ..PairResult::default()
        }),
        Ok(PairResult {
            unchecked: Some("certified_vertex_gaps".into()),
            distance: Some(BodyDistance { distance: 0.25, from: Vec3::new(10.0, 5.0, 5.0), to: Vec3::new(10.25, 5.0, 5.0), exact: false }),
            ..PairResult::default()
        }),
    ];
    let mut unchecked = Vec::new();
    let summary = collect_pair_results(&ctx, &close, results, &mut unchecked, |pairs| {
        pairs.iter().map(|&(i, j)| Ok(Some(if (i.min(j), i.max(j)) == (0, 2) { 15.0 } else { 5.0 }))).collect()
    });

    assert!(summary.overlaps.is_empty());
    assert!(summary.near.is_empty());
    assert_eq!(unchecked.len(), 2);
    assert_eq!(unchecked[0].distance_mm, Some(0.0));
    assert_eq!(unchecked[0].exact, None);
    assert_eq!(unchecked[1].distance_mm, Some(0.25));
    assert_eq!(unchecked[1].exact, Some(false));
    let exact_json = serde_json::to_value(&unchecked[0]).unwrap();
    let mesh_json = serde_json::to_value(&unchecked[1]).unwrap();
    assert!(exact_json.get("exact").is_none(), "{exact_json}");
    assert_eq!(mesh_json.get("exact"), Some(&json!(false)), "{mesh_json}");
    let a_near: Vec<(&str, f64)> = summary.nearest["A"].iter().map(|n| (n.body.as_str(), n.distance_mm)).collect();
    assert_eq!(a_near, vec![("C", 15.0)]);
}

#[test]
fn memory_policy_scales_concurrency_chunks_and_refuses_below_headroom_floor() {
    use super::{
        DEFAULT_CHUNK_PAIRS, MAX_CLEARANCE_THREADS, MAX_SHAPE_CACHE_BYTES, MIN_HEADROOM_BYTES, MIN_SHAPE_CACHE_BYTES, check_memory_headroom,
        chunk_size_for_memory, clearance_worker_limit, shape_cache_budget,
    };

    const GB: u64 = 1 << 30;
    const MB: u64 = 1 << 20;

    // Unknown host memory uses normal bounded defaults (never unbounded `usize::MAX`).
    assert_eq!(clearance_worker_limit(1000, None), MAX_CLEARANCE_THREADS);
    assert_eq!(chunk_size_for_memory(None), DEFAULT_CHUNK_PAIRS);
    assert_eq!(shape_cache_budget(None), MAX_SHAPE_CACHE_BYTES);

    // Concurrency and chunk sizes step down smoothly as headroom tightens.
    assert_eq!(clearance_worker_limit(1000, Some(16 * GB)), MAX_CLEARANCE_THREADS);
    assert_eq!(clearance_worker_limit(1000, Some(6 * GB)), 4);
    assert_eq!(clearance_worker_limit(1000, Some(3 * GB)), 2);
    assert_eq!(clearance_worker_limit(1000, Some(GB)), 1);
    assert_eq!(clearance_worker_limit(0, Some(16 * GB)), 1);
    assert_eq!(clearance_worker_limit(3, Some(16 * GB)), 3);

    assert_eq!(chunk_size_for_memory(Some(16 * GB)), DEFAULT_CHUNK_PAIRS);
    assert_eq!(chunk_size_for_memory(Some(3 * GB)), 32);
    assert_eq!(chunk_size_for_memory(Some(GB)), 16);

    assert_eq!(shape_cache_budget(Some(16 * GB)), MAX_SHAPE_CACHE_BYTES);
    assert_eq!(shape_cache_budget(Some(512 * MB)), 64 << 20);
    assert_eq!(shape_cache_budget(Some(64 * MB)), MIN_SHAPE_CACHE_BYTES);

    // Above the headroom floor succeeds; below it returns a clear error instead of risking OOM abort.
    assert!(check_memory_headroom("inspect.clearance", None, 100, 100).is_ok());
    assert!(check_memory_headroom("inspect.clearance", Some(MIN_HEADROOM_BYTES), 100, 100).is_ok());
    let err = check_memory_headroom("inspect.clearance", Some(MIN_HEADROOM_BYTES - 1), 42, 200).unwrap_err().to_string();
    assert!(err.contains("not enough available memory") && err.contains("42 of 200 pairs remaining"), "{err}");
}

/// Placed component occurrences checked by `inspect.clearance` share their definition's display
/// mesh (never caching deep-cloned moved meshes on each occurrence), keep lazy B-rep records
/// unmaterialized for bodies pruned before exact checks, and release transient SMLib/ops kernel
/// forms on completion while preserving forms that were already cached beforehand.
#[test]
fn placed_occurrences_share_definition_mesh_and_release_transient_kernel_forms() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part"}));
    cube(&mut s, "Block", 10.0, [0.0, 0.0, 0.0]);
    run(&mut s, "component.activate", json!({"component": "root"}));
    // Place multiple rigid occurrences of `Part`:
    // - Part:1 at origin (identity)
    // - Part:2 overlapping Part:1 by 1 mm³ at (9, 9, 9)
    // - Part:3 near Part:1 with 2 mm gap at (12, 0, 0)
    // - Part:4 far away at (500, 0, 0) so broad-phase AABB prunes it before exact geometry.
    run(&mut s, "occurrence.copy", json!({"component": "Part", "translate": [9.0, 9.0, 9.0]}));
    run(&mut s, "occurrence.copy", json!({"component": "Part", "translate": [12.0, 0.0, 0.0]}));
    run(&mut s, "occurrence.copy", json!({"component": "Part", "translate": [500.0, 0.0, 0.0]}));

    let world_before = s.world_state().unwrap();
    let names: Vec<String> = world_before.bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 4, "{names:?}");
    // Ensure a pre-existing kernel form on the first body is preserved across `inspect.clearance`.
    let prewarmed_name = names[0].clone();
    let _ = world_before.bodies[0].body.exact_bounds().unwrap();
    assert!(world_before.bodies[0].body.holds_kernel_form());
    for b in world_before.bodies.iter().skip(1) {
        assert!(!b.body.holds_kernel_form(), "{} started cold", b.name);
        assert!(b.cached_mesh().is_none(), "{} started without a moved mesh copy", b.name);
        assert!(b.body.has_pending_lazy_record(), "{} started with a lazy moved record", b.name);
    }

    let r = run(&mut s, "inspect.clearance", json!({"bodies": [names[0], names[3]], "against": [names[1], names[2]], "clearance": 6.0}));
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    assert!((f(&overlaps[0]["volume_mm3"]) - 1.0).abs() < 1e-6, "{r}");
    assert_eq!(near.len(), 1, "{r}");
    assert!((f(&near[0]["distance_mm"]) - 2.0).abs() < 1e-6, "{r}");
    assert_eq!(r["unchecked"], json!([]), "{r}");

    let world_after = s.world_state().unwrap();
    for b in &world_after.bodies {
        if b.name == prewarmed_name {
            assert!(b.body.holds_kernel_form(), "pre-existing kernel form on {} must be preserved", b.name);
        } else {
            assert!(!b.body.holds_kernel_form(), "transient kernel form on {} must be released", b.name);
            assert!(b.cached_mesh().is_none(), "placed occurrence {} must not cache a moved display mesh copy", b.name);
        }
    }
    // The far-away occurrence at x = 500 was pruned before exact geometry and kept its lazy record.
    let far = world_after.bodies.iter().find(|b| b.name == names[3]).unwrap();
    assert!(far.body.has_pending_lazy_record(), "pruned occurrence must keep its lazy record unmaterialized");
}

/// Multi-chunk `inspect.clearance` (more than `DEFAULT_CHUNK_PAIRS` close pairs) produces exact
/// overlaps, near gaps, and nearest neighbours across chunk boundaries, and releases transient
/// kernel forms once done.
#[test]
fn chunked_clearance_across_many_close_pairs_matches_exact_results() {
    let mut s = Session::default();
    // Two rows of 9 cubes each (9 × 9 = 81 cross-pairs, > DEFAULT_CHUNK_PAIRS = 64) packed within
    // a 6 mm clearance window so all 81 pairs survive broad-phase pruning and span multiple chunks.
    let mut moving = Vec::new();
    let mut fixed = Vec::new();
    for i in 0..9usize {
        let m_name = format!("M{i}");
        let f_name = format!("F{i}");
        let y = (i as f64) * 1.5;
        cube(&mut s, &m_name, 1.0, [0.0, y, 0.0]);
        // F0 overlaps M0 by 0.2 × 1 × 1 = 0.2 mm³; F1..F8 sit 1.0 mm away along X from M1..M8.
        let x = if i == 0 { 0.8 } else { 2.0 };
        cube(&mut s, &f_name, 1.0, [x, y, 0.0]);
        moving.push(m_name);
        fixed.push(f_name);
    }

    // Large clearance (20 mm) makes all 81 moving×fixed pairs pass AABB pruning (> 64 chunk size).
    let r = run(&mut s, "inspect.clearance", json!({"bodies": moving, "against": fixed, "clearance": 20.0, "nearest": 1}));
    assert_eq!(r["pairs_checked"], 81, "{r}");
    assert_eq!(r["unchecked"], json!([]), "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    assert_eq!(pair(&overlaps[0]), ["F0", "M0"]);
    assert!((f(&overlaps[0]["volume_mm3"]) - 0.2).abs() < 1e-6, "{r}");
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 80, "all 80 non-overlapping pairs are within 20 mm: {r}");
    // M1's nearest fixed neighbour is F0 (X overlaps [0.8, 1.0], Y gap = 0.5 mm);
    // M2's nearest fixed neighbour is F2 at exactly 1.0 mm along X.
    assert_eq!(r["nearest"]["M1"][0]["body"], "F0", "{r}");
    assert!((f(&r["nearest"]["M1"][0]["distance_mm"]) - 0.5).abs() < 1e-6, "{r}");
    assert_eq!(r["nearest"]["M2"][0]["body"], "F2", "{r}");
    assert!((f(&r["nearest"]["M2"][0]["distance_mm"]) - 1.0).abs() < 1e-6, "{r}");
}

/// Under severe host memory pressure, `inspect.clearance` returns a structured error instead of
/// proceeding into an allocation abort, leaving the session intact for subsequent calls.
#[test]
fn low_host_memory_returns_error_and_preserves_session() {
    let mut s = Session::default();
    cube(&mut s, "A", 10.0, [0.0, 0.0, 0.0]);
    cube(&mut s, "B", 10.0, [11.0, 0.0, 0.0]);

    let low = solidcraft_kernel::with_available_memory_for_test(Some(64 << 20), || s.execute("inspect.clearance", &json!({"clearance": 2.0})));
    let err = low.unwrap_err().to_string();
    assert!(err.contains("not enough available memory"), "{err}");

    // Once memory headroom recovers, the session and design are intact and the same call succeeds.
    let ok =
        solidcraft_kernel::with_available_memory_for_test(Some(16u64 << 30), || s.execute("inspect.clearance", &json!({"clearance": 2.0}))).unwrap();
    let near = ok["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 1, "{ok}");
    assert!((f(&near[0]["distance_mm"]) - 1.0).abs() < 1e-6, "{ok}");
}

/// Algebraic `count_allowed_pairs` matches the brute-force `O(N^2)` loop identically across
/// `SkipWithin`, `against` (disjoint and overlapping with `subjects`), nested/root/same-path
/// component envelopes, and overlapping exclusions where both `skip` and `envelopes` exclude a pair.
#[test]
fn algebraic_allowed_pair_count_matches_brute_force() {
    use super::{SkipWithin, count_allowed_pairs};

    let brute_force = |n: usize,
                       in_subjects: &[bool],
                       in_against: &[bool],
                       has_against: bool,
                       path_by_body: &[&[u64]],
                       is_envelope: &[bool],
                       skip: SkipWithin|
     -> usize {
        let flag = |v: &[bool], i: usize| v.get(i).copied().unwrap_or(false);
        let path_of = |i: usize| path_by_body.get(i).copied().unwrap_or(&[]);
        let env_of = |i: usize| is_envelope.get(i).copied().unwrap_or(false);
        let allowed = |i: usize, j: usize| -> bool {
            if i == j {
                return false;
            }
            let paired = if !has_against {
                flag(in_subjects, i) && flag(in_subjects, j)
            } else {
                (flag(in_subjects, i) && flag(in_against, j)) || (flag(in_subjects, j) && flag(in_against, i))
            };
            if !paired {
                return false;
            }
            let (a, b) = (path_of(i), path_of(j));
            let encloses = (env_of(i) && b.starts_with(a)) || (env_of(j) && a.starts_with(b));
            if encloses {
                return false;
            }
            match skip {
                SkipWithin::Nothing => true,
                SkipWithin::Occurrence => a.is_empty() || a != b,
                SkipWithin::Component => a.is_empty() || a.first() != b.first(),
            }
        };
        let mut total = 0usize;
        for i in 0..n {
            for j in i + 1..n {
                if allowed(i, j) {
                    total += 1;
                }
            }
        }
        total
    };

    // Rich fixture exercising root, shallow, deep, same-path, nested, and inactive envelopes plus
    // overlapping subject/against roles.
    let paths: Vec<&[u64]> = vec![
        &[],        // 0: root envelope (both)
        &[],        // 1: root body (subject)
        &[],        // 2: root body (against)
        &[1],       // 3: comp 1 envelope A (against)
        &[1],       // 4: comp 1 envelope B (both) — same path as 3 & 5
        &[1],       // 5: comp 1 body (subject)
        &[1, 2],    // 6: nested envelope under comp 1 (subject)
        &[1, 2],    // 7: nested body under comp 1 (against)
        &[1, 2, 3], // 8: deep body enclosed by 0, 3, 4, 6 (both)
        &[2],       // 9: comp 2 body (subject)
        &[2, 5],    // 10: comp 2 child body (against)
        &[1],       // 11: inactive envelope (neither subject nor against)
    ];
    let in_subjects = [true, true, false, false, true, true, true, false, true, true, false, false];
    let in_against = [true, false, true, true, true, false, false, true, true, false, true, false];
    let is_env_all = [true, false, false, true, true, false, true, false, false, false, false, true];
    let no_env: &[bool] = &[];

    for skip in [SkipWithin::Nothing, SkipWithin::Occurrence, SkipWithin::Component] {
        for has_against in [false, true] {
            for is_env in [&is_env_all[..], no_env] {
                let got = count_allowed_pairs(paths.len(), &in_subjects, &in_against, has_against, &paths, is_env, skip);
                let want = brute_force(paths.len(), &in_subjects, &in_against, has_against, &paths, is_env, skip);
                assert_eq!(got, want, "has_against={has_against}, envs={}", !is_env.is_empty());
            }
        }
    }

    // Degenerate sizes (0 and 1 bodies) and deterministic pseudo-random stress sweep.
    for n in [0usize, 1] {
        let s = vec![true; n];
        let a = vec![true; n];
        let p: Vec<&[u64]> = vec![&[]; n];
        let e = vec![false; n];
        for skip in [SkipWithin::Nothing, SkipWithin::Occurrence, SkipWithin::Component] {
            for has_against in [false, true] {
                assert_eq!(count_allowed_pairs(n, &s, &a, has_against, &p, &e, skip), 0);
            }
        }
    }

    let palette: [&[u64]; 8] = [&[], &[1], &[2], &[1, 10], &[1, 11], &[1, 10, 20], &[2, 30], &[3]];
    let mut seed: u64 = 0x5EED_CAFE_1234_5678;
    let mut next_u64 = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    for case in 0..100 {
        let n = (next_u64() % 16) as usize + 1;
        let has_against = (next_u64() & 1) == 1;
        let skip = match next_u64() % 3 {
            0 => SkipWithin::Nothing,
            1 => SkipWithin::Occurrence,
            _ => SkipWithin::Component,
        };
        let mut s = Vec::with_capacity(n);
        let mut a = Vec::with_capacity(n);
        let mut p = Vec::with_capacity(n);
        let mut e = Vec::with_capacity(n);
        let allow_env = (next_u64() & 3) != 0;
        for _ in 0..n {
            let r = next_u64();
            s.push((r & 1) != 0);
            a.push((r & 2) != 0);
            p.push(palette[((r >> 2) % palette.len() as u64) as usize]);
            e.push(allow_env && ((r >> 6) & 3) == 0);
        }
        let got = count_allowed_pairs(n, &s, &a, has_against, &p, &e, skip);
        let want = brute_force(n, &s, &a, has_against, &p, &e, skip);
        assert_eq!(got, want, "case {case} failed (n={n}, has_against={has_against})");
    }
}

/// `nearest_lists` deduplicates shared candidate pairs across subjects in `O(1)` via `todo_set`
/// so each pair is sent to `measure` at most once per round.
#[test]
fn nearest_lists_deduplicates_shared_candidates_in_each_round() {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    use solidcore_record::Vec3;
    use solidcraft_geom::Aabb3;

    use super::nearest_lists;

    let bx = |x: f64| Aabb3 { min: Vec3::new(x, 0.0, 0.0), max: Vec3::new(x + 1.0, 1.0, 1.0) };
    let boxes = [bx(0.0), bx(2.0), bx(5.0), bx(9.0)];
    let subjects = [0, 1, 2, 3];
    let allowed = |i: usize, j: usize| i != j;
    let mut known = HashMap::new();
    let batches = RefCell::new(Vec::<Vec<(usize, usize)>>::new());

    let lists = nearest_lists(&subjects, boxes.len(), 2, &boxes, &allowed, &mut known, |pairs| {
        batches.borrow_mut().push(pairs.to_vec());
        let unique: HashSet<(usize, usize)> = pairs.iter().copied().collect();
        assert_eq!(unique.len(), pairs.len(), "duplicate pairs in round: {pairs:?}");
        pairs.iter().map(|&(i, j)| Ok(Some(solidcraft_geom::mesh_distance::box_gap(&boxes[i], &boxes[j])))).collect()
    });

    assert!(!batches.borrow().is_empty());
    assert_eq!(lists[&0], vec![(1, 1.0), (2, 4.0)]);
    assert_eq!(lists[&1], vec![(0, 1.0), (2, 2.0)]);
    assert_eq!(lists[&2], vec![(1, 2.0), (3, 3.0)]);
    assert_eq!(lists[&3], vec![(2, 3.0), (1, 6.0)]);
}

/// Four identical countersunk nuts (Ø7 shank, 90° countersunk head Ø10×1.5, total length 13.5 mm)
/// seated flush (0 mm contact, zero overlap) in countersunk bores of two mirrored side rails placed
/// at 0° and 180° Z rotations far from the origin (`x = ±800 mm`). All four mirror-identical pairs
/// must resolve symmetrically to `near` at 0.0 mm with `unchecked_count == 0`, while a nut seated
/// 0.2 mm too deep must report its exact interference volume (~7.39 mm³).
#[test]
fn mirrored_countersunk_nuts_in_rails_are_contact_and_penetration_is_overlap() {
    let dir = std::env::temp_dir().join(format!("solidcraft-clearance-mirrored-nuts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rail_path = dir.join("rail.sc");
    let nuts_path = dir.join("nuts.sc");

    let hole_ys = [2.5_f64, -32.5_f64];
    let hole_z = -43.5_f64;

    // Rail side wall at x = 800..806 with two through 90° countersunk bores (Ø7 shank, Ø10×1.5 head).
    let mut rail = Session::default();
    run(&mut rail, "solid.box", json!({"length": 6, "width": 120, "height": 80, "corner": [800, -60, -80], "operation": "new", "body_name": "Rail"}));
    for cy in hole_ys {
        run(
            &mut rail,
            "solid.hole",
            json!({
                "position": [806, cy, hole_z],
                "direction": [-1, 0, 0],
                "diameter": 7,
                "type": "countersink",
                "cs_diameter": 10,
                "cs_angle": 90,
                "targets": ["Rail"],
            }),
        );
    }
    run(&mut rail, "file.save", json!({"path": rail_path.to_string_lossy()}));

    // Pair of identical flush countersunk nuts (90° Ø10×1.5 head + Ø7×12 shank, length 13.5 mm).
    let mut nuts = Session::default();
    for (idx, cy) in hole_ys.iter().enumerate() {
        let name = format!("Nut{}", idx + 1);
        run(
            &mut nuts,
            "solid.cone",
            json!({"base": [806, cy, hole_z], "axis": [-1, 0, 0], "radius": 5.0, "top_radius": 3.5, "height": 1.5, "operation": "new", "body_name": name}),
        );
        run(
            &mut nuts,
            "solid.cylinder",
            json!({"base": [804.5, cy, hole_z], "axis": [-1, 0, 0], "radius": 3.5, "height": 12.0, "operation": "join", "targets": [name]}),
        );
    }
    run(&mut nuts, "file.save", json!({"path": nuts_path.to_string_lossy()}));

    // Place both sides in exact 0° and 180° Z symmetry at x = ±800..±806 mm.
    let identity = json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    let rot_z_180 = json!([-1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0]);
    let mut asm = Session::default();
    let rail_pos = run(&mut asm, "component.insert_design", json!({"path": rail_path.to_string_lossy(), "name": "RailPos", "matrix": identity}));
    let rail_neg = run(&mut asm, "component.insert_design", json!({"path": rail_path.to_string_lossy(), "name": "RailNeg", "matrix": rot_z_180}));
    let nuts_pos = run(&mut asm, "component.insert_design", json!({"path": nuts_path.to_string_lossy(), "name": "NutsPos", "matrix": identity}));
    let nuts_neg = run(&mut asm, "component.insert_design", json!({"path": nuts_path.to_string_lossy(), "name": "NutsNeg", "matrix": rot_z_180}));
    let rebuilt = run(&mut asm, "timeline.compute_all", json!({}));
    assert_eq!(rebuilt["errors"], 0, "{rebuilt}");

    let mut rail_bodies: Vec<String> = Vec::new();
    for ins in [&rail_pos, &rail_neg] {
        rail_bodies.extend(ins["bodies"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)));
    }
    let mut nut_bodies: Vec<String> = Vec::new();
    for ins in [&nuts_pos, &nuts_neg] {
        nut_bodies.extend(ins["bodies"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)));
    }
    assert_eq!((rail_bodies.len(), nut_bodies.len()), (2, 4));

    let r = run(&mut asm, "inspect.clearance", json!({"bodies": nut_bodies, "against": rail_bodies, "clearance": 0.5, "min_volume": 0.001}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    assert_eq!(r["unchecked_count"], 0, "{r}");
    assert_eq!(r["overlaps"], json!([]), "{r}");
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 4, "all 4 mirror-identical nut/rail contact pairs must be near at 0 mm: {r}");
    for entry in &near {
        assert_eq!(f(&entry["distance_mm"]), 0.0, "{entry}");
        assert!(entry.get("exact").is_none(), "distance must be exact on analytic cone/cylinder faces: {entry}");
    }

    let inter = run(&mut asm, "inspect.interference", json!({}));
    assert_eq!(inter["count"], 0, "{inter}");
    assert_eq!(inter["unchecked"], json!([]), "{inter}");

    // Soundness check: a nut seated 0.2 mm too deep penetrates the countersink wall (~7.391 mm³)
    // and must be reported as an overlap, never cleared as contact.
    let mut deep = Session::default();
    run(&mut deep, "solid.box", json!({"length": 6, "width": 120, "height": 80, "corner": [800, -60, -80], "operation": "new", "body_name": "Rail"}));
    run(
        &mut deep,
        "solid.hole",
        json!({
            "position": [806, hole_ys[0], hole_z],
            "direction": [-1, 0, 0],
            "diameter": 7,
            "type": "countersink",
            "cs_diameter": 10,
            "cs_angle": 90,
            "targets": ["Rail"],
        }),
    );
    run(
        &mut deep,
        "solid.cone",
        json!({"base": [805.8, hole_ys[0], hole_z], "axis": [-1, 0, 0], "radius": 5.0, "top_radius": 3.5, "height": 1.5, "operation": "new", "body_name": "Bushing"}),
    );
    run(
        &mut deep,
        "solid.cylinder",
        json!({"base": [804.3, hole_ys[0], hole_z], "axis": [-1, 0, 0], "radius": 3.5, "height": 12.0, "operation": "join", "targets": ["Bushing"]}),
    );
    let r = run(&mut deep, "inspect.clearance", json!({"bodies": ["Bushing"], "against": ["Rail"], "clearance": 0.5, "min_volume": 0.001}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    assert_eq!(r["unchecked_count"], 0, "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    let want_vol = 7.391_120_316;
    let got_vol = f(&overlaps[0]["volume_mm3"]);
    assert!((got_vol - want_vol).abs() < 0.05 * want_vol, "got {got_vol} vs expected {want_vol}: {r}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `retry_reframed_intersect` recovers both empty flush contact and world-mapped overlap volumes
/// in recentered and rotated SO(3) local frames, respects cancellation, and `inspect.clearance`
/// surfaces `unchecked_count` matching `unchecked.len()`.
#[test]
fn reframed_intersect_recovers_empty_and_overlap_and_tracks_unchecked_count() {
    use solidcore_record::Vec3;
    use solidcraft_geom::prelude::*;

    use super::{ReframedIntersect, retry_reframed_intersect};

    // Far from origin: exercises translation recentering for both flush contact (Empty) and overlap.
    let far_a = solidcraft_kernel::box_solid(Vec3::new(800.0, -40.0, -40.0), Vec3::new(810.0, -30.0, -30.0)).unwrap();
    let far_flush = solidcraft_kernel::box_solid(Vec3::new(810.0, -40.0, -40.0), Vec3::new(820.0, -30.0, -30.0)).unwrap();
    let far_overlap = solidcraft_kernel::box_solid(Vec3::new(808.0, -40.0, -40.0), Vec3::new(818.0, -30.0, -30.0)).unwrap();

    assert!(matches!(retry_reframed_intersect(&far_a, &far_flush, Vec3::new(810.0, -35.0, -35.0), &|| false), ReframedIntersect::Empty));
    match retry_reframed_intersect(&far_a, &far_overlap, Vec3::new(809.0, -35.0, -35.0), &|| false) {
        ReframedIntersect::Overlap(m) => {
            assert!((m.volume - 200.0).abs() < 1e-6, "{m:?}");
            assert!((m.centroid - Vec3::new(809.0, -35.0, -35.0)).len() < 1e-6, "{m:?}");
            assert!((m.bbox.min - Vec3::new(808.0, -40.0, -40.0)).len() < 1e-6, "{m:?}");
            assert!((m.bbox.max - Vec3::new(810.0, -30.0, -30.0)).len() < 1e-6, "{m:?}");
        }
        other => panic!("expected far-from-origin overlap, got {other:?}"),
    }
    assert!(matches!(retry_reframed_intersect(&far_a, &far_overlap, Vec3::new(809.0, -35.0, -35.0), &|| true), ReframedIntersect::Cancelled));

    // Near origin (`at = ZERO`): exercises SO(3) rotation + inverse rotation back to world space.
    let near_a = solidcraft_kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let near_flush = solidcraft_kernel::box_solid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)).unwrap();
    let near_overlap = solidcraft_kernel::box_solid(Vec3::new(8.0, 0.0, 0.0), Vec3::new(18.0, 10.0, 10.0)).unwrap();

    assert!(matches!(retry_reframed_intersect(&near_a, &near_flush, Vec3::ZERO, &|| false), ReframedIntersect::Empty));
    match retry_reframed_intersect(&near_a, &near_overlap, Vec3::ZERO, &|| false) {
        ReframedIntersect::Overlap(m) => {
            assert!((m.volume - 200.0).abs() < 1e-6, "{m:?}");
            assert!((m.centroid - Vec3::new(9.0, 5.0, 5.0)).len() < 1e-6, "{m:?}");
            assert!((m.bbox.min - Vec3::new(8.0, 0.0, 0.0)).len() < 1e-6, "{m:?}");
            assert!((m.bbox.max - Vec3::new(10.0, 10.0, 10.0)).len() < 1e-6, "{m:?}");
        }
        other => panic!("expected rotated near-origin overlap, got {other:?}"),
    }

    // `unchecked_count` tracks `unchecked.len()` at the top level of the `inspect.clearance` reply.
    let mut s = Session::default();
    let meshes = insert_two_mesh_cubes(&mut s, "unchecked-count");
    assert_eq!(meshes.len(), 2);
    let r = run(&mut s, "inspect.clearance", json!({"clearance": 1}));
    assert_eq!(r["unchecked_count"], 2, "{r}");
    assert_eq!(r["unchecked"].as_array().map(Vec::len), Some(2), "{r}");
}

/// A Ø12 x 2 disk nested two sub-assembly levels deep, seated inside a Ø12.4 x 4.2 pocket of a
/// housing (0.2 mm radial clearance, bottom at z = 3.0 above pocket floor z = 0.8, top coplanar
/// with the housing top at z = 5.0) far from the origin at (688.2, 656.8), must report zero
/// overlaps and exact 0.2 mm clearance against both an identity instance and a 180°-rotated
/// instance of the housing — while a genuinely embedded disk inside a solid block reports its
/// full volume (π * 6² * 2 mm³).
#[test]
fn nested_disk_in_pocket_far_from_origin_has_no_overlap_at_identity_or_180_deg() {
    let dir = std::env::temp_dir().join(format!("solidcraft-clearance-pocket-disk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let housing_path = dir.join("housing.sc");
    let disk_path = dir.join("disk.sc");
    let inner_sa_path = dir.join("inner_subasm.sc");
    let outer_sa_path = dir.join("outer_subasm.sc");

    let (cx, cy) = (688.2, 656.8);

    // 1. Save Housing part: 450 x 50 x 5 block with Ø12.4 x 4.2 pocket at (cx, cy), z = 0.8..5.0.
    {
        let mut s = Session::default();
        run(
            &mut s,
            "solid.box",
            json!({"length": 450, "width": 50, "height": 5, "corner": [cx - 25.0, cy - 25.0, 0.0], "operation": "new", "body_name": "Housing"}),
        );
        run(&mut s, "solid.cylinder", json!({"radius": 6.2, "height": 5.2, "base": [cx, cy, 0.8], "operation": "cut", "targets": ["Housing"]}));
        run(&mut s, "file.save", json!({"path": housing_path.to_string_lossy()}));
    }

    // 2. Save Disk part: Ø12 x 2 at origin, rotated so its seam lies on the lower half.
    {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 6.0, "height": 2.0, "base": [0, 0, 0], "operation": "new", "body_name": "Disk"}));
        run(&mut s, "solid.move", json!({"bodies": ["Disk"], "axis": [0, 0, 1], "angle": "180 deg"}));
        run(&mut s, "file.save", json!({"path": disk_path.to_string_lossy()}));
    }

    // 3. Build 2-level nested sub-assembly: OuterSubAsm -> InnerSubAsm -> two Disk instances
    //    (one seated at (cx, cy, 3.0) for the identity housing, one at (-cx, -cy, 3.0) for the
    //    180°-rotated housing).
    {
        let mut inner = Session::default();
        run(&mut inner, "component.insert_design", json!({"path": disk_path.to_string_lossy(), "name": "DiskPos", "translate": [cx, cy, 3.0]}));
        run(&mut inner, "component.insert_design", json!({"path": disk_path.to_string_lossy(), "name": "DiskNeg", "translate": [-cx, -cy, 3.0]}));
        run(&mut inner, "file.save", json!({"path": inner_sa_path.to_string_lossy()}));
    }
    {
        let mut outer = Session::default();
        run(&mut outer, "component.insert_design", json!({"path": inner_sa_path.to_string_lossy(), "name": "InnerSubAsm"}));
        run(&mut outer, "file.save", json!({"path": outer_sa_path.to_string_lossy()}));
    }

    // 4. Assemble OuterSubAsm at identity + Housing twice (identity and rotated 180° about Z).
    let mut asm = Session::default();
    let sa_ins = run(&mut asm, "component.insert_design", json!({"path": outer_sa_path.to_string_lossy(), "name": "OuterSubAsm"}));
    let h_id_ins = run(&mut asm, "component.insert_design", json!({"path": housing_path.to_string_lossy(), "name": "HousingId"}));
    let rot180 = json!([-1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0]);
    let h_rot_ins = run(&mut asm, "component.insert_design", json!({"path": housing_path.to_string_lossy(), "name": "HousingRot", "matrix": rot180}));
    let sa_bodies: Vec<String> = sa_ins["bodies"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    let housing_bodies = vec![h_id_ins["bodies"][0].as_str().unwrap().to_string(), h_rot_ins["bodies"][0].as_str().unwrap().to_string()];
    assert_eq!(sa_bodies.len(), 2, "{sa_ins}");
    let rebuilt = run(&mut asm, "timeline.compute_all", json!({}));
    assert_eq!(rebuilt["errors"], 0, "{rebuilt}");

    let r = run(&mut asm, "inspect.clearance", json!({"bodies": sa_bodies, "against": housing_bodies, "clearance": 1.0, "min_volume": 0.001}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    assert_eq!(r["overlaps"], json!([]), "pocket-seated disks must not report false whole-body overlap: {r}");
    let near = r["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 2, "both identity and 180°-rotated housings should report 0.2 mm radial clearance: {r}");
    for entry in &near {
        assert!((f(&entry["distance_mm"]) - 0.2).abs() < 1e-4, "expected 0.2 mm radial clearance: {r}");
    }

    // 5. Contrast: a Ø12 x 2 disk truly embedded in a solid block (no pocket) reports its full
    //    volume π * 6² * 2.
    let mut embedded = Session::default();
    run(
        &mut embedded,
        "solid.box",
        json!({"length": 60, "width": 60, "height": 5, "corner": [cx - 30.0, cy - 30.0, 0.0], "operation": "new", "body_name": "SolidBlock"}),
    );
    run(
        &mut embedded,
        "solid.cylinder",
        json!({"radius": 6.0, "height": 2.0, "base": [cx, cy, 3.0], "operation": "new", "body_name": "EmbeddedDisk"}),
    );
    let r = run(&mut embedded, "inspect.clearance", json!({"clearance": 1.0, "min_volume": 0.001}));
    assert_eq!(r["unchecked"], json!([]), "{r}");
    let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
    assert_eq!(overlaps.len(), 1, "{r}");
    let want_vol = std::f64::consts::PI * 36.0 * 2.0;
    assert!((f(&overlaps[0]["volume_mm3"]) - want_vol).abs() < 1e-2, "expected full disk volume {want_vol}: {r}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// An 18 mm tenon seated flush inside a matching 18 mm square window of a plate that also has
/// circular bolt holes is verified cleanly as 0 mm contact (not `unchecked`), reports ~0.08 mm
/// when given a 0.08 mm clearance gap, and still reports a genuine 0.05 mm penetration as an
/// overlap (zero false negatives).
#[test]
fn tenon_in_window_with_bolt_holes_flush_gap_and_penetration() {
    let make_plate = |s: &mut Session| {
        run(s, "solid.box", json!({"length": 100, "width": 100, "height": 12, "corner": [-40, -40, 0], "body_name": "Plate"}));
        run(s, "solid.box", json!({"length": 18, "width": 18, "height": 16, "corner": [0, 0, -2], "operation": "new", "body_name": "WindowCut"}));
        run(s, "solid.combine", json!({"operation": "cut", "target": "Plate", "tools": ["WindowCut"]}));
        run(s, "solid.cylinder", json!({"radius": 3, "height": 16, "base": [-20, 9, -2], "operation": "cut", "targets": ["Plate"]}));
        run(s, "solid.cylinder", json!({"radius": 3, "height": 16, "base": [38, 9, -2], "operation": "cut", "targets": ["Plate"]}));
    };

    // 1. Flush contact (exact flush and 3e-5 mm axial shift): verified as 0 mm contact, not unchecked.
    for (label, z_shift) in [("exact_flush", 0.0), ("shifted_3e5", 3e-5)] {
        let mut s = Session::default();
        make_plate(&mut s);
        run(
            &mut s,
            "solid.box",
            json!({"length": 18, "width": 18, "height": 12, "corner": [0, 0, z_shift], "operation": "new", "body_name": "Tenon"}),
        );
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "{label}: {r}");
        assert_eq!(r["overlaps"], json!([]), "{label}: {r}");
        let near = r["near"].as_array().cloned().unwrap_or_default();
        assert_eq!(near.len(), 1, "{label}: {r}");
        assert_eq!(f(&near[0]["distance_mm"]), 0.0, "{label}: {r}");
        assert_eq!(f(&r["nearest"]["Tenon"][0]["distance_mm"]), 0.0, "{label}: {r}");

        let inter = run(&mut s, "inspect.interference", json!({}));
        assert_eq!(inter["count"], 0, "{label}: {inter}");
        assert_eq!(inter["unchecked"], json!([]), "{label}: {inter}");
    }

    // 2. Small 0.08 mm clearance gap all round: verified cleanly with distance_mm ≈ 0.08.
    {
        let mut s = Session::default();
        make_plate(&mut s);
        let gap = 0.08;
        let size = 18.0 - 2.0 * gap;
        run(
            &mut s,
            "solid.box",
            json!({"length": size, "width": size, "height": 12, "corner": [gap, gap, 0], "operation": "new", "body_name": "Tenon"}),
        );
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "{r}");
        assert_eq!(r["overlaps"], json!([]), "{r}");
        let near = r["near"].as_array().cloned().unwrap_or_default();
        assert_eq!(near.len(), 1, "{r}");
        assert!((f(&near[0]["distance_mm"]) - gap).abs() < 1e-6, "{r}");
        assert!((f(&r["nearest"]["Tenon"][0]["distance_mm"]) - gap).abs() < 1e-6, "{r}");
    }

    // 3. Genuine 0.05 mm penetration into one window wall: still detected as overlap.
    {
        let mut s = Session::default();
        make_plate(&mut s);
        let depth = 0.05;
        run(
            &mut s,
            "solid.box",
            json!({"length": 18, "width": 18, "height": 12, "corner": [-depth, 0, 3e-5], "operation": "new", "body_name": "Tenon"}),
        );
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "{r}");
        let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
        assert_eq!(overlaps.len(), 1, "{r}");
        let want = depth * 18.0 * (12.0 - 3e-5);
        assert!((f(&overlaps[0]["volume_mm3"]) - want).abs() < 1e-2 * want, "{r}");
    }
}

/// A shim/spacer rotated 180 degrees sitting flush against two side plates (with through bores)
/// and a dowel pin seated flush inside the matching bore: every pair is verified as 0 mm contact
/// (not `unchecked`), while a 0.05 mm penetration of either the spacer or the dowel pin is still
/// reported as an overlap.
#[test]
fn rotated_spacer_flush_against_side_plates_and_dowel_pin() {
    let build_assembly = |s: &mut Session, spacer_pen: f64, pin_pen: f64| {
        // Side plate A: z in [0, 10], with a Ø6 dowel bore at (0, 0) and a Ø4 hole at (12, 0).
        run(s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "SideA"}));
        run(s, "solid.cylinder", json!({"radius": 3, "height": 14, "base": [0, 0, -2], "operation": "cut", "targets": ["SideA"]}));
        run(s, "solid.cylinder", json!({"radius": 2, "height": 14, "base": [12, 0, -2], "operation": "cut", "targets": ["SideA"]}));

        // Spacer: z in [10 - spacer_pen, 16 - spacer_pen], rotated 180° around Z, with matching Ø6 bore.
        run(
            s,
            "solid.box",
            json!({"length": 40, "width": 40, "height": 6, "corner": [-20, -20, 10.0 - spacer_pen], "operation": "new", "body_name": "Spacer"}),
        );
        run(s, "solid.cylinder", json!({"radius": 3, "height": 10, "base": [0, 0, 8.0 - spacer_pen], "operation": "cut", "targets": ["Spacer"]}));
        run(s, "solid.move", json!({"bodies": ["Spacer"], "axis": [0, 0, 1], "angle": "180 deg"}));

        // Side plate B: z in [16, 26], flush with the opposite side of the spacer, with matching Ø6 bore.
        run(s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 16], "operation": "new", "body_name": "SideB"}));
        run(s, "solid.cylinder", json!({"radius": 3, "height": 14, "base": [0, 0, 14], "operation": "cut", "targets": ["SideB"]}));

        // Dowel pin: Ø6 (or Ø6 + 2*pin_pen) passing through all three bores.
        run(s, "solid.cylinder", json!({"radius": 3.0 + pin_pen, "height": 30, "base": [0, 0, -2], "operation": "new", "body_name": "Dowel"}));
    };

    // 1. All flush contact: zero overlaps, zero unchecked, all touching pairs at 0.0 mm.
    {
        let mut s = Session::default();
        build_assembly(&mut s, 0.0, 0.0);
        let r = run(&mut s, "inspect.clearance", json!({"clearance": 1, "nearest": 1}));
        assert_eq!(r["unchecked"], json!([]), "{r}");
        assert_eq!(r["overlaps"], json!([]), "{r}");
        let near = r["near"].as_array().cloned().unwrap_or_default();
        let pairs: Vec<[String; 2]> = near.iter().map(pair).collect();
        for expected in [
            ["SideA".to_string(), "Spacer".to_string()],
            ["SideB".to_string(), "Spacer".to_string()],
            ["Dowel".to_string(), "SideA".to_string()],
            ["Dowel".to_string(), "Spacer".to_string()],
            ["Dowel".to_string(), "SideB".to_string()],
        ] {
            assert!(pairs.contains(&expected), "missing flush contact {expected:?}: {r}");
        }
        assert!(near.iter().all(|n| f(&n["distance_mm"]) == 0.0), "{r}");
    }

    // 2. Genuine 0.05 mm spacer penetration into SideA: detected as overlap.
    {
        let mut s = Session::default();
        build_assembly(&mut s, 0.05, 0.0);
        let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Spacer"], "against": ["SideA"], "clearance": 1}));
        assert_eq!(r["unchecked"], json!([]), "{r}");
        let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
        assert_eq!(overlaps.len(), 1, "{r}");
        assert!(f(&overlaps[0]["volume_mm3"]) > 50.0, "{r}");
    }

    // 3. Genuine 0.05 mm radial penetration of Dowel into Spacer bore: detected as overlap.
    {
        let mut s = Session::default();
        build_assembly(&mut s, 0.0, 0.05);
        let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Dowel"], "against": ["Spacer"], "clearance": 1}));
        assert_eq!(r["unchecked"], json!([]), "{r}");
        let overlaps = r["overlaps"].as_array().cloned().unwrap_or_default();
        assert_eq!(overlaps.len(), 1, "{r}");
        let want = std::f64::consts::PI * (3.05_f64.powi(2) - 3.0_f64.powi(2)) * 6.0;
        assert!((f(&overlaps[0]["volume_mm3"]) - want).abs() < 0.02 * want, "want {want}: {r}");
    }
}
