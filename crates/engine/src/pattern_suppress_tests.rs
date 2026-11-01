//! A pattern of a suppressed feature has nothing to copy (#42): its copies go too, and the
//! pattern says so.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &Session, body: &str) -> f64 {
    s.model.state().body(body).map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0)
}

#[test]
fn suppressing_a_patterned_feature_removes_its_copies() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    let full = volume(&s, "Case");
    run(&mut s, "timeline.suppress", json!({"feature": "Standoff", "suppressed": true}));
    let without = volume(&s, "Case");
    // All four standoffs go (each 116.8 mm³), not just the seed.
    assert!((full - without - 4.0 * 116.8).abs() < 2.0, "{full} → {without}");
    let pattern = s.doc.find_feature("RectangularPattern1").unwrap().id;
    let r = s.model.result(pattern).unwrap();
    assert!(r.error.is_none() && r.warning.as_deref().is_some_and(|w| w.contains("Standoff")), "{r:?}");
    // Back on: the four standoffs return.
    run(&mut s, "timeline.suppress", json!({"feature": "Standoff", "suppressed": false}));
    assert!((volume(&s, "Case") - full).abs() < 1e-6 * full);
}

#[test]
fn enclosure_datums_follow_wall_height_and_board_depth_with_material_proofs() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    for (name, expression) in [("wall", "2.5 mm"), ("height", "30 mm"), ("board_d", "50 mm")] {
        run(&mut s, "parameters.change", json!({"name":name,"expression":expression}));
    }
    assert!(s.model.results.iter().all(|result| result.error.is_none()));
    for (name, z) in [("PCB standoff", 2.5), ("Lid boss", 2.5), ("Lid lip", 30.), ("Lid screw", 34.)] {
        let feature =
            s.doc.features.iter().find(|feature| feature.name == name && matches!(feature.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap();
        assert!((s.model.state().sketch(feature.id).unwrap().plane.origin.z - z).abs() < 1e-6);
    }
    let usb =
        s.doc.features.iter().find(|feature| feature.name == "USB" && matches!(feature.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap();
    assert!((s.model.state().sketch(usb.id).unwrap().plane.origin.y + 35.5).abs() < 1e-6);
    let assert_material = |session: &Session| {
        let state = session.world_state().unwrap();
        let case = state.body("Case").unwrap().mesh();
        let lid = state.body("Lid").unwrap().mesh();
        assert!(case.contains(solidcraft_geom::Vec3::new(-24.5, -21.5, 7.)));
        assert!(!case.contains(solidcraft_geom::Vec3::new(-26.5, -21.5, 7.)));
        assert!(!case.contains(solidcraft_geom::Vec3::new(-10., -35.25, 11.35)));
        assert!(case.contains(solidcraft_geom::Vec3::new(-10., -35.25, 15.)));
        assert!(
            lid.contains(solidcraft_geom::Vec3::new(0., 32.2, 29.)),
            "lid bounds {:?}; lip {:?}; probes {:?}",
            lid.bounds(),
            session
                .model
                .state()
                .sketch(
                    session
                        .doc
                        .features
                        .iter()
                        .find(|feature| feature.name == "Lid lip" && matches!(feature.kind, solidcraft_doc::FeatureKind::Sketch { .. }))
                        .unwrap()
                        .id
                )
                .unwrap()
                .sketch
                .links,
            (20..35).map(|y| (y, lid.contains(solidcraft_geom::Vec3::new(0., f64::from(y) + 0.2, 29.)))).collect::<Vec<_>>()
        );
        assert!(!lid.contains(solidcraft_geom::Vec3::new(0., 32.9, 29.)));
        assert!(!lid.contains(solidcraft_geom::Vec3::new(-33.5, -28.5, 32.)));
        assert!(lid.contains(solidcraft_geom::Vec3::new(-31.5, -28.5, 32.)));
    };
    assert_material(&s);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.results.iter().all(|result| result.error.is_none()));
    assert_material(&reopened);
}

#[test]
fn enclosure_arc_extinction_keeps_a_real_sharp_inner_lip_after_resize_and_reopen() {
    use solidcraft_doc::FeatureKind;
    use solidcraft_geom::{Vec2, Vec3};
    use solidcraft_sketch::CurveKind;

    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    for (name, expression) in [("height", "30 mm"), ("board_d", "50 mm"), ("wall", "3 mm")] {
        run(&mut s, "parameters.change", json!({"name":name,"expression":expression}));
    }
    let check = |session: &Session| {
        let errors = session.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| (&r.name, e))).collect::<Vec<_>>();
        let warnings = session.model.results.iter().filter_map(|r| r.warning.as_ref().map(|e| (&r.name, e))).collect::<Vec<_>>();
        assert!(errors.is_empty(), "errors {errors:?}");
        // The earlier snap-fit operation retains its notice about its face reference: the
        // shell's inner wall it sits on, which the resized boss join (no exact history there)
        // leaves to be found by geometry. Offset/projection failures or frozen borders are not
        // accepted.
        assert!(
            warnings.iter().all(|(name, message)| name.as_str() == "SnapFit1"
                && message.as_str() == "face `F4:inner:F2:side:l1` was found by geometry, not exact history; check it"),
            "warnings {warnings:?}"
        );
        assert!(session.model.results.iter().filter(|r| r.name == "Lid lip").all(|r| r.warning.is_none()));
        let state = session.model.state();
        let feature = session.doc.features.iter().find(|f| f.name == "Lid lip" && matches!(f.kind, FeatureKind::Extrude { .. })).unwrap();
        let FeatureKind::Extrude { sketch, profiles, .. } = &feature.kind else {
            panic!("lip extrusion");
        };
        let solved = state.sketch(sketch.0).unwrap();
        assert!((solved.plane.origin.z - 30.).abs() < 1e-8);
        let inner = &solved.sketch.offsets[1];
        assert_eq!(inner.outputs.len(), 4);
        let mut corners = Vec::new();
        for id in &inner.outputs {
            let curve = &solved.sketch.curves[solved.sketch.curve_index(id).unwrap()];
            let CurveKind::Line { a, b } = curve.kind else {
                panic!("extinct arcs must leave line supports: {curve:?}");
            };
            for point in [solved.sketch.points[a].pos, solved.sketch.points[b].pos] {
                assert!((point.x.abs() - 36.6).abs() < 1e-8 && (point.y.abs() - 31.6).abs() < 1e-8, "{point:?}");
                if !corners.iter().any(|p: &Vec2| p.dist(point) < 1e-8) {
                    corners.push(point);
                }
            }
        }
        assert_eq!(corners.len(), 4);
        let regions = solidcraft_doc::select_profiles(solved, profiles).unwrap();
        assert_eq!(regions.len(), 1);
        assert!((regions[0].area() - 332.570619298295).abs() < 1e-7, "area {}", regions[0].area());
        let ring = solidcraft_kernel::extrude(&solved.plane, &regions, 0., 2.).unwrap();
        assert_eq!(ring.len(), 1);
        let precise_ring = ring[0].mesh(&solidcraft_kernel::MeshTolerance::chord(1e-6)).unwrap();
        let bounds = precise_ring.bounds();
        for (actual, expected) in
            [(bounds.min.x, -37.8), (bounds.max.x, 37.8), (bounds.min.y, -32.8), (bounds.max.y, 32.8), (bounds.min.z, 28.), (bounds.max.z, 30.)]
        {
            assert!((actual - expected).abs() < 1e-8, "ring bounds {bounds:?}");
        }
        let height = bounds.max.z - bounds.min.z;
        assert!((height - 2.).abs() < 1e-8);
        let analytic_volume = regions[0].area() * height;
        assert!((analytic_volume - 665.141238596591).abs() < 2e-7, "analytic ring volume {analytic_volume}");
        let arc_perimeter = regions[0]
            .outer
            .segs
            .iter()
            .map(|segment| match segment {
                solidcraft_geom::Seg2::Arc { radius, sweep, .. } => radius.abs() * sweep.abs(),
                solidcraft_geom::Seg2::Line { .. } => 0.,
                other => panic!("unexpected ring boundary {other:?}"),
            })
            .sum::<f64>();
        assert!((arc_perimeter - std::f64::consts::TAU * 0.8).abs() < 1e-8);
        assert_eq!(regions[0].holes.len(), 1);
        assert!(regions[0].holes[0].segs.iter().all(|segment| matches!(segment, solidcraft_geom::Seg2::Line { .. })));
        // Body::tessellate floors the requested sag tolerance at size * 1e-6.
        // Only the outer arcs incur chord error: their tolerance-wide boundary
        // strip bounds the volume error without changing CAD or export tolerances.
        let actual_tol = 1e-6_f64.max(ring[0].size() * 1e-6);
        let discretization_bound = height * (arc_perimeter * actual_tol + std::f64::consts::PI * actual_tol.powi(2));
        let measured = precise_ring.measure().volume.abs();
        assert!(
            (measured - analytic_volume).abs() <= discretization_bound,
            "ring mesh volume {measured}, analytic {analytic_volume}, bound {discretization_bound}"
        );
        let lid = state.body("Lid").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap();
        for (point, material) in [
            (Vec3::new(0., 32.2, 29.), true),
            (Vec3::new(0., 32.9, 29.), false),
            (Vec3::new(0., 31.5, 29.), false),
            (Vec3::new(36.7, 31.7, 29.), true),
            (Vec3::new(36.5, 31.5, 29.), false),
            (Vec3::new(37.55, 32.55, 29.), true),
            (Vec3::new(37.7, 32.7, 29.), false),
            // These points expose the stale old rim and the old height.
            (Vec3::new(0., 27.2, 29.), false),
            (Vec3::new(0., 27.2, 24.), false),
        ] {
            assert_eq!(precise_ring.contains(point), material, "standalone ring {point:?}");
            assert_eq!(lid.contains(point), material, "joined lid {point:?}");
        }
        assert!(!lid.contains(Vec3::new(-33.5, -28.5, 32.)));
        assert!(lid.contains(Vec3::new(-31.5, -28.5, 32.)));
    };
    check(&s);
    let saved = s.doc.to_json();
    let reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    check(&reopened);
    assert_eq!(reopened.doc.to_json(), saved);
}
