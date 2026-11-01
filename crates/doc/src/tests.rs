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

/// #5184 (CE8): a feature's saved fields by what they are. Who it is and the names kept for its
/// inputs and picks are not its definition; the sketch geometry is left out by name; the body
/// names evaluation pins are in it, but say they are written back.
#[test]
fn a_feature_says_which_fields_are_its_definition() {
    let (doc, sketch) = plate_doc();
    let mut f = doc.feature(sketch).unwrap().clone();
    f.param_names = vec!["d9".into()];
    f.body_names = vec!["Plate".into()];
    f.edge_names = vec!["F1:top|F1:side".into()];
    let def = f.definition().unwrap();
    assert_eq!(def.fields.keys().collect::<Vec<_>>(), ["body_names", "plane"]);
    assert_eq!(def.omitted.iter().map(|(k, _)| *k).collect::<Vec<_>>(), ["sketch"]);
    assert_eq!(f.field_role("name"), FieldRole::Identity);
    assert_eq!(f.field_role("type"), FieldRole::Identity);
    assert_eq!(f.field_role("edge_names"), FieldRole::Bookkeeping);
    assert_eq!(f.field_role("param_names"), FieldRole::Bookkeeping);
    assert_eq!(f.field_role("body_names"), FieldRole::Derived);
    assert_eq!(f.field_role("link_faces"), FieldRole::Derived);
    assert!(matches!(f.field_role("sketch"), FieldRole::Heavy(_)));
    assert_eq!(f.field_role("plane"), FieldRole::Input);
    // `sketch` is heavy only on a sketch: an extrude's `sketch` is its input.
    let pad = doc.features.iter().find(|x| matches!(x.kind, FeatureKind::Extrude { .. })).unwrap();
    assert_eq!(pad.field_role("sketch"), FieldRole::Input);
    // The definition is the input and derived fields.
    let roles = [FieldRole::Identity, FieldRole::Bookkeeping, FieldRole::Heavy(""), FieldRole::Derived, FieldRole::Input];
    assert_eq!(roles.map(FieldRole::in_definition), [false, false, false, true, true]);
}
