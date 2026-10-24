use super::*;
use std::f64::consts::PI;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}
fn xy(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}
/// A 40 × 20 × 10 plate with a radius 5 hole through its middle.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [20, 10, 0], "operation": "cut", "targets": ["Plate"]}));
    s
}

#[test]
fn a_plate_with_a_hole_cuts_to_a_rectangle_and_a_circle() {
    let mut s = plate();
    let undo = s.undo.len();
    let r = run(&mut s, CMD, json!({"plane": "XY", "at": 5}));
    assert_eq!(s.undo.len(), undo, "nothing to undo");
    assert_eq!(r["frame"]["origin"], json!([0.0, 0.0, 5.0]));
    assert_eq!(r["frame"]["x"], json!([1.0, 0.0, 0.0]));
    let loops = r["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 2, "{r}");
    let (outer, hole) = if loops[0]["area"].as_f64().unwrap() > 0.0 { (&loops[0], &loops[1]) } else { (&loops[1], &loops[0]) };
    for l in loops {
        assert_eq!(l["body"], "Plate");
        assert_eq!(l["closed"], true);
    }
    // Outer: four exact lines around 40 × 20, counter-clockwise.
    let segs = outer["segments"].as_array().unwrap();
    assert_eq!(segs.len(), 4, "{outer}");
    let mut corners: Vec<Vec2> = Vec::new();
    for (i, g) in segs.iter().enumerate() {
        assert_eq!(g["type"], "line");
        let next = &segs[(i + 1) % 4];
        assert!(xy(&g["end"]).dist(xy(&next["start"])) < 1e-9, "joined end to end");
        corners.push(xy(&g["start"]));
    }
    for c in [Vec2::new(0., 0.), Vec2::new(40., 0.), Vec2::new(40., 20.), Vec2::new(0., 20.)] {
        assert!(corners.iter().any(|p| p.dist(c) < 1e-9), "corner {c:?} in {corners:?}");
    }
    assert!((outer["area"].as_f64().unwrap() - 800.0).abs() < 1e-9, "{}", outer["area"]);
    // Hole: one whole circle, clockwise.
    let segs = hole["segments"].as_array().unwrap();
    assert_eq!(segs.len(), 1, "{hole}");
    let c = &segs[0];
    assert_eq!(c["type"], "arc");
    assert!(xy(&c["center"]).dist(Vec2::new(20., 10.)) < 1e-9, "{c}");
    assert!((c["radius"].as_f64().unwrap() - 5.0).abs() < 1e-9, "{c}");
    assert!((c["sweep_deg"].as_f64().unwrap() + 360.0).abs() < 1e-9, "{c}");
    assert!((hole["area"].as_f64().unwrap() + 25.0 * PI).abs() < 1e-9, "{}", hole["area"]);
}

#[test]
fn a_cylinder_cut_at_30_degrees_is_an_ellipse() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40, "body_name": "Rod"}));
    // Tilted 30° about X: semi-axes 10 (along the frame's x, world −X) and 10 / cos 30°.
    let (sin, cos) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let r = run(&mut s, CMD, json!({"plane": {"origin": [0, 0, 20], "normal": [0, sin, cos]}}));
    let loops = r["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 1, "{r}");
    let (a, b) = (10.0, 10.0 / cos);
    let on = |p: Vec2| ((p.x / a).powi(2) + (p.y / b).powi(2)).sqrt() - 1.0;
    let mut beziers = 0;
    for g in loops[0]["segments"].as_array().unwrap() {
        assert_eq!(g["type"], "spline", "{g}");
        // Fit points on the ellipse; the curve between them within a micrometre.
        for p in g["points"].as_array().unwrap() {
            assert!(on(xy(p)).abs() < 1e-9, "{p}");
        }
        for piece in g["beziers"].as_array().unwrap() {
            let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| xy(&piece[i]));
            for t in [0.25, 0.5, 0.75] {
                let u = 1.0 - t;
                let q = p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t);
                assert!(on(q).abs() * a < 1e-3, "{q:?} is {} mm off", on(q) * a);
            }
            beziers += 1;
        }
    }
    assert!(beziers >= 8, "{r}");
    let area = loops[0]["area"].as_f64().unwrap();
    assert!((area - PI * a * b).abs() < 1e-4 * PI * a * b, "{area} vs {}", PI * a * b);
}

/// Each body's loops are its own: the plate's outline and hole, and the block's square.
#[test]
fn section_curves_give_each_body_its_loops() {
    let mut s = plate();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 10, "corner": [50, 0, 0], "body_name": "Block"}));
    let r = run(&mut s, CMD, json!({"plane": "XY", "at": 5}));
    let of = |body: &str| -> Vec<f64> {
        let mut areas: Vec<f64> = r["loops"].as_array().unwrap().iter().filter(|l| l["body"] == body).map(|l| l["area"].as_f64().unwrap()).collect();
        areas.sort_by(f64::total_cmp);
        areas
    };
    let plate = of("Plate");
    assert!(plate.len() == 2 && (plate[0] + 25.0 * PI).abs() < 1e-6 && (plate[1] - 800.0).abs() < 1e-6, "{plate:?}");
    let block = of("Block");
    assert!(block.len() == 1 && (block[0] - 25.0).abs() < 1e-9, "{block:?}");
}

#[test]
fn section_curves_select_bodies_and_reject_bad_input() {
    let mut s = plate();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 10, "corner": [50, 0, 0], "body_name": "Block"}));
    let r = run(&mut s, CMD, json!({"plane": "XY", "at": 5, "bodies": ["Block"]}));
    assert_eq!(r["loops"].as_array().map(Vec::len), Some(1));
    assert_eq!(r["loops"][0]["body"], "Block");
    // A plane that misses everything.
    assert_eq!(run(&mut s, CMD, json!({"plane": "XY", "at": 50}))["loops"], json!([]));
    // Flipped: the frame's y and the normal turn over, the loops stay counter-clockwise outside.
    let r = run(&mut s, CMD, json!({"plane": "XY", "at": 5, "flip": true, "bodies": ["Block"]}));
    assert_eq!(r["frame"]["normal"][2], -1.0);
    assert!((r["loops"][0]["area"].as_f64().unwrap() - 25.0).abs() < 1e-9);
    for p in [
        json!({}),
        json!({"plane": "XY", "bodies": ["Nope"]}),
        json!({"plane": "XY", "bodies": []}),
        json!({"plane": "XY", "tolerance": 0}),
        json!({"plane": "XY", "tolerance": "x"}),
        json!({"plane": {"origin": [0, 0, 0], "normal": [0, 0, 0]}}),
        json!({"plane": "XY", "offset": "1e400"}),
    ] {
        assert!(s.execute(CMD, &p).is_err(), "{p}");
    }
}

fn closed_areas(r: &Value) -> Vec<f64> {
    r["loops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            assert_eq!(l["closed"], true, "{r}");
            l["area"].as_f64().unwrap()
        })
        .collect()
}

fn types(l: &Value) -> Vec<String> {
    l["segments"].as_array().unwrap().iter().map(|g| g["type"].as_str().unwrap().to_string()).collect()
}

/// A torus cut just off its equator is two large closed splines: capped at the spline's point
/// limit (they came out empty, with area 0, before).
#[test]
fn a_large_closed_spline_keeps_its_points_and_area() {
    let mut s = Session::default();
    run(&mut s, "solid.torus", json!({"major": 40, "minor": 10, "body_name": "Ring"}));
    let r = run(&mut s, CMD, json!({"plane": "XY", "at": 0.1, "tolerance": 0.001}));
    let areas = closed_areas(&r);
    assert_eq!(areas.len(), 2, "{r}");
    let half = (100.0f64 - 0.01).sqrt();
    let (outer, inner) = (PI * (40.0 + half).powi(2), PI * (40.0 - half).powi(2));
    let total: f64 = areas.iter().sum();
    assert!((total - (outer - inner)).abs() < 1e-3 * (outer - inner), "{areas:?}");
    for l in r["loops"].as_array().unwrap() {
        for g in l["segments"].as_array().unwrap() {
            if g["type"] == "spline" {
                assert!(g["points"].as_array().unwrap().len() <= 500, "{g}");
                assert!(!g["beziers"].as_array().unwrap().is_empty(), "{g}");
            }
        }
    }
}

/// A plane on a body's face cuts it along that face's outline: a closed loop, not fragments.
#[test]
fn a_plane_on_a_face_cuts_along_its_outline() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Plate"}));
    for at in [10.0, 0.0] {
        let r = run(&mut s, CMD, json!({"plane": "XY", "at": at}));
        assert_eq!(closed_areas(&r).len(), 1, "z = {at}: {r}");
        assert!((closed_areas(&r)[0] - 800.0).abs() < 1e-9, "z = {at}: {r}");
        assert_eq!(types(&r["loops"][0]), ["line"; 4], "z = {at}: {r}");
    }
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40, "body_name": "Rod"}));
    for at in [40.0, 0.0] {
        let r = run(&mut s, CMD, json!({"plane": "XY", "at": at}));
        assert_eq!(closed_areas(&r).len(), 1, "z = {at}: {r}");
        assert!((closed_areas(&r)[0] - 100.0 * PI).abs() < 1e-6, "z = {at}: {r}");
        assert_eq!(types(&r["loops"][0]), ["arc"], "z = {at}: {r}");
    }
}

/// A plane through a cone's apex and its axis: a triangle, base line included.
#[test]
fn a_plane_through_a_cones_apex_keeps_its_base() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [0, -20]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"axis": "Z", "body_name": "Cone"}));
    let r = run(&mut s, CMD, json!({"plane": "XZ"}));
    let areas = closed_areas(&r);
    assert_eq!(areas.len(), 1, "{r}");
    assert!((areas[0] - 200.0).abs() < 1e-6, "{r}");
    assert_eq!(types(&r["loops"][0]), ["line"; 3], "{r}");
}

/// A cylinder cut through its axis (the section through a hole) has straight sides, whether
/// the plane runs through the mesh's vertices or between them.
#[test]
fn a_cylinder_cut_through_its_axis_has_straight_sides() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40, "base": [20, 10, 0], "body_name": "Rod"}));
    for deg in [0.0f64, 7.0, 45.0] {
        let (sin, cos) = (deg.to_radians().sin(), deg.to_radians().cos());
        let r = run(&mut s, CMD, json!({"plane": {"origin": [20, 10, 20], "normal": [sin, cos, 0]}}));
        let areas = closed_areas(&r);
        assert_eq!(areas.len(), 1, "{deg}°: {r}");
        assert!((areas[0] - 800.0).abs() < 1e-4, "{deg}°: {r}");
        assert_eq!(types(&r["loops"][0]), ["line"; 4], "{deg}°: {r}");
    }
}
