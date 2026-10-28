use super::*;
use crate::{FeatureKind, Model, PlaneRef};
use serde_json::json;
use solidcraft_geom::{Vec2, Vec3};

fn shell_document() -> (Document, u64, u64) {
    let mut doc = Document::new("certified shell");
    doc.set_param("width", "20", Some("mm"), None).unwrap();
    doc.set_param("wall", "2", Some("mm"), None).unwrap();
    let base = doc
        .add_feature(
            serde_json::from_value(json!({"type":"box","corner":[0,0,0],"length":"width","width":"12","height":"10"})).unwrap(),
            Some("Case"),
        )
        .unwrap();
    doc.feature_mut(base).unwrap().body_names = vec!["Case".into()];
    let shell = doc
        .add_feature(
            FeatureKind::Shell {
                faces: vec![crate::FacePick(Vec3::new(5., 6., 10.))],
                thickness: "wall".into(),
                body: Some("Case".into()),
                direction: "inside".into(),
                tangent_chain: false,
            },
            Some("Shell"),
        )
        .unwrap();
    (doc, base, shell)
}

fn face_index(body: &ModelBody, inner: bool) -> usize {
    body.body
        .face_sources()
        .unwrap()
        .iter()
        .position(|source| {
            matches!(source, Some(solidcraft_kernel::FaceSource::ShellPlanar { inner: current, source, .. })
            if *current == inner && matches!(source.as_ref(), solidcraft_kernel::FaceSource::Box { axis:0,high:true,.. }))
        })
        .unwrap()
}

#[test]
fn shell_planar_raw_bounds_flat_identity_and_clone_remap() {
    let plane = solidcraft_geom::Plane::XY;
    let same = |planes: &[Option<solidcraft_geom::Plane>]| super::super::styles::shell_plane_family(planes.iter().copied());
    assert!(same(&[Some(plane), Some(plane)]));
    assert!(!same(&[]));
    assert!(!same(&[Some(plane), None]));
    for different in [
        solidcraft_geom::Plane { origin: Vec3::X, ..plane },
        solidcraft_geom::Plane { x: -plane.x, y: -plane.y, ..plane },
        solidcraft_geom::Plane { y: -plane.y, ..plane },
    ] {
        assert!(!same(&[Some(plane), Some(different)]));
    }
    let valid = json!({"kind":"shell_planar","source_producer":3,"source":{"kind":"extrude_side","sketch":2,"curve":"c4"},"inner":true});
    assert!(validate_certificate_json(&valid).is_ok());
    let mut certificate: FaceCertificate = serde_json::from_value(valid.clone()).unwrap();
    certificate.remap_features(&[(2, 20), (3, 30)].into_iter().collect());
    assert_eq!(serde_json::to_value(&certificate).unwrap()["source_producer"], 30);
    assert_eq!(serde_json::to_value(&certificate).unwrap()["source"]["sketch"], 20);
    for bad in [
        json!({"kind":"shell_planar","source_producer":0,"source":{"kind":"box","axis":0,"high":true},"inner":false}),
        json!({"kind":"shell_planar","source_producer":3,"source":null,"inner":false}),
        json!({"kind":"shell_planar","source_producer":3,"source":valid,"inner":false}),
        json!({"kind":"shell_planar","source_producer":3,"source":{"kind":"extrude_side","sketch":2,"curve":"x".repeat(1_000_000)},"inner":false}),
        json!({"kind":"shell_planar","source_producer":3,"source":{"kind":"box","axis":0,"high":true,"extra":[[[]]]},"inner":false}),
    ] {
        assert!(validate_certificate_json(&bad).is_err());
    }
}

#[test]
fn shell_planar_anchor_rebuild_cold_geometry_and_missing_source_errors() {
    let (mut doc, base, shell) = shell_document();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|result| result.error.is_none()), "{:?}", model.results);
    let state = model.state();
    let body = state.body("Case").unwrap();
    let outer = doc.strong_face_target(body, face_index(body, false), Vec3::default()).unwrap();
    let outer_matches: Vec<_> = body.body.face_sources().unwrap().iter().enumerate().filter_map(|(index, source)| {
        matches!(source, Some(solidcraft_kernel::FaceSource::ShellPlanar { inner: false, source, .. }) if matches!(source.as_ref(), solidcraft_kernel::FaceSource::Box { axis: 0, high: true, .. })).then_some((index, body.body.face_plane(index)))
    }).collect();
    assert_eq!(outer_matches.len(), 1, "outer support must be one physical face: {outer_matches:?}");
    assert!(super::super::styles::shell_plane_family(outer_matches.iter().map(|(_, plane)| *plane)));
    let outer_plane = body.body.face_plane(face_index(body, false)).unwrap();
    let outer_anchor = doc.capture_face_anchor(body, face_index(body, false), &outer_plane, &[], 0, &[]).unwrap();
    let projected = crate::project::resolve_anchored_face(&doc, &state, &outer_plane, solidcraft_sketch::LinkKind::Project, &outer_anchor).unwrap();
    let mut linked = solidcraft_sketch::Sketch::new();
    let link = linked.add_link(solidcraft_sketch::LinkKind::Project, projected.source, &projected.geom).unwrap();
    assert!(!linked.update_link(&link, &projected.geom).unwrap());
    let inner_index = face_index(body, true);
    let inner = doc.strong_face_target(body, inner_index, Vec3::default()).unwrap();
    assert_ne!(outer.key(), inner.key());
    assert!(matches!(&inner.certificate, Some(FaceCertificate::ShellPlanar { source_producer,inner:true,.. }) if *source_producer==base));
    assert_eq!(inner.origin_feature, shell);
    let picked = body.body.face_plane(inner_index).unwrap();
    assert!((picked.origin.x - 18.).abs() < 1e-8);
    let anchor = doc.capture_face_anchor(body, inner_index, &picked, &[], 0, &[]).unwrap();
    let mut sketch = solidcraft_sketch::Sketch::new();
    let points: Vec<_> =
        [(2., 2.), (4., 2.), (4., 4.), (2., 4.)].into_iter().map(|(x, y)| sketch.add_point(Vec2::new(x, y), None).unwrap()).collect();
    for edge in 0..4 {
        sketch.add_line_pts(points[edge], points[(edge + 1) % 4], None).unwrap();
    }
    let sk = doc
        .add_feature(
            FeatureKind::Sketch {
                plane: PlaneRef::Face { plane: picked, at: picked.origin, name: None, anchor: Some(Box::new(anchor.clone())) },
                sketch,
            },
            Some("Inner face sketch"),
        )
        .unwrap();
    let boss = doc
        .add_feature(
            FeatureKind::Extrude {
                thin: None,
                sketch: crate::SketchId(sk),
                profiles: crate::ProfileSel::All,
                extent: serde_json::from_value(json!({"distance":"2"})).unwrap(),
                operation: crate::Operation::NewBody,
                targets: vec![],
            },
            Some("Face boss"),
        )
        .unwrap();
    model.evaluate(&doc);
    let before = model.state().bodies.iter().find(|body| body.feature == boss).unwrap().mesh().bounds();
    doc.set_param("width", "30", Some("mm"), None).unwrap();
    doc.set_param("wall", "3", Some("mm"), None).unwrap();
    model.evaluate(&doc);
    let changed = model.state();
    let plane = doc.resolve_face_anchor(&anchor, 0, &changed).unwrap();
    assert!((plane.origin.x - 27.).abs() < 1e-8);
    let after = changed.bodies.iter().find(|body| body.feature == boss).unwrap().mesh().bounds();
    assert!((solidcraft_kernel::measure(&changed.bodies.iter().find(|body| body.feature == boss).unwrap().body).unwrap().volume - 8.).abs() < 1e-8);
    assert!((doc.resolve_face_anchor(&outer_anchor, 0, &changed).unwrap().origin.x - 30.).abs() < 1e-8);
    let changed_outer_plane = doc.resolve_face_anchor(&outer_anchor, 0, &changed).unwrap();
    let refreshed =
        crate::project::resolve_anchored_face(&doc, &changed, &changed_outer_plane, solidcraft_sketch::LinkKind::Project, &outer_anchor).unwrap();
    assert!(!linked.update_link(&link, &refreshed.geom).unwrap(), "same exterior boundary must retain projected IDs");
    assert!((after.min.x - before.min.x - 9.).abs() < 1e-8, "{before:?} -> {after:?}");
    assert!((after.max.x - before.max.x - 9.).abs() < 1e-8, "{before:?} -> {after:?}");
    assert!((after.min.y - before.min.y).abs() < 1e-8 && (after.min.z - before.min.z).abs() < 1e-8);
    let saved = doc.to_json();
    assert!(saved.contains(&crate::format::format_string(crate::format::FORMAT)));
    let loaded = crate::format::read(&saved).unwrap();
    let mut cold = Model::new();
    cold.evaluate(&loaded);
    let cold_state = cold.state();
    let cold_bounds = cold_state.bodies.iter().find(|body| body.feature == boss).unwrap().mesh().bounds();
    assert!(after.min.dist(cold_bounds.min) < 1e-9 && after.max.dist(cold_bounds.max) < 1e-9);
    assert!(loaded.resolve_face_anchor(&anchor, 0, &cold_state).unwrap().origin.dist(plane.origin) < 1e-9);
    let mut wrong = anchor.clone();
    wrong.target.origin_feature = base;
    assert!(doc.resolve_face_anchor(&wrong, 0, &changed).is_err());
    let mut wrong_source = anchor.clone();
    if let Some(FaceCertificate::ShellPlanar { source_producer, .. }) = &mut wrong_source.target.certificate {
        *source_producer = shell;
    }
    assert!(doc.resolve_face_anchor(&wrong_source, 0, &changed).is_err());
    doc.feature_mut(shell).unwrap().suppressed = true;
    model.evaluate(&doc);
    let lost = model.state();
    assert!(doc.resolve_face_anchor(&anchor, 0, &lost).is_err());
    assert!(!lost.bodies.iter().any(|body| body.feature == boss), "lost attachment must prevent downstream geometry");
}

#[test]
fn shell_planar_format13_keeps_v12_documents_and_refuses_future() {
    let (doc, _, _) = shell_document();
    let mut legacy: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    legacy["format"] = json!("solidcraft/12");
    let loaded = crate::format::read(&legacy.to_string()).unwrap();
    let mut current: serde_json::Value = serde_json::from_str(&loaded.to_json()).unwrap();
    assert_eq!(current["format"], crate::format::format_string(crate::format::FORMAT));
    current["format"] = json!("solidcraft/12");
    crate::format::pieces_in_shell_order(&mut legacy["features"]);
    for f in legacy["features"].as_array_mut().into_iter().flatten() {
        f["kernel_piece_order"] = json!(true);
    }
    assert_eq!(legacy, current, "the migration preserves every prior field");
    legacy["format"] = json!(crate::format::format_string(crate::format::FORMAT + 1));
    assert!(crate::format::read(&legacy.to_string()).is_err());
}

#[test]
fn regularized_shell_keeps_legacy_and_explicit_single_face_projections_closed() {
    let (mut doc, _, _) = shell_document();
    let mut model = Model::new();
    for (width, wall) in [(20., 2.), (30., 3.)] {
        doc.set_param("width", &width.to_string(), Some("mm"), None).unwrap();
        doc.set_param("wall", &wall.to_string(), Some("mm"), None).unwrap();
        model.evaluate(&doc);
        let loaded = crate::format::read(&doc.to_json()).unwrap();
        let mut cold = Model::new();
        cold.evaluate(&loaded);
        for state in [model.state(), cold.state()] {
            let body = state.body("Case").unwrap();
            let index = face_index(body, false);
            let mesh = body.mesh();
            assert_eq!(body.body.face_sources().unwrap().iter().filter(|source|matches!(source,Some(solidcraft_kernel::FaceSource::ShellPlanar {inner:false,source,..}) if matches!(source.as_ref(),solidcraft_kernel::FaceSource::Box {axis:0,high:true,..}))).count(),1,"no physical outer-wall strip face may remain");
            let plane = body.body.face_plane(index).unwrap();
            let point = mesh
                .triangles
                .iter()
                .zip(&mesh.tri_face)
                .find_map(|(tri, face)| (*face as usize == index).then(|| mesh.tri(tri)).flatten())
                .map(|[a, b, c]| (a + b + c) / 3.)
                .unwrap();
            let source = solidcraft_sketch::LinkSource::Face { body: "Case".into(), at: point };
            let (values, _) = doc.param_values();
            let legacy = crate::project::resolve(&doc, &values, &state, &plane, solidcraft_sketch::LinkKind::Project, &source).unwrap();
            let explicit = crate::project::resolve_face_indices(&state, &plane, solidcraft_sketch::LinkKind::Project, "Case", &[index]).unwrap();
            assert_eq!(legacy.geom, explicit.geom);
            assert_eq!(explicit.geom.len(), 4, "raw planar piece keeps all four edges");
            let mut sketch = solidcraft_sketch::Sketch::new();
            let link = sketch.add_link(solidcraft_sketch::LinkKind::Project, source, &legacy.geom).unwrap();
            sketch.links[0].face_name = Some(crate::naming::face_names(body)[index].clone());
            let warnings = crate::project::refresh_links(&doc, &values, &state, &plane, &mut sketch);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(!sketch.links[0].lost);
            assert!(!sketch.update_link(&link, &explicit.geom).unwrap());
        }
    }
}

#[test]
fn saved_former_strip_projection_reports_lost_without_expanding_healthy_material() {
    let (mut doc, _, _) = shell_document();
    let mut model = Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let body = state.body("Case").unwrap();
    let index = face_index(body, false);
    let plane = body.body.face_plane(index).unwrap();
    let stem = crate::naming::face_names(body)[index].clone();
    assert_eq!(crate::naming::strip_piece(&stem), stem);
    // The builds before format 14 split this wall into strips along the opening and saved a
    // projection of one of them by its number. The kernel's shell leaves the wall one face now,
    // so the strip exists only as that saved name and geometry.
    assert_eq!(crate::naming::face_name_at_body(body, Vec3::new(20., 6., 1.)).as_deref(), Some(stem.as_str()), "the shelled wall is one face");
    // What the builds before format 14 saved for that piece: its number by centroid order.
    let former = format!("{stem}#1");
    let mut sketch = solidcraft_sketch::Sketch::new();
    let corners = [Vec2::new(0., 0.), Vec2::new(12., 0.), Vec2::new(12., 2.), Vec2::new(0., 2.)];
    let geometry: Vec<_> = (0..4).map(|i| solidcraft_sketch::LinkGeom::Line(corners[i], corners[(i + 1) % 4])).collect();
    sketch
        .add_link(
            solidcraft_sketch::LinkKind::Project,
            solidcraft_sketch::LinkSource::Face { body: "Case".into(), at: Vec3::new(20., 6., 1.) },
            &geometry,
        )
        .unwrap();
    sketch.links[0].face_name = Some(former.clone());
    let original = sketch.clone();
    let sk = doc
        .add_feature(
            FeatureKind::Sketch { plane: PlaneRef::Face { plane, at: Vec3::new(20., 6., 1.), name: Some(former), anchor: None }, sketch },
            Some("Saved strip sketch"),
        )
        .unwrap();
    let boss = doc
        .add_feature(
            FeatureKind::Extrude {
                thin: None,
                sketch: crate::SketchId(sk),
                profiles: crate::ProfileSel::All,
                extent: serde_json::from_value(json!({"distance":"1"})).unwrap(),
                operation: crate::Operation::NewBody,
                targets: vec![],
            },
            Some("Saved strip profile material"),
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    legacy["format"] = json!("solidcraft/12");
    let mut doc = crate::format::read(&legacy.to_string()).unwrap();
    for width in [20., 30.] {
        doc.set_param("width", &width.to_string(), Some("mm"), None).unwrap();
        doc.set_param("wall", "3", Some("mm"), None).unwrap();
        model.evaluate(&doc);
        let loaded = crate::format::read(&doc.to_json()).unwrap();
        let mut cold = Model::new();
        cold.evaluate(&loaded);
        for current in [&model, &cold] {
            let warning = current.results.iter().find(|r| r.id == sk).unwrap().warning.as_deref().unwrap();
            assert!(warning.contains("no longer exists; last geometry retained"), "{warning}");
            let state = current.state();
            let solved = state.sketch(sk).unwrap();
            assert!(solved.sketch.links[0].lost);
            assert_eq!(solved.sketch.curves, original.curves);
            assert_eq!(solved.sketch.points, original.points);
            let material = state.bodies.iter().find(|body| body.feature == boss).unwrap();
            assert!(
                (solidcraft_kernel::measure(&material.body).unwrap().volume - 24.).abs() < 1e-8,
                "old24mm3 profile must not silently become120mm3 wall profile"
            );
            let bounds = material.mesh().bounds();
            assert!((bounds.min.x - width).abs() < 1e-8 && (bounds.max.x - width - 1.).abs() < 1e-8);
            assert!((bounds.max.y - 12.).abs() < 1e-8 && (bounds.max.z - 2.).abs() < 1e-8);
        }
    }
}

#[test]
fn saved_curved_strip_projection_is_lost_after_exact_wall_coalescence() {
    use solidcraft_kernel::{FacePaint, Paint};
    use solidcraft_sketch::{LinkKind, LinkSource, Sketch};
    let mut doc = Document::new("saved curved strip");
    doc.set_param("width", "80", Some("mm"), None).unwrap();
    doc.set_param("wall", "2", Some("mm"), None).unwrap();
    let base = doc
        .add_feature(
            serde_json::from_value(json!({"type":"box","corner":["-width/2",-30,0],"length":"width","width":"60","height":"30"})).unwrap(),
            Some("Case"),
        )
        .unwrap();
    doc.feature_mut(base).unwrap().body_names = vec!["Case".into()];
    doc.add_feature(
        serde_json::from_value(json!({"type":"fillet","body":"Case","edges":[[40,30,15],[-40,30,15],[-40,-30,15],[40,-30,15]],"radius":"6"}))
            .unwrap(),
        Some("Corners"),
    )
    .unwrap();
    let shell = doc
        .add_feature(
            FeatureKind::Shell {
                faces: vec![crate::FacePick(Vec3::new(0., 0., 30.))],
                thickness: "wall".into(),
                body: Some("Case".into()),
                direction: "inside".into(),
                tangent_chain: false,
            },
            Some("Shell"),
        )
        .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    let before = model.state_before(shell);
    // Per-face paint disables the construction optimization: this control keeps
    // the real pre-repair slab faces without forging a name or source witness.
    let old_source = before
        .body("Case")
        .unwrap()
        .body
        .clone()
        .with_paint(Some(Paint { opacity: 1., faces: vec![FacePaint { face: 0, color: [1., 0., 0.], opacity: 1. }] }));
    let cavity_box = solidcraft_kernel::box_solid(Vec3::new(-38., -28., 2.), Vec3::new(38., 28., 35.)).unwrap();
    let cavity = solidcraft_kernel::fillet(
        &cavity_box,
        &[Vec3::new(38., 28., 15.), Vec3::new(-38., 28., 15.), Vec3::new(-38., -28., 15.), Vec3::new(38., -28., 15.)],
        4.,
    )
    .unwrap();
    let raw = solidcraft_kernel::boolean(&old_source, &cavity, solidcraft_kernel::BoolOp::Cut).unwrap().unwrap();
    let mut old_body = ModelBody::new("Case".into(), raw, shell);
    old_body.names = crate::naming::NameCell::new(Some(std::sync::Arc::new(crate::naming::NamingOrigin {
        feature: doc.feature(shell).unwrap().clone(),
        before,
        depth: 1,
        mats: vec![],
        sheets: vec![],
    })));
    let t = std::f64::consts::FRAC_1_SQRT_2;
    let at = Vec3::new(34. + 6. * t, 24. + 6. * t, 1.);
    let former = crate::naming::face_name_at_body(&old_body, at).unwrap();
    let stem = crate::naming::strip_piece(&former).to_string();
    assert_ne!(former, stem, "actual old cylindrical strip must have a piece suffix");
    let state = model.state();
    let current = state.body("Case").unwrap();
    let names = crate::naming::face_names(current);
    let index = names.iter().position(|name| name == &stem).unwrap();
    assert!(current.body.face_was_coalesced(index));
    assert!(current.body.face_plane(index).is_none(), "witness grants no planar attachment authority");
    assert!(old_body.body.face_sources().is_none_or(|sources| sources.iter().enumerate().all(|(index, _)| !old_body.body.face_was_coalesced(index))));
    let mut old_state = (*model.state()).clone();
    old_state.bodies = vec![old_body];
    let source = LinkSource::Face { body: "Case".into(), at };
    // Recover the saved projection from the old face's complete raw topology.
    // Display seam suppression must not discard a historical strip boundary.
    let old_body = old_state.body("Case").unwrap();
    let old_index = crate::naming::face_on_body(old_body, &former, at).unwrap().0;
    let mesh = old_body.mesh();
    let mut geometry = Vec::new();
    for (index, faces) in mesh.edge_faces.iter().enumerate() {
        if !faces.contains(&(old_index as u32)) {
            continue;
        }
        let points = &mesh.edges[index];
        let a = solidcraft_geom::Plane::YZ.to_local(*points.first().unwrap());
        let b = solidcraft_geom::Plane::YZ.to_local(*points.last().unwrap());
        let (a, b) = (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y));
        assert!(a.dist(b) > 1e-6);
        for point in points {
            let q = solidcraft_geom::Plane::YZ.to_local(*point);
            let q = Vec2::new(q.x, q.y);
            let direction = b - a;
            let t = ((q - a).dot(direction) / direction.dot(direction)).clamp(0., 1.);
            assert!(q.dist(a + direction * t) < 1e-7, "exact cylindrical quadrant projects to a straight support");
        }
        geometry.push(solidcraft_sketch::LinkGeom::Line(a, b));
    }
    assert_eq!(geometry.len(), 4, "historical curved strip has a complete four-edge rectangular projection");
    let mut sketch = Sketch::new();
    sketch.add_link(LinkKind::Project, source, &geometry).unwrap();
    sketch.links[0].face_name = Some(former);
    let original = sketch.clone();
    let sk = doc
        .add_feature(FeatureKind::Sketch { plane: PlaneRef::Custom { plane: solidcraft_geom::Plane::YZ }, sketch }, Some("Saved corner strip"))
        .unwrap();
    let boss = doc
        .add_feature(
            FeatureKind::Extrude {
                thin: None,
                sketch: crate::SketchId(sk),
                profiles: crate::ProfileSel::All,
                extent: serde_json::from_value(json!({"distance":"1"})).unwrap(),
                operation: crate::Operation::NewBody,
                targets: vec![],
            },
            Some("Retained strip material"),
        )
        .unwrap();
    for (width, wall) in [(80., 2.), (100., 3.)] {
        doc.set_param("width", &width.to_string(), Some("mm"), None).unwrap();
        doc.set_param("wall", &wall.to_string(), Some("mm"), None).unwrap();
        model.evaluate(&doc);
        let loaded = crate::format::read(&doc.to_json()).unwrap();
        let mut cold = Model::new();
        cold.evaluate(&loaded);
        for current in [&model, &cold] {
            let result = current.results.iter().find(|r| r.id == sk).unwrap();
            assert!(result.error.is_none());
            assert!(result.warning.as_deref().unwrap().contains("no longer exists; last geometry retained"));
            let state = current.state();
            let solved = state.sketch(sk).unwrap();
            assert!(solved.sketch.links[0].lost);
            assert_eq!(solved.sketch.points, original.points);
            assert_eq!(solved.sketch.curves, original.curves);
            let material = state.bodies.iter().find(|body| body.feature == boss).unwrap();
            assert!(
                (solidcraft_kernel::measure(&material.body).unwrap().volume - 12.).abs() < 1e-7,
                "two-mm strip cannot expand to a full thirty-mm wall"
            );
        }
    }
}

#[test]
fn saved_stepped_tab_cut_piece_projection_is_lost_without_expanding_material() {
    use solidcraft_sketch::{LinkKind, LinkSource, Sketch};
    let mut doc = Document::new("public stepped tab case");
    doc.set_param("length", "40", Some("mm"), None).unwrap();
    doc.set_param("wall", "2", Some("mm"), None).unwrap();
    for (name, kind) in [
        ("Case", json!({"type":"box","corner":[0,0,0],"length":"length","width":"30","height":"20"})),
        ("Tab", json!({"type":"box","corner":["length",10,12],"length":"8","width":"10","height":"8"})),
    ] {
        let id = doc.add_feature(serde_json::from_value(kind).unwrap(), Some(name)).unwrap();
        doc.feature_mut(id).unwrap().body_names = vec![name.into()];
    }
    doc.add_feature(
        FeatureKind::Combine { target: "Case".into(), tools: vec!["Tab".into()], operation: crate::Operation::Join, keep_tools: false },
        None,
    )
    .unwrap();
    let tool = doc
        .add_feature(
            serde_json::from_value(
                json!({"type":"box","corner":["wall","wall","wall"],"length":"length-2*wall","width":"30-2*wall","height":"22-wall"}),
            )
            .unwrap(),
            Some("Pocket tool"),
        )
        .unwrap();
    doc.feature_mut(tool).unwrap().body_names = vec!["Pocket tool".into()];
    doc.add_feature(
        FeatureKind::Combine { target: "Case".into(), tools: vec!["Pocket tool".into()], operation: crate::Operation::Cut, keep_tools: false },
        Some("Pocket"),
    )
    .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
    // Older builds left Case's -x wall split by the Join and the Cut and saved a projection of
    // the patch y 10..20, z 0..20 by its piece name. The kernel leaves the wall one face now: the
    // patch is only that saved name and its rectangle on YZ.
    let at = Vec3::new(0., 15., 1.);
    let plane = solidcraft_geom::Plane::YZ;
    let current_state = model.state();
    let current = current_state.body("Case").unwrap();
    let stem = crate::naming::face_name_at_body(current, at).unwrap();
    assert_eq!(crate::naming::strip_piece(&stem), stem, "the -x wall is one face");
    let former = format!("{stem}#1");
    let current_names = crate::naming::face_names(current);
    let merged = current_names.iter().position(|name| name == &stem).unwrap();
    // The Join leaves Case's -x wall on its own support, so the wall keeps its exact box
    // identity through the Join and the Cut: one face again, no longer the saved piece.
    assert!(
        matches!(
            current.body.face_sources().and_then(|sources| sources.get(merged)).cloned().flatten(),
            Some(solidcraft_kernel::FaceSource::Box { axis: 0, high: false, .. })
        ),
        "the merged wall carries its exact construction identity"
    );
    assert!(!current_names.contains(&former));
    let corners = [Vec2::new(10., 0.), Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(10., 20.)];
    let geometry: Vec<_> = (0..4).map(|i| solidcraft_sketch::LinkGeom::Line(corners[i], corners[(i + 1) % 4])).collect();
    let mut sketch = Sketch::new();
    sketch.add_link(LinkKind::Project, LinkSource::Face { body: "Case".into(), at }, &geometry).unwrap();
    sketch.links[0].face_name = Some(former);
    let original = sketch.clone();
    let captured = original.points.iter().filter(|p| p.link.is_some()).map(|p| p.pos).collect::<Vec<_>>();
    assert_eq!(captured.len(), 4);
    assert!(captured.iter().all(|p| (p.x == 10. || p.x == 20.) && (p.y == 0. || p.y == 20.)));
    let sk = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Custom { plane }, sketch }, Some("Saved wall patch")).unwrap();
    let material = doc
        .add_feature(
            FeatureKind::Extrude {
                thin: None,
                sketch: crate::SketchId(sk),
                profiles: crate::ProfileSel::All,
                extent: serde_json::from_value(json!({"distance":"1"})).unwrap(),
                operation: crate::Operation::NewBody,
                targets: vec![],
            },
            Some("Saved band stock"),
        )
        .unwrap();
    for (length, wall) in [(40., 2.), (50., 3.)] {
        doc.set_param("length", &length.to_string(), Some("mm"), None).unwrap();
        doc.set_param("wall", &wall.to_string(), Some("mm"), None).unwrap();
        model.evaluate(&doc);
        let reopened = crate::format::read(&doc.to_json()).unwrap();
        let mut cold = Model::new();
        cold.evaluate(&reopened);
        for current in [&model, &cold] {
            let result = current.results.iter().find(|r| r.id == sk).unwrap();
            assert!(result.error.is_none(), "{result:?}");
            assert!(result.warning.as_deref().is_some_and(|w| w.contains("no longer exists; last geometry retained")), "{result:?}");
            let state = current.state();
            let saved = state.sketch(sk).unwrap();
            assert!(saved.sketch.links[0].lost);
            assert_eq!(saved.sketch.points, original.points);
            assert_eq!(saved.sketch.curves, original.curves);
            let stock = state.bodies.iter().find(|b| b.feature == material).unwrap();
            assert!(
                (solidcraft_kernel::measure(&stock.body).unwrap().volume - 200.).abs() < 1e-7,
                "saved10x20 patch must not become30x20 wall; actualvolume={}, bounds={:?}, originalpoints={:?}",
                solidcraft_kernel::measure(&stock.body).unwrap().volume,
                stock.mesh().bounds(),
                original.points
            );
            let case = state.body("Case").unwrap();
            let expected = length * 30. * 20. + 8. * 10. * 8. - (length - 2. * wall) * (30. - 2. * wall) * (20. - wall);
            assert!((solidcraft_kernel::measure(&case.body).unwrap().volume - expected).abs() < 1e-6);
        }
    }
}
