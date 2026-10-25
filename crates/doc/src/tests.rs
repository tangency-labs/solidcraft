use solidcraft_geom::{Vec2, Vec3};
use solidcraft_kernel::measure;
use solidcraft_sketch::{ConstraintKind, Sketch};

use crate::*;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
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
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
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
            },
            None,
        )
        .unwrap();
    }
    let mut m = Model::new();
    m.evaluate(&doc);
    // Room for a few whole models: enough for both timelines' new bodies, not for every state
    // charged as a whole model.
    m.cache.lock().unwrap().budget = 3 * m.state().retained_bytes();
    doc.set_param("h", "12 mm", None, None).unwrap();
    m.evaluate(&doc);
    assert_eq!(m.last_recomputed, 30);
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
                taper: None,
                corner: Default::default(),
                to: None,
                to_offset: None,
                from: None,
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
    doc.timeline_groups.push(TimelineGroup { id: 100, name: "Pair".into(), parent: None, features: vec![first, second] });
    doc.marker = Some(1);

    let inserted =
        doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() }, Some("Inserted")).unwrap();

    assert_eq!(doc.features.iter().map(|feature| feature.id).collect::<Vec<_>>(), vec![first, inserted, second]);
    assert_eq!(doc.timeline_groups[0].features, vec![first, inserted, second]);
    assert_eq!(doc.marker, Some(2));
    assert!(non_contiguous_timeline_group(&doc.features, &doc.timeline_groups).is_none());
    assert!(Document::from_json(&doc.to_json()).is_ok(), "the inserted feature must leave a reopenable design");
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
    doc.add_feature(FeatureKind::Box { corner: point_expr(corner), length, width, height, operation }, None).unwrap()
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
