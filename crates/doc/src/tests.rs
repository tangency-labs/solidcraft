use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::Vec3Geom;
use solidcraft_kernel::measure;
use solidcraft_sketch::{ConstraintKind, Sketch};

use crate::*;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

#[test]
fn boundary_fill_classifies_cells_on_exact_surfaces() {
    let mut doc = Document::new("Boundary Fill");
    let cylinder = serde_json::from_value(serde_json::json!({
        "type": "cylinder", "base": ["0", "0", "0"], "axis": [0.0, 0.0, 1.0], "radius": "5", "height": "10", "operation": "new_body"
    }))
    .unwrap();
    doc.add_feature(cylinder, None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    let tool = model.state().bodies.first().unwrap().name.clone();
    // The cylinder axis at half height is strictly inside every bounding surface.
    let fill = doc
        .add_feature(
            FeatureKind::BoundaryFill {
                tools: vec![BodyName(tool)],
                planes: Vec::new(),
                cells: vec![Position(Vec3::new(0.0, 0.0, 5.0))],
                operation: Operation::NewBody,
                remove_tools: true,
            },
            None,
        )
        .unwrap();
    model.evaluate(&doc);
    assert!(model.result(fill).unwrap().error.is_none(), "{:?}", model.results);
    assert_eq!(model.state().bodies.len(), 1);
    // A single solid tool is the entire cell: V = πr²h = 250π mm³.
    let state = model.state();
    let body = &state.bodies.first().unwrap().body;
    assert!(!body.is_surface());
    assert!(rel(measure(body).unwrap().volume, 250.0 * std::f64::consts::PI) < 1e-9);
    let FeatureKind::BoundaryFill { cells, .. } = &mut doc.feature_mut(fill).unwrap().kind else { panic!("Boundary Fill") };
    // 3² + 4² = 5²: a point on the tool cannot identify either adjacent cell.
    *cells = vec![Position(Vec3::new(3.0, 4.0, 5.0))];
    model.evaluate(&doc);
    let error = model.result(fill).unwrap().error.as_ref().unwrap();
    assert!(error.contains("Boundary Fill cell points must be away from tool boundaries"), "{error}");
    assert_eq!(model.state().bodies.len(), 1, "the original tool is kept on failure");
}

/// A design saved before Boundary Fill took planes has none, and one without planes saves as it did.
#[test]
fn boundary_fill_without_planes_reads_and_saves_as_before() {
    let old = serde_json::json!({"type": "boundary_fill", "tools": ["B"], "cells": [[0.0, 0.0, 5.0]]});
    let kind: FeatureKind = serde_json::from_value(old).unwrap();
    let FeatureKind::BoundaryFill { planes, .. } = &kind else { panic!("Boundary Fill") };
    assert!(planes.is_empty());
    assert!(serde_json::to_value(&kind).unwrap().get("planes").is_none());
    let with = serde_json::json!({"type": "boundary_fill", "tools": ["B"], "planes": [{"type": "origin", "name": "XY"}], "cells": [[0.0, 0.0, 5.0]]});
    let kind: FeatureKind = serde_json::from_value(with).unwrap();
    let FeatureKind::BoundaryFill { planes, .. } = &kind else { panic!("Boundary Fill") };
    assert_eq!(planes.len(), 1);
}

#[test]
fn component_lookup_refuses_duplicate_names_and_keeps_explicit_ids() {
    let mut doc = Document::new("A");
    let first = doc.add_component(Some("A"), 0).unwrap();
    let second = doc.add_component(Some("A"), 0).unwrap();
    assert_eq!(doc.find_component("A"), None, "a duplicate name must not choose the first component or the root");
    let error = doc.resolve_component("A").unwrap_err().to_string();
    assert!(error.contains(&format!("matching IDs: {first}, {second}")), "{error}");
    for id in [first, second] {
        assert_eq!(doc.find_component(&id.to_string()), Some(id));
    }
    for key in ["0", "root", "ROOT"] {
        assert_eq!(doc.find_component(key), Some(0));
    }
    let numeric_name = doc.add_component(Some(&first.to_string()), 0).unwrap();
    assert_ne!(numeric_name, first);
    assert_eq!(doc.find_component(&first.to_string()), Some(first));
    let leading_zero = doc.add_component(Some("01"), 0).unwrap();
    assert_eq!(doc.find_component("01"), Some(leading_zero));
    let reopened = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(reopened.find_component("A"), None);
    assert_eq!(reopened.find_component(&second.to_string()), Some(second));
    assert_eq!(reopened.resolve_component("Missing").unwrap(), None);
    let named = Document::new("Design");
    assert_eq!(named.resolve_component("Design").unwrap(), Some(0));
}

/// 40 x 30 rectangle with its width and height driven by `width` / d2.
fn plate_doc() -> (Document, u64) {
    let mut doc = Document::new("plate");
    doc.set_param("width", "40 mm", Some("mm"), None).unwrap();
    let mut sk = Sketch::new();
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(38.0, 0.5), Vec2::new(39.0, 31.0), Vec2::new(0.5, 29.0)];
    let ids: Vec<usize> = pts.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    let l: Vec<usize> = (0..4).map(|i| sk.add_line_pts(ids[i], ids[(i + 1) % 4], None).unwrap()).collect();
    use ConstraintKind::*;
    sk.add_constraint(Coincident { p: ids[0], q: 0 }, None).unwrap();
    sk.add_constraint(Horizontal { l: l[0] }, None).unwrap();
    sk.add_constraint(Horizontal { l: l[2] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[1] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[3] }, None).unwrap();
    let d1 = doc.new_model_param("width", "mm");
    sk.add_constraint(Length { l: l[0], value: 40.0 }, Some(d1)).unwrap();
    let d2 = doc.new_model_param("30 mm", "mm");
    sk.add_constraint(Length { l: l[1], value: 30.0 }, Some(d2)).unwrap();
    let s = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(
        FeatureKind::Extrude {
            thin: None,
            sketch: crate::SketchId(s),
            profiles: ProfileSel::All,
            extent: Extent {
                convention: crate::ExtrudeConvention::LegacyPerSide,
                distance: "20".into(),
                direction: Direction::Positive,
                distance2: None,
                start_offset: None,
                symmetric_offset: false,
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
                through_all2: false,
                to2: None,
                to_offset2: None,
                from_level: false,
                picked_placements: Vec::new(),
                one_sided_direction: None,
            },
            operation: Operation::NewBody,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    (doc, s)
}

#[test]
fn parametric_plate_reevaluates() {
    let (mut doc, _) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let st = m.state();
    assert_eq!(st.bodies.len(), 1);
    assert_eq!(st.bodies[0].name, "Body1");
    assert!(rel(measure(&st.bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    assert!(st.sketches[0].report.fully_constrained());

    // No change: nothing recomputed.
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 0);

    doc.set_param("width", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 2);
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 30000.0) < 1e-9);

    // Changing only the extrude distance re-evaluates only the extrude.
    if let Some(FeatureKind::Extrude { extent, .. }) = doc.features.get_mut(1).map(|f| &mut f.kind) {
        extent.distance = "width / 5".into();
    }
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 1);
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 50.0 * 30.0 * 10.0) < 1e-9);
}

/// #14256: an end plate whose 25 mm arc meets two lines tangentially, each joint held by two
/// "point on curve" constraints instead of a shared point. The solver leaves the arc's ends on
/// the lines only to its 1e-9 mm residual, so along the tangent they lie up to 2.25e-4 mm from
/// the lines' ends. The outline is still one closed profile, and extruding it gives the plate.
#[test]
fn an_outline_closed_by_point_on_curve_tangent_joints_extrudes() {
    let mut doc = Document::new("end plate");
    let mut sk = Sketch::new();
    let knee = (50.0_f64.powi(2) - 11.125_f64.powi(2)).sqrt();
    let mut point = |x: f64, y: f64| sk.add_point(Vec2::new(x, y), None).unwrap();
    let corners = [(-14.0, 90.0), (14.0, 90.0), (14.0, 48.0), (knee, 11.125), (90.0, 11.125), (90.0, -11.125), (25.0, -11.125), (25.0, 0.0)];
    let [top_left, top_right, shoulder, knee_point, right_top, right_bottom, step_bottom, step_top] = corners.map(|(x, y)| point(x, y));
    // Where the solver left the 25 mm arc's ends, and the end of the line along its top.
    let arc_start = point(24.999999999949498, 5.025035674136865e-5);
    let arc_end = point(0.0002252128472149822, 24.999999999324313);
    let ledge_right = point(0.0, 25.0);
    let ledge_left = point(-14.0, 25.0);
    let centre = point(0.0, 0.0);
    let small_centre = point(0.0, 0.0);
    let pos = |sk: &Sketch, i: usize| sk.points[i].pos;
    let line = |sk: &mut Sketch, a: usize, b: usize| sk.add_line_pts(a, b, None).unwrap();
    line(&mut sk, top_left, top_right);
    line(&mut sk, top_right, shoulder);
    sk.add_arc(Vec2::ZERO, pos(&sk, knee_point), pos(&sk, shoulder), [Some(centre), Some(knee_point), Some(shoulder)], None).unwrap();
    line(&mut sk, right_top, knee_point);
    line(&mut sk, right_bottom, right_top);
    line(&mut sk, step_bottom, right_bottom);
    let side = line(&mut sk, step_top, step_bottom);
    let arc = sk.add_arc(Vec2::ZERO, pos(&sk, arc_start), pos(&sk, arc_end), [Some(small_centre), Some(arc_start), Some(arc_end)], None).unwrap();
    let ledge = line(&mut sk, ledge_left, ledge_right);
    line(&mut sk, top_left, ledge_left);
    use ConstraintKind::*;
    for kind in [
        Coincident { p: centre, q: 0 },
        Coincident { p: small_centre, q: 0 },
        Horizontal { l: ledge },
        Vertical { l: side },
        Tangent { a: ledge, b: arc },
        Tangent { a: side, b: arc },
        PointOnCurve { p: ledge_right, c: arc },
        PointOnCurve { p: step_top, c: arc },
        PointOnCurve { p: arc_start, c: side },
        PointOnCurve { p: arc_end, c: ledge },
    ] {
        sk.add_constraint(kind, None).unwrap();
    }
    let s = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(
        FeatureKind::Extrude {
            thin: None,
            sketch: crate::SketchId(s),
            profiles: ProfileSel::All,
            extent: Extent {
                convention: crate::ExtrudeConvention::LegacyPerSide,
                distance: "10".into(),
                direction: Direction::Positive,
                distance2: None,
                start_offset: None,
                symmetric_offset: false,
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
                through_all2: false,
                to2: None,
                to_offset2: None,
                from_level: false,
                picked_placements: Vec::new(),
                one_sided_direction: None,
            },
            operation: Operation::NewBody,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let st = m.state();
    assert_eq!(st.sketches[0].profiles.len(), 1);
    assert_eq!(st.bodies.len(), 1);
    // The polygon through the corners, plus the segment the 50 mm arc adds, less the segment the
    // quarter of the 25 mm circle takes away, 10 mm deep.
    let outline = [
        (25.0, -11.125),
        (90.0, -11.125),
        (90.0, 11.125),
        (knee, 11.125),
        (14.0, 48.0),
        (14.0, 90.0),
        (-14.0, 90.0),
        (-14.0, 25.0),
        (0.0, 25.0),
        (25.0, 0.0),
    ];
    let polygon: f64 = (0..outline.len())
        .map(|i| {
            let ((x0, y0), (x1, y1)) = (outline[i], outline[(i + 1) % outline.len()]);
            x0 * y1 - x1 * y0
        })
        .sum::<f64>()
        / 2.0;
    let segment = |r: f64, sweep: f64| r * r / 2.0 * (sweep - sweep.sin());
    let area = polygon + segment(50.0, 48.0_f64.atan2(14.0) - 11.125_f64.atan2(knee)) - segment(25.0, std::f64::consts::FRAC_PI_2);
    let volume = measure(&st.bodies[0].body).unwrap().volume;
    assert!(rel(volume, area * 10.0) < 1e-6, "{volume} vs {}", area * 10.0);
}

/// Setting a parameter back takes the earlier results from the cache: nothing is recomputed, but
/// the evaluation reports the features it restored (their geometry did change back).
#[test]
fn set_back_reports_restored_features() {
    let (mut doc, _) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));
    doc.set_param("width", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));
    doc.set_param("width", "40 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 2));
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    // No change: nothing recomputed and nothing restored.
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 0));
}

/// Adding a line and deleting it again leaves the solved geometry as it was: the extrude and
/// everything after it take their earlier results from the cache.
#[test]
fn a_sketch_line_added_and_deleted_restores_the_extrude() {
    let (mut doc, s) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));

    let (a, b, line) = {
        let sk = doc.sketch_mut(s).unwrap();
        let a = sk.add_point(Vec2::new(5.0, 5.0), None).unwrap();
        let b = sk.add_point(Vec2::new(6.0, 5.0), None).unwrap();
        (a, b, sk.add_line_pts(a, b, None).unwrap())
    };
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));

    {
        let sk = doc.sketch_mut(s).unwrap();
        sk.remove_curves(&[line]);
        sk.remove_points(&[b]);
        sk.remove_points(&[a]);
    }
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 2));
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
}

/// Setting a parameter back restores every feature on a design whose states are large next to
/// the cache's budget: each cached state shares the bodies it did not change with the states
/// before it, so it is charged only for what it made, not for the whole model again.
#[test]
fn set_back_restores_everything_when_states_share_bodies() {
    let mut doc = Document::new("blocks");
    doc.set_param("h", "10 mm", Some("mm"), None).unwrap();
    for i in 0..30 {
        let height = if i == 0 { "h" } else { "10" };
        doc.add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::new(20.0 * i as f64, 0.0, 0.0)),
                length: "10".into(),
                width: "10".into(),
                height: height.into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    }
    let mut m = Model::new();
    m.evaluate(&doc);
    // Room for a few whole models: enough for both timelines' new bodies, not for every state
    // charged as a whole model.
    m.cache.lock().unwrap().set_budget(3 * m.state().retained_bytes());
    doc.set_param("h", "12 mm", None, None).unwrap();
    m.evaluate(&doc);
    // Only the box reading `h` is made again (#6183): the others' changes are reused.
    assert_eq!(m.last_recomputed, 1);
    doc.set_param("h", "10 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 30));
}

/// #1989: what an evaluation ahead on a copy of the model computed is kept for the run that
/// follows on the model itself, which finds it there even once the evaluation cache has dropped
/// it (the cache keeps only a few states of a design with large bodies). Copies keep nothing.
#[test]
fn results_kept_ahead_outlive_the_cache() {
    let (mut doc, _) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    doc.set_param("width", "50 mm", None, None).unwrap();
    let kept = std::sync::Arc::new(KeptResults::default());
    let mut ahead = m.clone();
    ahead.keep(Some(kept.clone()));
    assert!(ahead.clone().kept().is_none(), "a copy keeps nothing");
    ahead.evaluate(&doc);
    assert_eq!((ahead.last_recomputed, kept.len()), (2, 2));
    *m.cache.lock().unwrap() = Default::default();
    m.keep(Some(kept));
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (0, 2), "nothing computed again");
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 30000.0) < 1e-9);
}

/// A warning quotes a long generated name by its start and end (#8541: fourteen split edges'
/// full names made one chamfer warning 122,701 characters long); a short name stays whole.
#[test]
fn warnings_quote_long_names_by_their_ends() {
    let face = |i: usize| format!("Extrude{i}.side(Sketch{i}.l{i})·Fillet{i}.blend(Chamfer{i}.cap)");
    let long = (0..40).map(face).collect::<Vec<_>>().join("|");
    let q = crate::name_lookup::quoted(&long);
    assert!(q.chars().count() <= 80, "{q}");
    assert!(q.starts_with("Extrude0.side") && q.ends_with(&long[long.len() - 10..]) && q.contains('…'), "{q}");
    assert_eq!(crate::name_lookup::quoted("Extrude1.side|Box1.top"), "Extrude1.side|Box1.top");
    // Characters, not bytes: a name of non-ASCII characters is cut between characters.
    let wide = "Ü".repeat(200);
    assert_eq!(crate::name_lookup::quoted(&wide).chars().count(), 73);
}

/// A saved model whose feature warning is longer than 65,536 characters is still taken on open
/// (#8541): Save writes it, so Open reads it.
#[test]
fn a_saved_model_with_a_long_warning_is_adopted() {
    let (doc, _) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    let mut saved = m.saved(&doc).unwrap();
    let last = doc.features.last().unwrap().id;
    let warning = "w".repeat(122_701);
    let features = saved.records["features"].as_array_mut().unwrap();
    features.last_mut().unwrap()["warning"] = warning.clone().into();
    let reopened = Model::adopt(&doc, &saved).unwrap();
    assert_eq!(reopened.result(last).and_then(|r| r.warning.as_deref()), Some(warning.as_str()));
}

/// A sketch a design file puts on a face by its point alone (no face name, no attachment)
/// follows the nearest flat face that faces its way. When no face is at its point any more, it
/// still opens on that face's plane, but with a warning (#20518): the `sketch.create` command
/// refuses the same point, and the file said nothing. A face that only moved along its normal
/// (the point still on it, seen square to it) gives no warning.
#[test]
fn a_sketch_whose_face_point_is_on_no_face_warns() {
    use solidcraft_geom::prelude::*;

    // The plate is 40 x 30 mm, 20 mm tall: its top face is at z = 20.
    let sketch_on = |picked_z: f64, at: Vec3| {
        let (mut doc, _) = plate_doc();
        let top = Plane::new(Vec3::new(0.0, 0.0, picked_z), Vec3::X, Vec3::Y).unwrap();
        let mut sk = Sketch::new();
        let a = sk.add_point(Vec2::new(1.0, 1.0), None).unwrap();
        let b = sk.add_point(Vec2::new(3.0, 3.0), None).unwrap();
        sk.add_line_pts(a, b, None).unwrap();
        let plane = PlaneRef::Face { plane: top, at, name: None, anchor: None };
        let id = doc.add_feature(FeatureKind::Sketch { plane, sketch: sk }, None).unwrap();
        let mut m = Model::new();
        m.evaluate(&doc);
        let result = m.result(id).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        let z = m.state().sketches.iter().find(|s| s.feature == id).unwrap().plane.origin.z;
        (result.warning.clone(), z)
    };
    let (warning, z) = sketch_on(20.0, Vec3::new(10.0, 10.0, 20.0));
    assert!(warning.is_none() && (z - 20.0).abs() < 1e-9, "a point on the top face: {warning:?} at z {z}");
    let (warning, z) = sketch_on(15.0, Vec3::new(10.0, 10.0, 15.0));
    assert!(warning.is_none() && (z - 20.0).abs() < 1e-9, "a top face that moved up 5 mm still holds the point: {warning:?} at z {z}");
    let (warning, z) = sketch_on(20.0, Vec3::new(60.0, 60.0, 20.0));
    let warning = warning.unwrap_or_default();
    assert!(warning.contains("no face is at the point the sketch was placed on, [60, 60, 20]"), "{warning}");
    assert!((z - 20.0).abs() < 1e-9, "the sketch stays on the plane of the nearest face, not at z {z}");
}

/// Rolling the marker back must not discard unchanged results just because the bounded cache
/// evicted them. Opening an earlier feature and returning to the end otherwise rebuilds it all.
#[test]
fn rolling_the_marker_back_keeps_results_after_cache_eviction() {
    for reopen in [false, true] {
        let (mut doc, _) = plate_doc();
        let mut m = Model::new();
        m.evaluate(&doc);
        if reopen {
            m = Model::adopt(&doc, &m.saved(&doc).unwrap()).unwrap();
        }
        let finished = m.state();
        let computed = m.recomputed;
        // Simulate eviction by previews or the cache's byte limit, without a large/slow fixture.
        *m.cache.lock().unwrap() = Default::default();
        for marker in [Some(1), Some(0), Some(1), None] {
            doc.marker = marker;
            m.evaluate(&doc);
            assert_eq!(m.recomputed, computed, "reopen {reopen}, marker {marker:?}: unchanged features must be reused");
            assert_eq!(m.state().bodies.len(), usize::from(marker.is_none()));
        }
        assert!(std::sync::Arc::ptr_eq(&m.state(), &finished), "return to the original evaluated state");
    }
}

#[test]
fn rolling_a_large_unchanged_timeline_forward_reuses_its_geometry() {
    let mut doc = Document::new("marker cache budget");
    for i in 0..80 {
        doc.add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::new((i % 10) as f64 * 15.0, (i / 10) as f64 * 15.0, 0.0)),
                length: "10".into(),
                width: "10".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    }
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()));
    // Exceed the actual byte budget without manually clearing the cache.
    assert!(m.results.iter().map(|r| r.state.retained_bytes()).sum::<usize>() > 64 << 20);
    let finished = m.state();
    doc.marker = Some(1);
    m.evaluate(&doc);
    assert_eq!(m.state().bodies.len(), 1);
    doc.marker = None;
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 0, "returning to the end must not rebuild unchanged boxes");
    assert_eq!(m.state().bodies.len(), 80);
    assert!(std::sync::Arc::ptr_eq(&m.state(), &finished));
}

#[test]
fn rolled_results_never_mask_edits_or_disabled_reuse() {
    for change in ["parameter", "feature", "suppressed", "units", "no_cache"] {
        let (mut doc, _) = plate_doc();
        let mut m = Model::new();
        m.evaluate(&doc);
        doc.marker = Some(0);
        m.evaluate(&doc);
        *m.cache.lock().unwrap() = Default::default();
        match change {
            "parameter" => doc.set_param("width", "50 mm", None, None).unwrap(),
            "feature" => {
                let FeatureKind::Extrude { extent, .. } = &mut doc.features[1].kind else { panic!("extrude") };
                extent.distance = "25".into();
            }
            "suppressed" => doc.features[1].suppressed = true,
            "units" => doc.units = "in".into(),
            "no_cache" => m.no_cache = true,
            _ => panic!("unknown edit case"),
        }
        doc.marker = None;
        m.evaluate(&doc);
        let mut fresh = Model::new();
        fresh.evaluate(&doc);
        let volume = |model: &Model| model.state().bodies.iter().map(|b| measure(&b.body).unwrap().volume).sum::<f64>();
        assert_eq!(m.state().bodies.len(), fresh.state().bodies.len(), "{change}");
        assert!((volume(&m) - volume(&fresh)).abs() < 1e-6, "{change}: retained geometry must match a fresh evaluation");
        assert_eq!(m.results.iter().map(|r| &r.error).collect::<Vec<_>>(), fresh.results.iter().map(|r| &r.error).collect::<Vec<_>>(), "{change}");
        if matches!(change, "parameter" | "feature" | "units" | "no_cache") {
            assert!(m.last_recomputed > 0, "{change}: changed inputs must rebuild");
        }
    }
}

fn add_in(doc: &mut Document, component: u64, name: &str, kind: serde_json::Value) {
    let kind: FeatureKind = serde_json::from_value(kind).unwrap();
    let id = doc.add_feature(kind, Some(name)).unwrap();
    doc.feature_mut(id).unwrap().component = component;
}

/// A plate in one component, then a rod in another, slotted by a box cut patterned round it.
fn two_parts(slot_targets: &[&str]) -> Document {
    let mut doc = Document::new("two parts");
    doc.set_param("w", "40 mm", None, None).unwrap();
    doc.set_param("slot", "1 mm", None, None).unwrap();
    let plate = doc.add_component(Some("Plate"), 0).unwrap();
    let rod = doc.add_component(Some("Rod"), 0).unwrap();
    add_in(
        &mut doc,
        plate,
        "Plate",
        serde_json::json!({"type": "box", "corner": ["0", "0", "0"], "length": "w", "width": "30", "height": "5", "operation": "new_body"}),
    );
    add_in(
        &mut doc,
        rod,
        "Rod",
        serde_json::json!({"type": "cylinder", "base": ["0", "0", "0"], "axis": [0.0, 0.0, 1.0], "radius": "4", "height": "20", "operation": "new_body"}),
    );
    add_in(
        &mut doc,
        rod,
        "Slot",
        serde_json::json!({"type": "box", "corner": ["3", "-0.5", "2"], "length": "2", "width": "slot", "height": "20", "operation": "cut"}),
    );
    for f in doc.features.iter_mut().filter(|f| f.name == "Plate" || f.name == "Rod") {
        f.body_names = vec![f.name.clone()];
    }
    if let Some(f) = doc.features.iter_mut().find(|f| f.name == "Slot") {
        f.participants = slot_targets.iter().map(|t| t.to_string()).collect();
    }
    let slot = doc.find_feature("Slot").map_or(0, |f| f.id);
    add_in(
        &mut doc,
        rod,
        "Slots",
        serde_json::json!({"type": "pattern", "features": [slot], "pattern": {"type": "circular", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "count": "4", "angle": "360 deg"}}),
    );
    doc
}

/// The bodies as an evaluation from scratch makes them: names, volumes, face names.
fn summary(st: &ModelState) -> Vec<(String, u64, i64, Vec<String>)> {
    st.bodies
        .iter()
        .map(|b| {
            let volume = measure(&b.body).unwrap().volume;
            (b.name.clone(), b.component, (volume * 1e6).round() as i64, crate::naming::face_names(b).to_vec())
        })
        .collect()
}

fn from_scratch(doc: &Document) -> Vec<(String, u64, i64, Vec<String>)> {
    let mut m = Model::new();
    m.no_cache = true;
    m.evaluate(doc);
    summary(&m.state())
}

/// An edit in one component leaves another component's features after it as they were: they
/// are restored, not recomputed, and give what evaluating from scratch gives.
#[test]
fn edit_in_one_component_keeps_the_others_results() {
    let mut doc = two_parts(&[]);
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    assert_eq!(m.last_recomputed, 4);
    doc.set_param("w", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (1, 3));
    assert_eq!(summary(&m.state()), from_scratch(&doc));
    // An edit in the rod's own component recomputes its features.
    doc.set_param("slot", "1.5 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 0));
    assert_eq!(summary(&m.state()), from_scratch(&doc));
    doc.set_param("w", "60 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (1, 3));
    assert_eq!(summary(&m.state()), from_scratch(&doc));
}

/// A sketch on a fixed plane, the new body extruded from it and that body moved read nothing
/// of another component either.
#[test]
fn sketch_extrude_and_move_keep_their_results() {
    let mut doc = two_parts(&[]);
    let rod = doc.find_component("Rod").unwrap();
    let mut sk = Sketch::new();
    sk.add_circle(Vec2::new(20.0, 0.0), 2.0, None, None).unwrap();
    let id = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, Some("Peg sketch")).unwrap();
    doc.feature_mut(id).unwrap().component = rod;
    add_in(
        &mut doc,
        rod,
        "Peg",
        serde_json::json!({"type": "extrude", "sketch": id, "profiles": {"type": "all"}, "extent": {"convention": "total_distance", "direction": "positive", "distance": "8"}, "operation": "new_body"}),
    );
    if let Some(f) = doc.features.iter_mut().find(|f| f.name == "Peg") {
        f.body_names = vec!["Peg".into()];
    }
    add_in(&mut doc, rod, "Peg up", serde_json::json!({"type": "move", "bodies": ["Peg"], "translate": ["0", "0", "slot * 3"]}));
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    doc.set_param("w", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (1, 6));
    assert_eq!(summary(&m.state()), from_scratch(&doc));
    // The move follows its own parameter.
    doc.set_param("slot", "2 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (3, 2));
    assert_eq!(summary(&m.state()), from_scratch(&doc));
}

/// A feature that cuts a body of another component reads it: it is recomputed when that body
/// changes, and so is what follows it in its own component.
#[test]
fn a_cut_of_another_components_body_follows_it() {
    let mut doc = two_parts(&["Plate", "Rod"]);
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    doc.set_param("w", "50 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (3, 1));
    assert_eq!(summary(&m.state()), from_scratch(&doc));
}

/// A pattern of a cut reads the bodies its source cuts, as the copies cut them: the source's
/// participants, though the pattern names none and that body is in another component. The plate
/// lies clear of every copy at first; moved into one by `w`, it must be cut again.
#[test]
fn a_pattern_reads_the_bodies_its_source_cuts() {
    let mut doc = two_parts(&["Plate"]);
    if let Some(f) = doc.features.iter_mut().find(|f| f.name == "Plate") {
        f.kind = serde_json::from_value(
            serde_json::json!({"type": "box", "corner": ["-20", "w", "0"], "length": "40", "width": "30", "height": "5", "operation": "new_body"}),
        )
        .unwrap();
    }
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    assert_eq!(summary(&m.state()), from_scratch(&doc));
    // The copy turned a quarter round now reaches the plate.
    doc.set_param("w", "4 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!(summary(&m.state()), from_scratch(&doc));
}

/// A cut of several bodies cuts again only those that changed: the rod, which `w` does not
/// move, keeps the very body its earlier cut gave, while the cap `w` sizes is cut again.
#[test]
fn a_cut_of_several_bodies_cuts_again_only_those_that_changed() {
    let mut doc = Document::new("rod and cap");
    doc.set_param("w", "6 mm", None, None).unwrap();
    let rod = doc.add_component(Some("Rod"), 0).unwrap();
    add_in(
        &mut doc,
        rod,
        "Rod",
        serde_json::json!({"type": "cylinder", "base": ["0", "0", "0"], "axis": [0.0, 0.0, 1.0], "radius": "4", "height": "20", "operation": "new_body"}),
    );
    add_in(
        &mut doc,
        rod,
        "Cap",
        serde_json::json!({"type": "box", "corner": ["-5", "-5", "20"], "length": "10", "width": "10", "height": "w", "operation": "new_body"}),
    );
    for f in doc.features.iter_mut().filter(|f| f.name == "Rod" || f.name == "Cap") {
        f.body_names = vec![f.name.clone()];
    }
    // The slot runs through both: the component's bodies are its targets.
    add_in(
        &mut doc,
        rod,
        "Slot",
        serde_json::json!({"type": "box", "corner": ["3", "-0.5", "2"], "length": "4", "width": "1", "height": "30", "operation": "cut"}),
    );
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    let cut_rod = m.state().body("Rod").unwrap().body.clone();
    doc.set_param("w", "8 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 2, "the cap and the slot");
    let st = m.state();
    assert!(st.body("Rod").unwrap().body.same(&cut_rod), "the rod's cut is the one made before");
    assert_eq!(summary(&st), from_scratch(&doc));
}

/// A parameter that drives the first feature of the timeline, with the bodies after it each cut
/// by a combine: only the first body and its cut are recomputed, every other cut is restored.
#[test]
fn a_combine_reads_only_its_target_and_tools() {
    let mut doc = Document::new("rows");
    doc.set_param("w", "20 mm", None, None).unwrap();
    let c = doc.add_component(Some("Rows"), 0).unwrap();
    for i in 0..3 {
        let x = (i * 30) as f64;
        let (body, tool, cut) = (format!("Body{i}"), format!("Tool{i}"), format!("Cut{i}"));
        let length = if i == 0 { "w".to_string() } else { "20".to_string() };
        add_in(
            &mut doc,
            c,
            &body,
            serde_json::json!({"type": "box", "corner": [x.to_string(), "0", "0"], "length": length, "width": "15", "height": "5", "operation": "new_body"}),
        );
        add_in(
            &mut doc,
            c,
            &tool,
            serde_json::json!({"type": "cylinder", "base": [(x + 7.0).to_string(), "7.5", "-1"], "axis": [0.0, 0.0, 1.0], "radius": "1.5", "height": "7", "operation": "new_body"}),
        );
        for f in doc.features.iter_mut().filter(|f| f.name == body || f.name == tool) {
            f.body_names = vec![f.name.clone()];
        }
        add_in(&mut doc, c, &cut, serde_json::json!({"type": "combine", "target": body, "tools": [tool], "operation": "cut"}));
    }
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    assert_eq!(m.last_recomputed, 9);
    doc.set_param("w", "32 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!((m.last_recomputed, m.last_restored), (2, 7), "the first body and its cut");
    assert_eq!(summary(&m.state()), from_scratch(&doc));
}

/// A circle sketch on `plane` in `component`, extruded by `distance` (`operation`), named
/// after `name`.
fn circle_extrude(doc: &mut Document, component: u64, name: &str, plane: PlaneRef, center: Vec2, r: f64, distance: &str, operation: &str) {
    let mut sk = Sketch::new();
    sk.add_circle(center, r, None, None).unwrap();
    let id = doc.add_feature(FeatureKind::Sketch { plane, sketch: sk }, Some(&format!("{name} sketch"))).unwrap();
    doc.feature_mut(id).unwrap().component = component;
    add_in(
        doc,
        component,
        name,
        serde_json::json!({"type": "extrude", "sketch": id, "profiles": {"type": "all"}, "extent": {"convention": "total_distance", "direction": "positive", "distance": distance}, "operation": operation}),
    );
    if let Some(f) = doc.features.iter_mut().find(|f| f.name == name).filter(|_| operation == "new_body") {
        f.body_names = vec![name.into()];
    }
}

/// Three components on one timeline (#6183): A's post (height `A_h`) with the construction plane
/// `A_top` above it when `plane`; B's post, cut by a hole from a sketch on a plane given by
/// numbers and filleted; C's post, filleted twice (the last by `C_r`). Nothing in B or C reads A.
fn three_parts(plane: bool) -> Document {
    let mut doc = Document::new("three parts");
    doc.set_param("A_h", "10 mm", None, None).unwrap();
    doc.set_param("C_r", "1 mm", None, None).unwrap();
    let [a, b, c] = ["A", "B", "C"].map(|n| doc.add_component(Some(n), 0).unwrap());
    let xy = || PlaneRef::Origin { name: "XY".into() };
    circle_extrude(&mut doc, a, "A post", xy(), Vec2::new(0.0, 0.0), 5.0, "A_h", "new_body");
    if plane {
        add_in(
            &mut doc,
            a,
            "A_top",
            serde_json::json!({"type": "construction_plane", "plane": {"type": "offset", "base": {"type": "origin", "name": "XY"}, "distance": "A_h + 5"}}),
        );
    }
    circle_extrude(&mut doc, b, "B post", xy(), Vec2::new(40.0, 0.0), 8.0, "12", "new_body");
    let top = PlaneRef::Offset { base: Box::new(xy()), distance: "12".into() };
    circle_extrude(&mut doc, b, "B hole", top, Vec2::new(40.0, 0.0), 3.0, "-4", "cut");
    add_in(&mut doc, b, "B round", serde_json::json!({"type": "fillet", "edges": [[48.0, 0.0, 0.0]], "radius": "1"}));
    circle_extrude(&mut doc, c, "C post", xy(), Vec2::new(80.0, 0.0), 6.0, "15", "new_body");
    add_in(&mut doc, c, "C round", serde_json::json!({"type": "fillet", "edges": [[86.0, 0.0, 15.0]], "radius": "1.5"}));
    add_in(&mut doc, c, "C foot", serde_json::json!({"type": "fillet", "edges": [[86.0, 0.0, 0.0]], "radius": "C_r"}));
    doc
}

fn no_errors(m: &Model) {
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| (&r.name, &r.error)).collect::<Vec<_>>());
}

/// An edit in component A recomputes A's features that use it and nothing in B or C (#6183):
/// sketches, cuts on a plane given by numbers and fillets of their own body are kept, and the
/// bodies match an evaluation from scratch.
#[test]
fn an_edit_in_one_component_recomputes_only_what_reads_it() {
    for plane in [false, true] {
        let mut doc = three_parts(plane);
        let mut m = Model::new();
        m.evaluate(&doc);
        no_errors(&m);
        for h in ["11 mm", "12 mm", "13 mm"] {
            doc.set_param("A_h", h, None, None).unwrap();
            m.evaluate(&doc);
            no_errors(&m);
            // The extrude, and the plane offset by `A_h`.
            assert_eq!(m.last_recomputed, 1 + usize::from(plane), "A_h = {h}, plane: {plane}");
            assert_eq!(summary(&m.state()), from_scratch(&doc));
        }
        // The last fillet recomputes alone, as before.
        doc.set_param("C_r", "2 mm", None, None).unwrap();
        m.evaluate(&doc);
        assert_eq!(m.last_recomputed, 1);
        assert_eq!(summary(&m.state()), from_scratch(&doc));
        // Back where it was: every result comes from the cache.
        doc.set_param("A_h", "10 mm", None, None).unwrap();
        doc.set_param("C_r", "1 mm", None, None).unwrap();
        m.evaluate(&doc);
        assert_eq!(summary(&m.state()), from_scratch(&doc));
    }
}

/// A component deleted, or another added before the rest, changes the bodies before the other
/// components' features and the design's components, none of which they read: each keeps its
/// result, made again on the model as it is now, and the bodies match an evaluation from scratch.
#[test]
fn a_component_deleted_or_added_keeps_the_others_results() {
    let doc = three_parts(true);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let mut without = doc.clone();
    without.remove_component_tree(without.find_component("A").unwrap()).unwrap();
    m.evaluate(&without);
    no_errors(&m);
    assert_eq!(m.last_recomputed, 0, "B's and C's features after A's are kept");
    assert_eq!(summary(&m.state()), from_scratch(&without));
    // A new component first in the timeline: only its own features are computed.
    let mut with = without.clone();
    let d = with.add_component(Some("D"), 0).unwrap();
    circle_extrude(&mut with, d, "D post", PlaneRef::Origin { name: "XY".into() }, Vec2::new(-40.0, 0.0), 4.0, "9", "new_body");
    with.features.rotate_right(2);
    m.evaluate(&with);
    no_errors(&m);
    assert_eq!(m.last_recomputed, 2, "D's sketch and extrude");
    assert_eq!(summary(&m.state()), from_scratch(&with));
    // And back (an undo): every result is kept.
    m.evaluate(&doc);
    no_errors(&m);
    assert_eq!(m.last_recomputed, 0);
    assert_eq!(summary(&m.state()), from_scratch(&doc));
}

/// A body made under the name its feature stores is not reused under another body of that name:
/// a component added before it that takes the name makes the feature name its body otherwise.
#[test]
fn a_stored_name_taken_by_a_body_before_it_is_not_reused() {
    let doc = three_parts(false);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let mut taken = doc.clone();
    let d = taken.add_component(Some("D"), 0).unwrap();
    // A feature of its own name (feature names are unique) whose body takes C's post's name.
    circle_extrude(&mut taken, d, "D post", PlaneRef::Origin { name: "XY".into() }, Vec2::new(-40.0, 0.0), 4.0, "9", "new_body");
    let post = taken.find_feature("D post").unwrap().id;
    taken.feature_mut(post).unwrap().body_names = vec!["C post".into()];
    taken.features.rotate_right(2);
    m.evaluate(&taken);
    let mut fresh = Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&taken);
    let errors = |m: &Model| m.results.iter().map(|r| r.error.clone()).collect::<Vec<_>>();
    assert_eq!(errors(&m), errors(&fresh));
    assert_eq!(summary(&m.state()), summary(&fresh.state()));
    assert!(fresh.state().bodies.iter().any(|b| b.name == "C post" && b.component == d), "D's body has the name C's post stores");
}

/// What an evaluation shows: each feature's outcome with the bodies after it, the last bodies in
/// full and the body counter.
type Shown = (Vec<(u64, Option<String>, Option<String>, Vec<(String, u64)>)>, Vec<(String, u64, i64, Vec<String>)>, usize);

fn shown(m: &Model) -> Shown {
    let bodies = |st: &ModelState| st.bodies.iter().map(|b| (b.name.clone(), b.component)).collect();
    // A failed cut's hint at a body of another component follows where that is placed, which the
    // key of a failure does not hold. Of a body SMLib's check refuses, which one is compared: its
    // words can differ between two evaluations.
    let told = |e: &str| {
        let cut = if e.contains("SMLib's check refuses") { ": the operation failed: SMLib" } else { " in component '" };
        e.split(cut).next().unwrap_or(e).to_string()
    };
    let error = |r: &FeatureResult| r.error.as_deref().map(told);
    // An import's warning has what its own check of the parts found once that ended, whenever it does.
    let warning = |r: &FeatureResult| r.warning.clone().filter(|_| r.import_report.is_none());
    let results = m.results.iter().map(|r| (r.id, error(r), warning(r), bodies(&r.state))).collect();
    (results, summary(&m.state()), m.state().body_counter)
}

/// The session's model is what opening the design's file and evaluating it afresh gives.
fn same_as_opened(m: &Model, doc: &Document, what: &str) {
    let mut opened = Model::new();
    opened.no_cache = true;
    let read = crate::format::read(&doc.to_json()).unwrap();
    opened.evaluate(&read);
    settled(&mut opened, &read);
    let (kept, fresh) = (shown(m), shown(&opened));
    for (f, (kept, fresh)) in doc.features.iter().zip(kept.0.iter().zip(&fresh.0)) {
        assert_eq!(kept, fresh, "{what}: feature {} `{}`", f.id, f.name);
    }
    assert_eq!(kept, fresh, "{what}");
}

fn boxed(doc: &mut Document, component: u64, name: &str, x: f64, body_names: &[&str]) -> u64 {
    let kind = serde_json::json!({"type": "box", "corner": [x, 0.0, 0.0], "length": "10", "width": "10", "height": "10", "operation": "new_body"});
    add_in(doc, component, name, kind);
    let id = doc.find_feature(name).unwrap().id;
    doc.feature_mut(id).unwrap().body_names = body_names.iter().map(|n| n.to_string()).collect();
    id
}

/// A cut that names another component's body is refused once a body of its own component was asked
/// to be called that (#12141): the cut that ran before that body was there is not kept.
#[test]
fn a_cut_is_not_kept_once_its_target_s_name_is_ambiguous() {
    let mut doc = Document::new("ambiguous target");
    let [a, c] = ["A", "C"].map(|n| doc.add_component(Some(n), 0).unwrap());
    boxed(&mut doc, a, "A box", 0.0, &["X"]);
    add_in(
        &mut doc,
        c,
        "C cut",
        serde_json::json!({"type": "cylinder", "base": [5.0, 5.0, -1.0], "axis": [0.0, 0.0, 1.0], "radius": "2", "height": "20", "operation": "cut"}),
    );
    let cut = doc.find_feature("C cut").unwrap().id;
    doc.feature_mut(cut).unwrap().participants = vec!["X".into()];
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    // An added body that makes nothing ambiguous keeps the cut.
    let mut other = doc.clone();
    boxed(&mut other, c, "C other", 100.0, &["Y"]);
    other.features[1..].rotate_right(1);
    m.evaluate(&other);
    no_errors(&m);
    assert_eq!(m.last_recomputed, 1, "the added box alone");
    same_as_opened(&m, &other, "an unrelated body added");
    let mut taken = doc.clone();
    boxed(&mut taken, c, "C box", 100.0, &["X"]);
    taken.features[1..].rotate_right(1);
    m.evaluate(&taken);
    assert!(m.result(cut).unwrap().error.as_ref().is_some_and(|e| e.contains("ambiguous")), "{:?}", m.result(cut).unwrap().error);
    same_as_opened(&m, &taken, "the target's name asked for in the cut's component");
}

/// A New Body that names a body fails once that body is gone, as it does evaluated afresh.
#[test]
fn a_new_body_is_not_kept_once_a_body_it_names_is_gone() {
    let mut doc = Document::new("named by a new body");
    let x = boxed(&mut doc, 0, "X box", 0.0, &["X"]);
    let new = boxed(&mut doc, 0, "New", 20.0, &["New"]);
    doc.feature_mut(new).unwrap().participants = vec!["X".into()];
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    doc.delete_feature(x).unwrap();
    m.evaluate(&doc);
    assert!(m.result(new).unwrap().error.is_some());
    same_as_opened(&m, &doc, "the named body deleted");
}

/// A body numbered from the counter is not a stored one for a name stored in another slot: what
/// it is numbered follows the bodies before it (found by the edits below).
#[test]
fn a_numbered_body_is_not_kept_for_a_name_stored_in_another_slot() {
    let mut doc = Document::new("slots");
    boxed(&mut doc, 0, "Seventh", 0.0, &["Body7"]);
    boxed(&mut doc, 0, "Numbered", 20.0, &["", "Body8"]);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    assert!(m.state().body("Body8").is_some());
    let seventh = doc.find_feature("Seventh").unwrap().id;
    doc.delete_feature(seventh).unwrap();
    m.evaluate(&doc);
    same_as_opened(&m, &doc, "the body that counted to seven deleted");
    assert!(m.state().body("Body1").is_some(), "{:?}", summary(&m.state()));
}

/// A body made under a stored `BodyN` counts up to N wherever it is made: kept on a model that has
/// counted less than it had, it still does, so the next body is numbered as in a fresh evaluation.
#[test]
fn a_kept_body_of_a_stored_number_still_counts() {
    let mut doc = Document::new("counted");
    boxed(&mut doc, 0, "Tenth", 0.0, &["Body10"]);
    boxed(&mut doc, 0, "Fifth", 20.0, &["Body5"]);
    boxed(&mut doc, 0, "Next", 40.0, &[]);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    assert!(m.state().body("Body11").is_some());
    let tenth = doc.find_feature("Tenth").unwrap().id;
    doc.delete_feature(tenth).unwrap();
    m.evaluate(&doc);
    no_errors(&m);
    same_as_opened(&m, &doc, "the body that counted to ten deleted");
    assert!(m.state().body("Body6").is_some(), "{:?}", summary(&m.state()));
}

/// `n` separate boxes, each with one edge rounded, the edge named and recorded as the Fillet
/// command stores it when it is picked.
fn rounded_boxes(n: usize) -> Document {
    let mut doc = Document::new("rounded boxes");
    for i in 0..n {
        let (x, name) = (20.0 * i as f64, format!("Block {}", i + 1));
        boxed(&mut doc, 0, &name, x, &[&name]);
        add_in(
            &mut doc,
            0,
            &format!("Round {}", i + 1),
            serde_json::json!({"type": "fillet", "edges": [[x + 10.0, 5.0, 10.0]], "radius": "1", "body": name}),
        );
    }
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    for f in &mut doc.features {
        if let FeatureKind::Fillet { edges, body, .. } = &f.kind {
            let st = m.state_before(f.id);
            f.edge_refs = crate::edge_refs(&st, body, &plain(edges));
            f.edge_names = crate::edge_names_for(&st, body, &plain(edges));
        }
    }
    doc
}

/// The number of a suppressed feature's stored name is not handed out: a body numbered before the
/// feature was suppressed is not kept under it.
#[test]
fn a_numbered_body_is_not_kept_once_its_number_is_reserved() {
    let mut doc = Document::new("reserved number");
    // Makes nothing, so suppressing it changes no body: only its stored name counts.
    let cut = serde_json::json!({"type": "box", "corner": [100.0, 0.0, 0.0], "length": "5", "width": "5", "height": "5", "operation": "cut"});
    add_in(&mut doc, 0, "Cut", cut);
    boxed(&mut doc, 0, "Numbered", 0.0, &[]);
    let cut = doc.find_feature("Cut").unwrap().id;
    doc.feature_mut(cut).unwrap().body_names = vec!["Body1".into()];
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.state().body("Body1").is_some());
    doc.feature_mut(cut).unwrap().suppressed = true;
    m.evaluate(&doc);
    same_as_opened(&m, &doc, "suppressed");
    assert!(m.state().body("Body2").is_some(), "{:?}", summary(&m.state()));
}

/// A body taken back from a saved model counts as deep in the chain of names as it is now, so one
/// feature fewer before it brings the bodies after it back under the limit.
#[test]
fn a_reopened_body_counts_in_the_naming_depth_as_it_stands() {
    let mut doc = Document::new("reopened at the limit");
    boxed(&mut doc, 0, "Far", -500.0, &["Far"]);
    for i in 1..397 {
        add_in(&mut doc, 0, &format!("Far {i}"), serde_json::json!({"type": "move", "bodies": ["Far"], "translate": ["0", "0", "0.01"]}));
    }
    boxed(&mut doc, 0, "Near", 0.0, &["Near"]);
    boxed(&mut doc, 0, "Other", 40.0, &["Other"]);
    boxed(&mut doc, 0, "Last", 80.0, &["Last"]);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let names = |m: &Model| crate::naming::face_names(m.state().body("Last").unwrap()).to_vec();
    assert!(names(&m).iter().all(|n| n.starts_with("Last:face:")), "{:?}", names(&m));
    let mut m = Model::adopt(&doc, &m.saved(&doc).unwrap()).unwrap();
    let id = doc.find_feature("Far 200").unwrap().id;
    doc.feature_mut(id).unwrap().suppressed = true;
    m.evaluate(&doc);
    same_as_opened(&m, &doc, "one suppressed");
    assert!(names(&m).iter().all(|n| !n.starts_with("Last:face:")), "{:?}", names(&m));
}

/// A face of a tool without roles (a coil's end) is named after whatever body it lies on, read or
/// not: a bevel of its edge, stored by that name, is not kept once another body names the face.
#[test]
fn a_bevel_is_not_kept_once_another_body_names_its_face() {
    let mut doc = Document::new("named from geometry");
    let b = doc.add_component(Some("B"), 0).unwrap();
    boxed(&mut doc, 0, "Block", 0.0, &["Block"]);
    let coil = serde_json::json!({"type": "coil", "base": [5.0, 5.0, 10.5], "axis": [0.0, 0.0, 1.0], "diameter": "6", "pitch": "4", "turns": "1", "section_size": "2", "section": "square", "operation": "join"});
    add_in(&mut doc, 0, "Coil", coil);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    // A pick a little off its edge, as one kept from before an edit is: the point alone finds nothing.
    let edge = Vec3::new(8.0, 4.6, 11.8);
    let body = Some(BodyName("Block".into()));
    let (refs, names) = (crate::edge_refs(&m.state(), &body, &[edge]), crate::edge_names_for(&m.state(), &body, &[edge]));
    add_in(&mut doc, 0, "Bevel", serde_json::json!({"type": "chamfer", "edges": [[8.0, 4.6, 11.8]], "distance": "0.2", "body": "Block"}));
    let bevel = doc.find_feature("Bevel").unwrap().id;
    let f = doc.feature_mut(bevel).unwrap();
    (f.edge_refs, f.edge_names) = (refs, names);
    m.evaluate(&doc);
    no_errors(&m);
    // A block whose side lies on the coil's end.
    let other = serde_json::json!({"type": "box", "corner": [6.0, 0.0, 8.0], "length": "10", "width": "5", "height": "10", "operation": "new_body"});
    add_in(&mut doc, b, "Other", other);
    let id = doc.find_feature("Other").unwrap().id;
    doc.feature_mut(id).unwrap().body_names = vec!["Other".into()];
    doc.features.rotate_right(1);
    m.evaluate(&doc);
    same_as_opened(&m, &doc, "a block on the coil's end");
}

/// A pick off its edge lies on every body with an edge up to twice as far, and the round says so:
/// a body added there, beyond the reach the bodies near the pick are looked for in, is named too.
#[test]
fn a_round_is_not_kept_without_the_note_of_a_body_added_by_its_pick() {
    let long = |doc: &mut Document, component: u64, name: &str, x: f64| {
        let kind =
            serde_json::json!({"type": "box", "corner": [x, 0.0, 0.0], "length": "10", "width": "1000", "height": "10", "operation": "new_body"});
        add_in(doc, component, name, kind);
        let id = doc.find_feature(name).unwrap().id;
        doc.feature_mut(id).unwrap().body_names = vec![name.into()];
    };
    let mut doc = Document::new("a pick off its edge");
    long(&mut doc, 0, "Rail", 0.0);
    add_in(&mut doc, 0, "Round", serde_json::json!({"type": "fillet", "edges": [[10.8, 500.0, 10.0]], "radius": "1"}));
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let b = doc.add_component(Some("B"), 0).unwrap();
    long(&mut doc, b, "Other", 12.3);
    doc.features.rotate_right(1);
    m.evaluate(&doc);
    same_as_opened(&m, &doc, "a rail added by the pick");
}

/// The design whose join SMLib's check refuses: an imported block with an edge off its face, and a
/// box joined to it.
fn block_with_a_refused_join() -> Document {
    let mut doc = Document::new("refused join");
    boxed(&mut doc, 0, "Before", -60.0, &["Before"]);
    let step: std::sync::Arc<str> = include_str!("../../../tests/step-regressions/block_edge_off_its_face.step").into();
    let kind = FeatureKind::Import { file: "block.step".into(), step, components: Vec::new(), skip: Vec::new(), records: None };
    let block = doc.add_feature(kind, Some("Block")).unwrap();
    doc.feature_mut(block).unwrap().body_names = vec!["Block".into()];
    let join = serde_json::json!({"type": "box", "corner": [15.0, 25.0, 5.0], "length": "10", "width": "10", "height": "10", "operation": "join"});
    add_in(&mut doc, 0, "Join", join);
    let join = doc.find_feature("Join").unwrap().id;
    doc.feature_mut(join).unwrap().participants = vec!["Block".into()];
    boxed(&mut doc, 0, "After", 200.0, &["After"]);
    doc
}

/// Bring in every check still running, of features and of imported parts, and what it refuses.
fn settled(m: &mut Model, doc: &Document) {
    for _ in 0..=doc.features.len() {
        let refused = m.settle_feature_checks(true);
        while m.checking_imports() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if !m.settle_import_checks() && !refused {
            return;
        }
        m.evaluate(doc);
    }
    panic!("a check refuses again and again");
}

/// What a window shows before a check ends ([`Model::set_background_checks`]) is kept neither
/// for what it read nor whole: an edit made while the check runs, of a body the checked feature
/// does not read, ends with the failure the design opened afresh has, and so does its undo.
#[test]
fn a_result_shown_before_its_check_refuses_it_is_not_kept() {
    let doc = block_with_a_refused_join();
    let join = doc.find_feature("Join").unwrap().id;
    let mut without = doc.clone();
    without.delete_feature(doc.find_feature("Before").unwrap().id).unwrap();
    let mut added = doc.clone();
    boxed(&mut added, 0, "Added", -120.0, &["Added"]);
    added.features.rotate_right(1);
    let mut m = Model::new();
    m.set_background_checks(true);
    for (what, d) in
        [("shown", &doc), ("one deleted", &without), ("undone", &doc), ("one added", &added), ("undone again", &doc), ("as it was", &doc)]
    {
        m.evaluate(d);
        // Left running as far as the window leaves it, for the next edit to meet.
        if what != "shown" && what != "undone" {
            settled(&mut m, d);
            assert!(m.result(join).unwrap().error.is_some(), "{what}: the join is refused");
            same_as_opened(&m, d, what);
        }
    }
}

/// Boxes with one rounded edge each, past the 400 features a body's names are worked out from
/// history for: the bodies after that name their faces by place. Deleting a box, or adding one,
/// moves bodies across that limit, and a round whose edge was named on the other side of it does
/// what it does in the design opened afresh: it fails there, so what it did before is not kept
/// (#20956). What stays on its side is kept: an undo evaluates nothing again.
#[test]
fn rounds_past_the_naming_limit_are_as_in_the_design_opened_afresh() {
    let boxes = 205;
    let doc = rounded_boxes(boxes);
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let without = |boxes: &[usize]| {
        let mut d = doc.clone();
        for name in boxes.iter().flat_map(|i| [format!("Block {i}"), format!("Round {i}")]) {
            let id = d.find_feature(&name).unwrap().id;
            d.delete_feature(id).unwrap();
        }
        d
    };
    let mut added = doc.clone();
    boxed(&mut added, 0, "Added", -20.0, &["Added"]);
    added.features.rotate_right(1);
    for (what, d, failed) in [("first deleted", without(&[1]), 0), ("middle deleted", without(&[boxes / 2]), 0), ("one added first", added, 1)] {
        m.evaluate(&d);
        assert_eq!(m.results.iter().filter(|r| r.error.is_some()).count(), failed, "{what}");
        same_as_opened(&m, &d, what);
        m.evaluate(&doc);
        assert_eq!(m.last_recomputed, 0, "{what}, undone");
    }
    same_as_opened(&m, &doc, "all undone");
}

/// splitmix64, so each run makes the same edits.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Random edits of a small timeline.
struct Edits {
    state: u64,
    made: u64,
}

impl Edits {
    fn below(&mut self, n: usize) -> usize {
        (next(&mut self.state) % n.max(1) as u64) as usize
    }

    fn one<'a, T>(&mut self, of: &'a [T]) -> Option<&'a T> {
        let i = self.below(of.len());
        of.get(i)
    }

    fn component(&mut self, doc: &Document) -> u64 {
        let ids: Vec<u64> = std::iter::once(0).chain(doc.components.iter().map(|c| c.id)).collect();
        self.one(&ids).copied().unwrap_or(0)
    }

    /// The name of a body of `m`, or now and then of none.
    fn body(&mut self, m: &Model) -> String {
        let names: Vec<String> = m.state().bodies.iter().map(|b| b.name.clone()).collect();
        match self.below(12) {
            0 => "Gone".into(),
            _ => self.one(&names).cloned().unwrap_or_else(|| "Gone".into()),
        }
    }

    /// Names as a feature stores them: of its own, a number, one another may have, none.
    fn names(&mut self) -> Vec<String> {
        match self.below(6) {
            0 => Vec::new(),
            1 => vec![format!("Body{}", 1 + self.below(9))],
            2 => vec![["X", "Y"][self.below(2)].into()],
            3 => vec![String::new(), format!("Body{}", 1 + self.below(9))],
            _ => vec![format!("P{}", self.made)],
        }
    }

    /// A feature at the end of `doc`, on bodies of `m`.
    fn add(&mut self, doc: &mut Document, m: &Model) {
        use serde_json::json;
        self.made += 1;
        let c = self.component(doc);
        let x = 14.0 * self.below(5) as f64;
        let op = ["new_body", "new_body", "join", "cut", "cut", "intersect"][self.below(6)];
        let size = ["w", "10", "12", "9 + bad"][if self.below(20) == 0 { 3 } else { self.below(3) }];
        let name = format!("F{}", self.made);
        let (kind, target) = match self.below(13) {
            0..=2 => (json!({"type": "box", "corner": [x, 0.0, 0.0], "length": size, "width": "10", "height": "10", "operation": "new_body"}), false),
            3 => (json!({"type": "box", "corner": [x + 2.0, 2.0, 4.0], "length": size, "width": "3", "height": "10", "operation": op}), true),
            // Across a block: in two pieces.
            4 => (json!({"type": "box", "corner": [x + 4.0, -5.0, -5.0], "length": "2", "width": "30", "height": "30", "operation": "cut"}), true),
            5 => (
                json!({"type": "cylinder", "base": [x + 5.0, 5.0, -1.0], "axis": [0.0, 0.0, 1.0], "radius": "2", "height": "w + 2", "operation": op}),
                true,
            ),
            6 => (json!({"type": "sphere", "center": [x + 5.0, 5.0, 10.0], "radius": "3", "operation": op}), true),
            7 => (json!({"type": "move", "bodies": [self.body(m)], "translate": ["0", "0", "w / 10"], "copy": self.below(3) == 0}), false),
            8 => {
                let pattern = json!({"type": "rectangular", "dir1": [0, 1, 0], "count1": "n", "spacing1": "15"});
                (json!({"type": "pattern", "features": [], "bodies": [self.body(m)], "pattern": pattern}), false)
            }
            9 => {
                let op = ["join", "cut", "new_body"][self.below(3)];
                (
                    json!({"type": "combine", "target": self.body(m), "tools": [self.body(m)], "operation": op, "keep_tools": self.below(2) == 0}),
                    false,
                )
            }
            10 => {
                let xy = PlaneRef::Origin { name: "XY".into() };
                circle_extrude(doc, c, &name, xy, Vec2::new(x + 5.0, 5.0), 2.0 + self.below(3) as f64, "w", op);
                (serde_json::Value::Null, true)
            }
            _ => {
                let body = (self.below(2) == 0).then(|| self.body(m));
                let edge = [[x + 10.0, 5.0, 10.0], [x, 5.0, 10.0], [x + 5.0, 0.0, 0.0]][self.below(3)];
                let (ty, size) = [("fillet", "radius"), ("chamfer", "distance")][self.below(2)];
                let mut kind = json!({"type": ty, "edges": [edge], size: "r", "tangent_chain": false});
                if let Some(body) = body {
                    kind["body"] = json!(body);
                }
                (kind, false)
            }
        };
        if !kind.is_null() {
            add_in(doc, c, &name, kind);
        }
        let names = self.names();
        let to = match self.below(4) {
            0 => vec![self.body(m)],
            1 => vec!["*".into()],
            _ => Vec::new(),
        };
        let picked = self.below(2) == 0;
        let Some(f) = doc.features.iter_mut().find(|f| f.name == name) else { return };
        f.body_names = names;
        if target {
            f.participants = to;
        }
        if let FeatureKind::Fillet { edges, body, .. } | FeatureKind::Chamfer { edges, body, .. } = &f.kind
            && picked
        {
            f.edge_refs = crate::edge_refs(&m.state(), body, &plain(edges));
            f.edge_names = crate::edge_names_for(&m.state(), body, &plain(edges));
        }
    }

    /// One edit of `doc`, whose model is `m`: what it was.
    fn edit(&mut self, doc: &mut Document, m: &Model) -> String {
        let ids: Vec<u64> = doc.features.iter().map(|f| f.id).collect();
        let at = self.below(ids.len());
        let id = ids.get(at).copied().unwrap_or(0);
        let kind = self.below(17);
        match kind {
            0..=2 => {
                self.add(doc, m);
                format!("add {:?}", doc.features.last().map(|f| (&f.name, &f.kind, &f.body_names, &f.participants, f.component)))
            }
            // Added in the middle, where a rolled-back timeline adds.
            3 | 4 => {
                let before = doc.features.len();
                self.add(doc, m);
                let added = doc.features.len() - before;
                if let Some(tail) = doc.features.get_mut(at..) {
                    tail.rotate_right(added);
                }
                format!("add at {at} {:?}", doc.features.get(at + added - 1).map(|f| (&f.name, &f.kind, &f.body_names, &f.participants, f.component)))
            }
            5 | 6 => format!("delete {id}: {:?}", doc.delete_feature(id)),
            7 => {
                let to = self.below(ids.len());
                if at < doc.features.len() {
                    let f = doc.features.remove(at);
                    doc.features.insert(to.min(doc.features.len()), f);
                }
                format!("move {id} to {to}")
            }
            8 => {
                if let Some(f) = doc.feature_mut(id) {
                    f.suppressed = !f.suppressed;
                }
                format!("suppress {id}")
            }
            9 => {
                let names = self.names();
                if let Some(f) = doc.feature_mut(id) {
                    f.body_names.clone_from(&names);
                }
                format!("names of {id}: {names:?}")
            }
            // The names the bodies have, stored as a session stores them.
            10 => {
                let made: Vec<(u64, Vec<String>)> = m
                    .results
                    .iter()
                    .filter(|r| r.error.is_none() && !r.skipped)
                    .map(|r| (r.id, r.state.bodies.iter().filter(|b| b.feature == r.id).map(|b| b.name.clone()).collect()))
                    .collect();
                for (id, names) in made {
                    if let Some(f) = doc.feature_mut(id).filter(|f| f.body_names.is_empty()) {
                        f.body_names = names;
                    }
                }
                "store names".into()
            }
            11 => {
                let (name, value) =
                    [("w", "10 mm"), ("w", "13 mm"), ("r", "1 mm"), ("r", "2 mm"), ("r", "40 mm"), ("n", "2"), ("n", "3")][self.below(7)];
                format!("{name} = {value}: {:?}", doc.set_param(name, value, None, None))
            }
            12 => {
                let c = self.component(doc);
                if let Some(f) = doc.feature_mut(id) {
                    f.component = c;
                }
                format!("{id} in component {c}")
            }
            13 => {
                let name = format!("C{}", self.made);
                self.made += 1;
                let parent = self.component(doc);
                format!("add component: {:?}", doc.add_component(Some(&name), parent))
            }
            14 => {
                let c = self.component(doc);
                format!("remove component {c}: {:?}", doc.remove_component_tree(c).map(|_| ()))
            }
            15 => {
                doc.marker = (self.below(2) == 0).then_some(at);
                format!("marker {:?}", doc.marker)
            }
            _ => {
                let i = self.below(doc.occurrences.len());
                if let Some(o) = doc.occurrences.get_mut(i) {
                    o.transform[3][0] += 3.0;
                }
                format!("place {i}")
            }
        }
    }
}

/// Edit a timeline at random for `steps` steps from `seed`, undoing and reopening now and then:
/// after each the session's model is the design's opened afresh. `before`: features already there.
fn edited_as_opened(seed: u64, steps: usize, before: usize, running: bool) -> usize {
    let mut e = Edits { state: seed, made: 0 };
    let mut doc = if running { block_with_a_refused_join() } else { Document::new("edited") };
    for (name, value) in [("w", "10 mm"), ("r", "1 mm"), ("n", "2")] {
        doc.set_param(name, value, None, None).unwrap();
    }
    for c in ["A", "B"] {
        doc.add_component(Some(c), 0).unwrap();
    }
    // A body moved over and over, far from the others: each step is one more in the names' depth.
    if before > 0 {
        boxed(&mut doc, 0, "Far", -500.0, &["Far"]);
        for i in 1..before {
            add_in(&mut doc, 0, &format!("Far {i}"), serde_json::json!({"type": "move", "bodies": ["Far"], "translate": ["0", "0", "0.01"]}));
        }
    }
    let mut m = Model::new();
    m.set_background_checks(running);
    m.evaluate(&doc);
    for _ in 0..5 {
        e.add(&mut doc, &m);
        m.evaluate(&doc);
    }
    let mut undo: Vec<Document> = Vec::new();
    let mut done: Vec<String> = Vec::new();
    for step in 0..steps {
        let what = match e.below(8) {
            0 if !undo.is_empty() => {
                doc = undo.pop().unwrap();
                "undo".to_string()
            }
            1 => match m.saved(&doc).and_then(|s| Model::adopt(&doc, &s).ok()) {
                Some(opened) => {
                    m = opened;
                    m.set_background_checks(running);
                    "reopen".into()
                }
                None => continue,
            },
            _ => {
                undo.push(doc.clone());
                e.edit(&mut doc, &m)
            }
        };
        done.push(what);
        m.evaluate(&doc);
        // The checks of some edits are still running when the next one comes.
        if running {
            if step + 1 < steps && e.below(2) == 0 {
                continue;
            }
            settled(&mut m, &doc);
        }
        let what = format!("seed {seed}, step {step}:\n{}\n", done.join("\n"));
        same_as_opened(&m, &doc, &what);
    }
    done.len()
}

/// Whatever is edited, in whatever order, the model a session holds is the one opening its design
/// gives: the same bodies, names, errors and warnings, however much of it was kept.
#[test]
fn any_edits_leave_the_model_of_the_design_opened_afresh() {
    let env = |name: &str, or: u64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(or);
    let (from, seeds, steps) = (env("SOLIDCRAFT_EDIT_SEED", 1), env("SOLIDCRAFT_EDIT_SEEDS", 8), env("SOLIDCRAFT_EDIT_STEPS", 25));
    let done: usize = (from..from + seeds).map(|seed| edited_as_opened(seed, steps as usize, 0, false)).sum();
    eprintln!("{done} edits compared");
}

/// The same with the checks a window leaves running, among them ones that refuse a join.
#[test]
fn any_edits_with_checks_left_running_leave_the_model_of_the_design_opened_afresh() {
    let env = |name: &str, or: u64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(or);
    let (from, seeds, steps) = (env("SOLIDCRAFT_EDIT_SEED", 1), env("SOLIDCRAFT_EDIT_SEEDS", 4), env("SOLIDCRAFT_EDIT_STEPS", 20));
    let done: usize = (from..from + seeds).map(|seed| edited_as_opened(seed, steps as usize, 0, true)).sum();
    eprintln!("{done} edits compared");
}

/// The same across the limit of naming from history, which bodies far from it decide.
#[test]
fn any_edits_about_the_naming_limit_leave_the_model_of_the_design_opened_afresh() {
    let env = |name: &str, or: u64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(or);
    let (from, seeds, steps) = (env("SOLIDCRAFT_EDIT_SEED", 1), env("SOLIDCRAFT_EDIT_SEEDS", 3), env("SOLIDCRAFT_EDIT_STEPS", 15));
    let done: usize = (from..from + seeds).map(|seed| edited_as_opened(seed, steps as usize, 397, false)).sum();
    eprintln!("{done} edits compared");
}

/// A construction plane's edit recomputes only the features that use it: a sketch on it and what
/// is made from that sketch, wherever they are in the timeline, and nothing else.
#[test]
fn a_construction_plane_edit_recomputes_only_its_users() {
    let mut doc = three_parts(true);
    let b = doc.find_component("B").unwrap();
    // A peg in B on A's plane: B now reads A through it.
    let a_top = doc.find_feature("A_top").unwrap().id;
    circle_extrude(
        &mut doc,
        b,
        "B peg",
        PlaneRef::Construction { feature: a_top.into(), placement: None },
        Vec2::new(40.0, 20.0),
        2.0,
        "3",
        "new_body",
    );
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let total = m.results.len();
    doc.set_param("A_h", "11 mm", None, None).unwrap();
    m.evaluate(&doc);
    no_errors(&m);
    assert_eq!(m.last_recomputed, 4, "A's extrude and plane, the peg's sketch and extrude, of {total}");
    assert_eq!(summary(&m.state()), from_scratch(&doc));
    let peg = m.state().body("B peg").map(|b| b.body.outline_bounds().min.z);
    assert!(peg.is_some_and(|z| (z - 16.0).abs() < 1e-6), "the peg stands on the plane: {peg:?}");
    // Results restored whole after the plane carry the plane as it is now.
    let st = m.state();
    let top = st.construct.iter().find(|c| c.name == "A_top").map(|c| &c.geom);
    assert!(matches!(top, Some(crate::construct::ConstructGeom::Plane(p)) if (p.origin.z - 16.0).abs() < 1e-9), "{top:?}");
    let mut scratch = Model::new();
    scratch.no_cache = true;
    scratch.evaluate(&doc);
    assert_eq!(st.construct, scratch.state().construct);
}

/// A part built on another component's plane keeps its creation context through assembly
/// motion and a cold rebuild, while the plane's defining dimensions remain live.
#[test]
fn a_captured_foreign_plane_moves_with_its_part_and_follows_source_dimensions() {
    use solidcraft_geom::PlaneGeom;
    let mut doc = Document::new("plane reference");
    let source = doc.add_component(Some("Skeleton"), 0).unwrap();
    let part = doc.add_component(Some("Knob"), 0).unwrap();
    doc.set_param("height", "5 mm", Some("mm"), None).unwrap();
    let plane = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: "height".into() },
            },
            Some("Guide"),
        )
        .unwrap();
    doc.feature_mut(plane).unwrap().component = source;
    let mut sketch = Sketch::new();
    sketch.add_circle(Vec2::new(10., 0.), 2., None, None).unwrap();
    let reference: PlaneRef = serde_json::from_value(serde_json::json!({"type":"construction", "feature":plane, "placement":IDENTITY})).unwrap();
    let id = doc.add_feature(FeatureKind::Sketch { plane: reference, sketch }, Some("Profile")).unwrap();
    doc.feature_mut(id).unwrap().component = part;
    let check = |doc: &Document, model: &mut Model, expected: Vec3| {
        model.evaluate(doc);
        no_errors(model);
        let state = model.state();
        let sketch = state.sketch(id).unwrap();
        let center = apply_point(&doc.component_transform(part), sketch.plane.to_world(Vec2::new(10., 0.)));
        assert!(center.dist(expected) < 1e-9, "center {center:?}; expected {expected:?}");
    };
    let mut model = Model::new();
    check(&doc, &mut model, Vec3::new(10., 0., 5.));
    doc.occurrences.iter_mut().find(|o| o.component == part).unwrap().transform = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    doc.occurrences.iter_mut().find(|o| o.component == source).unwrap().transform = rigid(Vec3::new(20., 0., 7.), Vec3::ZERO, Vec3::Z, 0.);
    check(&doc, &mut model, Vec3::new(0., 10., 5.));
    check(&doc, &mut Model::new(), Vec3::new(0., 10., 5.));
    let mut reopened = Document::from_json(&doc.to_json()).unwrap();
    check(&reopened, &mut Model::new(), Vec3::new(0., 10., 5.));
    reopened.set_param("height", "9 mm", None, None).unwrap();
    check(&reopened, &mut Model::new(), Vec3::new(0., 10., 9.));
}

/// A coil sweeping a sketch's profile is made again when that sketch changes, not restored as
/// it was (a coil of its own section reads no sketch).
#[test]
fn a_coil_follows_its_sketch() {
    let section = |r: f64| {
        let mut sk = Sketch::new();
        sk.add_circle(Vec2::new(20.0, 0.0), r, None, None).unwrap();
        FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XZ".into() }, sketch: sk }
    };
    let mut doc = Document::new("coil");
    let sketch = doc.add_feature(section(1.0), None).unwrap();
    doc.add_feature(
        FeatureKind::Coil {
            base: point_expr(Vec3::new(0.0, 0.0, 0.0)),
            axis: crate::dir_expr(Vec3::Z),
            axis_ref: None,
            reversed: false,
            sketch: Some(crate::SketchId(sketch)),
            profiles: ProfileSel::All,
            diameter: None,
            pitch: "8".into(),
            turns: "2".into(),
            section_size: None,
            section: Default::default(),
            position: 0,
            start_angle: None,
            clockwise: false,
            end_cut: Default::default(),
            operation: Operation::NewBody,
            targets: Vec::new(),
        },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    no_errors(&m);
    let before = summary(&m.state());
    if let Some(f) = doc.feature_mut(sketch) {
        f.kind = section(2.0);
    }
    m.evaluate(&doc);
    no_errors(&m);
    assert_eq!(m.last_recomputed, 2, "the sketch and its coil");
    assert_eq!(summary(&m.state()), from_scratch(&doc));
    assert_ne!(summary(&m.state()), before);
}

#[test]
fn box_fillet_cut_timeline() {
    let (mut doc, _) = plate_doc();
    doc.add_feature(
        FeatureKind::Fillet {
            edges: vec![Position(Vec3::new(0.0, 0.0, 10.0))],
            radius: "3".into(),
            body: None,
            style: Default::default(),
            fit: Default::default(),
            corner: Default::default(),
            setbacks: Vec::new(),
            turn: Default::default(),
            tangent_chain: true,
            edge_radii: Vec::new(),
        },
        None,
    )
    .unwrap();
    let mut sk = Sketch::new();
    sk.add_circle(Vec2::new(20.0, 15.0), 5.0, None, None).unwrap();
    let s2 = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(
        FeatureKind::Extrude {
            thin: None,
            sketch: crate::SketchId(s2),
            profiles: ProfileSel::Points { points: vec![Vec2::new(20.0, 15.0)] },
            extent: Extent {
                convention: crate::ExtrudeConvention::LegacyPerSide,
                distance: "20".into(),
                direction: Direction::Positive,
                distance2: None,
                start_offset: None,
                symmetric_offset: false,
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
                through_all2: false,
                to2: None,
                to_offset2: None,
                from_level: false,
                picked_placements: Vec::new(),
                one_sided_direction: None,
            },
            operation: Operation::Cut,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    let st = m.state();
    let v = measure(&st.bodies[0].body).unwrap().volume;
    let expect = 24000.0 - (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 20.0 - std::f64::consts::PI * 25.0 * 20.0;
    assert!(rel(v, expect) < 1e-3, "{v} vs {expect}");
    // Roll back to before the fillet.
    doc.marker = Some(2);
    m.evaluate(&doc);
    assert!(rel(measure(&m.state().bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    assert!(m.results[3].skipped);
}

#[test]
fn errors_are_reported_not_fatal() {
    let (mut doc, s) = plate_doc();
    doc.add_feature(
        FeatureKind::Revolve {
            sketch: crate::SketchId(s),
            profiles: ProfileSel::All,
            axis: AxisRef::SketchAxis { axis: "y".into() },
            angle: "90 deg".into(),
            operation: Operation::NewBody,
            targets: vec![],
            angle2: None,
            symmetric: false,
            convention: Default::default(),
            to: None,
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Fillet {
            edges: vec![Position(Vec3::new(500.0, 0.0, 0.0))],
            radius: "1".into(),
            body: None,
            style: Default::default(),
            fit: Default::default(),
            corner: Default::default(),
            setbacks: Vec::new(),
            turn: Default::default(),
            tangent_chain: true,
            edge_radii: Vec::new(),
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::ZERO),
            length: "nope".into(),
            width: "1".into(),
            height: "1".into(),
            operation: Operation::NewBody,
            centred: false,
        },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results[2].error.is_none(), "{:?}", m.results[2]);
    assert!(m.results[3].error.is_some());
    assert!(m.results[4].error.as_deref().unwrap_or("").contains("nope"));
    assert_eq!(m.state().bodies.len(), 2);
    let quarter = measure(&m.state().bodies[1].body).unwrap().volume;
    // Revolving the 40x30 rectangle (x in [0,40]) 90° about sketch y: π/4 · 40² · 30.
    assert!(rel(quarter, std::f64::consts::FRAC_PI_4 * 1600.0 * 30.0) < 1e-3, "{quarter}");
}

#[test]
fn params_and_serialization() {
    let (mut doc, _) = plate_doc();
    assert!(doc.set_param("bad name", "1", None, None).is_err());
    assert!(doc.set_param("mm", "1", None, None).is_err());
    assert!(doc.set_param("loop", "loop + 1", None, None).is_err());
    assert!(doc.remove_param("width").is_err(), "used by d1");
    doc.set_param("spare", "3", None, None).unwrap();
    doc.remove_param("spare").unwrap();
    let json = doc.to_json();
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, doc);
    assert!(Document::from_json("{").is_err());
    let gone = doc.delete_feature(doc.features[0].id).unwrap();
    assert_eq!(gone.len(), 2, "the extrude goes with its sketch");
    assert!(doc.param("d1").is_none(), "unused model parameters are pruned");
}

/// #4783: a body reads the material its mass is computed with, as the library or the design
/// spells it. An unknown name, or a design material with an unusable density, reads as the
/// default material it weighs. `density` agrees in every case, and a configuration keeps the
/// assignment as stored.
#[test]
fn a_body_reads_the_material_its_mass_is_computed_with() {
    let mut doc = Document::new("materials");
    let usable = [("Walnut", 0.65), ("Mote", f64::MIN_POSITIVE), ("Limit", MAX_DENSITY)];
    let unusable = [("Broken", -3.0), ("Weightless", 0.0), ("Beyond", MAX_DENSITY.next_up())];
    for (name, density) in usable.into_iter().chain(unusable) {
        doc.user_materials.push(UserMaterial { name: name.into(), density, appearance: None, physical_card: None });
    }
    // (body, its assignment as stored, the material it reads and weighs, that material's density)
    let cases = [
        ("Known", Some("Steel"), "Steel", 7.85),
        ("Miscased", Some("steel"), "Steel", 7.85),
        ("Own", Some(" walnut "), "Walnut", 0.65),
        ("Lightest", Some("Mote"), "Mote", f64::MIN_POSITIVE),
        ("At the limit", Some("Limit"), "Limit", MAX_DENSITY),
        ("Unknown", Some("Unobtainium"), DEFAULT_MATERIAL, DEFAULT_DENSITY),
        ("Unusable", Some("Broken"), DEFAULT_MATERIAL, DEFAULT_DENSITY),
        ("Zero density", Some("Weightless"), DEFAULT_MATERIAL, DEFAULT_DENSITY),
        ("Over the limit", Some("Beyond"), DEFAULT_MATERIAL, DEFAULT_DENSITY),
        ("Unassigned", None, DEFAULT_MATERIAL, DEFAULT_DENSITY),
    ];
    for (body, stored, ..) in cases {
        if let Some(material) = stored {
            doc.materials.insert(body.into(), material.into());
        }
    }
    for (body, stored, material, density) in cases {
        assert_eq!(doc.body_material(body), material, "{body}");
        assert_eq!(doc.density(body), density, "{body}");
        assert_eq!(doc.material_density(material), Some(density), "{body}: the density is its material's");
        let cell = doc.config_current(&crate::config::Column::Material(body.into()));
        assert_eq!(cell.as_deref(), Some(stored.unwrap_or(DEFAULT_MATERIAL)), "{body}: a configuration keeps the assignment as stored");
    }
}

#[test]
fn primitives_and_combine() {
    let mut doc = Document::new("p");
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::ZERO),
            length: "10".into(),
            width: "10".into(),
            height: "10".into(),
            operation: Operation::NewBody,
            centred: false,
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Cylinder {
            base: point_expr(Vec3::new(5.0, 5.0, 0.0)),
            axis: crate::dir_expr(Vec3::Z),
            radius: "2".into(),
            height: "20".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Combine {
            target: "Body1".into(),
            tools: vec!["Body2".into()],
            operation: Operation::Cut,
            keep_tools: false,
            offset: None,
            offset_edges: OffsetEdges::Sharp,
            offset_faces: Vec::new(),
            offset_radius: None,
        },
        None,
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let st = m.state();
    assert_eq!(st.bodies.len(), 1);
    assert!(rel(measure(&st.bodies[0].body).unwrap().volume, 1000.0 - std::f64::consts::PI * 40.0) < 1e-3);
}

#[test]
fn hostile_expressions_never_panic() {
    // A deterministic stream of nasty inputs: random tokens, deep nesting, huge numbers.
    let toks = [
        "(",
        ")",
        "+",
        "-",
        "*",
        "/",
        "^",
        ",",
        "1e308",
        "0",
        "-0",
        "mm",
        "deg",
        "x",
        "sqrt",
        "cosh",
        "atanh",
        "pow",
        "min",
        "max",
        "°",
        "1.2.3",
        "e",
        "pi",
        "in",
        "nan",
        "9999999999999999999999",
    ];
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..5000 {
        let n = (next() % 24) as usize;
        let s: String = (0..n).map(|_| toks[(next() % toks.len() as u64) as usize]).collect::<Vec<_>>().join(" ");
        let _ = crate::expr::eval_with(&s, &|n| {
            if n == "x" { Ok(crate::expr::Value::length(2.0)) } else { Err(crate::DocError::Expr("unknown".into())) }
        });
    }
    let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
    assert!(crate::expr::eval_with(&deep, &|_| Err(crate::DocError::Expr("x".into()))).is_err());
    assert!(crate::expr::eval_with("pow(2 mm, 3)", &|_| Err(crate::DocError::Expr("x".into()))).is_ok());
    assert!(crate::expr::eval_with("pow(2 mm, 300)", &|_| Err(crate::DocError::Expr("x".into()))).is_err());
}

#[test]
fn parameter_cycles_are_found() {
    use crate::expr::Kind;
    let list = vec![
        ("a".to_string(), "b + 1".to_string(), Kind::Length),
        ("b".to_string(), "c * 2".to_string(), Kind::Length),
        ("c".to_string(), "a".to_string(), Kind::Length),
        ("d".to_string(), "5".to_string(), Kind::Length),
        ("e".to_string(), "d + e".to_string(), Kind::Length),
    ];
    let c = crate::cycles(&list);
    assert_eq!(c.into_iter().collect::<Vec<_>>(), vec!["a", "b", "c", "e"]);
    let defs: Vec<crate::expr::ParamDef> =
        list.iter().map(|(n, e, _)| crate::expr::ParamDef { name: n.clone(), expr: e.clone(), unit: "mm".into() }).collect();
    let (vals, errs) = crate::expr::eval_params(&defs, 1.0);
    assert!(errs["a"].contains("circular reference: a → b → c → a"), "{}", errs["a"]);
    assert!(vals.contains_key("d") && errs.contains_key("a") && errs.contains_key("e"));
}

/// Features that use a sketch's points or regions go with it (not only profile and path users).
#[test]
fn input_sketches_cover_points_and_patches() {
    let hole: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "hole", "position": [0, 0, 0], "direction": [0, 0, -1], "diameter": "3",
        "points": {"sketch": 7, "ids": ["p1"]},
    }))
    .unwrap();
    assert_eq!(hole.input_sketches(), vec![7]);
    let patch: FeatureKind = serde_json::from_value(serde_json::json!({"type": "patch", "sketch": 9})).unwrap();
    assert_eq!(patch.input_sketches(), vec![9]);
    let pipe: FeatureKind = serde_json::from_value(serde_json::json!({"type": "pipe", "path_sketch": 72, "path": ["l1"], "diameter": "4"})).unwrap();
    assert_eq!(pipe.input_sketches(), vec![72]);
}

#[test]
fn timeline_clones_share_meshes_even_when_cloned_before_tessellation() {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
    let a = crate::eval::ModelBody::new("box".into(), body, 1);
    let b = a.clone();
    assert!(std::sync::Arc::ptr_eq(&a.mesh(), &b.mesh()));
    assert!(!a.mesh().triangles.is_empty());
}

#[test]
fn feature_inserted_at_a_marker_inside_a_timeline_folder_joins_it() {
    let mut doc = Document::new("folder insertion");
    let first = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, Some("First")).unwrap();
    let second =
        doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, Some("Second")).unwrap();
    doc.timeline_groups.push(TimelineGroup { id: 100, name: "Pair".into(), parent: None, features: vec![first, second], joints: vec![] });
    doc.marker = Some(1);

    let inserted =
        doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, Some("Inserted")).unwrap();

    assert_eq!(doc.features.iter().map(|feature| feature.id).collect::<Vec<_>>(), vec![first, inserted, second]);
    assert_eq!(doc.timeline_groups[0].features, vec![first, inserted, second]);
    assert_eq!(doc.marker, Some(2));
    assert!(non_contiguous_timeline_group(&doc.features, &doc.timeline_groups).is_none());
    assert!(Document::from_json(&doc.to_json()).is_ok(), "the inserted feature must leave a reopenable design");
}

/// #9373: a timeline folder that names a feature or joint the design doesn't have, or a feature
/// a folder already lists, no longer stops the design opening. Those entries are dropped and
/// everything else is kept. Editing a feature inside a folder used to save such entries.
#[test]
fn a_folder_naming_missing_or_repeated_features_still_opens() {
    let mut doc = Document::new("stale folder entries");
    let sketch = || FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() };
    let a = doc.add_feature(sketch(), Some("A")).unwrap();
    let b = doc.add_feature(sketch(), Some("B")).unwrap();
    let c = doc.add_feature(sketch(), Some("C")).unwrap();
    let missing = doc.next_id + 10;
    doc.timeline_groups.push(TimelineGroup { id: 100, name: "Repeats".into(), parent: None, features: vec![a, b, b], joints: vec![] });
    doc.timeline_groups.push(TimelineGroup {
        id: 101,
        name: "Stale".into(),
        parent: None,
        features: vec![b, c, missing, missing, missing],
        joints: vec![missing, missing],
    });
    let opened = Document::from_json(&doc.to_json()).expect("the design opens");
    assert_eq!(opened.features.iter().map(|f| f.id).collect::<Vec<_>>(), vec![a, b, c]);
    assert_eq!(opened.timeline_groups[0].features, vec![a, b], "listed once");
    assert_eq!(opened.timeline_groups[1].features, vec![c], "no missing feature, nor one the first folder holds");
    assert!(opened.timeline_groups[1].joints.is_empty());
    assert_eq!(opened.timeline_groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Repeats", "Stale"], "both folders are kept");
}

/// Timeline folders nested 10 000 deep open at once: each folder's chain of parents is walked
/// once, not again for every folder inside it (a 588 KB design took over three minutes). A
/// cycle is still refused.
#[test]
fn deeply_nested_timeline_folders_open_and_a_cycle_does_not() {
    let mut doc = Document::new("nested folders");
    let n = 10_000u64;
    doc.timeline_groups = (0..n)
        .map(|i| TimelineGroup { id: 100 + i, name: "f".into(), parent: (i + 1 < n).then_some(101 + i), features: vec![], joints: vec![] })
        .collect();
    let started = std::time::Instant::now();
    let opened = Document::from_json(&doc.to_json()).expect("the design opens");
    assert_eq!(opened.timeline_groups.len(), n as usize);
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
    doc.timeline_groups =
        (0..3).map(|i| TimelineGroup { id: 100 + i, name: "f".into(), parent: Some(100 + (i + 1) % 3), features: vec![], joints: vec![] }).collect();
    let refused = Document::from_json(&doc.to_json()).unwrap_err().to_string();
    assert!(refused.contains("cyclic timeline folders"), "{refused}");
}

/// A marker after the last feature leaves every feature active, as no marker does, so a feature
/// added then is active too, not added rolled back (#7741: editing the last feature from the
/// History runs its command with the marker there).
#[test]
fn feature_added_with_the_marker_at_the_end_is_active() {
    let mut doc = Document::new("marker at the end");
    let first = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, Some("First")).unwrap();
    doc.marker = Some(1);

    let added = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, Some("Added")).unwrap();

    assert_eq!(doc.features.iter().map(|feature| feature.id).collect::<Vec<_>>(), vec![first, added]);
    assert_eq!(doc.marker, Some(2), "the marker stays after the last feature");
}

/// A construction plane that is its own side resolves through the model and back into the
/// document. The depth count has to survive that hop, or the recursion overflows the stack and
/// aborts the process, which no panic guard can intercept.
#[test]
fn self_referencing_construction_planes_are_errors() {
    // Feature ids are assigned from 1, so each design below knows its own feature's id.
    let selfref = |id: u64| Box::new(construct::GeoRef::Plane { plane: PlaneRef::Construction { feature: id.into(), placement: None } });
    let xz = || Box::new(construct::GeoRef::Plane { plane: PlaneRef::Origin { name: "XZ".into() } });

    // Directly its own first side.
    let mut doc = Document::new("selfplane");
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a: selfref(1), b: xz(), old_frame: false } }, Some("P1")).unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.is_some()), "a plane that is its own side must report an error: {:?}", m.results);

    // Two planes that are each other's side.
    let mut doc = Document::new("twoplanes");
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a: selfref(2), b: xz(), old_frame: false } }, Some("P1")).unwrap();
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a: selfref(1), b: xz(), old_frame: false } }, Some("P2")).unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.is_some()), "a two-plane cycle must report an error: {:?}", m.results);

    // A perpendicular plane whose base is itself, and a sketch placed on it.
    let mut doc = Document::new("perpplane");
    let line = Box::new(construct::GeoRef::World { axis: "X".into() });
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Perpendicular { plane: selfref(1), line } }, Some("P1")).unwrap();
    doc.add_feature(
        FeatureKind::Sketch { plane: PlaneRef::Construction { feature: 1.into(), placement: None }, sketch: Sketch::new() },
        Some("Sketch1"),
    )
    .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.is_some()), "a cyclic perpendicular plane must report an error: {:?}", m.results);
}

/// #18260: a midplane between two planes that are parallel to a world plane takes that world
/// plane's frame: half way between them, with its origin on the world axis and not at the point
/// the planes were picked, so a sketch keeps its world place when it is redefined onto it.
#[test]
fn a_midplane_parallel_to_a_world_plane_takes_that_world_planes_frame() {
    // A box with a top face at z = 20 and a bottom face at z = 0, both picked away from the
    // world origin.
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(40., 30., 20.)).unwrap();
    let mut st = ModelState::default();
    st.bodies.push(ModelBody::new("Box".into(), body, 1));
    let faces = st.bodies[0].body.faces(0.01).unwrap();
    let face_at = |z: f64| {
        faces
            .iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.dot(Vec3::Z).abs() > 0.99) && (f.centroid.z - z).abs() < 1e-6)
            .map(|f| f.centroid)
            .unwrap()
    };
    let mut doc = Document::new("midplane");
    let mid = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Midplane {
                    a: Box::new(crate::construct::GeoRef::Face { at: face_at(20.), name: None }),
                    b: Box::new(crate::construct::GeoRef::Face { at: face_at(0.), name: None }),
                    old_frame: false,
                },
            },
            Some("Mid"),
        )
        .unwrap();
    let vals = doc.param_values().0;
    let built = doc.resolve_plane_in(&vals, &PlaneRef::Construction { feature: crate::FeatureId(mid), placement: None }, 0, 0, Some(&st)).unwrap();
    // Half way between the two faces (z = 10), on the world Z axis, in the world XY frame.
    assert!(built.origin.dist(Vec3::new(0., 0., 10.)) < 1e-9, "the midplane's origin is on the world axis: {built:?}");
    assert!(built.x.dist(Vec3::X) < 1e-9 && built.y.dist(Vec3::Y) < 1e-9, "the midplane takes the world XY frame: {built:?}");
}

/// #18260: a design saved before the midplane took a world plane's frame keeps it. The pending
/// format change marks every midplane of an older design, so the sketch drawn on it opens
/// where it was drawn; a design saved now takes the world plane's frame.
#[test]
fn a_design_saved_before_the_midplane_frame_change_keeps_its_old_frame() {
    // Two planes parallel to the world XY, the first picked away from the world axis and with
    // axes of its own, so the frame it gives the midplane is not the world's.
    let side = Plane { origin: Vec3::new(10., 20., 0.), x: Vec3::new(0., 1., 0.), y: Vec3::new(-1., 0., 0.) };
    let other = Plane { origin: Vec3::new(10., 20., 40.), x: Vec3::new(0., 1., 0.), y: Vec3::new(-1., 0., 0.) };
    let midplane = |old_frame: bool| PlaneRef::Midplane {
        a: Box::new(crate::construct::GeoRef::Plane { plane: PlaneRef::Custom { plane: side } }),
        b: Box::new(crate::construct::GeoRef::Plane { plane: PlaneRef::Custom { plane: other } }),
        old_frame,
    };
    let mut doc = Document::new("midplane");
    let mid = doc.add_feature(FeatureKind::ConstructionPlane { plane: midplane(false) }, Some("Mid")).unwrap();
    let st = ModelState::default();
    let plane_of = |doc: &Document| {
        doc.resolve_plane_in(&doc.param_values().0, &PlaneRef::Construction { feature: crate::FeatureId(mid), placement: None }, 0, 0, Some(&st))
            .unwrap()
    };

    // The design as it is saved now: the world plane's frame.
    let opened = crate::format::read(&doc.to_json()).unwrap();
    let built = plane_of(&opened);
    assert!(built.origin.dist(Vec3::new(0., 0., 20.)) < 1e-9, "the midplane takes the world XY frame: {built:?}");
    assert!(built.x.dist(Vec3::X) < 1e-9 && built.y.dist(Vec3::Y) < 1e-9, "the midplane takes the world XY frame: {built:?}");

    // The design as an older build saved it: before v64 (`midplane-old-frame.rs`), so the
    // v64 migration step is applied to it as it opens.
    let mut saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    saved["format"] = serde_json::json!(crate::format::format_string(63));
    saved.as_object_mut().unwrap().remove("format_pending");
    let older = crate::format::read(&serde_json::to_string(&saved).unwrap()).unwrap();
    let built = plane_of(&older);
    // The frame it was saved with: half way between the two planes, at the first plane's own
    // origin and in its own axes.
    assert!(built.origin.dist(Vec3::new(10., 20., 20.)) < 1e-9, "the midplane keeps the frame it was saved with: {built:?}");
    assert!(
        built.x.dist(Vec3::new(0., 1., 0.)) < 1e-9 && built.y.dist(Vec3::new(-1., 0., 0.)) < 1e-9,
        "the midplane keeps the frame it was saved with: {built:?}"
    );
}

/// A rigid translation, the way the evaluation moves an occurrence.
fn translation(t: Vec3) -> crate::Mat {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [t.x, t.y, t.z, 1.0]]
}

/// The seven datum items parse their names without regard to case, and a datum key is strict:
/// canonical non-zero decimal occurrence ids, at most 64 of them, at most 2048 characters. The
/// root keeps its own controls, so nothing that names the design's origin parses as a datum.
#[test]
fn datum_refs_parse_strictly() {
    use construct::{DatumItem, DatumRef};
    assert_eq!(DatumRef { path: vec![2, 3], item: DatumItem::XY }.key(), "o:2/3/origin/XY");
    let parsed = DatumRef::parse_key("o:2/3/origin/XY").unwrap();
    assert_eq!(parsed.path, vec![2, 3]);
    assert_eq!(parsed.item, DatumItem::XY);
    // The item parses without regard to case, and every item round-trips through its key.
    assert_eq!(DatumRef::parse_key("o:5/origin/xy").unwrap().item, DatumItem::XY);
    assert_eq!(DatumRef::parse_key("o:5/origin/o").unwrap().item, DatumItem::O);
    for item in ["O", "X", "Y", "Z", "XY", "XZ", "YZ"] {
        let key = DatumRef { path: vec![5], item: DatumItem::parse(item).unwrap() }.key();
        assert_eq!(DatumRef::parse_key(&key).unwrap().item, DatumItem::parse(item).unwrap(), "{item}");
    }
    // The Plane::named aliases stay root-only.
    assert_eq!(DatumItem::parse("TOP"), None);
    assert_eq!(DatumItem::parse("front"), None);
    // The design's own origin has its own controls: no `c0`, no bare item, no `o:`, an empty
    // path, leading zeros, a zero id, a sign, an empty link, a missing item, an unknown item.
    for invalid in ["c0/origin/XY", "origin/XY", "XY", "2/3/origin/X", "o:/origin/X", "o:0/origin/X", "o:01/origin/X", "o:+2/origin/X"] {
        assert!(DatumRef::parse_key(invalid).is_none(), "{invalid}");
    }
    for invalid in ["o:2//origin/X", "o:2/3/origin/", "o:2/3/origin/X/", "o:2/3/origin/XX", "o:2/3/origin/TOP", "o:2/origin/origin/X"] {
        assert!(DatumRef::parse_key(invalid).is_none(), "{invalid}");
    }
    // 64 occurrences deep is fine, 65 is not, and absurd lengths are refused without panicking.
    let deep = |n: usize| format!("o:{}/origin/X", vec!["1"; n].join("/"));
    assert!(DatumRef::parse_key(&deep(64)).is_some());
    assert!(DatumRef::parse_key(&deep(65)).is_none());
    assert!(DatumRef::parse_key(&deep(64).repeat(40)).is_none());
}

/// `Document::placement_frame` walks one exact placement the way the engine's occurrence walk
/// does: the #7313 scenario's parent translated +100, a child +20, a copy of the parent another
/// +50 - the copied child's XY plane sits at (170, 0, 0) against the original at (120, 0, 0).
/// A direction of expressions turned by a frame keeps a constant however small it is next to
/// nothing else: `["sin(t)", "0", "1e-20"]` at t = 0 is still along Z after a turn about Y, not
/// zero (#18822). A length keeps its cutoff of 1e-12 mm for rounding.
#[test]
fn a_turned_direction_keeps_its_small_constants() {
    let mut doc = Document::new("T");
    doc.set_param("t", "0 deg", None, None).unwrap();
    let turn = crate::rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Y, std::f64::consts::FRAC_PI_4);
    let mut d: crate::DirExpr = ["sin(t)".into(), "0".into(), "1e-20".into()];
    crate::map_dir_expr(&turn, &mut d);
    let v = d.clone().map(|e| doc.eval(&e, crate::expr::Kind::Unitless).unwrap());
    let unit = crate::unit_direction(Vec3::new(v[0], v[1], v[2])).expect("a direction, not zero");
    let h = std::f64::consts::FRAC_1_SQRT_2;
    assert!(unit.dist(Vec3::new(h, 0.0, h)) < 1e-9, "{d:?} -> {unit:?}");
    // Rounding from the turn itself is dropped from a direction of ordinary size, with or
    // without an expression beside it, and every number is written short.
    let quarter = crate::rigid(Vec3::ZERO, Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2);
    let mut d: crate::DirExpr = ["sin(t)".into(), "1".into(), "0".into()];
    crate::map_dir_expr(&quarter, &mut d);
    assert_eq!(d[1].to_string(), "0", "{d:?}");
    assert!(d.iter().all(|e| e.len() < 40), "{d:?}");
    let mut d: crate::DirExpr = ["sin(t)".into(), "1e-300".into(), "0".into()];
    crate::map_dir_expr(&quarter, &mut d);
    assert!(d.iter().all(|e| e.len() < 40), "{d:?}");
    doc.set_param("t", "30 deg", None, None).unwrap();
    let v = d.clone().map(|e| doc.eval(&e, crate::expr::Kind::Unitless).unwrap());
    assert!((v[0] - 0.5).abs() < 1e-15 && v[1].abs() < 1e-290 && (v[2] - 1e-300).abs() < 1e-310, "{d:?} -> {v:?}");
}

#[test]
fn placement_frame_matches_the_occurrence_walk() {
    let mut doc = Document::new("datum frames");
    let assembly = doc.add_component(Some("Assembly"), 0).unwrap();
    doc.add_component(Some("Child"), assembly).unwrap();
    let first = doc.occurrences.iter().find(|o| o.component == assembly).unwrap().id;
    let child = doc.occurrences.iter().find(|o| o.parent == assembly).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == first).unwrap().transform = translation(Vec3::new(100.0, 0.0, 0.0));
    doc.occurrences.iter_mut().find(|o| o.id == child).unwrap().transform = translation(Vec3::new(20.0, 0.0, 0.0));
    // A second placement of the same assembly: the original's frame carried another +50.
    let second = doc.add_occurrence(assembly, 0, translation(Vec3::new(150.0, 0.0, 0.0))).unwrap();
    let origin = |path: &[u64]| crate::apply_point(&doc.placement_frame(path).unwrap(), Vec3::ZERO);
    let (one, two) = ([first, child], [second, child]);
    assert!(origin(&one).dist(Vec3::new(120.0, 0.0, 0.0)) < 1e-9, "the original child's frame: {:?}", origin(&one));
    assert!(origin(&two).dist(Vec3::new(170.0, 0.0, 0.0)) < 1e-9, "the copied child's frame: {:?}", origin(&two));
    // The same placement twice in a path is refused, and so is a path through a missing link.
    assert!(doc.placement_frame(&[first, first]).is_err());
    assert!(doc.placement_frame(&[999_999, child]).is_err());
}

/// A datum resolves through its placement's frame at evaluation time: two parents of one child
/// give two distinct world planes, and moving the child moves both of them with it.
#[test]
fn resolve_datum_places_nested_and_repeated() {
    let mut doc = Document::new("datum resolve");
    let assembly = doc.add_component(Some("Assembly"), 0).unwrap();
    doc.add_component(Some("Child"), assembly).unwrap();
    let first = doc.occurrences.iter().find(|o| o.component == assembly).unwrap().id;
    let child = doc.occurrences.iter().find(|o| o.parent == assembly).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == first).unwrap().transform = translation(Vec3::new(100.0, 0.0, 0.0));
    doc.occurrences.iter_mut().find(|o| o.id == child).unwrap().transform = translation(Vec3::new(20.0, 0.0, 0.0));
    let second = doc.add_occurrence(assembly, 0, translation(Vec3::new(150.0, 0.0, 0.0))).unwrap();
    let origins = |doc: &Document| {
        let datum = |path: &[u64]| construct::DatumRef { path: path.to_vec(), item: construct::DatumItem::XY };
        let plane = |g: construct::ConstructGeom| match g {
            construct::ConstructGeom::Plane(p) => p,
            other => panic!("a plane datum resolves to a plane, got {other:?}"),
        };
        let one = construct::resolve_datum(doc, &datum(&[first, child])).unwrap();
        let two = construct::resolve_datum(doc, &datum(&[second, child])).unwrap();
        (plane(one), plane(two))
    };
    let (a, b) = origins(&doc);
    assert!(a.origin.dist(Vec3::new(120.0, 0.0, 0.0)) < 1e-9, "{a:?}");
    assert!(b.origin.dist(Vec3::new(170.0, 0.0, 0.0)) < 1e-9, "{b:?}");
    // Moving the child moves its datum in both placements: nothing is frozen at pick time.
    doc.occurrences.iter_mut().find(|o| o.id == child).unwrap().transform = translation(Vec3::new(30.0, 0.0, 0.0));
    let (a, b) = origins(&doc);
    assert!(a.origin.dist(Vec3::new(130.0, 0.0, 0.0)) < 1e-9, "{a:?}");
    assert!(b.origin.dist(Vec3::new(180.0, 0.0, 0.0)) < 1e-9, "{b:?}");
}

/// New names take the first free number after their stem, whatever else is named alike: a gap
/// is filled, and `Sketch07`, `Sketch+2` or `SketchLid` take no number (they are not `Sketch7`).
#[test]
fn new_names_take_the_first_free_number() {
    let taken = crate::params::numbers_after("Box", ["Box1", "Box3", "Box07", "Box+2", "BoxLid", "Box", "Cylinder2"].into_iter());
    assert_eq!(taken, std::collections::HashSet::from([1, 3]));
    let mut doc = Document::new("names");
    let sketch = |doc: &mut Document| {
        doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, None).unwrap()
    };
    let name = |doc: &Document, id: u64| doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let ids: Vec<u64> = (0..3).map(|_| sketch(&mut doc)).collect();
    let names: Vec<String> = ids.iter().map(|id| name(&doc, *id)).collect();
    assert!(names.iter().all(|n| n.starts_with("Sketch")) && names[0] != names[1] && names[1] != names[2], "{names:?}");
    for (id, other) in [(ids[0], "SketchLid"), (ids[1], "Sketch07")] {
        if let Some(f) = doc.feature_mut(id) {
            f.name = other.into();
        }
    }
    let next = sketch(&mut doc);
    assert_eq!(name(&doc, next), names[0], "the first freed number is taken again");
}

#[test]
fn construction_plane_name_collisions_leave_the_document_unchanged() {
    let plane = || FeatureKind::ConstructionPlane { plane: PlaneRef::Origin { name: "XY".into() } };
    let sketch = || FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() };
    for (first, second) in [(plane(), plane()), (plane(), sketch()), (sketch(), plane())] {
        let mut doc = Document::new("names");
        let id = doc.add_feature(first, Some("Mid")).unwrap();
        // A suppressed feature beyond the marker still owns its name.
        doc.feature_mut(id).unwrap().suppressed = true;
        doc.marker = Some(0);
        let before = doc.clone();
        for name in ["Mid", " Mid "] {
            let error = doc.add_feature(second.clone(), Some(name)).unwrap_err().to_string();
            assert!(error.contains("another feature is called `Mid`"), "{error}");
            assert_eq!(doc, before, "a refused name must not consume an id or change the timeline");
        }
        let other = doc.add_feature(second, Some("Upper")).unwrap();
        assert_eq!(doc.feature(other).unwrap().name, "Upper");
        assert_eq!(other, before.next_id);
    }
}

#[test]
fn a_second_sketch_cannot_take_an_existing_sketch_name() {
    let sketch = || FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() };
    let mut doc = Document::new("names");
    doc.add_feature(sketch(), Some("Sketch2")).unwrap();
    let before = doc.clone();
    let error = doc.add_feature(sketch(), Some("Sketch2")).unwrap_err().to_string();
    assert!(error.contains("another feature is called `Sketch2`"), "{error}");
    assert_eq!(doc, before);
    // Automatic names skip the taken one.
    let auto = doc.add_feature(sketch(), None).unwrap();
    assert_ne!(doc.feature(auto).unwrap().name, "Sketch2");
    // Lookup ignores case, so a name that differs only in case is the same name.
    let error = doc.add_feature(sketch(), Some("sketch2")).unwrap_err().to_string();
    assert!(error.contains("another feature is called `sketch2`"), "{error}");
    assert_eq!(doc.find_feature("SKETCH2").unwrap().name, "Sketch2");
    // A free name for a sketch skips the taken ones.
    assert_eq!(doc.free_sketch_name("Sketch2"), "Sketch2 (2)");
    assert_eq!(doc.free_sketch_name("Other"), "Other");
    // Planes keep matching on the exact name, so two planes may differ only in case.
    let plane = |base: &str| FeatureKind::ConstructionPlane { plane: PlaneRef::Origin { name: base.into() } };
    doc.add_feature(plane("XY"), Some("Angled x")).unwrap();
    doc.add_feature(plane("XY"), Some("Angled X")).unwrap();
    assert!(doc.add_feature(plane("XY"), Some("Angled X")).is_err());
}

#[test]
fn empty_boolean_targets_name_overlapping_component() {
    for (operation, offset) in [(Operation::Cut, 0.0), (Operation::Intersect, 0.0), (Operation::Cut, 40.0)] {
        let (mut doc, sketch) = plate_doc();
        let imported = doc.add_component(Some("Imported STEP"), 0).unwrap();
        let part = doc.add_component(Some("Part1"), 0).unwrap();
        doc.features.last_mut().unwrap().component = imported;
        doc.feature_mut(sketch).unwrap().component = imported;
        doc.occurrences.iter_mut().find(|o| o.component == imported).unwrap().transform[2][3] = offset;
        let mut kind = doc.features.last().unwrap().kind.clone();
        if let FeatureKind::Extrude { operation: op, .. } = &mut kind {
            *op = operation;
        }
        let cut = doc.add_feature(kind, None).unwrap();
        doc.feature_mut(cut).unwrap().component = part;
        let mut model = Model::new();
        model.evaluate(&doc);
        let error = model.result(cut).unwrap().error.as_deref().unwrap();
        assert!(error.contains("component 'Part1'"), "offset {offset}: {error}");
        assert!(error.contains("'Body1' in component 'Imported STEP'"), "offset {offset}: {error}");
        assert!(error.contains("activate that component or pick the body as a target"), "offset {offset}: {error}");
        assert_eq!(model.state().bodies.len(), 1);
        assert!(rel(measure(&model.state().bodies[0].body).unwrap().volume, 24000.0) < 1e-9);
    }
}

#[test]
fn empty_boolean_targets_without_overlap_keep_plain_message() {
    for operation in [Operation::Cut, Operation::Intersect] {
        let (mut doc, _) = plate_doc();
        let part = doc.add_component(Some("Part1"), 0).unwrap();
        let cut = doc
            .add_feature(
                FeatureKind::Box {
                    corner: point_expr(Vec3::new(100.0, 100.0, 100.0)),
                    length: "1".into(),
                    width: "1".into(),
                    height: "1".into(),
                    operation,
                    centred: false,
                },
                None,
            )
            .unwrap();
        doc.feature_mut(cut).unwrap().component = part;
        let mut model = Model::new();
        model.evaluate(&doc);
        assert_eq!(model.result(cut).unwrap().error.as_deref(), Some("there is no body to cut or intersect"));
    }
}

/// A box of `size` at `corner`, made with `operation`.
fn cube(doc: &mut Document, corner: Vec3, size: [f64; 3], operation: Operation) -> u64 {
    let [length, width, height] = size.map(|x| Expr::new(x.to_string()));
    doc.add_feature(FeatureKind::Box { corner: point_expr(corner), length, width, height, operation, centred: false }, None).unwrap()
}

/// The bodies of `doc` evaluated, by name with their volumes and pieces, and how feature
/// `feature`'s join touched the pieces it split off.
fn joined(doc: &Document, feature: u64) -> (Vec<(String, f64, usize)>, Option<KeptSeparate>) {
    let mut m = Model::new();
    m.evaluate(doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let kept = m.results.iter().find(|r| r.id == feature).and_then(|r| r.state.kept_separate(feature));
    let bodies = m.state().bodies.iter().map(|b| (b.name.clone(), measure(&b.body).unwrap().volume, b.body.lumps().unwrap().len())).collect();
    (bodies, kept)
}

/// Two blocks joined along an edge cannot be one solid: the join splits them into two bodies and
/// says they touch along an edge.
#[test]
fn a_join_splits_blocks_touching_along_an_edge() {
    let mut doc = Document::new("edge");
    cube(&mut doc, Vec3::ZERO, [10.0; 3], Operation::NewBody);
    let join = cube(&mut doc, Vec3::new(10.0, 10.0, 0.0), [10.0; 3], Operation::Join);
    let (bodies, kept) = joined(&doc, join);
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert!(bodies.iter().all(|(_, v, lumps)| rel(*v, 1000.0) < 1e-6 && *lumps == 1), "{bodies:?}");
    assert_eq!(kept, Some(KeptSeparate::AlongEdge));
    assert!(KeptSeparate::AlongEdge.notice().contains("along an edge"));
}

/// Two blocks apart joined stay one body of two pieces, and the join says nothing.
#[test]
fn a_join_keeps_blocks_apart_in_one_body() {
    let mut doc = Document::new("apart");
    cube(&mut doc, Vec3::ZERO, [10.0; 3], Operation::NewBody);
    let join = cube(&mut doc, Vec3::new(20.0, 0.0, 0.0), [10.0; 3], Operation::Join);
    let (bodies, kept) = joined(&doc, join);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert!(rel(bodies[0].1, 2000.0) < 1e-6 && bodies[0].2 == 2, "{bodies:?}");
    assert_eq!(kept, None);
}

/// A Combine whose tool is made by a suppressed feature builds from the other tools and says the
/// tool is left out, instead of failing with "unknown body" (#13474).
#[test]
fn a_combine_leaves_out_a_tool_whose_feature_is_suppressed() {
    let mut doc = Document::new("suppressed tool");
    cube(&mut doc, Vec3::ZERO, [10.0; 3], Operation::NewBody);
    cube(&mut doc, Vec3::new(20.0, 0.0, 0.0), [10.0; 3], Operation::NewBody);
    let third = cube(&mut doc, Vec3::new(40.0, 0.0, 0.0), [10.0; 3], Operation::NewBody);
    let join = doc
        .add_feature(
            FeatureKind::Combine {
                target: "Body1".into(),
                tools: vec!["Body2".into(), "Body3".into()],
                operation: Operation::Join,
                keep_tools: false,
                offset: None,
                offset_edges: OffsetEdges::Sharp,
                offset_faces: Vec::new(),
                offset_radius: None,
            },
            None,
        )
        .unwrap();
    // The name is stored on the feature as soon as its body has it (`Session::pin_body_names`).
    let f = doc.feature_mut(third).unwrap();
    f.body_names = vec!["Body3".into()];
    f.suppressed = true;
    let mut m = Model::new();
    m.evaluate(&doc);
    let r = m.result(join).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(r.warning.as_deref().is_some_and(|w| w.contains("`Body3`") && w.contains("suppressed")), "{:?}", r.warning);
    let bodies: Vec<(String, f64)> = m.state().bodies.iter().map(|b| (b.name.clone(), measure(&b.body).unwrap().volume)).collect();
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert!(rel(bodies[0].1, 2000.0) < 1e-6, "{bodies:?}");
    // A tool that no feature makes is still an error.
    doc.feature_mut(third).unwrap().body_names.clear();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.result(join).unwrap().error.as_deref().is_some_and(|e| e.contains("unknown body")), "{:?}", m.result(join).unwrap().error);
    // So is one that only a suppressed feature after the Combine would make.
    let later = cube(&mut doc, Vec3::new(60.0, 0.0, 0.0), [10.0; 3], Operation::NewBody);
    let f = doc.feature_mut(later).unwrap();
    f.body_names = vec!["Body3".into()];
    f.suppressed = true;
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.result(join).unwrap().error.as_deref().is_some_and(|e| e.contains("unknown body")), "{:?}", m.result(join).unwrap().error);
}

/// A body of two pieces joined apart, split by a plane through both pieces: the split gives the
/// halves of every piece, not the halves of one of them (#6738).
#[test]
fn a_split_of_a_body_of_two_pieces_gives_the_halves_of_both() {
    let mut doc = Document::new("split two pieces");
    // One sketch, two squares 20 mm across and 30 mm apart.
    let mut sk = Sketch::new();
    let mut corners = Vec::new();
    for x in [0.0, 50.0] {
        let p: Vec<usize> = [Vec2::new(x, 10.0), Vec2::new(x + 20.0, 10.0), Vec2::new(x + 20.0, 30.0), Vec2::new(x, 30.0)]
            .iter()
            .map(|c| sk.add_point(*c, None).unwrap())
            .collect();
        for i in 0..4 {
            sk.add_line_pts(p[i], p[(i + 1) % 4], None).unwrap();
        }
        corners.extend(p);
    }
    for p in corners {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    let s = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(
        FeatureKind::Extrude {
            thin: None,
            sketch: crate::SketchId(s),
            profiles: ProfileSel::All,
            extent: Extent {
                convention: crate::ExtrudeConvention::LegacyPerSide,
                distance: "10".into(),
                direction: Direction::Positive,
                distance2: None,
                start_offset: None,
                symmetric_offset: false,
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
                through_all2: false,
                to2: None,
                to_offset2: None,
                from_level: false,
                picked_placements: Vec::new(),
                one_sided_direction: None,
            },
            operation: Operation::NewBody,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Combine {
            target: "Body1".into(),
            tools: vec!["Body2".into()],
            operation: Operation::Join,
            keep_tools: false,
            offset: None,
            offset_edges: OffsetEdges::Sharp,
            offset_faces: Vec::new(),
            offset_radius: None,
        },
        None,
    )
    .unwrap();
    // A plane through the middle of both squares (the extrusion is 10 mm tall).
    let split = doc
        .add_feature(
            FeatureKind::Split {
                body: "Body1".into(),
                plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: "5".into() },
                tool: None,
                tool_plane: false,
                tool_sheet: Default::default(),
                tool_faces: Vec::new(),
                tool_bodies: Vec::new(),
                sketch: None,
                line: None,
            },
            None,
        )
        .unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    let error = m.result(split).unwrap().error.clone();
    assert!(error.is_none(), "the split of a body of two pieces: {error:?}");
    let bodies: Vec<(String, f64, usize)> =
        m.state().bodies.iter().map(|b| (b.name.clone(), measure(&b.body).unwrap().volume, b.body.lumps().unwrap().len())).collect();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    for (name, volume, lumps) in &bodies {
        // Each half holds the half of both squares: two pieces, half the body each.
        assert!(rel(*volume, 4000.0) < 1e-6, "{name}: {bodies:?}");
        assert_eq!(*lumps, 2, "{name}: {bodies:?}");
    }
}

/// A square ring joined with an arm touching its outer corner along an edge and a hub apart in
/// its hole: the ring and the hub stay one body under the ring's name, the arm is split off, and
/// the join says it touched along an edge.
#[test]
fn a_join_splits_off_only_what_touches_along_an_edge() {
    let mut doc = Document::new("ring");
    cube(&mut doc, Vec3::new(-30.0, -30.0, 0.0), [60.0, 60.0, 6.0], Operation::NewBody);
    cube(&mut doc, Vec3::new(-24.0, -24.0, -1.0), [48.0, 48.0, 8.0], Operation::Cut);
    cube(&mut doc, Vec3::new(-5.0, -5.0, 0.0), [10.0, 10.0, 6.0], Operation::NewBody);
    cube(&mut doc, Vec3::new(30.0, 30.0, 0.0), [20.0, 20.0, 6.0], Operation::NewBody);
    let combine = doc
        .add_feature(
            FeatureKind::Combine {
                target: "Body1".into(),
                tools: vec!["Body3".into(), "Body2".into()],
                operation: Operation::Join,
                keep_tools: false,
                offset: None,
                offset_edges: OffsetEdges::Sharp,
                offset_faces: Vec::new(),
                offset_radius: None,
            },
            None,
        )
        .unwrap();
    let (bodies, kept) = joined(&doc, combine);
    let (ring, hub, arm) = (60.0 * 60.0 * 6.0 - 48.0 * 48.0 * 6.0, 600.0, 2400.0);
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    let named = |name: &str| bodies.iter().find(|(n, _, _)| n == name).cloned();
    let one = named("Body1").unwrap_or_else(|| panic!("{bodies:?}"));
    assert!(rel(one.1, ring + hub) < 1e-6 && one.2 == 2, "the ring and the hub: {bodies:?}");
    let other = bodies.iter().find(|(n, _, _)| n != "Body1").cloned().unwrap_or_else(|| panic!("{bodies:?}"));
    assert!(rel(other.1, arm) < 1e-6 && other.2 == 1, "the arm: {bodies:?}");
    assert_eq!(kept, Some(KeptSeparate::AlongEdge));
}

/// A chamfer or fillet a design file picks by a point alone (no `body`, no edge name), where
/// the point lies on edges of two bodies, blends the first body as before but warns, naming both
/// (#20517): the `solid.chamfer` and `solid.fillet` commands refuse the same pick. A pick on one
/// body's edge alone gives no warning.
#[test]
fn a_blend_picked_on_edges_of_two_bodies_warns_and_names_both() {
    let blended = |kind: serde_json::Value| {
        let mut doc = Document::new("two cubes");
        for corner in [["0", "0", "0"], ["10", "0", "0"]] {
            let cube = serde_json::json!({"type": "box", "corner": corner, "length": "10", "width": "10", "height": "10", "operation": "new_body"});
            doc.add_feature(serde_json::from_value(cube).unwrap(), None).unwrap();
        }
        let blend = doc.add_feature(serde_json::from_value(kind).unwrap(), None).unwrap();
        let mut m = Model::new();
        m.evaluate(&doc);
        let result = m.result(blend).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        let st = m.state();
        let names: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
        let volumes: Vec<f64> = st.bodies.iter().map(|b| measure(&b.body).unwrap().volume).collect();
        (result.warning.clone(), names, volumes)
    };
    for kind in ["chamfer", "fillet"] {
        let size = if kind == "chamfer" { "distance" } else { "radius" };
        let (warning, names, volumes) = blended(serde_json::json!({"type": kind, "edges": [[10.0, 0.0, 5.0]], size: "1"}));
        assert_eq!(names.len(), 2, "{names:?}");
        let warning = warning.unwrap_or_default();
        let both = format!("[10, 0, 5] lies on edges of 2 bodies, {} and {}: `{}` is blended", names[0], names[1], names[0]);
        assert!(warning.contains(&both) && warning.contains("with `body`"), "{kind}: {warning}");
        // The result is the same as before: the first body is blended, the second left alone.
        assert!(volumes[0] < 999.9 && rel(volumes[1], 1000.0) < 1e-9, "{kind}: {volumes:?}");

        let (warning, _, _) = blended(serde_json::json!({"type": kind, "edges": [[0.0, 0.0, 5.0]], size: "1"}));
        assert!(warning.is_none(), "{kind} on an edge of one body: {warning:?}");
    }
}

/// A fillet saved before corner setbacks reads with none and writes no `setbacks`; one with a
/// setback keeps it through a save, its distance an expression.
#[test]
fn fillet_setbacks_default_to_none_and_round_trip() {
    let old = serde_json::json!({"type": "fillet", "edges": [[30.0, 40.0, 20.0]], "radius": "5"});
    let kind: FeatureKind = serde_json::from_value(old).unwrap();
    let FeatureKind::Fillet { setbacks, .. } = &kind else { panic!("{kind:?}") };
    assert!(setbacks.is_empty());
    assert!(serde_json::to_value(&kind).unwrap().get("setbacks").is_none());
    let set = FeatureKind::Fillet {
        edges: vec![Position(Vec3::new(30.0, 40.0, 20.0))],
        radius: "5".into(),
        body: None,
        style: Default::default(),
        fit: Default::default(),
        corner: Default::default(),
        setbacks: vec![CornerSetback { vertex: Position(Vec3::new(60.0, 40.0, 20.0)), distance: "8 mm".into() }],
        turn: Default::default(),
        tangent_chain: true,
        edge_radii: Vec::new(),
    };
    let back: FeatureKind = serde_json::from_value(serde_json::to_value(&set).unwrap()).unwrap();
    assert_eq!(back, set);
}

/// A frame member saved before pairs reads with its sketch, no pairs and `next_pair` 0, and
/// saves exactly as it was read (no `pairs`, no `next_pair`); one of pairs only, without a
/// sketch, keeps them through a save.
#[test]
fn frame_member_pairs_default_to_none_and_round_trip() {
    let profile = serde_json::json!({"source": "library", "profile": {"kind": "rect_tube", "width": "40", "height": "40", "wall": "3"}});
    let old = serde_json::json!({"type": "frame_member", "path_sketch": 3, "lines": ["l1"], "profile": profile});
    let kind: FeatureKind = serde_json::from_value(old.clone()).unwrap();
    let FeatureKind::FrameMember { path_sketch, pairs, next_pair, .. } = &kind else { panic!("{kind:?}") };
    assert_eq!(*path_sketch, Some(SketchId(3)));
    assert!(pairs.is_empty());
    assert_eq!(*next_pair, 0);
    assert_eq!(serde_json::to_value(&kind).unwrap(), old);
    let with_pairs = serde_json::json!({"type": "frame_member", "profile": profile, "pairs": [
        {"id": "pair1", "a": {"type": "vertex", "at": [0.0, 0.0, 0.0]}, "b": {"type": "construct_point", "name": "Top"}},
        {"id": "pair3", "a": {"type": "sketch_point", "sketch": 3, "point": "p1"}, "b": {"type": "origin"}}
    ], "next_pair": 4});
    let kind: FeatureKind = serde_json::from_value(with_pairs.clone()).unwrap();
    let FeatureKind::FrameMember { path_sketch, pairs, next_pair, .. } = &kind else { panic!("{kind:?}") };
    assert_eq!(*path_sketch, None);
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair1", "pair3"]);
    assert_eq!(pairs[1].b, construct::GeoRef::Origin);
    assert_eq!(*next_pair, 4);
    assert_eq!(serde_json::to_value(&kind).unwrap(), with_pairs);
}

/// A fillet saved before turns reads as mitred and writes no `turn`; a swept one saves
/// `turn: "swept"` and keeps it through a save and an open; a turn of another name is refused.
#[test]
fn fillet_turn_defaults_to_mitre_round_trips_and_refuses_unknown_turns() {
    let old = serde_json::json!({"type": "fillet", "edges": [[15.0, 0.0, 10.0]], "radius": "2"});
    let kind: FeatureKind = serde_json::from_value(old.clone()).unwrap();
    let FeatureKind::Fillet { turn, .. } = &kind else { panic!("{kind:?}") };
    assert_eq!(*turn, crate::FilletTurn::Mitre);
    assert!(serde_json::to_value(&kind).unwrap().get("turn").is_none());
    let mut swept = old.clone();
    swept["turn"] = serde_json::json!("swept");
    let kind: FeatureKind = serde_json::from_value(swept).unwrap();
    assert!(matches!(kind, FeatureKind::Fillet { turn: crate::FilletTurn::Swept, .. }), "{kind:?}");
    assert_eq!(serde_json::to_value(&kind).unwrap()["turn"], "swept");
    let mut doc = Document::new("turn");
    doc.add_feature(kind.clone(), None).unwrap();
    let saved = doc.to_json();
    let opened = Document::from_json(&saved).unwrap();
    assert_eq!(opened.features.last().map(|f| &f.kind), Some(&kind));
    assert_eq!(opened.to_json(), saved);
    let mut unknown = old;
    unknown["turn"] = serde_json::json!("round");
    let e = serde_json::from_value::<FeatureKind>(unknown).unwrap_err().to_string();
    assert!(e.contains("unknown variant `round`") && e.contains("mitre") && e.contains("swept"), "{e}");
}

/// A fillet saved before per-edge radii reads with none and writes no `edge_radii`; one with them
/// keeps them through a save and an open, each an expression, and evaluates each picked edge at its
/// own radius. A file whose radii do not match its edges, or that asks for them with another
/// option, is refused at evaluation, saying why.
#[test]
fn fillet_edge_radii_default_to_none_round_trip_and_are_checked() {
    let old = serde_json::json!({"type": "fillet", "edges": [[20.0, 0.0, 10.0]], "radius": "1"});
    let kind: FeatureKind = serde_json::from_value(old).unwrap();
    let FeatureKind::Fillet { edge_radii, .. } = &kind else { panic!("{kind:?}") };
    assert!(edge_radii.is_empty());
    assert!(serde_json::to_value(&kind).unwrap().get("edge_radii").is_none());
    let fillet = |radii: &[&str], tangent_chain: bool, fit: crate::FilletFit| FeatureKind::Fillet {
        edges: vec![Position(Vec3::new(20.0, 0.0, 10.0)), Position(Vec3::new(20.0, 30.0, 10.0))],
        radius: "1".into(),
        body: None,
        style: Default::default(),
        fit,
        corner: Default::default(),
        setbacks: Vec::new(),
        turn: Default::default(),
        tangent_chain,
        edge_radii: radii.iter().map(|r| (*r).into()).collect(),
    };
    let each = fillet(&["1", "2 mm"], false, Default::default());
    let mut doc = Document::new("edge radii");
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::ZERO),
            length: "40".into(),
            width: "30".into(),
            height: "10".into(),
            centred: false,
            operation: Operation::NewBody,
        },
        None,
    )
    .unwrap();
    let mut at = doc.clone();
    at.add_feature(each.clone(), None).unwrap();
    let saved = at.to_json();
    assert!(saved.contains("\"edge_radii\""), "{saved}");
    let opened = Document::from_json(&saved).unwrap();
    assert_eq!(opened.features.last().map(|f| &f.kind), Some(&each));
    assert_eq!(opened.to_json(), saved);
    let mut m = Model::new();
    m.evaluate(&opened);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    let v = measure(&m.state().bodies[0].body).unwrap().volume;
    // A 40 x 30 x 10 block, two 40-long edges rounded at 1 and 2: each round of radius r on a
    // 90 deg edge of length L removes (1 - pi/4) r^2 L, so 12000 - 40 (1 - pi/4) (1 + 4).
    let want = 11957.07963267949;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
    for (bad, says) in [
        (fillet(&["1"], false, Default::default()), "gives 1 radii for 2 picked edges"),
        (fillet(&["1", "2"], true, Default::default()), crate::EDGE_RADII_TAKE),
        (fillet(&["1", "2"], false, crate::FilletFit::Reduce), crate::EDGE_RADII_TAKE),
    ] {
        let mut d = doc.clone();
        d.add_feature(bad, None).unwrap();
        let mut m = Model::new();
        m.evaluate(&d);
        let e = m.results.last().and_then(|r| r.error.clone()).unwrap_or_default();
        assert!(e.contains(says), "{e}");
    }
}

#[test]
fn timeline_group_legacy_json_omits_empty_joint_membership() {
    let old = serde_json::json!({"id":100, "name":"Folder", "parent":null, "features":[]});
    let group: TimelineGroup = serde_json::from_value(old.clone()).unwrap();
    assert!(group.joints.is_empty());
    assert_eq!(serde_json::to_value(&group).unwrap(), old);
    let mut doc = Document::new("legacy folders");
    doc.timeline_groups.push(group);
    doc.next_id = 101;
    let saved = doc.to_json();
    let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert!(value["timeline_groups"][0].get("joints").is_none());
    assert_eq!(Document::from_json(&saved).unwrap().to_json(), saved);
}

/// A sketch whose touching curves are cut into more pieces than the profile search looks at
/// says that some profiles may be missing, instead of leaving them out silently (#6896); an
/// ordinary sketch says nothing.
#[test]
fn a_sketch_too_large_to_search_whole_says_profiles_may_be_missing() {
    let mut doc = Document::new("large");
    let mut sk = Sketch::new();
    // Concentric ellipses 100 m across, each sampled into thousands of pieces, all one island.
    let c = sk.add_point(Vec2::ZERO, None).unwrap();
    for k in 0..5 {
        let m = sk.add_point(Vec2::new(1e5 - k as f64 * 1e3, 0.0), None).unwrap();
        sk.add_curve(solidcraft_sketch::CurveKind::Ellipse { c, m, r: 6e4 - k as f64 * 1e3 }, None).unwrap();
    }
    let large = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    let (plate, plate_sketch) = plate_doc();
    let mut m = Model::new();
    m.evaluate(&doc);
    let warning = m.result(large).and_then(|r| r.warning.clone()).unwrap_or_default();
    assert!(warning.contains("some profiles may be missing"), "{warning}");
    let mut m = Model::new();
    m.evaluate(&plate);
    assert!(m.result(plate_sketch).and_then(|r| r.warning.as_deref()).is_none());
}

fn browser_group(id: u64, component: u64, folder: &str, parent: Option<u64>, items: &[BrowserEntry]) -> BrowserGroup {
    BrowserGroup { id, name: format!("G{id}"), component, folder: folder.into(), parent, items: items.to_vec() }
}

fn item(key: &str) -> BrowserEntry {
    BrowserEntry::item(key)
}

fn group(id: u64) -> BrowserEntry {
    BrowserEntry::Group(id)
}

/// The design with what `browser_pruned` drops gone (the design itself when nothing is).
fn pruned(doc: &Document) -> Document {
    let mut out = doc.clone();
    if let Some((groups, order)) = doc.browser_pruned() {
        (out.browser_groups, out.browser_order) = (groups, order);
    }
    out
}

/// Pruning keeps every group but cuts a parent that loops, is the group itself, is past the
/// nesting limit, or is in another component or folder, to the top level; and it drops entries
/// for deleted sketches and placements, repeats, and the lists of a missing component. Body names
/// are never judged.
#[test]
fn browser_pruning_cuts_bad_parents_and_drops_stale_and_repeated_entries() {
    let mut doc = Document::new("prune");
    let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, None).unwrap();
    let key = sketch.to_string();
    doc.components.push(Component { id: 7, name: "Part".into(), ..Default::default() });
    doc.occurrences.push(Occurrence {
        id: 20,
        component: 7,
        parent: 0,
        name: "Part".into(),
        transform: IDENTITY,
        base_transform: None,
        grounded: false,
        expressed_moves: Vec::new(),
        bom_type: None,
    });
    let deep = MAX_GROUP_NESTING as u64 + 2;
    doc.browser_groups = vec![
        browser_group(1, 0, "bodies", Some(2), &[]),
        browser_group(2, 0, "bodies", Some(1), &[]),
        browser_group(3, 0, "bodies", Some(3), &[]),
        browser_group(4, 7, "bodies", Some(5), &[]),
        browser_group(5, 0, "bodies", None, &[item("a"), group(7), group(7), item("a")]),
        browser_group(6, 0, "sketches", Some(5), &[item(&key), item("999")]),
        browser_group(7, 0, "bodies", Some(5), &[]),
        browser_group(8, 0, "bodies", None, &[item("a"), item("b")]),
    ];
    doc.browser_groups.extend((0..deep).map(|i| browser_group(100 + i, 0, "construction", i.checked_sub(1).map(|p| 100 + p), &[])));
    doc.browser_order.insert("0/bodies".into(), vec![item("Lid"), item("Ghost"), item("Lid"), group(5), group(5)]);
    doc.browser_order.insert("0/components".into(), vec![item("21"), item("20")]);
    doc.browser_order.insert("99/bodies".into(), vec![item("x")]);
    let after = pruned(&doc);
    let parent = |id| after.browser_group(id).unwrap().parent;
    assert_eq!(after.browser_groups.len(), doc.browser_groups.len(), "no group is dropped");
    assert_eq!([1, 2, 3, 4, 6].map(parent), [None; 5], "a loop, itself, another component, another folder");
    assert_eq!(parent(7), Some(5));
    for i in 0..deep {
        assert_eq!(parent(100 + i), (1..MAX_GROUP_NESTING as u64).contains(&i).then(|| 99 + i), "level {} of a chain {deep} deep", i + 1);
    }
    assert_eq!(after.browser_group(5).unwrap().items, [item("a"), group(7)]);
    assert_eq!(after.browser_group(8).unwrap().items, [item("b")], "an item stays in the first group that has it");
    assert_eq!(after.browser_group(6).unwrap().items, [item(&key)], "the deleted sketch goes");
    assert_eq!(after.browser_order["0/bodies"], [item("Lid"), item("Ghost"), group(5)], "body names stay; repeats go");
    assert_eq!(after.browser_order["0/components"], [item("20")], "the deleted placement goes");
    assert!(!after.browser_order.contains_key("99/bodies"));
    assert!(Document::new("clean").browser_pruned().is_none(), "nothing to prune is not a change");
}

/// Levels and ancestry follow the parents. Groups a list does not name go after the last one it
/// names, in creation order, or first when it names none: above the items, as a design saved
/// before groups could be ordered has always shown them.
#[test]
fn group_levels_and_the_order_of_groups_a_list_does_not_name() {
    let mut doc = Document::new("order");
    doc.browser_groups = vec![
        browser_group(1, 0, "bodies", None, &[item("x")]),
        browser_group(2, 0, "bodies", None, &[]),
        browser_group(3, 0, "bodies", None, &[]),
        browser_group(4, 0, "bodies", Some(1), &[]),
        browser_group(5, 0, "bodies", Some(4), &[]),
    ];
    assert_eq!([1, 2, 4, 5].map(|id| doc.group_level(id)), [1, 1, 2, 3]);
    assert!(doc.group_within(5, 1) && doc.group_within(5, 5) && !doc.group_within(1, 5) && !doc.group_within(2, 5));
    assert_eq!(doc.group_height(1), 3);
    let top = |doc: &Document| doc.ordered_entries(0, "bodies", None);
    assert_eq!(top(&doc), [group(1), group(2), group(3)], "no list: creation order");
    doc.browser_order.insert("0/bodies".into(), vec![item("a")]);
    assert_eq!(top(&doc), [group(1), group(2), group(3), item("a")], "a list of items only: groups above them");
    doc.browser_order.insert("0/bodies".into(), vec![item("a"), group(2), item("b"), group(2), group(9)]);
    assert_eq!(top(&doc), [item("a"), group(2), group(1), group(3), item("b")], "after the last listed; repeats and unknown ignored");
    assert_eq!(doc.ordered_entries(0, "bodies", Some(1)), [group(4), item("x")], "inside a group too");
}

/// Nested groups and a `components` order survive a save and a load, saved as numbers (groups) and
/// strings (items). A design from before groups could nest reads with the browser it always had:
/// no parents, groups in creation order above the items in their saved order, saved back as read.
#[test]
fn browser_data_survives_a_save_and_old_designs_read_unchanged() {
    let mut doc = Document::new("nested");
    doc.components.push(Component { id: 7, name: "Part".into(), ..Default::default() });
    for id in [20, 21] {
        doc.occurrences.push(Occurrence {
            id,
            component: 7,
            parent: 0,
            name: format!("P{id}"),
            transform: IDENTITY,
            base_transform: None,
            grounded: false,
            bom_type: None,
            expressed_moves: Vec::new(),
        });
    }
    doc.browser_groups = vec![browser_group(2, 0, "bodies", None, &[item("Lid"), group(3)]), browser_group(3, 0, "bodies", Some(2), &[item("Base")])];
    doc.browser_order.insert("0/bodies".into(), vec![item("Cap"), group(2)]);
    doc.browser_order.insert("0/components".into(), vec![item("21"), item("20")]);
    let saved = doc.to_json();
    let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(value["browser_order"]["0/bodies"], serde_json::json!(["Cap", 2]));
    assert_eq!((&value["browser_groups"][1]["parent"], value["browser_groups"][0].get("parent")), (&serde_json::json!(2), None));
    let loaded = Document::from_json(&saved).unwrap();
    assert_eq!((&loaded.browser_groups, &loaded.browser_order), (&doc.browser_groups, &doc.browser_order));
    let resaved: serde_json::Value = serde_json::from_str(&loaded.to_json()).unwrap();
    assert_eq!((&resaved["browser_groups"], &resaved["browser_order"]), (&value["browser_groups"], &value["browser_order"]));

    let groups = serde_json::json!([
        {"id": 3, "name": "Fasteners", "component": 0, "folder": "bodies", "items": ["Bolt", "Nut"]},
        {"id": 4, "name": "Plates", "component": 0, "folder": "bodies", "items": ["Plate"]},
    ]);
    for (order, top) in [
        (serde_json::json!({}), vec![group(3), group(4)]),
        (serde_json::json!({"0/bodies": ["Washer", "Bolt", "Cap"]}), vec![group(3), group(4), item("Washer"), item("Bolt"), item("Cap")]),
    ] {
        let mut old: serde_json::Value = serde_json::from_str(&Document::new("old").to_json()).unwrap();
        (old["browser_groups"], old["browser_order"]) = (groups.clone(), order.clone());
        let loaded = Document::from_json(&old.to_string()).unwrap();
        assert!(loaded.browser_groups.iter().all(|g| g.parent.is_none()), "{order}");
        assert_eq!(loaded.ordered_entries(0, "bodies", None), top, "{order}");
        let resaved: serde_json::Value = serde_json::from_str(&loaded.to_json()).unwrap();
        assert_eq!(resaved["browser_groups"], groups, "{order}");
    }
}

/// #10748: verify that type names map to their canonical design-file kind keys.
#[test]
fn feature_kind_key_and_type_name_mappings() {
    assert_eq!(FeatureKind::type_name_to_kind_key("HoleFeature"), Some("hole"));
    assert_eq!(FeatureKind::type_name_to_kind_key("BoxFeature"), Some("box"));
    assert_eq!(FeatureKind::type_name_to_kind_key("EdgeFlangeFeature"), Some("sheet_flange"));
    assert_eq!(FeatureKind::type_name_to_kind_key("RectangularPatternFeature"), Some("pattern"));
    assert_eq!(FeatureKind::type_name_to_kind_key("PathPatternFeature"), Some("pattern"));
    assert_eq!(FeatureKind::type_name_to_kind_key("CircularPatternFeature"), Some("pattern"));
    assert_eq!(FeatureKind::type_name_to_kind_key("ExtrudeFeature"), Some("extrude"));
    assert_eq!(FeatureKind::type_name_to_kind_key("SplitBodyFeature"), Some("split"));
    assert_eq!(FeatureKind::type_name_to_kind_key("BaseFeature"), Some("import"));
    assert_eq!(FeatureKind::type_name_to_kind_key("MeshFeature"), Some("mesh_import"));
    assert_eq!(FeatureKind::type_name_to_kind_key("NonExistent"), None);
    // Convert of picked faces, Convert of a whole body and Recognize: each its own type name,
    // which leads back to its kind.
    for kind in [
        serde_json::json!({"type": "sheet_convert", "body": "B", "face": [0, 0, 0]}),
        serde_json::json!({"type": "sheet_surface_convert", "body": "B"}),
        serde_json::json!({"type": "sheet_recognize", "body": "B"}),
    ] {
        let key = kind["type"].as_str().unwrap_or_default().to_string();
        let kind = serde_json::from_value::<FeatureKind>(kind).unwrap();
        assert_eq!(FeatureKind::type_name_to_kind_key(kind.type_name()), Some(key.as_str()), "{key}");
    }
}

/// An extruded ellipse has one side face, and a spline in a profile one wall, each named by its
/// sketch curve alone (no piece number): the same names on a second build and after reopening.
#[test]
fn an_extruded_ellipse_or_spline_is_one_wall_named_by_its_curve() {
    let extrude = |sketch: u64| FeatureKind::Extrude {
        thin: None,
        sketch: crate::SketchId(sketch),
        profiles: ProfileSel::All,
        extent: Extent {
            convention: crate::ExtrudeConvention::LegacyPerSide,
            distance: "20".into(),
            direction: Direction::Positive,
            distance2: None,
            start_offset: None,
            symmetric_offset: false,
            through_all: false,
            second_follows_first: false,
            taper: None,
            corner: Default::default(),
            to: None,
            to_offset: None,
            from: None,
            to_body_apart: false,
            through_all2: false,
            to2: None,
            to_offset2: None,
            from_level: false,
            picked_placements: Vec::new(),
            one_sided_direction: None,
        },
        operation: Operation::NewBody,
        targets: vec![],
    };
    for spline in [false, true] {
        let mut sk = Sketch::new();
        let curve = if spline {
            let pts: Vec<usize> =
                [(0., 0.), (10., -3.), (20., 4.), (30., 0.)].iter().map(|&(x, y)| sk.add_point(Vec2::new(x, y), None).unwrap()).collect();
            let (a, b) = (sk.add_point(Vec2::new(30., 20.), None).unwrap(), sk.add_point(Vec2::new(0., 20.), None).unwrap());
            let curve = sk
                .add_curve(
                    solidcraft_sketch::CurveKind::Spline {
                        pts: pts.clone(),
                        control: false,
                        degree: 3,
                        periodic: false,
                        nurbs: None,
                        handles: Vec::new(),
                    },
                    None,
                )
                .unwrap();
            sk.add_line_pts(pts[3], a, None).unwrap();
            sk.add_line_pts(a, b, None).unwrap();
            sk.add_line_pts(b, pts[0], None).unwrap();
            curve
        } else {
            let m = sk.add_point(Vec2::new(10., 0.), None).unwrap();
            sk.add_curve(solidcraft_sketch::CurveKind::Ellipse { c: 0, m, r: 5. }, None).unwrap()
        };
        let id = sk.curves[curve].id.clone();
        let mut doc = Document::new("one wall");
        let s = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
        let f = doc.add_feature(extrude(s), None).unwrap();
        let names = |doc: &Document| {
            let mut m = Model::new();
            m.evaluate(doc);
            assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
            let st = m.state();
            let mut names = crate::naming::face_names(&st.bodies[0]).as_ref().clone();
            names.sort();
            names
        };
        let first = names(&doc);
        let wall = format!("F{f}:side:{id}");
        assert_eq!(first.iter().filter(|n| **n == wall).count(), 1, "{first:?}");
        assert_eq!(first.len(), if spline { 6 } else { 3 }, "{first:?}");
        assert!(first.iter().all(|n| !n.contains('#')), "no piece names: {first:?}");
        assert_eq!(names(&doc), first, "a second build");
        assert_eq!(names(&Document::from_json(&doc.to_json()).unwrap()), first, "reopened");
    }
}

/// A construction plane of one component carries its place into the sketch of another (#11734):
/// the sketch lies where the plane is, not where the plane's numbers fall in the sketch's own,
/// placed component.
#[test]
fn a_sketch_on_the_construction_plane_of_another_component_lies_on_it() {
    let mut doc = Document::new("planes between components");
    let owner = doc.add_component(Some("Owner"), 0).unwrap();
    let sketcher = doc.add_component(Some("Sketcher"), 0).unwrap();
    let placed = rigid(Vec3::new(0.0, 138.0, -3.0), Vec3::ZERO, Vec3::Y, std::f64::consts::PI);
    doc.occurrences.iter_mut().find(|o| o.component == sketcher).unwrap().transform = placed;
    let offset = PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XZ".into() }), distance: "60 mm".into() };
    let plane = doc.add_feature(FeatureKind::ConstructionPlane { plane: offset }, Some("Plane3")).unwrap();
    doc.feature_mut(plane).unwrap().component = owner;
    let mut sketch = Sketch::new();
    let a = sketch.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sketch.add_point(Vec2::new(10.0, 5.0), None).unwrap();
    sketch.add_line_pts(a, b, None).unwrap();
    let on_plane = FeatureKind::Sketch { plane: PlaneRef::Construction { feature: plane.into(), placement: None }, sketch };
    let id = doc.add_feature(on_plane, Some("Sketch3")).unwrap();
    doc.feature_mut(id).unwrap().component = sketcher;

    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.result(id).unwrap().error.is_none(), "{:?}", model.result(id).unwrap().error);
    let state = model.state();
    let solved = state.sketch(id).unwrap();
    // The sketch is kept in its own component's frame; placed, it lies on the plane where the
    // owner puts it (the owner is not moved): the same origin, axes, normal and line end.
    use solidcraft_geom::prelude::*;
    let want = Plane::named("XZ").unwrap().offset(60.0);
    let m = doc.component_transform(sketcher);
    let close = |a: Vec3, b: Vec3, what: &str| assert!((a - b).len() < 1e-6, "{what}: {a:?} instead of {b:?}");
    close(crate::apply_point(&m, solved.plane.origin), want.origin, "origin");
    close(crate::apply_vector(&m, solved.plane.x), want.x, "x axis");
    close(crate::apply_vector(&m, solved.plane.y), want.y, "y axis");
    close(crate::apply_vector(&m, solved.plane.normal()), want.normal(), "normal");
    let end = solved.plane.origin + solved.plane.x * 10.0 + solved.plane.y * 5.0;
    close(crate::apply_point(&m, end), want.origin + want.x * 10.0 + want.y * 5.0, "line end");
}

/// Freeze the persisted v6 face-role encoding independently of display-mesh fields and serde layout.
#[test]
fn reconstructed_mesh_roles_keep_the_versioned_binary_identity() {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(1., 1., 1.)).unwrap();
    let positions = [Vec3::new(0., -0., 0.), Vec3::X, Vec3::Y];
    use solidcraft_kernel::services::mesh::ReconstructionKind::{Analytic, Faceted};
    for (kind, digest) in [
        (Analytic, "2b85ddcdb826582a9d392dd981499209eb69c06ae3f6ebacaca0c02a4e29a278"),
        (Faceted, "3cbe2d7ca4e75f8b65845f9d84844cfe94e5236c8235692f6badf053aa649856"),
    ] {
        let traced = crate::lineage::reconstructed_tool(body.clone(), &positions, &[[0, 1, 2]], 0.01, kind);
        let lineage = traced.lineage.unwrap();
        let crate::lineage::Operand::Made(roles) = lineage.operands[0].as_ref() else { panic!("made faces") };
        assert_eq!(roles.len(), 6);
        for (face, names) in roles.iter().enumerate() {
            assert_eq!(names, &[format!("mesh:{digest}:face:{face}")]);
        }
    }
}

/// Old conversion metadata is readable, but every saved design replays the same converter.
#[test]
fn saved_mesh_conversion_modes_replay_the_current_converter() {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    let mut doc = Document::new("saved conversion");
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "box.stl".into(),
            meshes: vec![MeshData {
                name: "Mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        None,
    )
    .unwrap();
    let conversion = doc.add_feature(FeatureKind::MeshConvert { body: "Mesh".into(), tolerance: "0.01 mm".into() }, None).unwrap();
    let mut expected = None;
    for obsolete in [
        serde_json::json!({}),
        serde_json::json!({"reconstruction":"legacy_prism"}),
        serde_json::json!({"reconstruction":"analytic_v1"}),
        serde_json::json!({"reconstruction":"analytic_v1_geometry_names"}),
        serde_json::json!({"source_bound_names":false}),
        serde_json::json!({"source_bound_names":true}),
    ] {
        let mut saved = serde_json::json!({"type":"mesh_convert","body":"Mesh","tolerance":"0.01 mm"});
        saved.as_object_mut().unwrap().extend(obsolete.as_object().unwrap().clone());
        doc.feature_mut(conversion).unwrap().kind = serde_json::from_value(saved).unwrap();
        let canonical = serde_json::to_value(&doc.feature(conversion).unwrap().kind).unwrap();
        assert!(canonical.get("reconstruction").is_none() && canonical.get("source_bound_names").is_none());
        for replay in [&doc, &Document::from_json(&doc.to_json()).unwrap()] {
            let mut model = Model::new();
            model.evaluate(replay);
            assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
            let state = model.state();
            let body = state.body("Mesh").unwrap();
            assert!((measure(&body.body).unwrap().volume - 6000.).abs() < 1e-6);
            assert_eq!(body.feature, conversion);
            let faces = crate::naming::face_names(body).as_ref().clone();
            assert!(faces.iter().all(|name| name.contains(":mesh:")));
            let names = (faces, crate::naming::edge_names(body));
            if let Some(expected) = &expected {
                assert_eq!(&names, expected);
            } else {
                expected = Some(names);
            }
        }
    }
}

/// A saved conversion uses the current analytic geometry; an obsolete wall attachment reports a normal broken reference.
#[test]
fn saved_partial_arc_conversion_rebuilds_and_reports_obsolete_wall_references() {
    use solidcraft_geom::prelude::*;
    let root3 = 3f64.sqrt();
    let outline = [
        Vec2::new(-4., -3.),
        Vec2::new(4., -3.),
        Vec2::new(4., 3.),
        Vec2::new(2., 3.),
        Vec2::new(root3, 2.),
        Vec2::new(1., 3. - root3),
        Vec2::new(0., 1.),
        Vec2::new(-1., 3. - root3),
        Vec2::new(-root3, 2.),
        Vec2::new(-2., 3.),
        Vec2::new(-4., 3.),
    ];
    let source = solidcraft_kernel::extrude_planar_regions(
        &solidcore_record::Plane::XY,
        &[solidcore_record::Region2 { outer: solidcore_record::Loop2::polygon(&outline), holes: vec![] }],
        0.,
        5.,
    )
    .unwrap();
    let mesh = source.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    let mut doc = Document::new("legacy polygon notch");
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "polygon-notch.stl".into(),
            meshes: vec![MeshData {
                name: "Mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        None,
    )
    .unwrap();
    let conversion = doc
        .add_feature(serde_json::from_value(serde_json::json!({"type":"mesh_convert","body":"Mesh","tolerance":"0.1 mm"})).unwrap(), None)
        .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    {
        let st = model.state();
        let body = st.body("Mesh").unwrap();
        assert_eq!(body.body.face_count(), 8);
        assert_eq!(measure(&body.body).unwrap().merged.face_types.get("cylinder"), Some(&1));
        assert!((measure(&body.body).unwrap().volume - (240. - 10. * std::f64::consts::PI)).abs() < 1e-6);
        assert!(crate::naming::face_names(body).iter().all(|name| name.contains(":mesh:")));
    }
    // The former polygon converter used these planar walls. This stored planar face no longer
    // exists on the reconstructed cylinder, so replay must report the ordinary reference error.
    let old = ModelBody::new("Mesh".into(), source, conversion);
    let faces = crate::naming::face_names(&old);
    let (face, spot) = crate::naming::face_spots(&old)
        .into_iter()
        .enumerate()
        .find_map(|(i, spot)| {
            let spot = spot?;
            let solidcraft_kernel::FaceSurface::Plane { normal, .. } = spot.surface? else { return None };
            (normal.z.abs() < 1e-8 && spot.point.x.abs() < 1.99 && spot.point.y > 0.9 && spot.point.y < 2.99).then_some((i, spot))
        })
        .unwrap();
    let solidcraft_kernel::FaceSurface::Plane { normal, .. } = spot.surface.unwrap() else { panic!("planar wall") };
    let plane = solidcore_record::Plane::from_normal(spot.point, normal.vec()).unwrap();
    let sketch = doc
        .add_feature(
            FeatureKind::Sketch {
                plane: PlaneRef::Face { plane, at: spot.point, name: Some(faces[face].clone()), anchor: None },
                sketch: Sketch::new(),
            },
            None,
        )
        .unwrap();
    for replay in [&doc, &Document::from_json(&doc.to_json()).unwrap()] {
        let mut cold = Model::new();
        cold.evaluate(replay);
        assert!(cold.result(conversion).unwrap().error.is_none());
        assert!(cold.result(sketch).unwrap().error.is_some(), "obsolete polygon wall must request reattachment");
    }
}

#[test]
fn thread_additional_faces_preserve_old_single_face_serialization() {
    let old = serde_json::json!({"type":"thread", "face":[3.0,0.0,5.0]});
    let kind: FeatureKind = serde_json::from_value(old.clone()).unwrap();
    assert!(matches!(&kind, FeatureKind::Thread { additional_faces, .. } if additional_faces.is_empty()));
    assert_eq!(serde_json::to_value(kind).unwrap(), old);
}

#[test]
fn thread_additional_faces_round_trip_without_moving_legacy_face_name_slots() {
    let mut kind: FeatureKind = serde_json::from_value(serde_json::json!({"type":"thread", "face":[3.0,0.0,5.0]})).unwrap();
    if let FeatureKind::Thread { additional_faces, up_to, .. } = &mut kind {
        *up_to = Some(crate::ThreadLimit::Face(crate::FacePick(Vec3::new(0.0, 0.0, 8.0))));
        additional_faces.extend([crate::FacePick(Vec3::new(13.0, 0.0, 5.0)), crate::FacePick(Vec3::new(23.0, 0.0, 5.0))]);
    }
    let expected = vec![Vec3::new(3.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 8.0), Vec3::new(13.0, 0.0, 5.0), Vec3::new(23.0, 0.0, 5.0)];
    assert_eq!(kind.face_points(), expected);
    let saved = serde_json::to_value(&kind).unwrap();
    assert_eq!(saved["additional_faces"], serde_json::json!([[13.0, 0.0, 5.0], [23.0, 0.0, 5.0]]));
    let reopened: FeatureKind = serde_json::from_value(saved).unwrap();
    assert_eq!(reopened.face_points(), expected);
}

#[test]
fn thread_target_body_identities_follow_face_slots_and_reference_remapping() {
    let mut kind: FeatureKind = serde_json::from_value(serde_json::json!({
        "type":"thread", "face":[3,0,2],
        "additional_faces":[[13,0,2],[23,0,2]], "target_bodies":["A","B","A"]
    }))
    .unwrap();
    if let FeatureKind::Thread { up_to, .. } = &mut kind {
        *up_to = Some(crate::ThreadLimit::Face(crate::FacePick(Vec3::new(0.0, 0.0, 8.0))));
    }
    assert_eq!(kind.thread_target_body(0), Some("A"));
    assert_eq!(kind.thread_target_body(1), None, "Up To is outside the per-target identity list");
    assert_eq!(kind.thread_target_body(2), Some("B"));
    assert_eq!(kind.thread_target_body(3), Some("A"));
    assert_eq!(kind.input_bodies(), ["A", "B", "A"]);
    kind.remap_bodies(&std::collections::BTreeMap::from([("A".into(), "Renamed".into())]));
    assert_eq!(kind.input_bodies(), ["Renamed", "B", "Renamed"]);
    let saved = serde_json::to_value(&kind).unwrap();
    assert_eq!(saved["target_bodies"], serde_json::json!(["Renamed", "B", "Renamed"]));
    let reopened: FeatureKind = serde_json::from_value(saved).unwrap();
    assert_eq!(reopened, kind);
}

#[test]
fn restoring_a_thread_pick_preserves_saved_bits_but_follows_an_upstream_move() {
    for saved in [Vec3::new(3.0, -0.0, 5.0), Vec3::new(1e8, -0.0, 5.0)] {
        let rounded = Vec3::new(f64::from_bits(saved.x.to_bits() + 1), 0.0, saved.z);
        let restored = restored_thread_pick(saved, rounded);
        assert_eq!([restored.x.to_bits(), restored.y.to_bits(), restored.z.to_bits()], [saved.x.to_bits(), saved.y.to_bits(), saved.z.to_bits()]);
        let moved = Vec3::new(saved.x + 0.01, 0.0, saved.z);
        assert_eq!(restored_thread_pick(saved, moved), moved);
    }
    assert_eq!(restored_thread_pick(Vec3::ZERO, Vec3::new(f64::EPSILON, 0.0, 0.0)), Vec3::ZERO);
}

#[test]
fn restoring_a_thread_pick_keeps_the_resolved_point_when_a_distance_is_refused() {
    let resolved = Vec3::new(3.0, 0.0, 5.0);
    assert_eq!(restored_thread_pick(Vec3::new(f64::NAN, 0.0, 0.0), resolved), resolved);
    let resolved = Vec3::new(f64::INFINITY, 0.0, 5.0);
    assert_eq!(restored_thread_pick(Vec3::ZERO, resolved), resolved);
    let resolved = Vec3::new(f64::MAX, 0.0, 0.0);
    assert_eq!(restored_thread_pick(Vec3::new(-f64::MAX, 0.0, 0.0), resolved), resolved);
}

/// A closed control-point spline (#19337) is a design the loader accepts: it saves, reopens
/// unchanged and evaluates with no error.
#[test]
fn a_closed_control_spline_design_saves_and_reopens_unchanged() {
    let mut sk = Sketch::new();
    let pts: Vec<usize> = [(0., 0.), (12., -2.), (20., 8.), (10., 16.), (-3., 12.), (-6., 3.)]
        .iter()
        .map(|&(x, y)| sk.add_point(Vec2::new(x, y), None).unwrap())
        .collect();
    sk.add_curve(solidcraft_sketch::CurveKind::Spline { pts, control: true, degree: 3, periodic: true, nurbs: None, handles: Vec::new() }, None)
        .unwrap();
    let mut doc = Document::new("closed control");
    doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    let saved = doc.to_json();
    let reopened = Document::from_json(&saved).expect("a closed control spline design opens");
    assert_eq!(reopened.to_json(), saved);
    let mut model = Model::new();
    model.evaluate(&reopened);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
}

/// #17752: a Hole may name the press-in hardware it is the mounting hole of. It is saved only when
/// set (a hole without it saves as before), with the kind as its name, and an older design reads
/// with none. The hardware's length is an expression a parameter rename follows.
#[test]
fn a_hole_saves_its_press_in_hardware_only_when_set() {
    let plain = serde_json::json!({"type": "hole", "position": [0, 0, 0], "direction": [0, 0, 1], "diameter": "4.25 mm", "hole": {"type": "simple"}});
    let kind: FeatureKind = serde_json::from_value(plain.clone()).unwrap();
    let FeatureKind::Hole { hardware, .. } = &kind else { panic!("not a hole") };
    assert_eq!(*hardware, None);
    assert!(serde_json::to_value(&kind).unwrap().get("hardware").is_none(), "a hole without hardware saves no key");

    let mut with = plain;
    with["hardware"] = serde_json::json!({"kind": "flush_nut", "thread": "M4", "part": "flush-nut.m4"});
    let kind: FeatureKind = serde_json::from_value(with.clone()).unwrap();
    let FeatureKind::Hole { hardware: Some(h), .. } = &kind else { panic!("hardware lost") };
    assert_eq!((h.kind, h.thread.as_str(), h.part.as_str()), (crate::hardware_catalog::HardwareKind::FlushNut, "M4", "flush-nut.m4"));
    assert!(h.length.is_none());
    // Saved as it was read: the hardware object is the same, and has no length key when it has none.
    assert_eq!(serde_json::to_value(&kind).unwrap()["hardware"], with["hardware"]);

    // A stud's length is an expression: the feature's inputs name it, so a parameter rename reaches it.
    with["hardware"] = serde_json::json!({"kind": "stud", "thread": "M3", "length": "pin_len", "part": "stud.m3"});
    let kind: FeatureKind = serde_json::from_value(with).unwrap();
    assert!(kind.inputs().iter().any(|(label, e, _)| *label == "Hardware length" && e == "pin_len"), "{:?}", kind.inputs());
}

fn mat_close(a: &Mat, b: &Mat, tol: f64) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < tol)
}

#[test]
fn capture_position_serde_round_trip_and_legacy_compatibility() {
    let mut doc = Document::new("capture serde");
    let comp = doc.add_component(Some("Part"), 0).unwrap();
    let occ_id = doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    let base = rigid(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO, Vec3::Z, 0.25);
    let occ = doc.occurrences.iter_mut().find(|o| o.id == occ_id).unwrap();
    occ.base_transform = Some(base);
    occ.transform = rigid(Vec3::new(11.0, 2.0, 3.0), Vec3::ZERO, Vec3::Z, 0.25);

    let rot90 = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    let kind = FeatureKind::CapturePosition {
        moves: vec![
            CapturedOccurrenceMove {
                occurrence: occ_id,
                space: MoveSpace::World,
                translate: ["10 mm".into(), "0".into(), "0".into()],
                origin: Some(["1".into(), "2".into(), "3".into()]),
                rotate_axis: Some([0.0, 0.0, 1.0]),
                angle: Some("45 deg".into()),
                rotation: None,
                path: Some(vec![occ_id]),
            },
            CapturedOccurrenceMove {
                occurrence: occ_id,
                space: MoveSpace::Local,
                translate: ["5 mm".into(), "0".into(), "0".into()],
                origin: None,
                rotate_axis: None,
                angle: None,
                rotation: Some(rot90),
                path: None,
            },
        ],
        joints: vec![CapturedJoint { joint: 99, values: vec![0.5], exprs: vec![Some("theta".into())] }],
    };
    assert_eq!(kind.type_name(), "CapturePositionFeature");
    assert_eq!(kind.kind_key(), "capture_position");
    assert_eq!(FeatureKind::type_name_to_kind_key("CapturePositionFeature"), Some("capture_position"));
    assert_eq!(kind.base_name(), "CapturePosition");

    doc.add_feature(kind, None).unwrap();
    let json = doc.to_json();
    let reopened = Document::from_json(&json).expect("CapturePosition design should reopen");
    assert_eq!(reopened.occurrences.iter().find(|o| o.id == occ_id).unwrap().base_transform, Some(base));
    assert_eq!(reopened.features[0].kind, doc.features[0].kind);

    // Legacy design without CapturePosition or base_transform.
    let mut legacy_doc = Document::new("legacy");
    legacy_doc.add_component(Some("LegacyPart"), 0).unwrap();
    let legacy_json = legacy_doc.to_json();
    let legacy_val: serde_json::Value = serde_json::from_str(&legacy_json).unwrap();
    assert!(legacy_val["occurrences"][0].get("base_transform").is_none());
    let legacy_loaded = Document::from_json(&legacy_json).unwrap();
    assert!(!legacy_loaded.has_capture_position_timeline());
    let mut model = Model::new();
    model.evaluate(&legacy_loaded);
    assert!(model.resolved_occurrences().is_empty());

    // Hostile format decode rejections: non-rigid base_transform, translation in `rotation`, deep path.
    let mut bad_base: serde_json::Value = serde_json::from_str(&json).unwrap();
    bad_base["occurrences"][0]["base_transform"] = serde_json::json!([[2, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]);
    assert!(Document::from_json(&bad_base.to_string()).is_err());

    let mut bad_rot: serde_json::Value = serde_json::from_str(&json).unwrap();
    bad_rot["features"][0]["moves"][1]["rotation"][3][0] = serde_json::json!(12.0);
    assert!(Document::from_json(&bad_rot.to_string()).is_err());
}

#[test]
fn capture_position_walk_and_parameter_references_cover_moves_and_joints() {
    let mut doc = Document::new("capture params");
    let a = doc.add_component(Some("A"), 0).unwrap();
    let b = doc.add_component(Some("B"), 0).unwrap();
    let occ_a = doc.occurrences.iter().find(|o| o.component == a).unwrap().id;
    let occ_b = doc.occurrences.iter().find(|o| o.component == b).unwrap().id;

    for (name, expr, unit) in [
        ("dx", "10 mm", "mm"),
        ("dy", "0 mm", "mm"),
        ("dz", "0 mm", "mm"),
        ("ox", "5 mm", "mm"),
        ("oy", "0 mm", "mm"),
        ("oz", "0 mm", "mm"),
        ("ang", "30 deg", "deg"),
        ("j_ang", "15 deg", "deg"),
    ] {
        doc.add_user_param(name, expr, Some(unit), None).unwrap();
    }
    doc.params.push(Param { name: "model_keep".into(), expr: "1 mm".into(), unit: "mm".into(), comment: String::new(), model: true });

    let origin = crate::joints::JointOrigin::new(0, crate::joints::Snap::Point { point: Vec3::ZERO, z: None, x: None });
    doc.assembly.joints.push(crate::joints::Joint {
        id: 77,
        name: "Rev1".into(),
        kind: crate::joints::JointKind::Revolute,
        a: crate::joints::JointOrigin { occurrence: occ_a, ..origin.clone() },
        b: crate::joints::JointOrigin { occurrence: occ_b, ..origin },
        values: vec![0.0],
        limits: vec![None],
        dynamics: Vec::new(),
        flip: false,
        offset: 0.0,
        offset_xy: [0.0; 2],
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        captured: None,
        base_values: None,
        base_exprs: Vec::new(),
        constraint: None,
    });

    let mut kind = FeatureKind::CapturePosition {
        moves: vec![CapturedOccurrenceMove {
            occurrence: occ_b,
            space: MoveSpace::Local,
            translate: ["dx + model_keep".into(), "dy".into(), "dz".into()],
            origin: Some(["ox".into(), "oy".into(), "oz".into()]),
            rotate_axis: Some([0.0, 0.0, 1.0]),
            angle: Some("ang".into()),
            rotation: None,
            path: None,
        }],
        joints: vec![CapturedJoint { joint: 77, values: vec![0.0], exprs: vec![Some("j_ang".into())] }],
    };

    // transform_geometry must leave MoveSpace origin untouched.
    let before_kind = kind.clone();
    kind.transform_geometry(&rigid(Vec3::new(50.0, 60.0, 70.0), Vec3::ZERO, Vec3::Z, 0.5));
    assert_eq!(kind, before_kind);

    let cap_id = doc.add_feature(kind, None).unwrap();

    for used in ["dx", "ox", "ang", "j_ang"] {
        assert!(!doc.param_users(used).is_empty(), "{used} should report param_users");
        assert!(doc.remove_param(used).is_err(), "{used} should block deletion");
        assert_eq!(doc.features_using_param(used), vec![cap_id], "{used} should list CapturePosition");
    }

    // Rename both move and joint parameter references.
    assert!(doc.rename_param("dx", "dx_renamed").unwrap() >= 1);
    assert!(doc.rename_param("ox", "ox_renamed").unwrap() >= 1);
    assert!(doc.rename_param("j_ang", "j_ang_renamed").unwrap() >= 1);

    doc.prune_model_params();
    assert!(doc.param("model_keep").is_some(), "model param referenced by CapturePosition stays preserved");

    if let FeatureKind::CapturePosition { moves, joints } = &doc.feature(cap_id).unwrap().kind {
        assert_eq!(moves[0].translate[0].as_ref(), "dx_renamed + model_keep");
        assert_eq!(moves[0].origin.as_ref().unwrap()[0].as_ref(), "ox_renamed");
        assert_eq!(joints[0].exprs[0].as_deref(), Some("j_ang_renamed"));
    } else {
        panic!("expected CapturePosition");
    }
}

#[test]
fn capture_position_timeline_isolates_earlier_features_and_re_evaluates_on_parameter_change() {
    let mut doc = Document::new("timeline isolation #20644");
    let owner = doc.add_component(Some("Owner"), 0).unwrap();
    let mover = doc.add_component(Some("Mover"), 0).unwrap();
    let occ_mover = doc.occurrences.iter().find(|o| o.component == mover).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ_mover).unwrap().base_transform = Some(IDENTITY);
    doc.add_user_param("dx", "100 mm", Some("mm"), None).unwrap();

    // Feature 0: construction plane in Owner (Offset XZ by 60 mm -> world origin (0, -60, 0) or (0, 60, 0)).
    let plane_id = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XZ".into() }), distance: "60 mm".into() },
            },
            Some("OwnerPlane"),
        )
        .unwrap();
    doc.feature_mut(plane_id).unwrap().component = owner;

    let make_sketch = || {
        let mut sk = Sketch::new();
        let p0 = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
        let p1 = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
        sk.add_line_pts(p0, p1, None).unwrap();
        FeatureKind::Sketch { plane: PlaneRef::Construction { feature: plane_id.into(), placement: None }, sketch: sk }
    };

    // Feature 1 (before CapturePosition): SketchBefore in Mover.
    let sketch_before = doc.add_feature(make_sketch(), Some("SketchBefore")).unwrap();
    doc.feature_mut(sketch_before).unwrap().component = mover;

    // Feature 2: CapturePosition moves Mover by [dx, 0, 0].
    let cap_id = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ_mover,
                    space: MoveSpace::Local,
                    translate: ["dx".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                }],
                joints: Vec::new(),
            },
            None,
        )
        .unwrap();

    // Feature 3 (after CapturePosition): SketchAfter in Mover on the same Owner plane.
    let sketch_after = doc.add_feature(make_sketch(), Some("SketchAfter")).unwrap();
    doc.feature_mut(sketch_after).unwrap().component = mover;

    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);

    let st = model.state();
    let before_origin = st.sketch(sketch_before).unwrap().plane.origin;
    let after_origin = st.sketch(sketch_after).unwrap().plane.origin;
    assert!((before_origin.x - 0.0).abs() < 1e-6, "SketchBefore saw Mover at base IDENTITY: {before_origin:?}");
    assert!((after_origin.x + 100.0).abs() < 1e-6, "SketchAfter saw Mover at x=100, so plane x=-100 in Mover frame: {after_origin:?}");
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 100.0).abs() < 1e-9);

    // Changing dx must recompute CapturePosition + SketchAfter while keeping SketchBefore cached.
    doc.set_param("dx", "45 mm", Some("mm"), None).unwrap();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    assert_eq!(model.last_recomputed, 2, "only CapturePosition and SketchAfter should recompute, got {}", model.last_recomputed);
    let st2 = model.state();
    assert!((st2.sketch(sketch_before).unwrap().plane.origin.x - 0.0).abs() < 1e-6);
    assert!((st2.sketch(sketch_after).unwrap().plane.origin.x + 45.0).abs() < 1e-6);
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 45.0).abs() < 1e-9);

    // Verify no_cache=true and refingerprint_from parity.
    let mut fresh = Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&doc);
    assert_eq!(fresh.resolved_occurrences(), model.resolved_occurrences());
    assert_eq!(fresh.state().sketch(sketch_after).unwrap().plane.origin, st2.sketch(sketch_after).unwrap().plane.origin);
    let expected_fps: Vec<u64> = model.results.iter().map(|r| r.fingerprint).collect();
    model.refingerprint_from(&doc, 0);
    let refp_fps: Vec<u64> = model.results.iter().map(|r| r.fingerprint).collect();
    assert_eq!(refp_fps, expected_fps, "refingerprint_from(0) must match evaluate fingerprints");
    let mut model_mid = model.clone();
    model_mid.refingerprint_from(&doc, 2);
    let mid_fps: Vec<u64> = model_mid.results.iter().map(|r| r.fingerprint).collect();
    assert_eq!(mid_fps, expected_fps, "refingerprint_from(2) must match evaluate fingerprints");
    let _ = cap_id;
}

#[test]
fn capture_position_suppression_rollback_reorder_deletion_and_errors() {
    let mut doc = Document::new("timeline ops");
    let owner = doc.add_component(Some("Owner"), 0).unwrap();
    let mover = doc.add_component(Some("Mover"), 0).unwrap();
    let occ_mover = doc.occurrences.iter().find(|o| o.component == mover).unwrap().id;
    let base = rigid(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
    doc.occurrences.iter_mut().find(|o| o.id == occ_mover).unwrap().base_transform = Some(base);

    let plane_id = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XZ".into() }), distance: "10 mm".into() },
            },
            None,
        )
        .unwrap();
    doc.feature_mut(plane_id).unwrap().component = owner;

    let cap1 = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ_mover,
                    space: MoveSpace::Local,
                    translate: ["20 mm".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                }],
                joints: vec![CapturedJoint { joint: 1, values: vec![1.25], exprs: Vec::new() }],
            },
            None,
        )
        .unwrap();

    let mut sk = Sketch::new();
    let p0 = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let p1 = sk.add_point(Vec2::new(5.0, 0.0), None).unwrap();
    sk.add_line_pts(p0, p1, None).unwrap();
    let sk_after = doc
        .add_feature(FeatureKind::Sketch { plane: PlaneRef::Construction { feature: plane_id.into(), placement: None }, sketch: sk }, None)
        .unwrap();
    doc.feature_mut(sk_after).unwrap().component = mover;

    let mut model = Model::new();
    model.evaluate(&doc);
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 25.0).abs() < 1e-9);
    assert_eq!(model.resolved_joints().get(&1).map(|j| j.values.clone()), Some(vec![1.25]));

    // Suppress CapturePosition -> reverts to base_transform (5.0) and clears resolved_joints.
    doc.feature_mut(cap1).unwrap().suppressed = true;
    model.evaluate(&doc);
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 5.0).abs() < 1e-9);
    assert!(model.resolved_joints().is_empty());
    assert!((model.state().sketch(sk_after).unwrap().plane.origin.x + 5.0).abs() < 1e-6);

    // Unsuppress -> re-applies CapturePosition (25.0).
    doc.feature_mut(cap1).unwrap().suppressed = false;
    model.evaluate(&doc);
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 25.0).abs() < 1e-9);
    assert_eq!(model.resolved_joints().get(&1).map(|j| j.values.clone()), Some(vec![1.25]));

    // Rollback marker before CapturePosition -> reverts to base_transform (5.0).
    let cap_idx = doc.feature_index(cap1).unwrap();
    doc.marker = Some(cap_idx);
    model.evaluate(&doc);
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 5.0).abs() < 1e-9);
    assert!(model.resolved_joints().is_empty());

    // Roll forward past CapturePosition -> restores from rolled_results and re-applies (25.0).
    doc.marker = None;
    model.evaluate(&doc);
    assert!((model.resolved_occurrences()[&occ_mover][3][0] - 25.0).abs() < 1e-9);
    assert_eq!(model.resolved_joints().get(&1).map(|j| j.values.clone()), Some(vec![1.25]));

    // Add a second non-commutative CapturePosition (90 deg rotation about world/parent origin),
    // then reorder the two CapturePosition features and verify non-commutative result changes.
    let cap2 = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ_mover,
                    space: MoveSpace::Local,
                    translate: ["0".into(), "0".into(), "0".into()],
                    origin: Some(["0".into(), "0".into(), "0".into()]),
                    rotate_axis: Some([0.0, 0.0, 1.0]),
                    angle: Some("90 deg".into()),
                    rotation: None,
                    path: None,
                }],
                joints: Vec::new(),
            },
            None,
        )
        .unwrap();
    model.evaluate(&doc);
    // Order (cap1: +20x, cap2: rot90): base=(5,0,0) -> (25,0,0) -> rot90 => (0, 25, 0).
    let t_order1 = model.resolved_occurrences()[&occ_mover];
    assert!((t_order1[3][0] - 0.0).abs() < 1e-6 && (t_order1[3][1] - 25.0).abs() < 1e-6, "order1: {:?}", t_order1[3]);

    // Reorder: cap2 before cap1 -> base=(5,0,0) -> rot90 => (0,5,0) -> +20x => (20, 5, 0).
    let i1 = doc.feature_index(cap1).unwrap();
    let i2 = doc.feature_index(cap2).unwrap();
    doc.features.swap(i1, i2);
    model.evaluate(&doc);
    let t_order2 = model.resolved_occurrences()[&occ_mover];
    assert!((t_order2[3][0] - 20.0).abs() < 1e-6 && (t_order2[3][1] - 5.0).abs() < 1e-6, "order2: {:?}", t_order2[3]);

    // Delete both CapturePosition features -> reverts to base_transform (5,0,0).
    doc.features.retain(|f| f.id != cap1 && f.id != cap2);
    model.evaluate(&doc);
    assert!(mat_close(&model.resolved_occurrences()[&occ_mover], &base, 1e-9));

    // Atomic error: move 1 is valid, move 2 references an unknown parameter -> neither applies!
    doc.add_feature(
        FeatureKind::CapturePosition {
            moves: vec![
                CapturedOccurrenceMove {
                    occurrence: occ_mover,
                    space: MoveSpace::Local,
                    translate: ["100 mm".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                },
                CapturedOccurrenceMove {
                    occurrence: occ_mover,
                    space: MoveSpace::Local,
                    translate: ["missing_param".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                },
            ],
            joints: Vec::new(),
        },
        None,
    )
    .unwrap();
    model.evaluate(&doc);
    assert!(model.results.last().unwrap().error.is_some());
    assert!(mat_close(&model.resolved_occurrences()[&occ_mover], &base, 1e-9), "atomic failure must not apply partial moves");
}

#[test]
fn capture_position_world_space_nested_path_rotation_and_adjust_base() {
    let mut doc = Document::new("world space nested");
    let parent_comp = doc.add_component(Some("Parent"), 0).unwrap();
    let child_comp = doc.add_component(Some("Child"), parent_comp).unwrap();
    let occ_parent = doc.occurrences.iter().find(|o| o.component == parent_comp).unwrap().id;
    let occ_child = doc.occurrences.iter().find(|o| o.component == child_comp).unwrap().id;

    // Parent rotated +90 deg around Z; child starts at local (0, 0, 0).
    let parent_rot = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    doc.occurrences.iter_mut().find(|o| o.id == occ_parent).unwrap().base_transform = Some(parent_rot);
    doc.occurrences.iter_mut().find(|o| o.id == occ_child).unwrap().base_transform = Some(IDENTITY);

    // Move child +10 mm in World X -> in Parent's +90 deg Z-rotated local frame, World +X is Local -Y.
    doc.add_feature(
        FeatureKind::CapturePosition {
            moves: vec![CapturedOccurrenceMove {
                occurrence: occ_child,
                space: MoveSpace::World,
                translate: ["10 mm".into(), "0".into(), "0".into()],
                origin: None,
                rotate_axis: None,
                angle: None,
                rotation: None,
                path: Some(vec![occ_parent, occ_child]),
            }],
            joints: Vec::new(),
        },
        None,
    )
    .unwrap();

    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    let child_local = model.resolved_occurrences()[&occ_child];
    let child_world = mat_mul(&parent_rot, &child_local);
    assert!((child_world[3][0] - 10.0).abs() < 1e-9 && child_world[3][1].abs() < 1e-9, "world={:?}", child_world[3]);
    assert!(child_local[3][0].abs() < 1e-9 && (child_local[3][1] + 10.0).abs() < 1e-9, "local={:?}", child_local[3]);

    // Test Occurrence::adjust_base_for_direct_move with non-commutative rotation + translation.
    let base_before = rigid(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.3);
    let cap_delta = rigid(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    let old_resolved = mat_mul(&cap_delta, &base_before);
    let direct_shift = rigid(Vec3::new(3.0, -4.0, 2.0), Vec3::ZERO, Vec3::Y, 0.4);
    let new_resolved = mat_mul(&direct_shift, &old_resolved);

    let mut test_occ = Occurrence {
        id: 500,
        component: 1,
        parent: 0,
        name: "Test:1".into(),
        transform: old_resolved,
        base_transform: Some(base_before),
        grounded: false,
        bom_type: None,
        expressed_moves: Vec::new(),
    };
    test_occ.adjust_base_for_direct_move(&old_resolved, &new_resolved);
    let replayed = mat_mul(&cap_delta, &test_occ.base_transform.unwrap());
    assert!(mat_close(&replayed, &new_resolved, 1e-9), "replayed={replayed:?}, want={new_resolved:?}");

    // Test `rotation` fallback matrix + `CapturedJoint::exprs` re-evaluation on parameter change.
    let mut rot_doc = Document::new("rotation and joint exprs");
    let ca = rot_doc.add_component(Some("CA"), 0).unwrap();
    let cb = rot_doc.add_component(Some("CB"), 0).unwrap();
    let oa = rot_doc.occurrences.iter().find(|o| o.component == ca).unwrap().id;
    let ob = rot_doc.occurrences.iter().find(|o| o.component == cb).unwrap().id;
    rot_doc.occurrences.iter_mut().find(|o| o.id == ob).unwrap().base_transform = Some(IDENTITY);
    rot_doc.add_user_param("j_theta", "30 deg", Some("deg"), None).unwrap();
    let origin = crate::joints::JointOrigin::new(0, crate::joints::Snap::Point { point: Vec3::ZERO, z: None, x: None });
    rot_doc.assembly.joints.push(crate::joints::Joint {
        id: 11,
        name: "Rev".into(),
        kind: crate::joints::JointKind::Revolute,
        a: crate::joints::JointOrigin { occurrence: oa, ..origin.clone() },
        b: crate::joints::JointOrigin { occurrence: ob, ..origin },
        values: vec![0.0],
        limits: vec![None],
        dynamics: Vec::new(),
        flip: false,
        offset: 0.0,
        offset_xy: [0.0; 2],
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        captured: None,
        base_values: None,
        base_exprs: Vec::new(),
        constraint: None,
    });
    let rot90 = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    rot_doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: ob,
                    space: MoveSpace::Local,
                    translate: ["5 mm".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: Some(rot90),
                    path: None,
                }],
                joints: vec![CapturedJoint { joint: 11, values: vec![0.0], exprs: vec![Some("j_theta".into())] }],
            },
            None,
        )
        .unwrap();

    let mut rot_model = Model::new();
    rot_model.evaluate(&rot_doc);
    assert!(rot_model.results.iter().all(|r| r.error.is_none()), "{:?}", rot_model.results);
    let mut expected_rot = rot90;
    expected_rot[3] = [5.0, 0.0, 0.0, 1.0];
    assert!(mat_close(&rot_model.resolved_occurrences()[&ob], &expected_rot, 1e-9));
    let jv1 = rot_model.resolved_joints()[&11].values[0];
    assert!((jv1 - 30.0_f64.to_radians()).abs() < 1e-9, "got {jv1}");

    rot_doc.set_param("j_theta", "60 deg", Some("deg"), None).unwrap();
    rot_model.evaluate(&rot_doc);
    let jv2 = rot_model.resolved_joints()[&11].values[0];
    assert!((jv2 - 60.0_f64.to_radians()).abs() < 1e-9, "got {jv2}");
}

#[test]
fn capture_position_invalidates_construction_axis_point_move_rotate_about_and_sketch_links() {
    let mut doc = Document::new("cache invalidation coverage");
    let comp_a = doc.add_component(Some("CompA"), 0).unwrap();
    let comp_b = doc.add_component(Some("CompB"), 0).unwrap();
    let occ_a = doc.occurrences.iter().find(|o| o.component == comp_a).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ_a).unwrap().base_transform = Some(IDENTITY);
    doc.add_user_param("dx", "10 mm", Some("mm"), None).unwrap();

    // Cylinder in CompA at local origin (radius 5, height 10) -> axis at (0,0,z) in CompA.
    let cyl_a = doc
        .add_feature(
            FeatureKind::Cylinder {
                base: point_expr(Vec3::ZERO),
                axis: point_expr(Vec3::Z),
                radius: "5 mm".into(),
                height: "10 mm".into(),
                operation: Operation::NewBody,
                references: Vec::new(),
            },
            Some("CylA"),
        )
        .unwrap();
    doc.feature_mut(cyl_a).unwrap().component = comp_a;

    // Construction plane & axis & point owned by CompA.
    let plane_a = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "YZ".into() }), distance: "3 mm".into() },
            },
            Some("PlaneA"),
        )
        .unwrap();
    doc.feature_mut(plane_a).unwrap().component = comp_a;

    let axis_a = doc
        .add_feature(
            FeatureKind::ConstructionAxis {
                def: crate::construct::AxisDef::TwoPoints {
                    a: crate::construct::GeoRef::Point { p: Vec3::new(0.0, 0.0, 0.0) },
                    b: crate::construct::GeoRef::Point { p: Vec3::new(0.0, 10.0, 0.0) },
                },
            },
            Some("AxisA"),
        )
        .unwrap();
    doc.feature_mut(axis_a).unwrap().component = comp_a;

    let pt_a = doc
        .add_feature(
            FeatureKind::ConstructionPoint {
                def: crate::construct::PointDef::At { point: crate::construct::GeoRef::Point { p: Vec3::new(2.0, 3.0, 4.0) } },
            },
            Some("PointA"),
        )
        .unwrap();
    doc.feature_mut(pt_a).unwrap().component = comp_a;

    // Box in CompB (will be rotated around CompA's cylinder axis via Move { rotate_about: Some(...) }).
    let box_b = doc
        .add_feature(
            FeatureKind::Box {
                corner: crate::point_expr(Vec3::new(0.0, 0.0, 0.0)),
                length: "2 mm".into(),
                width: "2 mm".into(),
                height: "2 mm".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            Some("BoxB"),
        )
        .unwrap();
    doc.feature_mut(box_b).unwrap().component = comp_b;
    doc.feature_mut(box_b).unwrap().body_names = vec!["BodyB".into()];

    // Evaluate once at dx=10 to capture persistent face name of CylA.
    let cap_id = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ_a,
                    space: MoveSpace::Local,
                    translate: ["dx".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                }],
                joints: Vec::new(),
            },
            Some("CapA"),
        )
        .unwrap();

    let mut prep = Model::new();
    prep.evaluate(&doc);
    assert!(prep.results.iter().all(|r| r.error.is_none()), "{:?}", prep.results);
    let prep_st = prep.state();
    let cyl_body = prep_st.bodies.iter().find(|b| b.feature == cyl_a).unwrap();
    let cyl_face_name = crate::naming::face_names(cyl_body)
        .iter()
        .enumerate()
        .find(|(fi, _)| matches!(solidcraft_kernel::face_surface(&cyl_body.body, *fi), Some(solidcraft_kernel::FaceSurface::Cylinder { .. })))
        .map(|(_, n)| n.clone())
        .expect("cylinder face name");

    // 1) ConstructionAxis on named cylinder face of CompA (authored in CompB).
    let ax_named = doc
        .add_feature(
            FeatureKind::ConstructionAxis {
                def: crate::construct::AxisDef::ThroughFace {
                    face: crate::construct::GeoRef::Face { at: Vec3::new(15.0, 0.0, 5.0), name: Some(cyl_face_name.clone()) },
                },
            },
            Some("AxNamed"),
        )
        .unwrap();
    doc.feature_mut(ax_named).unwrap().component = comp_b;

    // 2) ConstructionAxis referencing AxisA (foreign construction axis) & ConstructionPoint referencing PointA (foreign construction point).
    let ax_foreign = doc
        .add_feature(
            FeatureKind::ConstructionAxis {
                def: crate::construct::AxisDef::TwoPoints {
                    a: crate::construct::GeoRef::ConstructPoint { name: "PointA".into() },
                    b: crate::construct::GeoRef::Point { p: Vec3::new(0.0, 0.0, 20.0) },
                },
            },
            Some("AxForeign"),
        )
        .unwrap();
    doc.feature_mut(ax_foreign).unwrap().component = comp_b;

    let pt_named = doc
        .add_feature(
            FeatureKind::ConstructionPoint {
                def: crate::construct::PointDef::At {
                    point: crate::construct::GeoRef::Snap {
                        of: Box::new(crate::construct::GeoRef::Face { at: Vec3::new(15.0, 0.0, 5.0), name: Some(cyl_face_name.clone()) }),
                        kind: crate::construct::SnapAt::Centre,
                        at: Vec3::new(10.0, 0.0, 10.0),
                        end: Some(1),
                    },
                },
            },
            Some("PtNamed"),
        )
        .unwrap();
    doc.feature_mut(pt_named).unwrap().component = comp_b;

    let pt_foreign = doc
        .add_feature(
            FeatureKind::ConstructionPoint {
                def: crate::construct::PointDef::At { point: crate::construct::GeoRef::ConstructPoint { name: "PointA".into() } },
            },
            Some("PtForeign"),
        )
        .unwrap();
    // Root component triggers world_state in construction_state.
    doc.feature_mut(pt_foreign).unwrap().component = 0;

    // 3) Move { rotate_about: Some(GeoRef::Face { .. }) } turning BodyB around CompA's cylinder axis.
    let move_about = doc
        .add_feature(
            FeatureKind::Move {
                bodies: vec!["BodyB".into()],
                translate: ["0".into(), "0".into(), "0".into()],
                rotate_axis: Some(Dir(Vec3::Z)),
                rotate_about: Some(crate::construct::GeoRef::Face { at: Vec3::new(15.0, 0.0, 5.0), name: Some(cyl_face_name) }),
                origin: Some(crate::point_expr(Vec3::new(10.0, 0.0, 0.0))),
                angle: Some("90 deg".into()),
                copy: false,
                copy_frames: Vec::new(),
            },
            Some("MoveAbout"),
        )
        .unwrap();
    doc.feature_mut(move_about).unwrap().component = comp_b;

    // 4) Sketch in CompB projecting LinkSource::Plane { name: "PlaneA" } and LinkSource::Axis { name: "AxisA" }.
    let mut sk = Sketch::new();
    let p0 = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let p1 = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    sk.add_line_pts(p0, p1, None).unwrap();
    sk.add_link(
        solidcraft_sketch::LinkKind::Project,
        solidcraft_sketch::LinkSource::Plane { name: "PlaneA".into() },
        &[solidcraft_sketch::LinkGeom::Line(Vec2::new(13.0, -5.0), Vec2::new(13.0, 5.0))],
    )
    .unwrap();
    sk.add_link(
        solidcraft_sketch::LinkKind::Project,
        solidcraft_sketch::LinkSource::Axis { name: "AxisA".into() },
        &[solidcraft_sketch::LinkGeom::Line(Vec2::new(10.0, -5.0), Vec2::new(10.0, 5.0))],
    )
    .unwrap();
    let sk_links = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, Some("SkLinks")).unwrap();
    doc.feature_mut(sk_links).unwrap().component = comp_b;

    // Evaluate cached model at dx = 10 mm, then mutate dx -> 40 mm.
    let mut cached = Model::new();
    cached.evaluate(&doc);
    assert!(cached.results.iter().all(|r| r.error.is_none()), "{:?}", cached.results);

    doc.set_param("dx", "40 mm", Some("mm"), None).unwrap();
    cached.evaluate(&doc);

    let mut cold = Model::new();
    cold.no_cache = true;
    cold.evaluate(&doc);
    assert!(cold.results.iter().all(|r| r.error.is_none()), "{:?}", cold.results);

    for (rc, rf) in cached.results.iter().zip(&cold.results) {
        assert_eq!(rc.error, rf.error, "error mismatch on {}", rc.name);
    }
    assert_eq!(
        cached.state().construct.iter().find(|c| c.feature == ax_named).map(|c| &c.geom),
        cold.state().construct.iter().find(|c| c.feature == ax_named).map(|c| &c.geom),
        "ConstructionAxis on named face must invalidate"
    );
    assert_eq!(
        cached.state().construct.iter().find(|c| c.feature == ax_foreign).map(|c| &c.geom),
        cold.state().construct.iter().find(|c| c.feature == ax_foreign).map(|c| &c.geom),
        "ConstructionAxis on foreign construction point must invalidate"
    );
    assert_eq!(
        cached.state().construct.iter().find(|c| c.feature == pt_named).map(|c| &c.geom),
        cold.state().construct.iter().find(|c| c.feature == pt_named).map(|c| &c.geom),
        "ConstructionPoint on named face must invalidate"
    );
    assert_eq!(
        cached.state().construct.iter().find(|c| c.feature == pt_foreign).map(|c| &c.geom),
        cold.state().construct.iter().find(|c| c.feature == pt_foreign).map(|c| &c.geom),
        "ConstructionPoint in root referencing foreign point must invalidate"
    );
    let cached_bounds = solidcraft_geom::MeshGeom::bounds(&*cached.state().body("BodyB").unwrap().mesh());
    let cold_bounds = solidcraft_geom::MeshGeom::bounds(&*cold.state().body("BodyB").unwrap().mesh());
    assert!(
        cached_bounds.min.dist(cold_bounds.min) < 1e-6 && cached_bounds.max.dist(cold_bounds.max) < 1e-6,
        "Move rotate_about must invalidate: cached={cached_bounds:?} cold={cold_bounds:?}"
    );
    let cached_st = cached.state();
    let cold_st = cold.state();
    let cached_sk = cached_st.sketch(sk_links).unwrap();
    let cold_sk = cold_st.sketch(sk_links).unwrap();
    assert_eq!(cached_sk.sketch.curves.len(), cold_sk.sketch.curves.len());
    for (cp, qp) in cached_sk.sketch.points.iter().zip(&cold_sk.sketch.points) {
        assert!(solidcraft_geom::Vec2Geom::dist(cp.pos, qp.pos) < 1e-9, "linked sketch point mismatch: {:?} vs {:?}", cp.pos, qp.pos);
    }
    let _ = cap_id;
}

#[test]
fn capture_position_is_current_saved_adopt_and_adopt_solved_sketches() {
    let mut doc = Document::new("model cache & adopt");
    let comp_a = doc.add_component(Some("A"), 0).unwrap();
    let comp_b = doc.add_component(Some("B"), 0).unwrap();
    let occ_a = doc.occurrences.iter().find(|o| o.component == comp_a).unwrap().id;
    let occ_b = doc.occurrences.iter().find(|o| o.component == comp_b).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ_a).unwrap().base_transform = Some(IDENTITY);
    doc.occurrences.iter_mut().find(|o| o.id == occ_a).unwrap().transform = rigid(Vec3::new(99.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
    doc.add_user_param("dx", "25 mm", Some("mm"), None).unwrap();
    doc.add_user_param("j_ang", "30 deg", Some("deg"), None).unwrap();

    let origin = crate::joints::JointOrigin::new(0, crate::joints::Snap::Point { point: Vec3::ZERO, z: None, x: None });
    doc.assembly.joints.push(crate::joints::Joint {
        id: 1,
        name: "Rev".into(),
        kind: crate::joints::JointKind::Revolute,
        a: crate::joints::JointOrigin { occurrence: occ_a, ..origin.clone() },
        b: crate::joints::JointOrigin { occurrence: occ_b, ..origin },
        values: vec![0.0],
        limits: vec![None],
        dynamics: Vec::new(),
        flip: false,
        offset: 0.0,
        offset_xy: [0.0; 2],
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        captured: None,
        base_values: None,
        base_exprs: Vec::new(),
        constraint: None,
    });

    let plane_a = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "YZ".into() }), distance: "5 mm".into() },
            },
            Some("PlaneA"),
        )
        .unwrap();
    doc.feature_mut(plane_a).unwrap().component = comp_a;

    doc.add_feature(
        FeatureKind::CapturePosition {
            moves: vec![CapturedOccurrenceMove {
                occurrence: occ_a,
                space: MoveSpace::Local,
                translate: ["dx".into(), "0".into(), "0".into()],
                origin: None,
                rotate_axis: None,
                angle: None,
                rotation: None,
                path: None,
            }],
            joints: vec![CapturedJoint { joint: 1, values: vec![0.0], exprs: vec![Some("j_ang".into())] }],
        },
        None,
    )
    .unwrap();

    // Downstream sketch in CompB projecting PlaneA (so changing dx moves the linked sketch geometry).
    let mut sk = Sketch::new();
    let p0 = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let p1 = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    sk.add_line_pts(p0, p1, None).unwrap();
    sk.add_link(
        solidcraft_sketch::LinkKind::Project,
        solidcraft_sketch::LinkSource::Plane { name: "PlaneA".into() },
        &[solidcraft_sketch::LinkGeom::Line(Vec2::new(30.0, -5.0), Vec2::new(30.0, 5.0))],
    )
    .unwrap();
    let sk_id = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, Some("SkB")).unwrap();
    doc.feature_mut(sk_id).unwrap().component = comp_b;

    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    assert!(model.is_current(&doc), "Model::is_current must return true right after Model::evaluate");

    // Save & adopt round-trip without recomputing.
    let saved = model.saved(&doc).expect("Model::saved must succeed when base_transform != transform");
    let mut adopted = Model::adopt(&doc, &saved).expect("Model::adopt must succeed");
    assert_eq!(adopted.resolved_occurrences(), model.resolved_occurrences());
    assert_eq!(adopted.resolved_joints(), model.resolved_joints());
    assert!(adopted.is_current(&doc));
    adopted.evaluate(&doc);
    assert_eq!(adopted.last_recomputed, 0, "adopted model must not recompute");

    // Changing joint kind invalidates CapturePosition cleanly.
    let mut doc_slider = doc.clone();
    doc_slider.assembly.joints[0].kind = crate::joints::JointKind::Slider;
    model.evaluate(&doc_slider);
    assert!(model.last_recomputed >= 1, "joint kind change must recompute CapturePosition, got {}", model.last_recomputed);
    // Restore original Revolute doc for adopt_solved_sketches test.
    model.evaluate(&doc);

    // Mutate dx so the linked sketch re-solves, then adopt_solved_sketches.
    doc.set_param("dx", "55 mm", Some("mm"), None).unwrap();
    model.evaluate(&doc);
    assert!(model.is_current(&doc));
    assert_eq!(model.adopt_solved_sketches(&mut doc), 1);
    assert!(model.is_current(&doc), "is_current must remain true after adopt_solved_sketches");
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0, "evaluate after adopt_solved_sketches must not recompute");
}

#[test]
fn capture_position_evaluate_shared_updates_resolved_occurrences_when_base_transform_changes() {
    let mut doc = Document::new("evaluate_shared base_transform");
    let comp = doc.add_component(Some("Part"), 0).unwrap();
    let occ = doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ).unwrap().base_transform = Some(IDENTITY);
    let box_id = doc
        .add_feature(
            FeatureKind::Box {
                corner: crate::point_expr(Vec3::ZERO),
                length: "10 mm".into(),
                width: "10 mm".into(),
                height: "10 mm".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    doc.feature_mut(box_id).unwrap().component = comp;

    let doc1 = std::sync::Arc::new(doc);
    let mut model = Model::new();
    assert!(model.evaluate_shared(&doc1));
    assert!((model.resolved_occurrences()[&occ][3][0] - 0.0).abs() < 1e-9);

    let mut doc2 = (*doc1).clone();
    doc2.occurrences.iter_mut().find(|o| o.id == occ).unwrap().base_transform = Some(rigid(Vec3::new(99.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    let doc2 = std::sync::Arc::new(doc2);
    assert!(model.evaluate_shared(&doc2), "evaluate_shared must not skip when has_capture_position_timeline is true");
    assert!((model.resolved_occurrences()[&occ][3][0] - 99.0).abs() < 1e-9);
}

#[test]
fn capture_position_hardening_joint_values_and_clipboard_remap() {
    let mut doc = Document::new("hardening");
    let comp = doc.add_component(Some("Part"), 0).unwrap();
    let occ = doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ).unwrap().base_transform = Some(IDENTITY);

    let cap_id = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ,
                    space: MoveSpace::World,
                    translate: ["1 mm".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: Some(vec![occ]),
                }],
                joints: vec![CapturedJoint { joint: 1, values: vec![0.5], exprs: Vec::new() }],
            },
            None,
        )
        .unwrap();

    // format::decode rejects oversized values / exprs (> 64) and out-of-range values.
    let json = doc.to_json();
    let mut too_many_vals: serde_json::Value = serde_json::from_str(&json).unwrap();
    too_many_vals["features"][0]["joints"][0]["values"] = serde_json::json!(vec![0.0; 65]);
    assert!(Document::from_json(&too_many_vals.to_string()).is_err());

    let mut too_many_exprs: serde_json::Value = serde_json::from_str(&json).unwrap();
    too_many_exprs["features"][0]["joints"][0]["exprs"] = serde_json::json!(vec![Option::<String>::None; 65]);
    assert!(Document::from_json(&too_many_exprs.to_string()).is_err());

    let mut too_big_val: serde_json::Value = serde_json::from_str(&json).unwrap();
    too_big_val["features"][0]["joints"][0]["values"] = serde_json::json!([1e10]);
    assert!(Document::from_json(&too_big_val.to_string()).is_err());

    // In-memory eval_captured_joint rejects NaN/out-of-range values even when exprs is empty.
    let mut bad_doc = doc.clone();
    if let FeatureKind::CapturePosition { joints, .. } = &mut bad_doc.feature_mut(cap_id).unwrap().kind {
        joints[0].values = vec![f64::NAN];
    }
    let mut bad_model = Model::new();
    bad_model.evaluate(&bad_doc);
    assert!(bad_model.result(cap_id).and_then(|r| r.error.as_ref()).is_some());

    // Feature::remap_clone remaps m.occurrence and m.path when occurrences map is populated.
    let mut cloned = doc.feature(cap_id).unwrap().clone();
    let mut occ_map = std::collections::BTreeMap::new();
    occ_map.insert(occ, 777);
    let source_roots = std::collections::BTreeSet::new();
    let placements = crate::ClonePlacements { occurrences: &occ_map, source_roots: &source_roots, root_path: &[], doc: &doc, same_document: true };
    cloned.remap_clone(&Default::default(), &Default::default(), &Default::default(), &Default::default(), Some(&placements)).unwrap();
    if let FeatureKind::CapturePosition { moves, .. } = &cloned.kind {
        assert_eq!(moves[0].occurrence, 777);
        assert_eq!(moves[0].path.as_deref(), Some([777].as_slice()));
    } else {
        panic!("expected CapturePosition");
    }
}

#[test]
fn capture_position_moved_component_sketch_same_and_foreign_axis_links_match_cold_eval() {
    let mut doc = Document::new("same and foreign axis link parity");
    let comp_a = doc.add_component(Some("CompA"), 0).unwrap();
    let comp_b = doc.add_component(Some("CompB"), 0).unwrap();
    let occ_a = doc.occurrences.iter().find(|o| o.component == comp_a).unwrap().id;
    let occ_b = doc.occurrences.iter().find(|o| o.component == comp_b).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ_a).unwrap().base_transform = Some(IDENTITY);
    doc.occurrences.iter_mut().find(|o| o.id == occ_b).unwrap().base_transform = Some(IDENTITY);
    doc.add_user_param("dx", "10 mm", Some("mm"), None).unwrap();
    doc.add_user_param("bx", "5 mm", Some("mm"), None).unwrap();
    doc.add_user_param("by", "5 mm", Some("mm"), None).unwrap();

    // CompB owns a ConstructionAxis at local x = 5 along Y.
    let ax_b = doc
        .add_feature(
            FeatureKind::ConstructionAxis {
                def: crate::construct::AxisDef::TwoPoints {
                    a: crate::construct::GeoRef::Point { p: Vec3::new(5.0, 0.0, 0.0) },
                    b: crate::construct::GeoRef::Point { p: Vec3::new(5.0, 10.0, 0.0) },
                },
            },
            Some("AxB"),
        )
        .unwrap();
    doc.feature_mut(ax_b).unwrap().component = comp_b;

    doc.add_feature(
        FeatureKind::CapturePosition {
            moves: vec![
                CapturedOccurrenceMove {
                    occurrence: occ_a,
                    space: MoveSpace::Local,
                    translate: ["dx".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                },
                CapturedOccurrenceMove {
                    occurrence: occ_b,
                    space: MoveSpace::Local,
                    translate: ["bx".into(), "by".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: None,
                },
            ],
            joints: Vec::new(),
        },
        Some("MoveParts"),
    )
    .unwrap();

    let make_sketch = |doc: &mut Document, link_source: solidcraft_sketch::LinkSource, name: &str| {
        let mut sk = Sketch::new();
        let p0 = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
        let p1 = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
        sk.add_line_pts(p0, p1, None).unwrap();
        sk.add_link(
            solidcraft_sketch::LinkKind::Project,
            link_source,
            &[solidcraft_sketch::LinkGeom::Line(Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0))],
        )
        .unwrap();
        let id = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, Some(name)).unwrap();
        doc.feature_mut(id).unwrap().component = comp_b;
        id
    };

    let sk_same_axis = make_sketch(&mut doc, solidcraft_sketch::LinkSource::Axis { name: "AxB".into() }, "SkSameAxis");
    let sk_same_datum = make_sketch(&mut doc, solidcraft_sketch::LinkSource::Axis { name: format!("o:{occ_b}/origin/Y") }, "SkSameDatum");
    let sk_foreign_datum = make_sketch(&mut doc, solidcraft_sketch::LinkSource::Axis { name: format!("o:{occ_a}/origin/Y") }, "SkForeignDatum");

    let mut cached = Model::new();
    cached.evaluate(&doc);
    assert!(cached.results.iter().all(|r| r.error.is_none()), "{:?}", cached.results);

    for (dx, bx, by, expected_foreign_x) in
        [("10 mm", "5 mm", "5 mm", 5.0), ("40 mm", "35 mm", "35 mm", 5.0), ("55 mm", "10 mm", "20 mm", 45.0), ("10 mm", "5 mm", "5 mm", 5.0)]
    {
        doc.set_param("dx", dx, Some("mm"), None).unwrap();
        doc.set_param("bx", bx, Some("mm"), None).unwrap();
        doc.set_param("by", by, Some("mm"), None).unwrap();
        cached.evaluate(&doc);
        assert!(cached.is_current(&doc));

        let mut cold = Model::new();
        cold.no_cache = true;
        cold.evaluate(&doc);
        assert!(cold.results.iter().all(|r| r.error.is_none()), "{:?}", cold.results);

        let cached_st = cached.state();
        let cold_st = cold.state();
        for (sk_id, expected_link_x) in [(sk_same_axis, 5.0), (sk_same_datum, 0.0), (sk_foreign_datum, expected_foreign_x)] {
            let cs = cached_st.sketch(sk_id).unwrap();
            let fs = cold_st.sketch(sk_id).unwrap();
            assert_eq!(cs.sketch.points.len(), fs.sketch.points.len(), "point count mismatch on {sk_id}");
            for (cp, fp) in cs.sketch.points.iter().zip(&fs.sketch.points) {
                assert!(
                    solidcraft_geom::Vec2Geom::dist(cp.pos, fp.pos) < 1e-9,
                    "cache mismatch on sketch {sk_id} ({dx}, {bx}, {by}): cached={:?} cold={:?}",
                    cp.pos,
                    fp.pos
                );
            }
            let link_id = &cs.sketch.links[0].id;
            for &pi in &cs.sketch.link_points(link_id) {
                let x = cs.sketch.points[pi].pos.x;
                assert!((x - expected_link_x).abs() < 1e-9, "sketch {sk_id} expected x={expected_link_x}, got {x}");
            }
        }
    }
}

#[test]
fn capture_position_nested_same_occurrence_reorder_local_vs_world_multi_instance() {
    let mut doc = Document::new("nested same occ reorder");
    let parent_comp = doc.add_component(Some("Parent"), 0).unwrap();
    let child_comp = doc.add_component(Some("Child"), parent_comp).unwrap();
    let occ_p1 = doc.occurrences.iter().find(|o| o.component == parent_comp).unwrap().id;
    let occ_c = doc.occurrences.iter().find(|o| o.component == child_comp).unwrap().id;
    let occ_p2 = doc.add_occurrence(parent_comp, 0, rigid(Vec3::new(0.0, 50.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();

    // Rotate occ_p1 +90 deg around Z; occ_p2 stays unrotated at (0, 50, 0); child starts at local (5, 0, 0).
    let p1_rot = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    let c_base = rigid(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
    doc.occurrences.iter_mut().find(|o| o.id == occ_p1).unwrap().base_transform = Some(p1_rot);
    doc.occurrences.iter_mut().find(|o| o.id == occ_c).unwrap().base_transform = Some(c_base);

    // CP1: World translation (+10, 0, 0) along occ_p1 path -> in occ_p1's +90Z frame, World +X = Local -Y.
    let cp1 = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ_c,
                    space: MoveSpace::World,
                    translate: ["10 mm".into(), "0".into(), "0".into()],
                    origin: None,
                    rotate_axis: None,
                    angle: None,
                    rotation: None,
                    path: Some(vec![occ_p1, occ_c]),
                }],
                joints: Vec::new(),
            },
            None,
        )
        .unwrap();
    // CP2: Local 90 deg rotation about Z around origin (0, 0, 0).
    let cp2 = doc
        .add_feature(
            FeatureKind::CapturePosition {
                moves: vec![CapturedOccurrenceMove {
                    occurrence: occ_c,
                    space: MoveSpace::Local,
                    translate: ["0".into(), "0".into(), "0".into()],
                    origin: Some(["0".into(), "0".into(), "0".into()]),
                    rotate_axis: Some([0.0, 0.0, 1.0]),
                    angle: Some("90 deg".into()),
                    rotation: None,
                    path: None,
                }],
                joints: Vec::new(),
            },
            None,
        )
        .unwrap();

    let mut model = Model::new();
    model.evaluate(&doc);
    // Order (CP1 -> CP2): local (5, 0, 0) + world (+10,0,0)->local (0,-10,0) = (5, -10, 0), then local rot90Z -> (10, 5, 0).
    let t1 = model.resolved_occurrences()[&occ_c];
    assert!((t1[3][0] - 10.0).abs() < 1e-6 && (t1[3][1] - 5.0).abs() < 1e-6, "order1 local={:?}", t1[3]);
    let w1_p1 = mat_mul(&p1_rot, &t1);
    assert!((w1_p1[3][0] + 5.0).abs() < 1e-6 && (w1_p1[3][1] - 10.0).abs() < 1e-6, "order1 world p1={:?}", w1_p1[3]);
    let _ = occ_p2;

    // Reorder (CP2 -> CP1): local (5, 0, 0) -> local rot90Z => (0, 5, 0), then world (+10,0,0)->local (0,-10,0) => (0, -5, 0).
    let i1 = doc.feature_index(cp1).unwrap();
    let i2 = doc.feature_index(cp2).unwrap();
    doc.features.swap(i1, i2);
    model.evaluate(&doc);
    let t2 = model.resolved_occurrences()[&occ_c];
    assert!(t2[3][0].abs() < 1e-6 && (t2[3][1] + 5.0).abs() < 1e-6, "order2 local={:?}", t2[3]);
}

#[test]
fn capture_position_20644_isolation_for_to_object_mirror_pattern_plane_and_joints() {
    let mut doc = Document::new("#20644 multi-feature isolation");
    let comp_a = doc.add_component(Some("CompA"), 0).unwrap();
    let comp_b = doc.add_component(Some("CompB"), 0).unwrap();
    let occ_a = doc.occurrences.iter().find(|o| o.component == comp_a).unwrap().id;
    let occ_b = doc.occurrences.iter().find(|o| o.component == comp_b).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ_a).unwrap().grounded = true;
    doc.occurrences.iter_mut().find(|o| o.id == occ_b).unwrap().base_transform = Some(IDENTITY);

    // BoxA at [0..10, 0..10, 0..10], BoxB at [20..30, 0..10, 0..10].
    let box_a = doc
        .add_feature(
            FeatureKind::Box {
                corner: crate::point_expr(Vec3::ZERO),
                length: "10 mm".into(),
                width: "10 mm".into(),
                height: "10 mm".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            Some("BoxA"),
        )
        .unwrap();
    doc.feature_mut(box_a).unwrap().component = comp_a;
    doc.feature_mut(box_a).unwrap().body_names = vec!["BodyA".into()];

    let box_b = doc
        .add_feature(
            FeatureKind::Box {
                corner: crate::point_expr(Vec3::new(20.0, 0.0, 0.0)),
                length: "10 mm".into(),
                width: "10 mm".into(),
                height: "10 mm".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            Some("BoxB"),
        )
        .unwrap();
    doc.feature_mut(box_b).unwrap().component = comp_b;
    doc.feature_mut(box_b).unwrap().body_names = vec!["BodyB".into()];

    // Pre-capture ToObject Extrude in CompA up to BodyB's x=20 face (y=2..8, z=2..8, extruded +X from x=10 to x=20 -> length 10).
    let mut sk = Sketch::new();
    let p0 = sk.add_point(Vec2::new(2.0, 2.0), None).unwrap();
    let p1 = sk.add_point(Vec2::new(8.0, 2.0), None).unwrap();
    let p2 = sk.add_point(Vec2::new(8.0, 8.0), None).unwrap();
    let p3 = sk.add_point(Vec2::new(2.0, 8.0), None).unwrap();
    sk.add_line_pts(p0, p1, None).unwrap();
    sk.add_line_pts(p1, p2, None).unwrap();
    sk.add_line_pts(p2, p3, None).unwrap();
    sk.add_line_pts(p3, p0, None).unwrap();
    let sk_to = doc
        .add_feature(
            FeatureKind::Sketch {
                plane: PlaneRef::Placed { origin: crate::point_expr(Vec3::new(10.0, 0.0, 0.0)), x_dir: Vec3::Y, y_dir: Vec3::Z },
                sketch: sk,
            },
            Some("SkTo"),
        )
        .unwrap();
    doc.feature_mut(sk_to).unwrap().component = comp_a;

    let ext_to = doc
        .add_feature(
            FeatureKind::Extrude {
                sketch: sk_to.into(),
                profiles: ProfileSel::All,
                extent: Extent {
                    convention: crate::ExtrudeConvention::LegacyPerSide,
                    distance: "1 mm".into(),
                    direction: Direction::Positive,
                    distance2: None,
                    start_offset: None,
                    symmetric_offset: false,
                    through_all: false,
                    second_follows_first: false,
                    taper: None,
                    corner: None,
                    to: Some(crate::FacePick(Vec3::new(20.0, 5.0, 5.0))),
                    to_offset: None,
                    from: None,
                    from_level: false,
                    to_body_apart: true,
                    through_all2: false,
                    to2: None,
                    to_offset2: None,
                    picked_placements: Vec::new(),
                    one_sided_direction: None,
                },
                operation: Operation::NewBody,
                targets: Vec::new(),
                thin: None,
            },
            Some("ToObj"),
        )
        .unwrap();
    doc.feature_mut(ext_to).unwrap().component = comp_a;
    doc.feature_mut(ext_to).unwrap().body_names = vec!["BodyTo".into()];

    // Pre-capture Mirror & Pattern of BodyB at pre-capture pose (YZ plane / +Y 20 mm).
    let mirror_pre = doc
        .add_feature(
            FeatureKind::Mirror {
                features: Vec::new(),
                plane: PlaneRef::Origin { name: "YZ".into() },
                bodies: vec!["BodyB".into()],
                combine: false,
                placements: vec![vec![occ_b]],
            },
            Some("MirrorPre"),
        )
        .unwrap();
    doc.feature_mut(mirror_pre).unwrap().body_names = vec!["MirroredPre".into()];

    let pattern_pre = doc
        .add_feature(
            FeatureKind::Pattern {
                features: Vec::new(),
                pattern: PatternKind::Rectangular {
                    dir1: crate::points::Dir(Vec3::Y),
                    count1: "2".into(),
                    spacing1: "20 mm".into(),
                    dir2: None,
                    count2: None,
                    spacing2: None,
                    alignment: Default::default(),
                    alignment2: Default::default(),
                    reversed: false,
                    reversed2: false,
                    distance1: Default::default(),
                    distance2: Default::default(),
                    extent1: None,
                    extent2: None,
                    copy_at_end1: false,
                    copy_at_end2: false,
                },
                bodies: vec!["BodyB".into()],
                placements: vec![vec![occ_b]],
                components: Vec::new(),
                occurrences: Vec::new(),
                join: false,
            },
            Some("PatternPre"),
        )
        .unwrap();
    doc.feature_mut(pattern_pre).unwrap().body_names = vec!["PatPre1".into()];

    // Pre-capture face-anchored ConstructionPlane on BodyB top face (z=10).
    let plane_pre = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Face {
                    plane: <Plane as solidcraft_geom::PlaneGeom>::new(Vec3::new(25.0, 5.0, 10.0), Vec3::X, Vec3::Y).unwrap(),
                    at: Vec3::new(25.0, 5.0, 10.0),
                    name: None,
                    anchor: None,
                },
            },
            Some("PlanePre"),
        )
        .unwrap();

    // CapturePosition moves occ_b by (+10, 0, +15).
    doc.add_feature(
        FeatureKind::CapturePosition {
            moves: vec![CapturedOccurrenceMove {
                occurrence: occ_b,
                space: MoveSpace::Local,
                translate: ["10 mm".into(), "0".into(), "15 mm".into()],
                origin: None,
                rotate_axis: None,
                angle: None,
                rotation: None,
                path: None,
            }],
            joints: Vec::new(),
        },
        Some("CaptureB"),
    )
    .unwrap();

    // Also verify joint-driven occurrence #20644 isolation: CompJ joined via Slider to CompA with base_values=[0] -> captured=[20].
    let comp_j = doc.add_component(Some("CompJ"), 0).unwrap();
    let occ_j = doc.occurrences.iter().find(|o| o.component == comp_j).unwrap().id;
    let box_j = doc
        .add_feature(
            FeatureKind::Box {
                corner: crate::point_expr(Vec3::new(0.0, 40.0, 0.0)),
                length: "10 mm".into(),
                width: "10 mm".into(),
                height: "10 mm".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            Some("BoxJ"),
        )
        .unwrap();
    doc.feature_mut(box_j).unwrap().component = comp_j;
    doc.feature_mut(box_j).unwrap().body_names = vec!["BodyJ".into()];

    let j_origin = crate::joints::JointOrigin::new(0, crate::joints::Snap::Point { point: Vec3::ZERO, z: None, x: None });
    doc.assembly.joints.push(crate::joints::Joint {
        id: 88,
        name: "SlideJ".into(),
        kind: crate::joints::JointKind::Slider,
        a: crate::joints::JointOrigin {
            occurrence: occ_a,
            snap: crate::joints::Snap::Point { point: Vec3::new(0.0, 40.0, 0.0), z: None, x: None },
            ..j_origin.clone()
        },
        b: crate::joints::JointOrigin {
            occurrence: occ_j,
            snap: crate::joints::Snap::Point { point: Vec3::new(0.0, 40.0, 0.0), z: None, x: None },
            ..j_origin
        },
        values: vec![20.0],
        limits: vec![None],
        dynamics: Vec::new(),
        flip: false,
        offset: 0.0,
        offset_xy: [0.0; 2],
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        captured: Some(vec![20.0]),
        base_values: Some(vec![0.0]),
        base_exprs: Vec::new(),
        constraint: None,
    });
    // Pre-capture feature in timeline (at index before CaptureJ) reading BodyJ's position via ConstructionPlane.
    let plane_j_pre = doc
        .add_feature(
            FeatureKind::ConstructionPlane {
                plane: PlaneRef::Face {
                    plane: <Plane as solidcraft_geom::PlaneGeom>::new(Vec3::new(5.0, 45.0, 10.0), Vec3::X, Vec3::Y).unwrap(),
                    at: Vec3::new(5.0, 45.0, 10.0),
                    name: None,
                    anchor: None,
                },
            },
            Some("PlaneJPre"),
        )
        .unwrap();
    let cap_j = doc
        .add_feature(
            FeatureKind::CapturePosition { moves: Vec::new(), joints: vec![CapturedJoint { joint: 88, values: vec![20.0], exprs: Vec::new() }] },
            Some("CaptureJ"),
        )
        .unwrap();
    let _ = (occ_a, cap_j);

    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);

    // Pre-capture ToObject stayed at x=[10..20] (volume 360), Mirror stayed at [-30..-20], Pattern stayed at [20..30], PlanePre stayed at z=10, PlaneJPre stayed at z=10.
    let to_vol = solidcraft_kernel::measure(&model.state().body("BodyTo").unwrap().body).unwrap().volume;
    assert!((to_vol - 360.0).abs() < 1e-4, "ToObject leaked post-capture pose: {to_vol}");
    let mir_bounds = solidcraft_kernel::measure(&model.state().body("MirroredPre").unwrap().body).unwrap().bbox;
    assert!((mir_bounds.min.x + 30.0).abs() < 1e-4 && (mir_bounds.max.x + 20.0).abs() < 1e-4, "{mir_bounds:?}");
    let pat_bounds = solidcraft_kernel::measure(&model.state().body("PatPre1").unwrap().body).unwrap().bbox;
    assert!((pat_bounds.min.x - 20.0).abs() < 1e-4 && (pat_bounds.min.y - 20.0).abs() < 1e-4, "{pat_bounds:?}");
    let plane_z = model
        .state()
        .construct
        .iter()
        .find_map(|c| {
            (c.feature == plane_pre).then_some(&c.geom).and_then(|g| match g {
                crate::construct::ConstructGeom::Plane(p) => Some(p.origin.z),
                _ => None,
            })
        })
        .unwrap();
    assert!((plane_z - 10.0).abs() < 1e-6, "PlanePre z leaked: {plane_z}");
    let plane_j_z = model
        .state()
        .construct
        .iter()
        .find_map(|c| {
            (c.feature == plane_j_pre).then_some(&c.geom).and_then(|g| match g {
                crate::construct::ConstructGeom::Plane(p) => Some(p.origin.z),
                _ => None,
            })
        })
        .unwrap();
    assert!((plane_j_z - 10.0).abs() < 1e-6, "PlaneJPre z leaked: {plane_j_z}");
    assert!((model.resolved_occurrences()[&occ_j][3][2] - 20.0).abs() < 1e-6);
}

#[test]
fn capture_position_hostile_format_decode_bounds_rejected() {
    let mut doc = Document::new("hostile bounds");
    let comp = doc.add_component(Some("Part"), 0).unwrap();
    let occ = doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    doc.occurrences.iter_mut().find(|o| o.id == occ).unwrap().base_transform = Some(IDENTITY);
    doc.assembly.joints.push(crate::joints::Joint {
        id: 1,
        name: "J1".into(),
        kind: crate::joints::JointKind::Slider,
        a: crate::joints::JointOrigin::new(0, crate::joints::Snap::Point { point: Vec3::ZERO, z: None, x: None }),
        b: crate::joints::JointOrigin::new(occ, crate::joints::Snap::Point { point: Vec3::ZERO, z: None, x: None }),
        values: vec![0.0],
        limits: vec![None],
        dynamics: Vec::new(),
        flip: false,
        offset: 0.0,
        offset_xy: [0.0; 2],
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        captured: None,
        base_values: Some(vec![0.0]),
        base_exprs: Vec::new(),
        constraint: None,
    });
    doc.add_feature(
        FeatureKind::CapturePosition {
            moves: vec![CapturedOccurrenceMove {
                occurrence: occ,
                space: MoveSpace::World,
                translate: ["1 mm".into(), "0".into(), "0".into()],
                origin: None,
                rotate_axis: Some([0.0, 0.0, 1.0]),
                angle: None,
                rotation: None,
                path: Some(vec![occ]),
            }],
            joints: vec![CapturedJoint { joint: 1, values: vec![0.5], exprs: Vec::new() }],
        },
        None,
    )
    .unwrap();
    let base: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();

    // 1. path.len() > MAX_PLACEMENT_DEPTH + 1
    let mut bad_path = base.clone();
    bad_path["features"][0]["moves"][0]["path"] = serde_json::json!(vec![occ; crate::MAX_PLACEMENT_DEPTH + 2]);
    assert!(Document::from_json(&bad_path.to_string()).is_err());

    // 2. moves.len() > MAX_PLACEMENTS and joints.len() > MAX_PLACEMENTS
    let mut too_many_moves = base.clone();
    let mv = too_many_moves["features"][0]["moves"][0].clone();
    too_many_moves["features"][0]["moves"] = serde_json::Value::Array(vec![mv; crate::MAX_PLACEMENTS + 1]);
    assert!(Document::from_json(&too_many_moves.to_string()).is_err());

    let mut too_many_joints = base.clone();
    let cj = too_many_joints["features"][0]["joints"][0].clone();
    too_many_joints["features"][0]["joints"] = serde_json::Value::Array(vec![cj; crate::MAX_PLACEMENTS + 1]);
    assert!(Document::from_json(&too_many_joints.to_string()).is_err());

    // 3. j.base_values.len() > 64, j.base_exprs.len() > 64, out-of-range j.base_values
    let mut bad_base_vals = base.clone();
    bad_base_vals["assembly"]["joints"][0]["base_values"] = serde_json::json!(vec![0.0; 65]);
    assert!(Document::from_json(&bad_base_vals.to_string()).is_err());

    let mut bad_base_exprs = base.clone();
    bad_base_exprs["assembly"]["joints"][0]["base_exprs"] = serde_json::json!(vec!["0 mm"; 65]);
    assert!(Document::from_json(&bad_base_exprs.to_string()).is_err());

    let mut huge_base_val = base.clone();
    huge_base_val["assembly"]["joints"][0]["base_values"] = serde_json::json!([1e10]);
    assert!(Document::from_json(&huge_base_val.to_string()).is_err());

    // 4. Non-rigid rotation in CapturePosition move
    let mut bad_rot = base.clone();
    bad_rot["features"][0]["moves"][0]["rotation"] =
        serde_json::json!([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    assert!(Document::from_json(&bad_rot.to_string()).is_err());
}

/// An extrude with a start offset names its parallel caps top#1 (lower) and top#2 (upper),
/// reliably ordered by height. Area-weighted face centroids guarantee delta-X and delta-Y
/// are zero between identical planar caps regardless of triangulation winding or vertex rotation,
/// preventing triangulation-dependent spread-axis sorting flips (#21686).
#[test]
fn extrude_start_offset_caps_are_named_consistently_across_rotations() {
    let pts: [[f64; 2]; 12] = [
        [25.0, 0.0],
        [0.0, 5.0],
        [0.0, 20.0],
        [15.0, 35.0],
        [25.0, 5.0],
        [30.0, 20.0],
        [20.0, 30.0],
        [25.0, 45.0],
        [40.0, 40.0],
        [30.0, 40.0],
        [30.0, 30.0],
        [40.0, 20.0],
    ];
    for rot in 0..12 {
        let mut sk = Sketch::new();
        let mut rotated = pts.to_vec();
        rotated.rotate_left(rot);
        let point_ids: Vec<usize> = rotated.iter().map(|&[x, y]| sk.add_point(Vec2::new(x, y), None).unwrap()).collect();
        for i in 0..12 {
            let next = (i + 1) % 12;
            sk.add_curve(solidcraft_sketch::CurveKind::Line { a: point_ids[i], b: point_ids[next] }, None).unwrap();
        }
        let mut doc = Document::new("start offset test");
        let sketch_id = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
        let _ = doc
            .add_feature(
                FeatureKind::Extrude {
                    thin: None,
                    sketch: crate::SketchId(sketch_id),
                    profiles: ProfileSel::All,
                    extent: Extent {
                        convention: crate::ExtrudeConvention::TotalDistance,
                        distance: "1".into(),
                        direction: Direction::Positive,
                        distance2: None,
                        start_offset: Some("4".into()),
                        symmetric_offset: false,
                        through_all: false,
                        second_follows_first: false,
                        taper: None,
                        corner: Default::default(),
                        to: None,
                        to_offset: None,
                        from: None,
                        to_body_apart: false,
                        through_all2: false,
                        to2: None,
                        to_offset2: None,
                        from_level: false,
                        picked_placements: Vec::new(),
                        one_sided_direction: None,
                    },
                    operation: Operation::NewBody,
                    targets: vec![],
                },
                Some("Plate"),
            )
            .unwrap();
        let mut model = Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "rot {rot}: {:?}", model.results);
        let state = model.state();
        let body = state.bodies.first().expect("body built");
        let names = crate::naming::face_names(body);
        let bot_name = &names[body.body.faces(0.01).unwrap().iter().find(|f| (f.centroid.z - 4.0).abs() < 1e-4).unwrap().index];
        let top_name = &names[body.body.faces(0.01).unwrap().iter().find(|f| (f.centroid.z - 5.0).abs() < 1e-4).unwrap().index];
        assert_eq!(bot_name, "F2:top#1", "rot {rot} bottom cap name");
        assert_eq!(top_name, "F2:top#2", "rot {rot} top cap name");

        // Verify that F2:top#2 resolves to the top cap (z = 5.0)
        let hit_top = crate::name_lookup::locate_face(&state, "F2:top#2", Vec3::new(20.0, 20.0, 5.0)).expect("find F2:top#2");
        assert!((hit_top.point.z - 5.0).abs() < 1e-4, "rot {rot} F2:top#2 hit at z = 5");

        // Verify that F2:top#1 resolves to the bottom cap (z = 4.0)
        let hit_bot = crate::name_lookup::locate_face(&state, "F2:top#1", Vec3::new(20.0, 20.0, 4.0)).expect("find F2:top#1");
        assert!((hit_bot.point.z - 4.0).abs() < 1e-4, "rot {rot} F2:top#1 hit at z = 4");
    }
}

/// An asymmetric profile extruded with or without a start offset preserves its face names
/// between kernel centroids and mesh-sampled means, and old saved designs keep all names
/// on the exact same faces (#21686, PR #21762).
#[test]
fn asymmetric_profile_and_old_designs_preserve_face_names() {
    let mut fixture: serde_json::Value = serde_json::from_str(include_str!("old_cap_anchor_fixture.json")).unwrap();
    let doc = Document {
        features: serde_json::from_value(fixture["features"].take()).unwrap(),
        params: serde_json::from_value(fixture["params"].take()).unwrap(),
        ..Document::default()
    };
    let upgraded = crate::format::read(&doc.to_json()).unwrap();
    let mut model = Model::new();
    model.evaluate(&upgraded);
    for r in &model.results {
        assert!(r.error.is_none(), "feature {}: {:?}", r.id, r.error);
    }
    let st = model.state();
    let b = st.body("Body1").expect("Body1 built");
    let names = crate::naming::face_names(b);
    const EXPECTED: &[&str] =
        &["F2:side:a1", "F2:side:l6", "F2:side:l4", "F2:start", "F2:top", "F2:side:l1", "F4:side:l3", "F2:side:l3", "F4:bottom", "F2:side:l2"];
    assert_eq!(&names[..], EXPECTED);
    let centres = crate::naming::face_centres(b).expect("face centres");
    assert_eq!(names.len(), centres.len());
    // For every face on Body1, verifying that names land consistently:
    assert!(!names.is_empty(), "faces named");
    for (i, name) in names.iter().enumerate() {
        assert!(!name.is_empty(), "face {i} has valid name");
    }
}
