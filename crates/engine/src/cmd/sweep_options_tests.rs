//! Sweep options through the commands: orientation, twist and end scale on solid.sweep and
//! surface.sweep, checked against analytic values; naming, saving and hostile values.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

/// A circle `C` (radius 10, curve `c`), a square `Q` (side 10, curves `a`…`d`) on XY, and a
/// path `P`: a line 40 up the Z axis (`up`).
fn setup() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "C"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10, "id": "c"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Q"}));
    run(&mut s, "sketch.line", json!({"points": [[-5, -5], [5, -5], [5, 5], [-5, 5]], "closed": true, "ids": ["a", "b", "c", "d"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "P"}));
    // The XZ sketch's y runs down world Z.
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -40]], "ids": ["up"]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn measure(s: &Session, body: &str) -> solidcraft_kernel::BodyMeasure {
    solidcraft_kernel::measure(&s.world_state().body(body).unwrap().body).unwrap()
}

fn sweep(s: &mut Session, sketch: &str, body: &str, extra: Value) -> Result<Value, String> {
    let mut p = json!({"sketch": sketch, "path_sketch": "P", "path": ["up"], "operation": "new", "body_name": body});
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    s.execute("solid.sweep", &p).map_err(|e| e.to_string())
}

fn face_names(s: &mut Session, body: &str) -> Vec<String> {
    let f = run(s, "model.faces", json!({ "body": body }));
    let mut n: Vec<String> = f["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x["name"].as_str())
        // Without the feature's own prefix (`F7:`), to compare features.
        .map(|x| x.split_once(':').map_or(x, |(_, rest)| rest).to_string())
        .collect();
    n.sort();
    n
}

/// A circle swept up a line to end scale 0.5 is a cone frustum; the surface sweep of its curve
/// is the frustum's lateral face. Saved and reopened, the design gives the same solid.
#[test]
fn scaled_sweep_is_a_frustum() {
    let mut s = setup();
    let (big, small, h) = (10.0, 5.0, 40.0);
    let volume = PI * h * (big * big + big * small + small * small) / 3.0;
    let lateral = PI * (big + small) * (h * h + (big - small) * (big - small)).sqrt();
    sweep(&mut s, "C", "Frustum", json!({"scale": 0.5})).unwrap();
    let m = measure(&s, "Frustum");
    assert!((m.volume - volume).abs() < 2e-3 * volume, "volume {} vs {volume}", m.volume);
    assert!((m.area - PI * (big * big + small * small) - lateral).abs() < 2e-3 * lateral, "area {}", m.area);
    run(&mut s, "surface.sweep", json!({"sketch": "C", "curves": ["c"], "path_sketch": "P", "path": ["up"], "scale": "1/2", "body_name": "Cone"}));
    let a = measure(&s, "Cone").area;
    assert!((a - lateral).abs() < 2e-3 * lateral, "sheet {a} vs {lateral}");
    // The options are saved with the feature and evaluate the same when reopened.
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let reopened = Session::new(doc);
    let again = measure(&reopened, "Frustum");
    assert!((again.volume - m.volume).abs() < 1e-9 * volume);
}

/// A twisted square keeps its faces' names (each wall after its profile curve), and the twist is
/// a parameter: editing it changes the solid, a plain sweep has the same names.
#[test]
fn twisted_sweep_faces_follow_the_profile_curves() {
    let mut s = setup();
    sweep(&mut s, "Q", "Plain", json!({})).unwrap();
    sweep(&mut s, "Q", "Twisted", json!({"twist": "90 deg", "name": "Twist"})).unwrap();
    let (plain, twisted) = (face_names(&mut s, "Plain"), face_names(&mut s, "Twisted"));
    assert_eq!(plain.len(), 6, "{plain:?}");
    assert_eq!(plain, twisted);
    for curve in ["a", "b", "c", "d"] {
        assert!(twisted.iter().any(|n| n.ends_with(&format!(":{curve}"))), "{curve} in {twisted:?}");
    }
    // The end cap turns with the twist: at 90 deg a square's corners are back where they were,
    // at 45 deg its top reaches out to the corner radius along the axes.
    let reach = |s: &Session| {
        let mesh = s.world_state().body("Twisted").unwrap().body.tessellate(0.01).unwrap();
        mesh.positions.iter().filter(|p| (p.z - 40.0).abs() < 1e-6).map(|p| p.x).fold(f64::MIN, f64::max)
    };
    assert!((reach(&s) - 5.0).abs() < 1e-3, "{}", reach(&s));
    run(&mut s, "timeline.edit", json!({"feature": "Twist", "set": {"twist": "45 deg"}}));
    assert!((reach(&s) - 50f64.sqrt()).abs() < 1e-2, "{}", reach(&s));
    let v = measure(&s, "Twisted").volume;
    assert!((v - 4000.0).abs() < 0.01 * 4000.0, "{v}");
}

/// Parallel along a straight path leaning out of the profile's plane gives a sheared prism (the
/// volume of the upright one); so does a direction, along a straight path.
#[test]
fn orientations() {
    let mut s = setup();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Lean"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, -40]], "ids": ["lean"]}));
    run(&mut s, "sketch.finish", json!({}));
    let lean = |s: &mut Session, body: &str, o: Value| {
        s.execute(
            "solid.sweep",
            &json!({"sketch": "Q", "path_sketch": "Lean", "path": ["lean"], "operation": "new", "body_name": body, "orientation": o}),
        )
    };
    lean(&mut s, "Sheared", json!("parallel")).unwrap();
    let m = measure(&s, "Sheared");
    assert!((m.volume - 4000.0).abs() < 2e-3 * 4000.0, "{}", m.volume);
    // Its ends stay level: flat at z = 0 and z = 40.
    assert!(m.bbox.min.z.abs() < 1e-6 && (m.bbox.max.z - 40.0).abs() < 1e-6, "{:?}", m.bbox);
    // Along a straight path every frame is the first: a direction changes nothing, the profile
    // keeps its relation to the path as in a plain sweep.
    lean(&mut s, "Directed", json!([0, 1, 0])).unwrap();
    let d = measure(&s, "Directed");
    assert!((d.volume - 4000.0).abs() < 2e-3 * 4000.0, "{}", d.volume);
    assert!(d.bbox.max.y <= 5.0 + 1e-6 && d.bbox.min.y >= -5.0 - 1e-6);
}

/// Hostile options are refused with an error and leave the design as it was.
#[test]
fn hostile_sweep_options_are_refused() {
    let mut s = setup();
    let before = s.doc.features.len();
    for bad in [
        json!({"twist": "NaN"}),
        json!({"twist": 1e300}),
        json!({"twist": "1e9 deg"}),
        json!({"twist": "1441 deg"}),
        json!({"twist": "nope"}),
        json!({"twist": [1, 2]}),
        json!({"scale": 0}),
        json!({"scale": -1}),
        json!({"scale": "0"}),
        json!({"scale": "1e9"}),
        json!({"scale": "1/0"}),
        json!({"orientation": "sideways"}),
        json!({"orientation": [0, 0, 0]}),
        json!({"orientation": [1e308, 0, 0]}),
        json!({"orientation": {}}),
        json!({"orientation": 7}),
        // Along the path itself: no turn about the path is defined.
        json!({"orientation": [0, 0, 1]}),
    ] {
        assert!(sweep(&mut s, "Q", "Bad", bad.clone()).is_err(), "{bad}");
        let mut p = json!({"sketch": "Q", "curves": ["a", "b"], "path_sketch": "P", "path": ["up"]});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(s.execute("surface.sweep", &p).is_err(), "surface {bad}");
        assert_eq!(s.doc.features.len(), before, "{bad}");
    }
}

/// A G2 round-over swept along a projected body edge and cut from the body.
///
/// The sweep tool itself is a G2 corner-removal profile swept along one chain of the
/// projected bottom-face loop (a G2 spline plus the line after it). The cut of the
/// tool from the box used to fail with `boolean Cut: the operation failed: no result`,
/// side-dependent: the same chain on the front-left side works. See solidcraft#3604.
#[test]
fn g2_swept_round_over_cut_along_a_projected_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 40, "width": 20, "height": 10, "operation": "new", "body_name": "B"}));
    run(
        &mut s,
        "solid.fillet",
        json!({
            "body": "B",
            "edges": [[0, 0, 5], [40, 0, 5], [40, 20, 5], [0, 20, 5]],
            "radius": 5,
            "type": "g2"
        }),
    );
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 10, 0]}, "project_edges": true, "name": "Bottom"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 15, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "RB"}));
    run(&mut s, "sketch.line", json!({"points": [[-0.2, -0.2], [0.8, -0.2]], "ids": ["o1"]}));
    run(&mut s, "sketch.line", json!({"points": [[0.8, -0.2], [0.8, 0]], "ids": ["o2"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0.8], [-0.2, 0.8]], "ids": ["o3"]}));
    run(&mut s, "sketch.line", json!({"points": [[-0.2, 0.8], [-0.2, -0.2]], "ids": ["o4"]}));
    run(&mut s, "sketch.blend_curve", json!({"a": "o2.end", "b": "o3.start", "continuity": "g2"}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.sweep",
        json!({
            "sketch": "RB",
            "path_sketch": "Bottom",
            "path": ["s2", "l3"],
            "orientation": "perpendicular",
            "operation": "new",
            "body_name": "Tool"
        }),
    );
    run(&mut s, "solid.combine", json!({"target": "B", "tools": ["Tool"], "operation": "cut"}));
}
