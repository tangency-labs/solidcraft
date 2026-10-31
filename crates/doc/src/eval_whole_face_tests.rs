//! Extrudes from a whole face whose outline is no lines and arcs (#2040), checked through the
//! tool `whole_face_tool` picks.

use std::cell::Cell;

use serde_json::json;
use solidcraft_geom::{Plane, Vec2, Vec3};
use solidcraft_kernel as kernel;
use solidcraft_sketch::{CurveKind, Sketch};

use crate::*;

thread_local! {
    static SLABS: Cell<usize> = const { Cell::new(0) };
}

/// A cut took a slab over the whole body as its tool, on this thread.
pub(super) fn slab_taken() {
    SLABS.with(|n| n.set(n.get() + 1));
}

/// A closed spline through six points round the origin, on the plane at height `z`.
fn spline_sketch(z: f64) -> FeatureKind {
    let mut sk = Sketch::new();
    let fit = [(8.0, 0.0), (4.0, 6.0), (-3.0, 7.0), (-7.0, 1.0), (-4.0, -6.0), (3.0, -5.0)];
    let mut pts: Vec<usize> = fit.iter().map(|&(x, y)| sk.add_point(Vec2::new(x, y), None).unwrap()).collect();
    pts.push(pts[0]);
    sk.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: true, handles: Vec::new() }, None).unwrap();
    let plane = Plane { origin: Vec3::new(0.0, 0.0, z), ..Plane::XY };
    FeatureKind::Sketch { plane: PlaneRef::Custom { plane }, sketch: sk }
}

fn extrude(doc: &mut Document, sketch: u64, extent: serde_json::Value, operation: Operation) {
    let kind = FeatureKind::Extrude {
        thin: None,
        sketch: SketchId(sketch),
        profiles: ProfileSel::All,
        extent: serde_json::from_value(extent).unwrap(),
        operation,
        targets: vec![],
    };
    doc.add_feature(kind, None).unwrap();
}

fn volume(model: &Model) -> f64 {
    let state = model.state();
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert_eq!(state.bodies.len(), 1);
    assert!(state.bodies[0].body.validity().is_empty(), "{:?}", state.bodies[0].body.validity());
    kernel::measure(&state.bodies[0].body).unwrap().volume
}

/// A prism 10 tall on a closed spline, standing on a 40 x 40 x 5 plate, cut 3 into its whole
/// top face. The walls only touch the prism, so the cut takes a slab over the whole body from 3
/// below the face up: it must take the face's area times 3 and leave the plate alone.
#[test]
fn slab_cut_into_a_spline_prism() {
    let mut doc = Document::new("spline prism");
    let plate = FeatureKind::Box {
        corner: point_expr(Vec3::new(-20.0, -20.0, 0.0)),
        length: "40 mm".into(),
        width: "40 mm".into(),
        height: "5 mm".into(),
        operation: Operation::NewBody,
    };
    doc.add_feature(plate, None).unwrap();
    let base = doc.add_feature(spline_sketch(5.0), None).unwrap();
    extrude(&mut doc, base, json!({"distance": "10 mm"}), Operation::Join);
    let mut model = Model::new();
    model.evaluate(&doc);
    let joined = volume(&model);
    let area = (joined - 40.0 * 40.0 * 5.0) / 10.0;
    assert!(area > 100.0, "the prism's area {area}");

    let top = doc.add_feature(spline_sketch(15.0), None).unwrap();
    extrude(&mut doc, top, json!({"distance": "3 mm", "direction": "negative"}), Operation::Cut);
    SLABS.with(|n| n.set(0));
    model.evaluate(&doc);
    let cut = volume(&model);
    assert_eq!(SLABS.with(Cell::get), 1, "the cut took the slab");
    assert!((joined - cut - area * 3.0).abs() < 1e-3 * area, "took {}, expected {}", joined - cut, area * 3.0);
    let bounds = model.state().bodies[0].mesh().bounds();
    assert!((bounds.max.z - 12.0).abs() < 1e-6, "top at {}", bounds.max.z);
    assert!(bounds.min.dist(Vec3::new(-20.0, -20.0, 0.0)) < 1e-6, "plate from {:?}", bounds.min);
}
