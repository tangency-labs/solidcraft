use serde_json::{Value, json};

use crate::Session;
use solidcraft_kernel::DistanceShape;

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

/// What the clearance check leans on to skip the intersection: where two bodies' faces cross
/// but their coarse meshes stay apart (the facets run inside a sphere), the distance on the exact
/// faces is still 0. A sphere dipping into a cube's top edge by a little, its mesh made coarse.
#[test]
fn crossing_faces_measure_0_where_coarse_meshes_stay_apart() {
    use solidcraft_geom::Vec3;
    let cube = solidcraft_kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let cube_mesh = cube.tessellate(0.01).unwrap();
    let mut apart = 0;
    for off in [1.0, 2.0, 3.0, 5.0] {
        for dip in [0.05, 0.1, 0.2] {
            let radius = 20.0_f64;
            let centre = Vec3::new(10.0 + off, 5.0, 10.0 - dip + (radius * radius - off * off).sqrt());
            let ball = solidcraft_kernel::sphere(centre, radius).unwrap();
            let chord = 1.0;
            let ball_mesh = ball.tessellate(chord).unwrap();
            let (a, b) = (DistanceShape::new(&cube, &cube_mesh, 0.01), DistanceShape::new(&ball, &ball_mesh, chord));
            if a.mesh_closest(&b, 10.0).is_none_or(|m| m.distance <= 1e-6) {
                continue;
            }
            apart += 1;
            let d = solidcraft_kernel::body_distance(&a, &b, 10.0).unwrap().unwrap();
            assert!(d.exact && d.distance < 1e-6, "off {off}, dip {dip}: {} apart", d.distance);
        }
    }
    assert!(apart > 0, "no case where the meshes stay apart");
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

/// A shared part whose centre lies outside its own box, or which holds more than its box, is
/// malformed: the pair is reported unchecked rather than with that volume.
#[test]
fn a_malformed_shared_part_is_not_measured() {
    use solidcraft_geom::{Aabb3, Vec3};
    let bbox = Aabb3 { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(10.0, 10.0, 1.0) };
    assert!(super::plausible(50.0, Vec3::new(5.0, 5.0, 0.5), bbox));
    assert!(!super::plausible(50.0, Vec3::new(5.0, 5.0, 3.0), bbox));
    assert!(!super::plausible(1239.0, Vec3::new(5.0, 5.0, 0.5), bbox));
    assert!(!super::plausible(f64::NAN, Vec3::new(5.0, 5.0, 0.5), bbox));
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
        let near = &r["near"][0];
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
/// is in the flange (about 0.49 mm³), next to walls that lie on one another: the boolean tried
/// for minutes there and gave up; now it is reported as such within seconds.
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
        let started = std::time::Instant::now();
        let r = run(&mut s, "inspect.clearance", json!({"bodies": ["Pin"], "against": ["Bush"], "min_volume": 0.01, "clearance": 1e-6}));
        assert!(started.elapsed().as_secs_f64() < 30.0, "sunk {sunk}: {:?}", started.elapsed());
        if fillet {
            assert!(r["unchecked"][0]["reason"].as_str().is_some_and(|w| w.contains("pin on one cylinder")), "{r}");
            continue;
        }
        assert_eq!(r["unchecked"], json!([]), "sunk {sunk}: {r}");
        if sunk == 0.0 {
            assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
            assert_eq!(f(&r["near"][0]["distance_mm"]), 0.0, "{r}");
        } else {
            let v = f(&r["overlaps"][0]["volume_mm3"]);
            assert!((v - ring).abs() <= 1e-3 * ring, "{v} {ring}: {r}");
        }
    }
}

/// A chamfered pin through a hole of its own radius, its axis a hair off the hole's: the boolean
/// cannot intersect walls all but tangent along two lines and once passed the pair as clear.
/// The crescent the pin shares with the plate is reported, against the lens of two circles.
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

/// The shared volume is checked against the bodies and their union: two 10 mm cubes sharing a
/// 3 mm corner cube agree at 27 mm³, and a shared part read as three times that (as a mirrored
/// pair once was) does not.
#[test]
fn a_shared_volume_the_union_contradicts_is_caught() {
    use solidcraft_geom::Vec3;
    let a = solidcraft_kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap_or_else(|e| panic!("{e}"));
    let b = solidcraft_kernel::box_solid(Vec3::new(7.0, 7.0, 7.0), Vec3::new(17.0, 17.0, 17.0)).unwrap_or_else(|e| panic!("{e}"));
    let at = Vec3::new(8.5, 8.5, 8.5);
    assert_eq!(super::union_disagrees(&a, &b, 27.0, at), None);
    assert!(super::union_disagrees(&a, &b, 81.0, at).is_some());
}

/// The check works on measured volumes, and vol(a) + vol(b) − vol(a ∪ b) carries the error of
/// all three: a small overlap on large bodies can come out near zero, or below it. A union that
/// cannot resolve the shared part leaves it standing; one that clearly contradicts it does not.
#[test]
fn the_union_check_allows_for_the_bodies_measuring_error() {
    use super::contradicted;
    // 0.96 mm³ shared where the union of 40 000 and 300 mm³ implies -0.75: noise, not a contradiction.
    assert_eq!(contradicted(0.96, 40_000.0, 300.0, 40_300.75), None);
    // 5.92 mm³ of a small body in a large sheet, with the union 0.4 mm³ off the bodies' sum.
    assert_eq!(contradicted(5.92, 250_000.0, 80.0, 250_074.48), None);
    // Past the noise, but the union implies nothing at all while the shared part is small next
    // to both bodies: the union cannot tell, and the shared part stands.
    assert_eq!(contradicted(6.0, 400.0, 200.0, 600.5), None);
    // 412 mm³ shared where bodies of a few thousand and their union imply 141: caught.
    assert!(contradicted(411.98, 3_200.0, 3_150.0, 6_350.0 - 141.18).is_some());
    // A large shared part the union says is not there at all: caught too.
    assert!(contradicted(900.0, 3_200.0, 3_150.0, 6_350.0).is_some());
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
    s.world_state().bodies.iter().filter(|b| b.body.is_mesh()).map(|b| b.name.clone()).collect()
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
