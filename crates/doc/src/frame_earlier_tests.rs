#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solidcore_record::Vec2;
use solidcraft_sketch::Sketch;

use super::TOOL_BUILDS;
use crate::{Document, FeatureKind, Model, PlaneRef};

/// A sketch on XY of `lines`, each from its first point to its second.
fn lines_sketch(lines: &[[[f64; 2]; 2]]) -> FeatureKind {
    let mut sk = Sketch::new();
    for [a, b] in lines {
        let a = sk.add_point(Vec2::new(a[0], a[1]), None).unwrap();
        let b = sk.add_point(Vec2::new(b[0], b[1]), None).unwrap();
        sk.add_line_pts(a, b, None).unwrap();
    }
    FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }
}

/// A Frame Member on sketch `sketch`, in `size` square tube.
fn member(sketch: u64, size: f64, extra: serde_json::Value) -> FeatureKind {
    let size = format!("{size}");
    let mut p = serde_json::json!({"type": "frame_member", "path_sketch": sketch,
        "profile": {"source": "library", "profile": {"kind": "rect_tube", "width": size, "height": size, "wall": "2"}}});
    for (k, v) in extra.as_object().into_iter().flatten() {
        p[k] = v.clone();
    }
    serde_json::from_value(p).unwrap()
}

#[test]
fn each_earlier_tool_is_built_once() {
    // A: one 40 tube along X, y −20…20. B: three uprights ending on it at x = 100, 300, 500, fitted to it. Their
    // three ends meet the one tube, whose tool is built once.
    let mut doc = Document::new("earlier");
    let a = doc.add_feature(lines_sketch(&[[[0.0, 0.0], [600.0, 0.0]]]), None).unwrap();
    doc.add_feature(member(a, 40.0, serde_json::json!({"body_name": "A"})), None).unwrap();
    let b = doc
        .add_feature(lines_sketch(&[[[100.0, 0.0], [100.0, 300.0]], [[300.0, 0.0], [300.0, 300.0]], [[500.0, 0.0], [500.0, 300.0]]]), None)
        .unwrap();
    doc.add_feature(member(b, 25.0, serde_json::json!({"corner": "auto", "fit_ends": true, "body_name": "B"})), None).unwrap();
    let mut model = Model::new();
    TOOL_BUILDS.set(0);
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert_eq!(TOOL_BUILDS.get(), 1);
    // Each upright is fitted to the tube's near face, y = 20, its far end square at 300.
    let st = model.state();
    let uprights: Vec<_> = st.bodies.iter().filter(|b| b.name.starts_with('B')).collect();
    assert_eq!(uprights.len(), 3);
    for b in uprights {
        let bb = b.body.exact_bounds().unwrap();
        assert!((bb.min.y - 20.0).abs() < 1e-6 && (bb.max.y - 300.0).abs() < 1e-6, "{}: {bb:?}", b.name);
    }
}

#[test]
fn later_frames_fit_to_each_earlier_groups_actual_placement() {
    let mut doc = Document::new("grouped earlier frames");
    let a = doc.add_feature(lines_sketch(&[[[0.0, 0.0], [600.0, 0.0]], [[0.0, 200.0], [600.0, 200.0]]]), None).unwrap();
    let ids: Vec<String> = doc.sketch(a).unwrap().curves.iter().map(|c| c.id.clone()).collect();
    doc.add_feature(
        member(
            a,
            40.0,
            serde_json::json!({"groups": [
                {"lines": [&ids[0]], "body_name": "Centred"},
                {"lines": [&ids[1]], "anchor": "bottom_left", "mirror": true, "angle": "90 deg", "body_name": "Offset"}
            ]}),
        ),
        None,
    )
    .unwrap();
    // The centred rail occupies y=-20..20. The second rail's rotated bottom-left
    // anchor puts its square at y=160..200, z=0..40. Each upright starts inside
    // its earlier tool and must stop at that tool's far face, y=20 or y=200.
    let b = doc.add_feature(lines_sketch(&[[[100.0, 0.0], [100.0, 100.0]], [[100.0, 180.0], [100.0, 300.0]]]), None).unwrap();
    doc.add_feature(member(b, 10.0, serde_json::json!({"anchor": "bottom_left", "corner": "auto", "fit_ends": true, "body_name": "B"})), None)
        .unwrap();
    let check = |doc: &Document| {
        let mut model = Model::new();
        TOOL_BUILDS.set(0);
        model.evaluate(doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
        assert_eq!(TOOL_BUILDS.get(), 2, "one tool for each earlier group's member");
        let st = model.state();
        for (name, low, high) in [("B", 20.0, 100.0), ("B (2)", 200.0, 300.0)] {
            let bounds = st.body(name).unwrap().body.exact_bounds().unwrap();
            assert!((bounds.min.y - low).abs() < 1e-6 && (bounds.max.y - high).abs() < 1e-6, "{name}: {bounds:?}");
        }
    };
    check(&doc);
    check(&Document::from_json(&doc.to_json()).unwrap());
}

#[test]
fn a_tube_whose_middle_lies_in_an_earlier_tube_keeps_what_sticks_out() {
    // A: one 40 tube along X, y −20…20. B: a 30 mm line from A's centre-line, (100, 0) to (100, 30), fitted to A: B's
    // middle (y = 15) lies inside A, and y 20…30 sticks out. That piece is kept (ruling C4), not cut away completely.
    let mut doc = Document::new("inside");
    let a = doc.add_feature(lines_sketch(&[[[0.0, 0.0], [600.0, 0.0]]]), None).unwrap();
    doc.add_feature(member(a, 40.0, serde_json::json!({"body_name": "A"})), None).unwrap();
    let b = doc.add_feature(lines_sketch(&[[[100.0, 0.0], [100.0, 30.0]]]), None).unwrap();
    doc.add_feature(member(b, 25.0, serde_json::json!({"corner": "auto", "fit_ends": true, "body_name": "B"})), None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    let st = model.state();
    let made: Vec<_> = st.bodies.iter().filter(|b| b.name.starts_with('B')).collect();
    assert_eq!(made.len(), 1);
    let bb = made[0].body.exact_bounds().unwrap();
    assert!((bb.min.y - 20.0).abs() < 1e-6 && (bb.max.y - 30.0).abs() < 1e-6, "{bb:?}");
}

#[test]
fn a_long_chain_of_overlap_pieces_is_followed_back_only_so_far() {
    // A staircase of 19 Frame Members, 10 square: F0 a line up x = 100, then F1, F2… with Fit Ends, each split by the
    // piece past the crossing of the one before (horizontals at y = 0, 100, 200…, uprights at x = 250, 350, 450…), so
    // each later piece's id names a piece of the frame before, back to F0. Working the ids out follows that chain
    // back at most 16 frames (past that a cutter is named by its body name), never deep enough to overflow the stack.
    let mut doc = Document::new("chain");
    let a = doc.add_feature(lines_sketch(&[[[100.0, -100.0], [100.0, 100.0]]]), None).unwrap();
    doc.add_feature(member(a, 10.0, serde_json::json!({"body_name": "F0"})), None).unwrap();
    for k in 1..=18u32 {
        let n = f64::from((k - 1) / 2);
        let line = if k % 2 == 1 {
            let y = 100.0 * n;
            if k == 1 { [[0.0, 0.0], [300.0, 0.0]] } else { [[50.0 + 100.0 * n, y], [300.0 + 100.0 * n, y]] }
        } else {
            let x = 150.0 + 100.0 * f64::from(k / 2);
            [[x, -250.0 + 100.0 * f64::from(k / 2)], [x, 50.0 + 100.0 * f64::from(k / 2)]]
        };
        let sk = doc.add_feature(lines_sketch(&[line]), None).unwrap();
        doc.add_feature(member(sk, 10.0, serde_json::json!({"corner": "auto", "fit_ends": true, "body_name": format!("F{k}")})), None).unwrap();
    }
    let mut model = Model::new();
    super::super::NAME_DEPTH.set(0);
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    let st = model.state();
    // Every frame past F0 is split: the last frame's later piece names the whole chain.
    let last = st.bodies.iter().filter(|b| b.name.starts_with("F18 · ")).map(|b| b.name.clone()).collect::<Vec<_>>();
    assert_eq!(last.len(), 1, "{:?}", st.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    assert!(last[0].ends_with("F2 · F1 · F0"), "{last:?}");
    assert!(super::super::NAME_DEPTH.get() <= super::super::MAX_NAME_DEPTH, "{}", super::super::NAME_DEPTH.get());
    let f18 = doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::FrameMember { .. })).unwrap();
    let id = crate::frame::member_of_body(&doc, &st, f18, &last[0]).unwrap().unwrap();
    assert!(id.contains('>'), "{id}");
}
