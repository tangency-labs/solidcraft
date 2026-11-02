//! Measure on holes, circles, arcs and slots (#1392): centre-to-centre and axis-to-edge
//! distances with the closest and farthest listed too, diameters and radii, slot sizes. Every
//! expected value is worked out by hand from the part's dimensions.

use std::f64::consts::{PI, TAU};

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn close(v: &Value, want: f64, tol: f64) -> bool {
    v.as_f64().is_some_and(|x| (x - want).abs() <= tol)
}

/// A 60 × 40 × 10 plate with a Ø6 hole at (15, 20) and a Ø8 hole at (45, 20).
fn plate(s: &mut Session) {
    run(s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [15, 20, -1], "operation": "cut"}));
    run(s, "solid.cylinder", json!({"radius": 4, "height": 12, "base": [45, 20, -1], "operation": "cut"}));
}

fn body(s: &Session) -> String {
    s.model.state().bodies.first().unwrap().name.clone()
}

/// The cylindrical face of radius `r`, as a pick.
fn hole(s: &Session, r: f64) -> Value {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let n = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap();
    let f = (0..n)
        .find(|&f| matches!(solidcraft_kernel::face_surface(&b.body, f), Some(solidcraft_kernel::FaceSurface::Cylinder { radius, .. }) if (radius - r).abs() < 1e-6))
        .unwrap_or_else(|| panic!("no hole of radius {r}"));
    let p = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf as usize == f).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    json!({"type": "face", "body": b.name, "index": f, "point": p})
}

/// The planar face all of whose points satisfy `on`, as a pick.
fn flat(s: &Session, on: impl Fn(Vec3) -> bool) -> Value {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let n = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap();
    let f = (0..n)
        .find(|&f| {
            let tris: Vec<_> = m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf as usize == f).filter_map(|(t, _)| m.tri(t)).collect();
            !tris.is_empty() && tris.iter().all(|t| t.iter().all(|p| on(*p)))
        })
        .unwrap();
    let p = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf as usize == f).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    json!({"type": "face", "body": b.name, "index": f, "point": p})
}

/// The mesh edge all of whose points satisfy `on`, as a pick.
fn edge(s: &Session, on: impl Fn(Vec3) -> bool) -> Value {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let i = m.edges.iter().position(|e| e.iter().all(|p| on(*p))).expect("edge");
    let e = &m.edges[i];
    json!({"type": "edge", "body": b.name, "index": i, "point": e[e.len() / 2]})
}

fn measure(s: &mut Session, items: Vec<Value>) -> Value {
    run(s, "inspect.measure", json!({ "items": items }))
}

#[test]
fn connected_lines_measure_the_corner_regardless_of_endpoint_or_selection_order() {
    // cos(angle) = -3/5: the corner is 126.869898°, not its 53.130102° supplement.
    for (end, want) in
        [([-24.0, 32.0], (-0.6f64).acos().to_degrees()), ([24.0, 32.0], 0.6f64.acos().to_degrees()), ([0.0, 40.0], 90.0), ([-40.0, 0.0], 180.0)]
    {
        for reverse_a in [false, true] {
            for reverse_b in [false, true] {
                let mut s = Session::default();
                run(&mut s, "sketch.create", json!({"plane": "XY"}));
                let mut a = [[0.0, 0.0], [40.0, 0.0]];
                let mut b = [[0.0, 0.0], end];
                if reverse_a {
                    a.reverse();
                }
                if reverse_b {
                    b.reverse();
                }
                run(&mut s, "sketch.line", json!({"points": a}));
                run(&mut s, "sketch.line", json!({"points": b}));
                let (a, b) = (json!({"type": "sketch_curve", "id": "l1"}), json!({"type": "sketch_curve", "id": "l2"}));
                for items in [vec![a.clone(), b.clone()], vec![b, a]] {
                    let r = measure(&mut s, items);
                    assert!(close(&r["angle_deg"], want, 1e-8), "{reverse_a} {reverse_b}: {r}");
                }
            }
        }
    }
}

#[test]
fn solid_edges_measure_the_obtuse_corner() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[40, 0], [0, 0], [-24, 32]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10}));
    let a = edge(&s, |p| p.z.abs() < 1e-8 && p.y.abs() < 1e-8);
    let b = edge(&s, |p| p.z.abs() < 1e-8 && (4.0 * p.x + 3.0 * p.y).abs() < 1e-8);
    for items in [vec![a.clone(), b.clone()], vec![b, a]] {
        let r = measure(&mut s, items);
        assert!(close(&r["angle_deg"], (-0.6f64).acos().to_degrees(), 1e-8), "{r}");
    }
}

#[test]
fn separate_and_duplicate_lines_keep_the_undirected_angle() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 10], [-24, 42]]}));
    let (a, b) = (json!({"type": "sketch_curve", "id": "l1"}), json!({"type": "sketch_curve", "id": "l2"}));
    let r = measure(&mut s, vec![a.clone(), b]);
    assert!(close(&r["angle_deg"], 0.6f64.acos().to_degrees(), 1e-8), "{r}");
    let r = measure(&mut s, vec![a.clone(), a]);
    assert!(close(&r["angle_deg"], 0.0, 1e-8), "{r}");
}

#[test]
fn two_holes_measure_centre_to_centre_with_min_and_max() {
    let mut s = Session::default();
    plate(&mut s);
    let (a, b) = (hole(&s, 3.0), hole(&s, 4.0));
    let r = measure(&mut s, vec![a.clone(), b.clone()]);
    assert!(close(&r["center_distance"], 30.0, 1e-9), "{r}");
    assert!(close(&r["min"], 30.0 - 3.0 - 4.0, 1e-9), "{r}");
    assert!(close(&r["max"], (37.0f64).hypot(10.0), 1e-9), "{r}");
    assert_eq!(r["measure"], "center");
    // Backward compatible: the closest distance is still `distance_mm`, with its points.
    assert!(close(&r["distance_mm"], 23.0, 1e-9) && r["from"].is_array() && r["to"].is_array(), "{r}");
    // The centre line runs between the two axes, square to them.
    let (f, t) = (&r["center_from"], &r["center_to"]);
    assert!(close(&f[0], 15.0, 1e-9) && close(&f[1], 20.0, 1e-9) && close(&t[0], 45.0, 1e-9) && close(&t[1], 20.0, 1e-9), "{r}");
    assert!(close(&f[2], t[2].as_f64().unwrap(), 1e-9), "{r}");
    // Each hole: Ø, R and circumference.
    assert!(close(&r["items"][0]["diameter"], 6.0, 1e-9) && close(&r["items"][0]["radius"], 3.0, 1e-9), "{r}");
    assert!(close(&r["items"][1]["circumference"], 8.0 * PI, 1e-9), "{r}");
    // The two rims on the top face: the same centres, the rims' gap and span.
    let rim = |c: f64, rr: f64| move |p: Vec3| (p.z - 10.0).abs() < 1e-6 && ((p - Vec3::new(c, 20.0, 10.0)).len() - rr).abs() < 1e-6;
    let (ea, eb) = (edge(&s, rim(15.0, 3.0)), edge(&s, rim(45.0, 4.0)));
    let r = measure(&mut s, vec![ea.clone(), eb]);
    assert!(close(&r["center_distance"], 30.0, 1e-9) && close(&r["min"], 23.0, 1e-9) && close(&r["max"], 37.0, 1e-9), "{r}");
    // One rim: Ø, R and circumference first-class.
    let r = measure(&mut s, vec![ea]);
    assert!(close(&r["diameter"], 6.0, 1e-9) && close(&r["radius"], 3.0, 1e-9) && close(&r["circumference"], TAU * 3.0, 1e-9), "{r}");
    // Its length is its circumference, not the length of the polyline drawn for it (#9086).
    assert!(close(&r["items"][0]["length_mm"], TAU * 3.0, 1e-9), "{r}");
    assert_eq!(r["items"][0]["round"], "circle", "{r}");
}

#[test]
fn elevated_cylinder_rims_measure_between_the_picked_circles_centres() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":10,"body_name":"A"}));
    run(&mut s, "solid.cylinder", json!({"base":[34,19,22],"radius":4,"height":10,"body_name":"B","operation":"new_body"}));
    let items: Vec<_> = s
        .model
        .state()
        .bodies
        .iter()
        .map(|b| {
            let z = if b.name == "A" { 10.0 } else { 32.0 };
            let m = b.mesh();
            let index = m.edges.iter().position(|e| e.len() > 2 && e.iter().all(|p| (p.z - z).abs() < 1e-9)).unwrap();
            let e = &m.edges[index];
            json!({"type":"edge","body":b.name,"index":index,"point":e[e.len()/2]})
        })
        .collect();
    assert_eq!(items.len(), 2);
    for picks in [items.clone(), items.into_iter().rev().collect()] {
        let r = measure(&mut s, picks);
        assert!(close(&r["center_distance"], 2001_f64.sqrt(), 1e-9), "{r}");
        for (endpoint, item) in [("center_from", 0), ("center_to", 1)] {
            for axis in 0..3 {
                assert!(close(&r[endpoint][axis], r["items"][item]["center"][axis].as_f64().unwrap(), 1e-9), "{r}");
            }
        }
        assert!((r["center_delta"][2].as_f64().unwrap().abs() - 22.0).abs() < 1e-9, "{r}");
        assert!(close(&r["min"], (1517_f64.sqrt() - 8.0).hypot(22.0), 1e-9), "{r}");
        assert!(close(&r["max"], (1517_f64.sqrt() + 8.0).hypot(22.0), 1e-9), "{r}");
    }
}

#[test]
fn an_elevated_circle_to_a_straight_edge_keeps_its_height_difference() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":68,"width":50,"height":6,"body_name":"Plate"}));
    run(&mut s, "solid.cylinder", json!({"base":[34,25,6],"radius":12,"height":14,"body_name":"Boss","operation":"new_body"}));
    let straight = edge(&s, |p| p.y.abs() < 1e-9 && (p.z - 6.0).abs() < 1e-9);
    let round = {
        let st = s.model.state();
        let b = st.bodies.iter().find(|b| b.name == "Boss").unwrap();
        let m = b.mesh();
        let index = m.edges.iter().position(|e| e.len() > 2 && e.iter().all(|p| (p.z - 20.0).abs() < 1e-9)).unwrap();
        let e = &m.edges[index];
        json!({"type":"edge","body":b.name,"index":index,"point":e[e.len()/2]})
    };
    for items in [vec![straight.clone(), round.clone()], vec![round, straight]] {
        let r = measure(&mut s, items);
        assert!(close(&r["center_distance"], 25_f64.hypot(14.0), 1e-9), "{r}");
        let (line, circle) = if r["items"][0]["round"] == "circle" { ("center_to", "center_from") } else { ("center_from", "center_to") };
        for (key, expected) in [(line, [34.0, 0.0, 6.0]), (circle, [34.0, 25.0, 20.0])] {
            for (axis, value) in expected.into_iter().enumerate() {
                assert!(close(&r[key][axis], value, 1e-9), "{r}");
            }
        }
        assert!((r["center_delta"][2].as_f64().unwrap().abs() - 14.0).abs() < 1e-9, "{r}");
        assert!(close(&r["min"], 13_f64.hypot(14.0), 1e-9), "{r}");
        assert!(close(&r["max"], (34_f64.hypot(25.0) + 12.0).hypot(14.0), 0.05), "{r}");
    }
}

#[test]
fn a_hole_to_the_plate_edge_and_side_measures_from_its_axis() {
    let mut s = Session::default();
    plate(&mut s);
    let a = hole(&s, 3.0);
    // The top front edge (y = 0, z = 10).
    let e = edge(&s, |p| p.y.abs() < 1e-9 && (p.z - 10.0).abs() < 1e-9);
    let r = measure(&mut s, vec![a.clone(), e]);
    assert!(close(&r["center_distance"], 20.0, 1e-9), "{r}");
    assert!(close(&r["min"], 17.0, 1e-9), "{r}");
    assert!(r["max"].as_f64().unwrap() > 20.0, "{r}");
    // The front face (y = 0): axis to plane, the rim's nearest and farthest.
    let f = flat(&s, |p| p.y.abs() < 1e-9);
    let r = measure(&mut s, vec![f, a.clone()]);
    assert!(close(&r["center_distance"], 20.0, 1e-9) && close(&r["min"], 17.0, 1e-9), "{r}");
    // The face reaches past the hole: its far corners are farther than the rim's far side.
    assert!(r["max"].as_f64().unwrap() > 23.0, "{r}");
    // Picked plane first: the centre line still starts at the plane.
    assert!(close(&r["center_from"][1], 0.0, 1e-9) && close(&r["center_to"][1], 20.0, 1e-9), "{r}");
    // Two planes keep today's measure: no centre.
    let top = flat(&s, |p| (p.z - 10.0).abs() < 1e-9);
    let bottom = flat(&s, |p| p.z.abs() < 1e-9);
    let r = measure(&mut s, vec![top, bottom]);
    assert!(r.get("center_distance").is_none() && close(&r["distance_mm"], 10.0, 1e-9) && r["measure"] == "min", "{r}");
}

#[test]
fn sketch_circle_arc_and_slot() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let c = run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}))["curves"][0].as_str().unwrap().to_string();
    let a =
        run(&mut s, "sketch.arc.center_point", json!({"center": [30, 0], "start": [34, 0], "sweep": 90}))["curves"][0].as_str().unwrap().to_string();
    let r = measure(&mut s, vec![json!({"type": "sketch_curve", "id": c})]);
    assert!(close(&r["diameter"], 10.0, 1e-9) && close(&r["radius"], 5.0, 1e-9) && close(&r["circumference"], 10.0 * PI, 1e-9), "{r}");
    assert!(close(&r["items"][0]["length_mm"], 10.0 * PI, 1e-9), "{r}");
    let r = measure(&mut s, vec![json!({"type": "sketch_curve", "id": a})]);
    assert!(close(&r["radius"], 4.0, 1e-9) && close(&r["diameter"], 8.0, 1e-9), "{r}");
    assert!(close(&r["angle_deg"], 90.0, 1e-9) && close(&r["arc_length"], 2.0 * PI, 1e-9), "{r}");
    assert!(close(&r["items"][0]["length_mm"], 2.0 * PI, 1e-9), "{r}");
    assert_eq!(r["items"][0]["round"], "arc");
    // Circle to arc: centre to centre.
    let r = measure(&mut s, vec![json!({"type": "sketch_curve", "id": c}), json!({"type": "sketch_curve", "id": a})]);
    assert!(close(&r["center_distance"], 30.0, 1e-9), "{r}");
    run(&mut s, "sketch.slot.center_to_center", json!({"p0": [0, 40], "p1": [20, 40], "width": 6}));
    let sid = s.active_sketch.unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let st = s.model.state();
    let ss = st.sketch(sid).unwrap();
    let slot =
        ss.profiles.iter().position(|p| p.region.outer.segs.len() >= 4 && (p.area - (20.0 * 6.0 + PI * 9.0)).abs() < 1e-6).expect("slot profile");
    let circle = ss.profiles.iter().position(|p| (p.area - PI * 25.0).abs() < 1e-6).expect("circle profile");
    let r = measure(&mut s, vec![json!({"type": "profile", "sketch": sid, "index": slot})]);
    let sl = &r["slot"];
    assert!(close(&sl["width"], 6.0, 1e-9) && close(&sl["center_distance"], 20.0, 1e-9) && close(&sl["overall_length"], 26.0, 1e-9), "{r}");
    let r = measure(&mut s, vec![json!({"type": "profile", "sketch": sid, "index": circle})]);
    assert!(close(&r["diameter"], 10.0, 1e-9), "{r}");
}

#[test]
fn a_slot_cut_in_a_plate_reports_its_sizes_from_an_end() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.slot.center_to_center", json!({"p0": [20, 20], "p1": [40, 20], "width": 8}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "operation": "cut", "profiles": "all"}));
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let n = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap();
    let end = (0..n)
        .find(|&f| matches!(solidcraft_kernel::face_surface(&b.body, f), Some(solidcraft_kernel::FaceSurface::Cylinder { radius, .. }) if (radius - 4.0).abs() < 1e-6))
        .expect("slot end");
    let p = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf as usize == end).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    let name = body(&s);
    let r = measure(&mut s, vec![json!({"type": "face", "body": name, "index": end, "point": p})]);
    let sl = &r["slot"];
    assert!(close(&sl["width"], 8.0, 1e-9) && close(&sl["center_distance"], 20.0, 1e-9) && close(&sl["overall_length"], 28.0, 1e-9), "{r}");
    // The end is a half cylinder: R 4, half a turn.
    assert!(close(&r["radius"], 4.0, 1e-9) && close(&r["angle_deg"], 180.0, 1.0), "{r}");
}

#[test]
fn a_fillet_edge_is_an_arc() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 30, 10]], "radius": 5}));
    // The fillet's quarter circle on the top face: centre (35, 25, 20).
    let e = edge(&s, |p| (p.z - 20.0).abs() < 1e-9 && ((p - Vec3::new(35.0, 25.0, 20.0)).len() - 5.0).abs() < 1e-6);
    let r = measure(&mut s, vec![e]);
    assert!(close(&r["radius"], 5.0, 1e-9) && close(&r["diameter"], 10.0, 1e-9), "{r}");
    assert!(close(&r["angle_deg"], 90.0, 1e-6) && close(&r["arc_length"], 2.5 * PI, 1e-6), "{r}");
    assert!(close(&r["items"][0]["length_mm"], 2.5 * PI, 1e-6), "{r}");
    assert!(close(&r["center"][0], 35.0, 1e-9) && close(&r["center"][1], 25.0, 1e-9), "{r}");
}

/// A cylinder's top circle is as long as its circumference, in the same reply: the length
/// comes from the exact circle, not from the polyline drawn for it, which runs inside it
/// (#9086, the report's own cylinder).
#[test]
fn a_cylinders_top_circle_is_as_long_as_its_circumference() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let e = edge(&s, |p| (p.z - 20.0).abs() < 1e-9 && ((p - Vec3::new(40.0, 10.0, 20.0)).len() - 5.0).abs() < 1e-6);
    let r = measure(&mut s, vec![e]);
    assert!(close(&r["circumference"], TAU * 5.0, 1e-9), "{r}");
    assert!(close(&r["items"][0]["length_mm"], TAU * 5.0, 1e-9), "{r}");
}

/// The first face whose analytic surface satisfies `want`, as a pick.
fn face_of(s: &Session, want: impl Fn(solidcraft_kernel::FaceSurface) -> bool) -> Value {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let n = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap();
    let f = (0..n).find(|&f| solidcraft_kernel::face_surface(&b.body, f).is_some_and(&want)).expect("face");
    let p = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf as usize == f).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    json!({"type": "face", "body": b.name, "index": f, "point": p})
}

/// A fillet round the foot of a boss is a toroidal face: it measures its radius (the fillet's)
/// next to its area, and says it is a torus with its ring radius too.
#[test]
fn a_fillet_round_a_boss_reports_its_radius() {
    use solidcraft_kernel::FaceSurface;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 12, "base": [20, 20, 10], "operation": "join"}));
    run(&mut s, "solid.fillet", json!({"edges": [[25, 20, 10]], "radius": 3}));
    let fillet = face_of(&s, |f| matches!(f, FaceSurface::Torus { minor, .. } if (minor - 3.0).abs() < 1e-6));
    let r = measure(&mut s, vec![fillet]);
    assert!(close(&r["radius"], 3.0, 1e-6) && close(&r["diameter"], 6.0, 1e-6), "{r}");
    assert_eq!(r["items"][0]["surface"], "torus", "{r}");
    // The ring through the middle of the rolling ball: the boss's radius plus the fillet's.
    assert!(close(&r["items"][0]["major_radius"], 8.0, 1e-6), "{r}");
    // The area is still there.
    assert!(r["items"][0]["area_mm2"].as_f64().is_some_and(|a| a > 1.0), "{r}");
}

/// A ball's face reports its radius.
#[test]
fn a_sphere_face_reports_its_radius() {
    use solidcraft_kernel::FaceSurface;
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 7, "center": [0, 0, 0]}));
    let ball = face_of(&s, |f| matches!(f, FaceSurface::Sphere { .. }));
    let r = measure(&mut s, vec![ball]);
    assert!(close(&r["radius"], 7.0, 1e-6) && close(&r["diameter"], 14.0, 1e-6), "{r}");
    assert_eq!(r["items"][0]["surface"], "sphere", "{r}");
}

/// A plate with holes through STEP: the holes come back as cylinders, and measure the same.
#[test]
fn step_round_trip_of_a_plate_with_holes() {
    let dir = std::env::temp_dir().join(format!("solidcraft-measure-1392-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Plate.step");
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": path.to_string_lossy()}));
    let (a, b) = (hole(&t, 3.0), hole(&t, 4.0));
    let r = measure(&mut t, vec![a, b]);
    assert!(close(&r["center_distance"], 30.0, 1e-6), "{r}");
    assert!(close(&r["min"], 23.0, 1e-6), "{r}");
    assert!(close(&r["items"][0]["diameter"], 6.0, 1e-6) && close(&r["items"][1]["diameter"], 8.0, 1e-6), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

// A part ring cut by `solid.combine` (two prisms through its ends, a notch through its chamfer)
// measures what it encloses, wherever it is moved. The reference mesher left a crack where the
// notch meets the chamfer's cone; the volume beside it was left out (0.2 %), and integrated
// about the world origin the loss grew with the distance (7 % moved 1000 mm away).

fn volume(s: &mut Session, body: &str) -> f64 {
    run(s, "inspect.measure", json!({"bodies": [body]}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn prism(s: &mut Session, offset: f64, points: Value, distance: f64, name: &str) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": offset}));
    run(s, "sketch.line", json!({"points": points, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": distance, "operation": "new", "body_name": name}));
}

/// A part ring (210° of a chamfered section) less two oblique prisms and a notch.
fn cut_ring(s: &mut Session) {
    let (c, si) = (15f64.to_radians().cos(), 15f64.to_radians().sin());
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [si, c, 0], "y_dir": [0, 0, -1]}}));
    let section = json!([[16.6, 0], [17.5, 0], [18.5, -1.0], [18.5, -7.0], [17.5, -8.0], [16.6, -8.0], [16.4, -7.8], [16.4, -0.2]]);
    run(s, "sketch.line", json!({"points": section, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.revolve", json!({"axis": "Z", "angle": "210 deg", "body_name": "Ring"}));
    let quad = |sy: f64| json!([[4.644832, 15.101839 * sy], [5.17399, 16.822301 * sy], [4.555215, 17.000295 * sy], [2.187187, 15.574386 * sy]]);
    prism(s, -1.0, quad(1.0), 10.0, "T0");
    prism(s, -1.0, quad(-1.0), 10.0, "T1");
    prism(s, 7.1, json!([[-17.6, 0.45], [-19.5, 0.45], [-19.5, -0.45], [-17.6, -0.45]]), 1.9, "N0");
    run(s, "solid.combine", json!({"target": "Ring", "tools": ["T0", "T1", "N0"], "operation": "cut"}));
}

#[test]
fn combine_cut_volume_does_not_depend_on_placement() {
    let mut s = Session::default();
    cut_ring(&mut s);
    let here = volume(&mut s, "Ring");
    // The 210° ring by Pappus less what the prisms and the notch take (a fine grid quadrature
    // of each, independent of the kernel's meshes): 1005.489 − 2 × 5.639 − 0.287.
    let want = 993.924;
    assert!((here - want).abs() <= 2e-4 * want, "at the origin: {here} vs {want}");
    let mut moved = Vec::new();
    for by in [[100.0, 0.0, 0.0], [900.0, 0.0, 0.0], [-1000.0, 100.0, 0.0], [0.0, 0.0, 10000.0], [0.0, -10000.0, 0.0]] {
        run(&mut s, "solid.move", json!({"bodies": ["Ring"], "translate": by}));
        moved.push((by, volume(&mut s, "Ring")));
    }
    for (by, v) in moved {
        assert!((v - here).abs() <= 1e-6 * here, "after a move by {by:?}: {v} vs {here} at the origin");
    }
}

#[test]
fn box_faces_measure_by_point_only() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 20, "body_name": "Box"}));
    for (point, area) in [([5.0, 5.0, 20.0], 100.0), ([10.0, 5.0, 10.0], 200.0)] {
        let pick = json!({"type": "face", "body": "Box", "point": point});
        let r = measure(&mut s, vec![pick.clone()]);
        assert!(close(&r["items"][0]["area_mm2"], area, 1e-9), "{r}");
        let index = solidcraft_doc::appearance::face_index_at(s.model.state().body("Box").unwrap(), Vec3::from(point)).unwrap();
        let mut indexed = pick.clone();
        indexed["index"] = json!(index);
        assert_eq!(measure(&mut s, vec![indexed.clone()]), r);
        assert_eq!(measure(&mut s, vec![pick, indexed]), r);
    }
    for pick in [
        json!({"type": "face", "body": "Missing", "point": [5, 5, 20]}),
        json!({"type": "face", "body": "Box", "point": [100, 100, 100]}),
        json!({"type": "face", "body": "Box"}),
    ] {
        assert!(s.execute("inspect.measure", &json!({"items": [pick]})).is_err());
    }
}

#[test]
fn box_edges_measure_by_point_only() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 15, "height": 20, "body_name": "Box"}));
    for (point, length) in [([5, 0, 0], 10.0), ([10, 15, 10], 20.0)] {
        let pick = json!({"type": "edge", "body": "Box", "point": point});
        let r = measure(&mut s, vec![pick]);
        assert!(close(&r["items"][0]["length_mm"], length, 1e-9), "{r}");
    }
    for pick in [json!({"type": "edge", "body": "Missing", "point": [5, 0, 0]}), json!({"type": "edge", "body": "Box", "point": [100, 100, 100]})] {
        assert!(s.execute("inspect.measure", &json!({"items": [pick]})).is_err());
    }
}

fn open_revolve(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Profile"}));
    run(s, "sketch.line", json!({"points": [[30, 0], [30, 40]], "ids": ["side"]}));
    run(s, "sketch.finish", json!({}));
    run(s, "surface.revolve", json!({"sketch": "Profile", "curves": ["side"], "axis": "y", "angle": "270 deg", "body_name": "OpenTube"}));
}

#[test]
fn open_surface_measurements_have_no_volume_properties() {
    let mut s = Session::default();
    open_revolve(&mut s);
    let r = run(&mut s, "inspect.measure", json!({"bodies": ["OpenTube"]}));
    let b = &r["bodies"][0];
    assert_eq!(b["closed"], false);
    for key in ["volume_mm3", "center_of_mass", "density_g_cm3", "mass_g"] {
        assert!(b[key].is_null(), "{key}: {b}");
    }
    assert!(b["area_mm2"].as_f64().unwrap() > 0.0);
    assert!(b["bbox"].is_object() && b["faces"].as_u64().unwrap() > 0);
    assert_eq!(r["total"]["open_bodies_skipped"], 1);
    assert_eq!(r["total"]["volume_mm3"], 0.0);
    assert_eq!(r["total"]["mass_g"], 0.0);
    let document = run(&mut s, "document.inspect", json!({"measure": true}));
    assert_eq!(document["bodies"][0]["closed"], false);
    assert!(document["bodies"][0]["center_of_mass"].is_null());
    let picked = run(&mut s, "inspect.measure", json!({"items": [{"type": "body", "name": "OpenTube"}]}));
    assert_eq!(picked["items"][0]["closed"], false);
    assert!(picked["items"][0]["volume_mm3"].is_null());
    let props = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(props["total"]["open_bodies_skipped"], 1);
    assert!(props["bodies"][0]["mass_g"].is_null());
    assert!(props["total"]["center_of_mass"].is_null());
}

#[test]
fn volume_totals_skip_open_surfaces_and_keep_closed_box_properties() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box"}));
    let before = run(&mut s, "inspect.measure", json!({}));
    let b = &before["bodies"][0];
    assert_eq!(b["closed"], true);
    assert!(close(&b["volume_mm3"], 6000.0, 1e-9));
    assert!(close(&b["mass_g"], 6.0 * s.doc.density("Box"), 1e-9));
    for (axis, expected) in [5.0, 10.0, 15.0].into_iter().enumerate() {
        assert!(close(&b["center_of_mass"][axis], expected, 1e-9));
    }
    let props_before = run(&mut s, "component.properties", json!({"component": "root"}));
    open_revolve(&mut s);
    let after = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(after["body_count"], 2);
    assert_eq!(after["total"]["open_bodies_skipped"], 1);
    for key in ["volume_mm3", "mass_g"] {
        assert_eq!(before["total"][key], after["total"][key]);
    }
    assert!(after["total"]["area_mm2"].as_f64().unwrap() > before["total"]["area_mm2"].as_f64().unwrap());
    let props_after = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(props_after["total"]["open_bodies_skipped"], 1);
    for key in ["volume_mm3", "mass_g", "center_of_mass"] {
        assert_eq!(props_before["total"][key], props_after["total"][key]);
    }
}

/// An ASCII STL of the tetrahedron (0,0,0), (10,0,0), (0,10,0), (0,0,10), outward facing; `open`
/// leaves its slanted face out.
fn tetra_stl(open: bool) -> String {
    let faces: &[[[i32; 3]; 3]] = &[
        [[0, 0, 0], [0, 10, 0], [10, 0, 0]],
        [[0, 0, 0], [10, 0, 0], [0, 0, 10]],
        [[0, 0, 0], [0, 0, 10], [0, 10, 0]],
        [[10, 0, 0], [0, 10, 0], [0, 0, 10]],
    ];
    let mut stl = String::from("solid t\n");
    for f in &faces[..if open { 3 } else { 4 }] {
        stl += "facet normal 0 0 0\nouter loop\n";
        for [x, y, z] in f {
            stl += &format!("vertex {x} {y} {z}\n");
        }
        stl += "endloop\nendfacet\n";
    }
    stl + "endsolid t\n"
}

#[test]
fn a_watertight_mesh_keeps_its_volume_and_an_open_mesh_has_none() {
    let dir = std::env::temp_dir().join(format!("solidcraft-measure-open-mesh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    for (name, open) in [("Closed", false), ("Open", true)] {
        let path = dir.join(format!("{name}.stl"));
        std::fs::write(&path, tetra_stl(open)).unwrap();
        run(&mut s, "file.insert_mesh", json!({"path": path.to_string_lossy()}));
    }
    let _ = std::fs::remove_dir_all(&dir);
    let r = run(&mut s, "inspect.measure", json!({}));
    let bodies = r["bodies"].as_array().unwrap();
    let by = |n: &str| bodies.iter().find(|b| b["name"] == n).unwrap_or_else(|| panic!("{n}: {r}"));
    assert_eq!(by("Closed")["closed"], true);
    assert!(close(&by("Closed")["volume_mm3"], 1000.0 / 6.0, 1e-6), "{r}");
    assert_eq!(by("Open")["closed"], false);
    assert!(by("Open")["volume_mm3"].is_null() && by("Open")["mass_g"].is_null(), "{r}");
    assert_eq!(r["total"]["open_bodies_skipped"], 1);
    assert!(close(&r["total"]["volume_mm3"], 1000.0 / 6.0, 1e-6), "{r}");
}
