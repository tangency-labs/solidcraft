//! `surface.boundary` through loops of three or four sketch curves and through curve networks
//! (#763): analytic areas and shapes, G1 and G2 measured across the edge a surface shares with a
//! body (`inspect.continuity`), and refusals of hostile networks that never panic.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn first(v: &Value) -> String {
    v.as_array().and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string).unwrap_or_default()
}

/// A sketch line's id.
fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    first(&run(s, "sketch.line", json!({"points": [a, b]}))["created"]["curves"])
}

/// A 3D sketch line's id (the active sketch must have 3D Sketch on).
fn line3d(s: &mut Session, a: [f64; 3], b: [f64; 3]) -> String {
    first(&run(s, "sketch.line3d", json!({"points": [a, b]}))["curves"])
}

/// The area of the one face of `body`, and the area `inspect.measure` gives for the whole model.
fn areas(s: &mut Session, body: &str) -> (f64, f64) {
    let f = run(s, "model.faces", json!({"body": body}));
    let faces = f["faces"].as_array().cloned().unwrap_or_default();
    assert_eq!(faces.len(), 1, "one face: {f}");
    let face = faces[0]["area"].as_f64().unwrap_or(f64::NAN);
    let m = run(s, "inspect.measure", json!({}));
    (face, m["total"]["area_mm2"].as_f64().unwrap_or(f64::NAN))
}

/// Three and four sketch curves in loop order: the flat patch of the loop. A triangle and a square
/// of lines, and a square whose top is a half circle bulging out, each measure their own area.
#[test]
fn three_and_four_curve_loops_are_flat_patches() {
    use std::f64::consts::PI;
    let cases: Vec<(Vec<[[f64; 2]; 2]>, f64, f64)> = vec![
        (vec![[[0.0, 0.0], [20.0, 0.0]], [[20.0, 0.0], [10.0, 15.0]], [[10.0, 15.0], [0.0, 0.0]]], 150.0, 1e-6),
        // Issue #20380: loop order holds when curves are drawn or mirrored in reverse
        (vec![[[0.0, 0.0], [20.0, 0.0]], [[20.0, 0.0], [10.0, 15.0]], [[0.0, 0.0], [10.0, 15.0]]], 150.0, 1e-6),
        (vec![[[20.0, 0.0], [0.0, 0.0]], [[20.0, 0.0], [10.0, 15.0]], [[10.0, 15.0], [0.0, 0.0]]], 150.0, 1e-6),
        (vec![[[0.0, 0.0], [20.0, 0.0]], [[10.0, 15.0], [20.0, 0.0]], [[10.0, 15.0], [0.0, 0.0]]], 150.0, 1e-6),
        (vec![[[0.0, 0.0], [10.0, 0.0]], [[10.0, 0.0], [10.0, 10.0]], [[10.0, 10.0], [0.0, 10.0]], [[0.0, 10.0], [0.0, 0.0]]], 100.0, 1e-6),
        (vec![[[0.0, 0.0], [10.0, 0.0]], [[10.0, 10.0], [10.0, 0.0]], [[10.0, 10.0], [0.0, 10.0]], [[0.0, 0.0], [0.0, 10.0]]], 100.0, 1e-6),
    ];
    for (lines, want, tol) in cases {
        for continuity in [json!("g0"), json!("g1"), json!("g3")] {
            let mut s = Session::default();
            run(&mut s, "sketch.create", json!({"plane": "XY"}));
            let curves: Vec<String> = lines.iter().map(|[a, b]| line(&mut s, *a, *b)).collect();
            let r = run(&mut s, "surface.boundary", json!({"curves": curves, "continuity": continuity}));
            let body = first(&r["created_bodies"]);
            let (face, total) = areas(&mut s, &body);
            assert!(
                (face - want).abs() < tol * want && (total - want).abs() < tol * want,
                "{} sides, {continuity}: {face} / {total} vs {want}",
                lines.len()
            );
        }
    }
    // A square with a half circle for its top, bulging out: 100 + 12.5π.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let bottom = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let right = line(&mut s, [10.0, 0.0], [10.0, 10.0]);
    let arc = first(&run(&mut s, "sketch.arc.three_point", json!({"start": [10, 10], "end": [0, 10], "through": [5, 15]}))["created"]["curves"]);
    let left = line(&mut s, [0.0, 10.0], [0.0, 0.0]);
    let r = run(&mut s, "surface.boundary", json!({"curves": [bottom, right, arc, left]}));
    let (face, total) = areas(&mut s, &first(&r["created_bodies"]));
    let want = 100.0 + 12.5 * PI;
    assert!((face - want).abs() < 2e-3 * want && (total - want).abs() < 2e-3 * want, "{face} / {total} vs {want}");
}

/// Simpson's rule over [0, a] × [0, a] with n (even) intervals each way.
fn simpson2(f: impl Fn(f64, f64) -> f64, a: f64, n: usize) -> f64 {
    let h = a / n as f64;
    let w = |k: usize| {
        if k == 0 || k == n {
            1.0
        } else if k % 2 == 1 {
            4.0
        } else {
            2.0
        }
    };
    let mut sum = 0.0;
    for i in 0..=n {
        for j in 0..=n {
            sum += w(i) * w(j) * f(i as f64 * h, j as f64 * h);
        }
    }
    sum * h * h / 9.0
}

/// Four straight 3D sketch lines bounding the hyperbolic paraboloid z = x·y/10 over the 10 × 10
/// square: the network's Coons patch is that surface (every vertex of its mesh on it), and its
/// area is the paraboloid's, ∫∫ √(1 + (x/10)² + (y/10)²).
#[test]
fn a_network_of_3d_lines_is_a_hyperbolic_paraboloid() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let a = line3d(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let c = line3d(&mut s, [0.0, 10.0, 0.0], [10.0, 10.0, 10.0]);
    let d = line3d(&mut s, [0.0, 0.0, 0.0], [0.0, 10.0, 0.0]);
    let b = line3d(&mut s, [10.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let r = run(&mut s, "surface.boundary", json!({"u": [a, c], "v": [d, b]}));
    let body = first(&r["created_bodies"]);
    let want = simpson2(|x, y| (1.0 + (x / 10.0).powi(2) + (y / 10.0).powi(2)).sqrt(), 10.0, 200);
    let (face, total) = areas(&mut s, &body);
    // The face's own area comes from the coarse reference mesh (a chord of a fiftieth of a per
    // cent of the body); the measuring mesh is refined onto the surface.
    assert!((face - want).abs() < 1e-2 * want && (total - want).abs() < 1e-3 * want, "{face} / {total} vs {want}");
    let st = s.world_state().unwrap();
    let mesh = st.body(&body).map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01))).and_then(Result::ok).unwrap_or_default();
    assert!(!mesh.positions.is_empty());
    for p in &mesh.positions {
        assert!((p.z - p.x * p.y / 10.0).abs() < 1e-6, "{p:?} is off the paraboloid");
    }
}

/// Curves of two planar sketches, one picked by a bare id in the sketch being edited: the loft
/// between a line on the ground and one 10 mm across and 5 mm up is the flat strip between them.
#[test]
fn a_network_takes_curves_from_several_sketches() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Low"}));
    let low = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "High"}));
    let high = line(&mut s, [0.0, 10.0], [10.0, 10.0]);
    let r = run(&mut s, "surface.boundary", json!({"u": [{"sketch": "Low", "curves": [low]}, high]}));
    let (face, _) = areas(&mut s, &first(&r["created_bodies"]));
    let want = 10.0 * 125f64.sqrt();
    assert!((face - want).abs() < 1e-6 * want, "{face} vs {want}");
}

/// A quarter cylinder sheet (R = 10, axis along x, its straight top edge at y = 10, z = 0 open)
/// and a line 3D-sketched above it; returns the sheet's name and the line's id.
fn cylinder_and_line(s: &mut Session) -> (String, String) {
    run(s, "sketch.create", json!({"plane": "YZ", "name": "Arc"}));
    let h = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    let arc = first(&run(s, "sketch.arc.three_point", json!({"start": [0, -10], "end": [10, 0], "through": [h, -h]}))["created"]["curves"]);
    run(s, "sketch.finish", json!({}));
    let sheet = first(&run(s, "surface.extrude", json!({"sketch": "Arc", "curves": [arc], "distance": 10}))["created_bodies"]);
    run(s, "sketch.create", json!({"plane": "XY", "name": "Top"}));
    run(s, "sketch.options", json!({"sketch_3d": true}));
    let top = line3d(s, [0.0, 3.0, 12.0], [10.0, 3.0, 12.0]);
    run(s, "sketch.finish", json!({}));
    (sheet, top)
}

/// The edge a surface built from the cylinder's top edge shares with it, measured with
/// `inspect.continuity`: (normal angle in degrees, curvature jump per mm).
fn seam(s: &mut Session, body: &str) -> (f64, f64) {
    let v = run(s, "inspect.continuity", json!({"body": body, "edges": [[5.0, 10.0, 0.0]]}));
    let e = v["bodies"][0]["edges"].as_array().and_then(|a| a.first()).cloned().unwrap_or_default();
    (e["normal_angle_deg"]["value"].as_f64().unwrap_or(f64::NAN), e["curvature_jump_per_mm"]["value"].as_f64().unwrap_or(f64::NAN))
}

/// G1 and G2 against a body: a network from the cylinder's open top edge to a line above, joined
/// into the cylinder. Across the shared edge the normals agree within 0.1° for G1 and G2; the
/// curvature jumps by much of the cylinder's 1/R with G1 and by under 1% of it with G2.
#[test]
fn g1_and_g2_networks_meet_a_body_across_its_edge() {
    let mut jumps = Vec::new();
    for g in ["g1", "g2"] {
        let mut s = Session::default();
        let (sheet, top) = cylinder_and_line(&mut s);
        let r = run(
            &mut s,
            "surface.boundary",
            json!({"u": [{"body": sheet, "edges": [[5.0, 10.0, 0.0]], "continuity": g}, {"sketch": "Top", "curves": [top]}], "operation": "join"}),
        );
        assert!(r["created_bodies"].as_array().is_none_or(Vec::is_empty), "a join makes no new body: {r}");
        let f = run(&mut s, "model.faces", json!({"body": sheet}));
        assert_eq!(f["faces"].as_array().map(Vec::len), Some(2), "the surface is sewn into the sheet: {f}");
        let (angle, jump) = seam(&mut s, &sheet);
        assert!(angle < 0.1, "{g}: normals {angle}° apart across the edge");
        jumps.push(jump);
    }
    let r = 10.0;
    assert!(jumps[0] > 0.2 / r, "G1 leaves the curvature behind: jump {}", jumps[0]);
    assert!(jumps[1] < 0.01 / r, "G2 carries the cylinder's curvature across: jump {}", jumps[1]);
}

/// Networks that cannot be built are refused with a reason — by the command's own checks or by the
/// kernel — and leave nothing behind; none of them panics.
#[test]
fn hostile_networks_are_refused() {
    let mut s = Session::default();
    let (sheet, top) = cylinder_and_line(&mut s);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 40, 0]}));
    let boxed = s.world_state().unwrap().bodies.last().map(|b| b.name.clone()).unwrap_or_default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Flat"}));
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    let far = line(&mut s, [0.0, 20.0], [10.0, 20.0]);
    let ring = first(&run(&mut s, "sketch.circle.center", json!({"center": [50, 50], "radius": 3}))["created"]["curves"]);
    let bodies = s.world_state().unwrap().bodies.len();
    let edge = |g: &str| json!({"body": sheet, "edges": [[5.0, 10.0, 0.0]], "continuity": g});
    let many: Vec<Value> = (0..33).map(|_| json!(a)).collect();
    let cases: Vec<(Value, &str)> = vec![
        (json!({"u": "l1"}), "list"),
        (json!({"u": [42, a]}), "curve id"),
        (json!({"u": [{}, a]}), "one of"),
        (json!({"u": [{"curves": [], "sketch": "Flat"}, a]}), "sketch curves"),
        (json!({"u": [{"curves": ["nope"]}, a]}), "no sketch curve"),
        (json!({"u": [{"curves": [a], "sketch": "Nope"}, b]}), "no sketch"),
        (json!({"u": [{"body": "Nope", "edges": [[0, 0, 0]]}, a]}), "no body"),
        (json!({"u": [{"body": sheet, "edges": []}, a]}), "edges"),
        (json!({"u": [{"body": sheet, "edges": [["x", 0, 0]]}, a]}), "[x, y, z]"),
        (json!({"u": [{"body": sheet, "edges": [[1e300, 0, 0]]}, a]}), "[x, y, z]"),
        (json!({"u": [{"point": [0, 0]}, a]}), "point"),
        (json!({"u": [{"curves": [a], "body": sheet, "edges": [[5, 10, 0]]}, b]}), "one of"),
        (json!({"u": [{"curves": [a], "continuity": "g1"}, b]}), "body's edges"),
        (json!({"u": [edge("g3"), {"sketch": "Top", "curves": [top]}]}), "not supported yet"),
        (json!({"u": [edge("g9"), {"sketch": "Top", "curves": [top]}]}), "continuity"),
        (json!({"u": many}), "at most"),
        (json!({"u": [a, b], "curves": [a, b]}), "loop"),
        (json!({"u": [a, b], "crown": 1}), "loop"),
        (json!({"u": [a, b], "operation": "cut"}), "operation"),
        (json!({"u": [a, b], "operation": "join"}), "join"),
        (json!({"u": [a]}), "two curves"),
        (json!({"u": [], "v": [a, b]}), "`u`"),
        // The kernel's refusals: curves that miss, a closed curve, G1 on an edge between two faces.
        (json!({"u": [a, b], "v": [{"curves": [far]}, {"curves": [a]}]}), "don't meet"),
        (json!({"u": [{"curves": [ring]}, b]}), "closed"),
        (json!({"u": [{"body": boxed, "edges": [[45, 40, 0]], "continuity": "g1"}, b]}), "open edge"),
        (json!({"u": [a, {"point": [5, 2, 0]}, b]}), "point"),
    ];
    for (params, want) in cases {
        let e = match s.execute("surface.boundary", &params) {
            Ok(v) => panic!("{params} was built: {v}"),
            Err(e) => e,
        };
        assert!(!matches!(e, crate::EngineError::Internal(..)), "{params}: {e}");
        assert!(e.to_string().contains(want), "{params}: {e}");
    }
    assert_eq!(s.world_state().unwrap().bodies.len(), bodies, "a refused network left a body behind");
}

/// Three curves that do not connect end to end are refused naming the gap.
#[test]
fn loops_that_do_not_connect_are_refused() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [20.0, 0.0], [30.0, 0.0]);
    let c = line(&mut s, [30.0, 0.0], [0.0, 0.0]);
    let err = s.execute("surface.boundary", &json!({"curves": [a, b, c]})).unwrap_err();
    assert!(err.to_string().contains("list the curves in loop order"), "{err}");
}

/// An edit (`timeline.redefine`) may turn a loop of sketch curves into the network of the same
/// curves and back: the feature keeps its place and its name, and the surface its area.
#[test]
fn an_edit_turns_a_loop_into_a_network_and_back() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Square"}));
    let sides: Vec<String> = [[[0.0, 0.0], [10.0, 0.0]], [[10.0, 0.0], [10.0, 10.0]], [[10.0, 10.0], [0.0, 10.0]], [[0.0, 10.0], [0.0, 0.0]]]
        .iter()
        .map(|[a, b]| line(&mut s, *a, *b))
        .collect();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.boundary", json!({"sketch": "Square", "curves": sides, "name": "Patch"}));
    let network = json!({"sketch": "Square", "u": [sides[0], sides[2]], "v": [sides[1], sides[3]]});
    run(&mut s, "timeline.redefine", json!({"feature": "Patch", "command": "surface.boundary", "params": network}));
    let kind = |s: &Session| s.doc.find_feature("Patch").map(|f| f.kind.clone());
    assert!(matches!(kind(&s), Some(solidcraft_doc::FeatureKind::SurfaceNetwork { .. })), "{:?}", kind(&s));
    let body = s.world_state().unwrap().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    assert!((areas(&mut s, &body).0 - 100.0).abs() < 1e-6);
    run(&mut s, "timeline.redefine", json!({"feature": "Patch", "command": "surface.boundary", "params": {"sketch": "Square", "curves": sides}}));
    assert!(matches!(kind(&s), Some(solidcraft_doc::FeatureKind::SurfaceBoundary { .. })), "{:?}", kind(&s));
    assert!((areas(&mut s, &body).0 - 100.0).abs() < 1e-6);
}

/// #16508: a boundary surface and a loft built on one arc stitch into one sheet. The case is the
/// issue's: a half circle (R 13.8 about z = 5.5) on the plane x = 6 is lofted to the same half
/// circle on x = 60, and is also one of the four sides of a boundary network whose other sides
/// are an arc on x = -9 and two lines in the XZ plane. The two surfaces are fitted to the arc each
/// on its own, and their edges along it end up about 7e-5 apart, so before the fix the stitch was
/// refused ("Bad Gap ... offset=6.81e-05") at any stitch tolerance.
///
/// Edge count: each surface is one four-sided face, 4 + 4 = 8 edges, and the stitch makes the
/// arc they share one edge, so the sheet has 8 - 1 = 7 edges on 2 faces.
#[test]
fn a_boundary_surface_and_a_loft_surface_on_one_curve_stitch() {
    let mut s = Session::default();
    let mut arc = |name: &str, offset: f64, start: [f64; 2], end: [f64; 2], through: [f64; 2]| {
        run(&mut s, "sketch.create", json!({"plane": "YZ", "offset": offset, "name": name}));
        run(&mut s, "sketch.arc.three_point", json!({"start": start, "end": end, "through": through}));
        run(&mut s, "sketch.finish", json!({}));
    };
    let half = ([0.0, -8.3], [0.0, 19.3], [-13.8, 5.5]);
    arc("Shared", 6.0, half.0, half.1, half.2);
    arc("Far", 60.0, half.0, half.1, half.2);
    // The issue's back arc: centre (1.5255, 0.9839), through both ends and its point farthest out.
    let (centre, start, end): ([f64; 2], [f64; 2], [f64; 2]) =
        ([1.5254923566139873, 0.9839315653407356], [0.0, -8.888970788629825], [0.0, 10.856833919298804]);
    let r = (centre[0] - start[0]).hypot(centre[1] - start[1]);
    arc("Back", -9.0, start, end, [centre[0] - r, centre[1]]);
    // The two rails in the XZ plane (a sketch point (u, v) is the model point (u, 0, -v)).
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Rails"}));
    let low = line(&mut s, [6.0, 8.3], [-9.0, -start[1]]);
    let high = line(&mut s, [-9.0, -end[1]], [6.0, -19.3]);
    run(&mut s, "sketch.finish", json!({}));
    let curve = |sketch: &str, id: &str| json!({"sketch": sketch, "curves": [id]});
    let loft = run(&mut s, "surface.loft", json!({"sections": [curve("Shared", "a1"), curve("Far", "a1")]}));
    let boundary = run(
        &mut s,
        "surface.boundary",
        json!({"u": [curve("Rails", &low), curve("Rails", &high)], "v": [curve("Shared", "a1"), curve("Back", "a1")]}),
    );
    let (a, b) = (first(&boundary["created_bodies"]), first(&loft["created_bodies"]));
    assert!(!a.is_empty() && !b.is_empty() && a != b, "{boundary} / {loft}");
    for body in [&a, &b] {
        let edges = run(&mut s, "model.edges", json!({"body": body}));
        assert_eq!(edges["edges"].as_array().map(Vec::len), Some(4), "each surface is four-sided: {edges}");
    }
    let stitched = run(&mut s, "surface.stitch", json!({"bodies": [a, b]}));
    let bodies: Vec<&Value> = ["created_bodies", "changed_bodies"].iter().filter_map(|k| stitched[*k].as_array()).flatten().collect();
    assert_eq!(bodies.len(), 1, "one sheet: {stitched}");
    let body = bodies[0].as_str().unwrap_or_default();
    let faces = run(&mut s, "model.faces", json!({"body": body}));
    assert_eq!(faces["faces"].as_array().map(Vec::len), Some(2), "{faces}");
    let edges = run(&mut s, "model.edges", json!({"body": body}));
    assert_eq!(edges["edges"].as_array().map(Vec::len), Some(7), "{edges}");
}

/// A long, tilted pair of curved sheets: the leave direction at the first corner crosses both
/// parameter boundaries. Reading its shape must not shrink a finite-difference step and carry
/// its round-off error into every station of the requested G3 blend.
#[test]
fn a_g3_blend_at_a_curved_sheet_corner_keeps_both_seams() {
    let mut s = Session::default();
    for (name, x, start, end, through) in [
        ("H0", 0, [0, 0], [150, 15], [75, 12]),
        ("H1", 600, [0, 5], [150, 20], [75, 17]),
        ("F0", 0, [180, 18], [320, -10], [250, 16]),
        ("F1", 600, [180, 23], [320, -5], [250, 21]),
    ] {
        run(&mut s, "sketch.create", json!({"plane": {"origin": [x, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": name}));
        run(&mut s, "sketch.arc.three_point", json!({"start": start, "end": end, "through": through}));
        run(&mut s, "sketch.finish", json!({}));
    }
    for (name, a, b) in [("Hood", "H0", "H1"), ("Fender", "F0", "F1")] {
        run(&mut s, "surface.loft", json!({"sections": [{"sketch": a, "curves": ["a1"]}, {"sketch": b, "curves": ["a1"]}], "body_name": name}));
    }
    run(
        &mut s,
        "surface.blend",
        json!({"from": {"body": "Hood", "edges": [[300, 150, 17.5]]}, "to": {"body": "Fender", "edges": [[300, 180, 20.5]]}, "continuity": ["g3", "g3"], "stitch": true, "body_name": "Transition"}),
    );
    let result = run(&mut s, "inspect.continuity", json!({"body": "Transition", "edges": [[300, 150, 17.5], [300, 180, 20.5]], "samples": 257}));
    let edges = result["bodies"][0]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2);
    for edge in edges {
        assert!(matches!(edge["order"].as_str(), Some("g3" | "g4")), "{}", edge["curvature_change_jump_per_mm2"]);
    }
}

/// #17197: a boundary curve made of two sketch lines that meet at a sharp corner can't be fitted
/// smoothly, so the surface is refused, and the refusal names the corner and how sharply the
/// curve turns there. The V runs from (0, 0) to (10, 0) and then back up to (4, 6): it turns by
/// 135° at (10, 0, 0).
#[test]
fn a_curve_with_a_corner_is_refused_naming_the_corner() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Vee"}));
    let out = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let back = line(&mut s, [10.0, 0.0], [4.0, 6.0]);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "High"}));
    let high = line(&mut s, [0.0, 0.0], [10.0, 6.0]);
    run(&mut s, "sketch.finish", json!({}));
    let params = json!({"u": [{"sketch": "Vee", "curves": [out, back]}, {"sketch": "High", "curves": [high]}]});
    let e = match s.execute("surface.boundary", &params) {
        Ok(v) => panic!("a curve with a corner was built: {v}"),
        Err(e) => e.to_string(),
    };
    assert!(e.contains("can't be fitted closely enough; its curves meet at (10, 0, 0) turning by 135.0°"), "{e}");
}

/// #17053: a boundary surface through two curves that meet at one end (a quarter arc on the
/// ground and a straight side rising from one of its ends) is a loft whose side between them
/// collapses to that point. Each side's distance from the surface is proved; a side starting at
/// the collapsed corner was refused ("gap is not enclosed") when its first nearest point fell
/// elsewhere along the collapsed side, which rounding decided: before the fix 7 of these 32
/// cases were refused. Every placement and direction builds the one face.
#[test]
fn two_curves_meeting_at_a_corner_bound_their_distance_from_the_surface() {
    let q = std::f64::consts::FRAC_PI_2;
    for r in [15.0, 7.3] {
        for t0 in [0.1f64, 0.37, 1.0, 5.5] {
            let at = |t: f64| [r * t.cos(), r * t.sin()];
            let corner = [r * t0.cos(), r * t0.sin(), 0.0];
            let far = [2.0 * r * (t0 - 0.4).cos(), 2.0 * r * (t0 - 0.4).sin(), 3.3 * r];
            for (start, end) in [(at(t0), at(t0 + q)), (at(t0 + q), at(t0))] {
                for (a, b) in [(corner, far), (far, corner)] {
                    let mut s = Session::default();
                    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ground"}));
                    let arc = run(&mut s, "sketch.arc.three_point", json!({"start": start, "end": end, "through": at(t0 + 0.5 * q)}));
                    let arc = first(&arc["created"]["curves"]);
                    run(&mut s, "sketch.finish", json!({}));
                    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Side"}));
                    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
                    let side = line3d(&mut s, a, b);
                    run(&mut s, "sketch.finish", json!({}));
                    let made =
                        s.execute("surface.boundary", &json!({"u": [{"sketch": "Ground", "curves": [arc]}, {"sketch": "Side", "curves": [side]}]}));
                    let made = match made {
                        Ok(v) => v,
                        Err(e) => panic!("r {r}, arc from {start:?}, side from {a:?}: {e}"),
                    };
                    let faces = run(&mut s, "model.faces", json!({"body": first(&made["created_bodies"])}));
                    assert_eq!(faces["faces"].as_array().map(Vec::len), Some(1), "r {r}, arc from {start:?}, side from {a:?}: {faces}");
                }
            }
        }
    }
}
