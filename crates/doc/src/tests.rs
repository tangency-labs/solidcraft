use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::Vec3Geom;
use solidcraft_kernel::measure;
use solidcraft_sketch::{ConstraintKind, Sketch};

use crate::*;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
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
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
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
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
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

/// A construction plane's edit recomputes only the features that use it: a sketch on it and what
/// is made from that sketch, wherever they are in the timeline, and nothing else.
#[test]
fn a_construction_plane_edit_recomputes_only_its_users() {
    let mut doc = three_parts(true);
    let b = doc.find_component("B").unwrap();
    // A peg in B on A's plane: B now reads A through it.
    let a_top = doc.find_feature("A_top").unwrap().id;
    circle_extrude(&mut doc, b, "B peg", PlaneRef::Construction { feature: a_top.into() }, Vec2::new(40.0, 20.0), 2.0, "3", "new_body");
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
            axis: Dir(Vec3::Z),
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
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
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
            axis: Dir(Vec3::Z),
            radius: "2".into(),
            height: "20".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
    doc.add_feature(FeatureKind::Combine { target: "Body1".into(), tools: vec!["Body2".into()], operation: Operation::Cut, keep_tools: false }, None)
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
    let selfref = |id: u64| Box::new(construct::GeoRef::Plane { plane: PlaneRef::Construction { feature: id.into() } });
    let xz = || Box::new(construct::GeoRef::Plane { plane: PlaneRef::Origin { name: "XZ".into() } });

    // Directly its own first side.
    let mut doc = Document::new("selfplane");
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a: selfref(1), b: xz() } }, Some("P1")).unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.is_some()), "a plane that is its own side must report an error: {:?}", m.results);

    // Two planes that are each other's side.
    let mut doc = Document::new("twoplanes");
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a: selfref(2), b: xz() } }, Some("P1")).unwrap();
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a: selfref(1), b: xz() } }, Some("P2")).unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.is_some()), "a two-plane cycle must report an error: {:?}", m.results);

    // A perpendicular plane whose base is itself, and a sketch placed on it.
    let mut doc = Document::new("perpplane");
    let line = Box::new(construct::GeoRef::World { axis: "X".into() });
    doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Perpendicular { plane: selfref(1), line } }, Some("P1")).unwrap();
    doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Construction { feature: 1.into() }, sketch: Sketch::new() }, Some("Sketch1")).unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.is_some()), "a cyclic perpendicular plane must report an error: {:?}", m.results);
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
                through_all: false,
                second_follows_first: false,
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
                to_body_apart: false,
            },
            operation: Operation::NewBody,
            targets: vec![],
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Combine { target: "Body1".into(), tools: vec!["Body2".into()], operation: Operation::Join, keep_tools: false },
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
    };
    let back: FeatureKind = serde_json::from_value(serde_json::to_value(&set).unwrap()).unwrap();
    assert_eq!(back, set);
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
    doc.occurrences.push(Occurrence { id: 20, component: 7, parent: 0, name: "Part".into(), transform: IDENTITY, grounded: false });
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
        doc.occurrences.push(Occurrence { id, component: 7, parent: 0, name: format!("P{id}"), transform: IDENTITY, grounded: false });
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
            through_all: false,
            second_follows_first: false,
            taper: None,
            corner: Default::default(),
            to: None,
            to_offset: None,
            from: None,
            to_body_apart: false,
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
                .add_curve(solidcraft_sketch::CurveKind::Spline { pts: pts.clone(), control: false, degree: 3, periodic: false, nurbs: None }, None)
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
    let on_plane = FeatureKind::Sketch { plane: PlaneRef::Construction { feature: plane.into() }, sketch };
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
