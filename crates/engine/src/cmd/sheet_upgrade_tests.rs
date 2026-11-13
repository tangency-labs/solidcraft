use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcore_record::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// The changes of format 55 main's builds had while they were pending, as main lists them in a
/// design's `format_pending`.
const MAINS_PENDING: [&str; 7] = [
    "blend-definition-faces",
    "construct-snap-points",
    "plugin-data-namespace",
    "sketch-vertex-link-names",
    "solid-move-origin",
    "spline-tangent-handles",
    "split-face-loops",
];

/// A design as main saves it (format 55, with main's pending changes; #6748's flat sheet model): a
/// rule R1, 1 mm thick with 1 mm bends; a 100 x 60 rectangle on XY from the origin, its line `l1`
/// along the front (y = 0); a base flange of it; an edge flange 20 high on the front edge, picked
/// on the base's top and saved by the sheet's own name for that edge, `base:l1:top`; a hem 8 long
/// with a 1 mm gap (main's: the hem's inner radius) on that flange's far edge, picked on the
/// flange's top, `F3.0:far:top`; and, suppressed, a hem on the flange's two ends, `side0` on its
/// top and `side1` on its bottom. Each saved field is one main writes, as it writes it.
fn saved_by_main() -> Value {
    let points = json!([
        {"id": "origin", "pos": [0.0, 0.0], "fixed": true},
        {"id": "l1.start", "pos": [0.0, 0.0], "fixed": false},
        {"id": "l1.end", "pos": [100.0, 0.0], "fixed": false},
        {"id": "l2.end", "pos": [100.0, 60.0], "fixed": false},
        {"id": "l3.end", "pos": [0.0, 60.0], "fixed": false},
    ]);
    let curves = json!([
        {"id": "l1", "type": "line", "a": 1, "b": 2, "construction": false},
        {"id": "l2", "type": "line", "a": 2, "b": 3, "construction": false},
        {"id": "l3", "type": "line", "a": 3, "b": 4, "construction": false},
        {"id": "l4", "type": "line", "a": 4, "b": 1, "construction": false},
    ]);
    let sketch = json!({"points": points, "curves": curves, "constraints": [], "counters": {"l": 4}});
    let features = json!([
        {"id": 1, "name": "Base", "suppressed": false, "type": "sketch", "plane": {"type": "origin", "name": "XY"}, "sketch": sketch},
        {"id": 2, "name": "BaseFlange1", "suppressed": false, "type": "sheet_base", "sketch": 1, "profiles": {"type": "all"}, "rule": "R1"},
        {"id": 3, "name": "EdgeFlange1", "suppressed": false, "param_names": ["d1", "d2"], "edge_names": ["base:l1:top"],
         "type": "sheet_flange", "edges": [[50.0, 0.0, 1.0]], "height": "20", "angle": "90 deg", "position": "inside"},
        {"id": 4, "name": "Hem1", "suppressed": false, "param_names": ["d3", "d4"], "edge_names": ["F3.0:far:top"],
         "type": "sheet_hem", "edges": [[50.0, 1.0, 20.0]], "length": "8", "gap": "1 mm"},
        {"id": 5, "name": "Hem2", "suppressed": true, "param_names": ["d5"], "edge_names": ["F3.0:side0:top", "F3.0:side1:bottom"],
         "type": "sheet_hem", "edges": [[0.0, 1.0, 10.0], [100.0, 0.0, 10.0]], "length": "8"},
    ]);
    let rule = json!({
        "name": "R1", "thickness": "1 mm", "k_factor": "0.44", "bend_radius": "1 mm", "relief_width": "Thickness", "relief_depth": "Thickness * 0.5",
        "corner_relief": "Thickness * 4.0", "corner_relief_shape": "notch", "hem_gap": "0.01 mm", "gap": "Thickness",
    });
    json!({
        "format": "solidcraft/55", "format_pending": MAINS_PENDING, "name": "Bracket", "units": "mm", "params": [], "features": features, "next_id": 6,
        "sheet": {"rules": [rule], "active": "R1"},
    })
}

/// A flange and a hem main saved by the sheet's own names for their edges open with each pick by
/// the model's name for that edge (format pending sheet-edge-names): the flange's
/// `F2:side:l1|F2:top`, where the base flange's top meets its rim on `l1`, and the hem's
/// `F3:far:F2:side:l1|F3:top:F2:side:l1`, where the flange's far rim meets its top. The part
/// rebuilds with no errors, as main built it.
#[test]
fn a_flange_and_a_hem_main_saved_rebuild_on_their_edges() {
    let doc = solidcraft_doc::Document::from_json(&saved_by_main().to_string()).unwrap();
    let names = |id: u64| doc.feature(id).map(|f| f.edge_names.clone()).unwrap_or_default();
    assert_eq!(names(3), ["F2:side:l1|F2:top"]);
    assert_eq!(names(4), ["F3:far:F2:side:l1|F3:top:F2:side:l1"]);
    // `side0` was at the start of the edge as the sheet ran it, with the base's material on its
    // left seen from the top (counterclockwise: the front edge from x = 0 to 100); the model's end
    // rims are named from the bend's anchor, which runs it the other way, `end0` at its start (x =
    // 100). So `side0` is `end1` and `side1` is `end0`.
    assert_eq!(names(5), ["F3:end1:F2:side:l1|F3:top:F2:side:l1", "F3:bottom:F2:side:l1|F3:end0:F2:side:l1"]);
    let mut s = Session::new(doc);
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results.iter().map(|r| (&r.name, &r.error)).collect::<Vec<_>>());
    // The flange's bend has its inside, radius R = 1, on the base's top, and its outer face lies on
    // the front edge (inside: 20 high from the outer virtual sharp, at y = z = 0): the base is cut
    // back by R + T = 2 to y = 2…60, 58 x 100 x 1 = 5800; the bend is a quarter ring, π/4 · (2² −
    // 1²) · 100 = 75π; the leg runs from z = R + T = 2 to 20 at y = 0…1, 18 x 100 x 1 = 1800. The
    // hem's gap opens as twice main's (format pending sheet-hem-gap), so its bend's inner radius is
    // 1, as main had it: half a ring over the flange's outer face from its top at z = 20, π/2 · (2²
    // − 1²) · 100 = 150π, and its leg the length less that radius and the thickness, 8 − 2 = 6,
    // 6 x 100 x 1 = 600. In all 8200 + 225π = 8906.858.
    let want = 5800.0 + 1800.0 + 600.0 + 225.0 * PI;
    assert!(rel(volume(&mut s), want) < 1e-6, "{} vs {want}", volume(&mut s));
    // The flange as the hem found it: its end rim at x = 0, mid-leg, is `end1`, the one at x = 100
    // `end0`, and the edges the suppressed hem's names name lie where main's `side0` on the top
    // (y = 1) and `side1` on the bottom (y = 0) did.
    let before = s.model.state_before(4);
    let body = before.bodies.first().unwrap();
    for (x, end) in [(0.0, "end1"), (100.0, "end0")] {
        assert_eq!(solidcraft_doc::naming::face_name_at_body(body, Vec3::new(x, 0.5, 12.0)), Some(format!("F3:{end}:F2:side:l1")), "x = {x}");
    }
    let edges = solidcraft_doc::naming::edge_names(body);
    for (name, near) in [
        ("F3:far:F2:side:l1|F3:top:F2:side:l1", Vec3::new(50.0, 1.0, 20.0)),
        ("F3:end1:F2:side:l1|F3:top:F2:side:l1", Vec3::new(0.0, 1.0, 10.0)),
        ("F3:bottom:F2:side:l1|F3:end0:F2:side:l1", Vec3::new(100.0, 0.0, 10.0)),
    ] {
        assert!(edges.iter().any(|n| n == name), "{name} is no edge's name: {edges:?}");
        let hit = solidcraft_doc::naming::locate_edge(body, name, near).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(hit.on_point, "{name} is not at {near:?}: {hit:?}");
    }
}

/// Without the upgrade (the design says it has it already), the flange's saved name is no edge's
/// and it asks for its edge to be picked again: the names are what the upgrade changes.
#[test]
fn a_flange_main_saved_without_the_upgrade_asks_for_its_edge() {
    let mut design = saved_by_main();
    if let Some(pending) = design["format_pending"].as_array_mut() {
        pending.push(json!("sheet-edge-names"));
    }
    let doc = solidcraft_doc::Document::from_json(&design.to_string()).unwrap();
    assert_eq!(doc.feature(3).map(|f| f.edge_names.clone()).unwrap_or_default(), ["base:l1:top"]);
    let s = Session::new(doc);
    let error = s.model.result(3).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(error.contains("`base:l1:top`"), "{error}");
}
