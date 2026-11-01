//! #2103: the live preview of an Extrude (Cut) on a sketch attached to a face of a body inside
//! a placed component occurrence must be drawn where the viewport shows the sketch, not at the
//! component's own frame. The committed result was always correct; the preview used to be built
//! from the component-local model state (`Session::preview`) while the viewport shows placed
//! occurrences, so the tool appeared offset by the placement — seen on a mirrored STEP copy,
//! and on a plain translated copy of a component. The fix routes the tool preview through
//! `Session::preview_world` (shipped in #4455); these tests pin the placement of the cut tool
//! the preview draws.

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};
use solidcraft_engine::geom::Vec3;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// The ear component: a 40 × 20 × 10 block `EarBlock` (local 0..40 x, 0..20 y, 0..10 z),
/// optionally with its body mirrored in the component's own YZ plane (the mirrored body of the
/// STEP copy — a document keeps every occurrence placement rigid, so an import's mirror lives
/// in the geometry), then its occurrence moved by [10, 20, 0] and left uncaptured: the
/// occurrence transform is what the viewport draws.
fn ear(mirror: bool) -> (Session, u64, u64) {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name": "Ear", "grounded": false}));
    let (component, occurrence) = (created["component"].as_u64().unwrap(), created["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "EarBlock"}));
    if mirror {
        run(&mut s, "solid.mirror", json!({"bodies": ["EarBlock"], "plane": "YZ"}));
    }
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [10, 20, 0]}));
    (s, component, occurrence)
}

/// A face-attached sketch on the placed block's top face at `face_point` (world), with two
/// circular cut profiles 8 mm either side of the picked point along the face plane's x axis,
/// and the Cut command the dialog would apply: One Side, 5 mm into the material.
/// Returns the command and the sketch.
fn two_circle_cut_on_face(s: &mut Session, component: u64, occurrence: u64, face_point: [f64; 3]) -> (Vec<(String, Value)>, u64) {
    run(s, "component.activate", json!({"component": component, "occurrence": occurrence}));
    let sketch = run(s, "sketch.create", json!({"plane": {"face": face_point}, "project_edges": false}))["sketch"].as_u64().unwrap();
    // The face plane's origin is the component's own, not the pick: place the circles around
    // the picked point in the plane's coordinates so both profiles sit on the face.
    let FeatureKind::Sketch { plane: PlaneRef::Face { plane, at, .. }, .. } = &s.doc.feature(sketch).unwrap().kind else {
        panic!("a sketch on a face")
    };
    let centre = plane.to_local(*at);
    for d in [-8.0, 8.0] {
        run(s, "sketch.circle.center", json!({"center": [centre.x + d, centre.y], "radius": 3}));
    }
    run(s, "sketch.finish", json!({}));
    let commands =
        vec![("solid.extrude".to_string(), json!({"sketch": sketch, "profiles": "all", "operation": "cut", "distance": 5, "direction": "negative"}))];
    (commands, sketch)
}

/// The vertices the cut preview draws as the see-through red tool (`GpuScene::xray`: 28-byte
/// records of position, normal and palette index), as their world bounds.
fn tool_bounds(xray: &[u8]) -> (Vec3, Vec3) {
    assert!(!xray.is_empty() && xray.len() % 28 == 0, "the cut preview must draw its tool");
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for record in xray.chunks_exact(28) {
        for k in 0..3 {
            let v = f32::from_le_bytes(record[k * 4..k * 4 + 4].try_into().unwrap()) as f64;
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }
    (Vec3::new(lo[0], lo[1], lo[2]), Vec3::new(hi[0], hi[1], hi[2]))
}

/// The tool must sit at `want` (world): its bounds centre within a tenth of a millimetre, its
/// spans within half a millimetre (an inscribed tessellation), and far from `buggy` — where a
/// preview built from the component-local state would draw it (#2103).
fn assert_tool_at(built: &crate::preview::Built, want: [f64; 3], buggy: [f64; 3]) {
    assert_eq!(built.error, None, "the cut preview must not fail");
    let (lo, hi) = tool_bounds(&built.scene.xray);
    let mid = [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, (lo.z + hi.z) / 2.0];
    let span = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
    for k in 0..3 {
        assert!((mid[k] - want[k]).abs() < 0.1, "tool centre {mid:?}, want {want:?} (the local-frame bug drew it centred near {buggy:?})");
    }
    let off = ((mid[0] - buggy[0]).powi(2) + (mid[1] - buggy[1]).powi(2) + (mid[2] - buggy[2]).powi(2)).sqrt();
    assert!(off > 15.0, "the tool sits at the component-local place the bug drew it ({buggy:?}, off by {off})");
    let want_span = [22.0, 6.0, 5.0];
    for k in 0..3 {
        assert!((span[k] - want_span[k]).abs() < 0.5, "tool span {span:?}, want {want_span:?}");
    }
}

/// #2103: a copy of a component translated by [10, 20, 0], a sketch on its top face with two
/// circular cut profiles: the red Cut preview sits on the sketch where the viewport shows it
/// (centred at the placed face, spanning both circles, 5 mm down into the material), not at the
/// component's own frame offset by the placement.
#[test]
fn extrude_cut_preview_on_a_face_sketch_in_a_translated_component_sits_at_the_sketch() {
    let (mut s, component, occurrence) = ear(false);
    let (commands, sketch) = two_circle_cut_on_face(&mut s, component, occurrence, [30.0, 30.0, 10.0]);
    let colors = crate::preview::Colors::from_tokens(&crate::theme::Tokens::dark());
    let built = crate::preview::compute(&s, &commands, colors).unwrap();
    // Placed: the local top-face centre (20, 10, 10) plus [10, 20, 0]; the tool cuts 5 mm down.
    assert_tool_at(&built, [30.0, 30.0, 7.5], [20.0, 10.0, 7.5]);
    // The sketch is anchored to the face of the placed body, as in the issue.
    let FeatureKind::Sketch { plane: PlaneRef::Face { .. }, .. } = s.doc.feature(sketch).unwrap().kind else { panic!("a sketch on a face") };
}

/// #2103: the shape of the STEP repro — a component whose body was mirrored, placed as a copy
/// (a document keeps occurrence placements rigid, so an import's mirror sits in the geometry).
/// The Cut preview must still sit on the sketch at its world position, not at the component's
/// own frame, and not be thrown off by the mirrored body beside it.
#[test]
fn extrude_cut_preview_on_a_face_sketch_in_a_mirrored_component_sits_at_the_sketch() {
    let (mut s, component, occurrence) = ear(true);
    let (commands, sketch) = two_circle_cut_on_face(&mut s, component, occurrence, [30.0, 30.0, 10.0]);
    let colors = crate::preview::Colors::from_tokens(&crate::theme::Tokens::dark());
    let built = crate::preview::compute(&s, &commands, colors).unwrap();
    assert_tool_at(&built, [30.0, 30.0, 7.5], [20.0, 10.0, 7.5]);
    let FeatureKind::Sketch { plane: PlaneRef::Face { .. }, .. } = s.doc.feature(sketch).unwrap().kind else { panic!("a sketch on a face") };
}
