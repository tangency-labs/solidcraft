use crate::{Document, ModelBody, ModelState};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{LinkGeom, LinkKind, LinkSource, Sketch};
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
fn tilted_spline_projection_keeps_one_exact_curve() {
    use solidcore_record::{Seg2, Vec2};
    use solidcraft_sketch::{CurveKind, LinkGeom, Nurbs};

    let from = Plane::XY.offset(9.);
    let to = Plane::new(Vec3::new(-2., 3., 1.), Vec3::new(0.8, 0., 0.6), Vec3::Y).unwrap();
    for (control, periodic, rational) in [(false, false, false), (false, true, false), (true, false, false), (true, false, true)] {
        let mut sketch = Sketch::default();
        let mut points: Vec<Vec2> = [[0., 0.], [5., 18.], [25., -5.], [40., 9.], [50., 1.]].into_iter().map(|[x, y]| Vec2::new(x, y)).collect();
        if periodic {
            points.push(points[0]);
        }
        let pts = points.iter().map(|p| sketch.add_point(*p, None).unwrap()).collect();
        let nurbs = rational.then(|| Nurbs { knots: vec![0., 0., 0., 0., 0.3, 1., 1., 1., 1.], weights: vec![1., 0.8, 1.4, 0.9, 1.] });
        let fit_spans = (!control).then(|| solidcraft_sketch::fit_beziers_mode(&points, periodic).unwrap());
        let chords: Vec<_> = points.windows(2).map(|p| p[0].dist(p[1]).max(1e-9)).collect();
        let total: f64 = chords.iter().sum();
        let expected: Vec<Vec2> = (0..=128)
            .map(|i| {
                let t = f64::from(i) / 128.;
                if let Some(spans) = &fit_spans {
                    // Compact knots follow source chord parameter, while the open sketch's
                    // public t advances uniformly per span. Compare the same geometric point.
                    let mut u = t * total;
                    let mut span = 0;
                    while span + 1 < spans.len() && u > chords[span] {
                        u -= chords[span];
                        span += 1;
                    }
                    let [p0, p1, p2, p3] = spans[span];
                    Seg2::Cubic { p0, p1, p2, p3 }.point_at((u / chords[span]).clamp(0., 1.))
                } else {
                    solidcraft_sketch::spline_point_mode(&points, control, 3, false, nurbs.as_ref(), t)
                }
            })
            .map(|p| to.to_local(from.to_world(p)))
            .collect();
        sketch.add_curve(CurveKind::Spline { pts, control, degree: 3, periodic, nurbs, handles: Vec::new() }, Some("source")).unwrap();
        let report = solidcraft_sketch::solve(&mut sketch);
        let mut state = ModelState::default();
        state.sketches.push(Arc::new(crate::eval::SolvedSketch {
            feature: 1,
            name: "Source".into(),
            plane: from,
            sketch,
            report,
            profiles: Vec::new(),
            profiles_3d: Vec::new(),
            ends: Default::default(),
        }));
        let resolved = crate::project::resolve(
            &Document::default(),
            &BTreeMap::new(),
            &state,
            &to,
            LinkKind::Project,
            &LinkSource::SketchCurve { sketch: 1, curve: "source".into() },
        )
        .unwrap();
        assert!(matches!(resolved.geom.as_slice(), [LinkGeom::Spline { .. }]), "projected spline must stay one spline: {:?}", resolved.geom);
        let mut projected = Sketch::default();
        let link = projected.add_link(LinkKind::Project, resolved.source, &resolved.geom).unwrap();
        let ci = projected.link_curves(&link)[0];
        assert!(projected.reverse_spline(ci).is_err(), "linked curves follow their source");
        projected.break_link(&link).unwrap();
        for reversed in [false, true, false] {
            if reversed {
                projected.reverse_spline(ci).unwrap();
            }
            let CurveKind::Spline { pts, control: projected_control, degree, periodic, nurbs, .. } = &projected.curves[ci].kind else {
                panic!("projection is not a spline")
            };
            let points: Vec<Vec2> = pts.iter().map(|p| projected.point(*p).unwrap()).collect();
            for (i, expected) in expected.iter().enumerate() {
                let t = i as f64 / 128.;
                let actual = solidcraft_sketch::spline_point_mode(
                    &points,
                    *projected_control,
                    *degree,
                    *periodic,
                    nurbs.as_ref(),
                    if reversed { 1. - t } else { t },
                );
                assert!(
                    actual.dist(*expected) < 1e-8,
                    "control={control}, periodic={periodic}, rational={rational}, reversed={reversed}, t={t}, actual={actual:?}, expected={expected:?}"
                );
            }
            if reversed {
                projected.reverse_spline(ci).unwrap();
            }
        }
    }
}

#[test]
fn projecting_a_fit_spline_keeps_edited_tangent_handles_on_parallel_and_tilted_planes() {
    use solidcraft_sketch::{CurveKind, LinkGeom};

    let from = Plane::XY.offset(9.);
    let tilted = Plane::new(Vec3::new(-2., 3., 1.), Vec3::new(0.8, 0., 0.6), Vec3::Y).unwrap();
    for periodic in [false, true] {
        let mut sketch = Sketch::default();
        let mut points = vec![Vec2::new(0., 0.), Vec2::new(8., 13.), Vec2::new(20., -2.)];
        if periodic {
            points.push(points[0]);
        }
        let pts = points.iter().map(|p| sketch.add_point(*p, None).unwrap()).collect();
        let curve = sketch
            .add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic, nurbs: None, handles: Vec::new() }, Some("source"))
            .unwrap();
        sketch.add_spline_handles(curve).unwrap();
        let CurveKind::Spline { handles, .. } = &sketch.curves[curve].kind else { panic!("not a spline") };
        let handle = handles[1];
        sketch.points[handle].pos += Vec2::new(3., -2.);
        for reversed in [false, true] {
            let mut source = sketch.clone();
            if reversed {
                source.reverse_spline(curve).unwrap();
            }
            let report = solidcraft_sketch::solve(&mut source);
            let CurveKind::Spline { pts, handles, .. } = &source.curves[curve].kind else { panic!("not a spline") };
            let points: Vec<_> = pts.iter().map(|p| source.point(*p).unwrap()).collect();
            let handles: Vec<_> = handles.iter().map(|p| source.point(*p).unwrap()).collect();
            let expected = solidcraft_sketch::fit_beziers_with_handles(&points, &handles, periodic).unwrap();
            let mut state = ModelState::default();
            state.sketches.push(Arc::new(crate::eval::SolvedSketch {
                feature: 1,
                name: "Source".into(),
                plane: from,
                sketch: source,
                report,
                profiles: Vec::new(),
                profiles_3d: Vec::new(),
                ends: Default::default(),
            }));
            for to in [Plane::XY, tilted] {
                let resolved = crate::project::resolve(
                    &Document::default(),
                    &BTreeMap::new(),
                    &state,
                    &to,
                    LinkKind::Project,
                    &LinkSource::SketchCurve { sketch: 1, curve: "source".into() },
                )
                .unwrap();
                assert_eq!(resolved.geom.len(), expected.len(), "one exact curve per handled span");
                for (actual, span) in resolved.geom.iter().zip(&expected) {
                    let LinkGeom::Spline { pts, control: true, degree: 3, .. } = actual else { panic!("not an exact cubic: {actual:?}") };
                    assert_eq!(pts.len(), 4);
                    for (actual, point) in pts.iter().zip(span) {
                        let expected = to.to_local(from.to_world(*point));
                        assert!(actual.dist(expected) < 1e-10, "periodic={periodic}, reversed={reversed}: {actual:?} != {expected:?}");
                    }
                }
            }
        }
    }
}

fn named_box(name: &str, width: f64, height: f64) -> ModelBody {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(width, width, height)).unwrap();
    let mut names = vec!["unrelated".to_string(); body.faces(0.01).unwrap().len()];
    let top = body.faces(0.01).unwrap().into_iter().find(|face| face.plane_normal.is_some_and(|normal| normal.z > 0.99)).unwrap().index;
    names[top] = "shared:top".into();
    let mut result = ModelBody::new(name.into(), body, 1);
    result.names = crate::naming::NameCell::known(Arc::new(names));
    result
}

#[test]
fn named_manual_projection_keeps_its_body_and_face_at_shared_boundaries() {
    let doc = Document::default();
    let mut initial = ModelState::default();
    initial.bodies.extend([named_box("Source", 20., 10.), named_box("Control", 5., 10.)]);
    let plane = Plane::XY.offset(20.);
    let source = LinkSource::Face { body: "Source".into(), at: Vec3::new(1., 1., 10.) };
    let resolved = crate::project::resolve(&doc, &BTreeMap::new(), &initial, &plane, LinkKind::Project, &source).unwrap();
    let mut sketch = Sketch::default();
    let link = sketch.add_link(LinkKind::Project, source, &resolved.geom).unwrap();
    sketch.links[0].face_name = Some("shared:top".into());
    sketch.links[0].source = LinkSource::Face { body: "Source".into(), at: Vec3::new(0., 0., 10.) };
    let mut current = ModelState::default();
    current.bodies.extend([named_box("Source", 20., 20.), named_box("Control", 5., 10.)]);
    assert!(crate::project::refresh_links(&doc, &BTreeMap::new(), &current, &plane, &mut sketch).unwrap().is_empty());
    assert!(!sketch.links[0].lost);
    assert!(sketch.points.iter().any(|point| point.pos.x > 19.));
    assert!(sketch.points.iter().any(|point| point.pos.y > 19.));
    assert!(matches!(&sketch.links[0].source,LinkSource::Face{body,at} if body=="Source" && (at.z-20.).abs()<1e-6));
    // The closest named point can sit on the wall/top seam: retain its actual
    // face index rather than re-picking the adjacent vertical wall.
    let index = crate::naming::face_on_body(current.body("Source").unwrap(), "shared:top", Vec3::new(0., 0., 10.)).unwrap().0;
    assert!(current.body("Source").unwrap().body.faces(0.01).unwrap()[index].plane_normal.unwrap().z > 0.99);
    let saved = sketch.clone();
    let mut missing = ModelState::default();
    missing.bodies.push(named_box("Control", 5., 10.));
    // Its body gone, the named face finds nothing: the sketch fails, asking for it again.
    let error = crate::project::refresh_links(&doc, &BTreeMap::new(), &missing, &plane, &mut sketch).unwrap_err();
    assert!(matches!(&error, crate::DocError::Reselect { reference, .. } if reference == "shared:top"), "{error}");
    assert_eq!(sketch.curves, saved.curves);
    assert_eq!(sketch.points, saved.points);
    assert_eq!(sketch.links[0].id, link);
    assert!(matches!(&sketch.links[0].source,LinkSource::Face{body,..} if body=="Source"));
}

#[test]
fn numbered_manual_projection_rebuilds_from_the_whole_planar_face() {
    let mut doc = Document::default();
    doc.set_param("size", "5", Some("mm"), None).unwrap();
    let kind = serde_json::from_value(serde_json::json!({
        "type": "box", "corner": [0, 0, 0], "length": "size", "width": "size", "height": "10"
    }))
    .unwrap();
    let feature = doc.add_feature(kind, None).unwrap();
    doc.feature_mut(feature).unwrap().body_names = vec!["Source".into()];
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    let plane = Plane::XY.offset(20.);
    let source = LinkSource::Face { body: "Source".into(), at: Vec3::new(2., 2., 10.) };
    let resolved = crate::project::resolve(&doc, &BTreeMap::new(), &model.state(), &plane, LinkKind::Project, &source).unwrap();
    let mut sketch = Sketch::default();
    let link = sketch.add_link(LinkKind::Project, source, &resolved.geom).unwrap();
    let numbered = format!("F{feature}:+z#1");
    sketch.links[0].face_name = Some(numbered.clone());

    doc.set_param("size", "20", Some("mm"), None).unwrap();
    model.evaluate(&doc);
    assert!(crate::project::refresh_links(&doc, &BTreeMap::new(), &model.state(), &plane, &mut sketch).unwrap().is_empty());
    assert!(!sketch.links[0].lost);
    assert_eq!(sketch.links[0].face_name.as_deref(), Some(numbered.as_str()));
    assert_eq!(sketch.link_curves(&link).len(), 4);
    assert!(sketch.points.iter().any(|point| point.pos.x > 19. && point.pos.y > 19.));

    let saved = model.saved(&doc).unwrap();
    let reopened = crate::format::read(&doc.to_json()).unwrap();
    let cached = crate::Model::adopt(&reopened, &saved).unwrap();
    let state = cached.state();
    assert!(state.body("Source").unwrap().names.origin.is_none());
    assert_eq!(state.body("Source").unwrap().names.cached_construction, Some(feature));
    assert!(crate::project::refresh_links(&reopened, &BTreeMap::new(), &state, &plane, &mut sketch).unwrap().is_empty());
    assert!(!sketch.links[0].lost);
}

#[test]
fn numbered_round_face_projection_recovers_the_whole_face() {
    let doc = Document {
        features: serde_json::from_value(serde_json::json!([
            {
                "id": 1, "name": "Sketch1", "type": "sketch", "plane": {"name": "XY", "type": "origin"},
                "sketch": {
                    "points": [{"fixed": true, "id": "origin", "pos": [0., 0.]}, {"fixed": false, "id": "c1.center", "pos": [0., 0.]}],
                    "curves": [{"c": 1, "construction": false, "id": "c1", "r": 20., "type": "circle"}],
                    "constraints": [{"id": "k1", "p": 0, "q": 1, "type": "coincident"}],
                    "counters": {"k": 1, "l": 0, "p": 1}
                }
            },
            {
                "id": 2, "name": "Extrude1", "type": "extrude", "body_names": ["Body1"],
                "extent": {"convention": "total_distance", "direction": "positive", "distance": "10"},
                "operation": "new_body", "profiles": {"loops": [["c1"]], "type": "curves"}, "sketch": 1
            },
            {
                "id": 3, "name": "Sketch2", "type": "sketch", "plane": {"name": "YZ", "type": "origin"},
                "sketch": {
                    "points": [
                        {"fixed": true, "id": "origin", "pos": [0., 0.]},
                        {"fixed": false, "id": "p1", "pos": [-20., 0.]},
                        {"fixed": false, "id": "p2", "pos": [20., 0.]},
                        {"fixed": false, "id": "p3", "pos": [-20., 10.]},
                        {"fixed": false, "id": "p4", "pos": [20., 10.]}
                    ],
                    "curves": [
                        {"a": 1, "b": 2, "construction": false, "id": "l1", "link": "j1", "type": "line"},
                        {"a": 3, "b": 4, "construction": false, "id": "l2", "link": "j1", "type": "line"}
                    ],
                    "constraints": [],
                    "counters": {"k": 0, "l": 2, "p": 4},
                    "links": [{
                        "face_name": "F2:side:c1#1", "id": "j1", "kind": "project",
                        "source": {"at": [-19.87, -2.3, 6.], "body": "Body1", "type": "face"}
                    }]
                }
            }
        ]))
        .unwrap(),
        ..Document::default()
    };
    let doc = crate::format::read(&doc.to_json()).unwrap();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let body = state.body("Body1").unwrap();
    assert!(crate::naming::face_names(body).iter().any(|name| name == "F2:side:c1"));
    let result = model.result(3).unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let projected = &state.sketch(3).expect("the saved projection should rebuild").sketch;
    assert_eq!(projected.links[0].face_name.as_deref(), Some("F2:side:c1#1"));
    assert_eq!(projected.link_curves("j1").len(), 2);

    // The edge-on projection of the round wall can retain its saved geometry with a warning.
    // The numbered reference must evaluate the same way as the whole face's existing name.
    let mut exact_doc = doc.clone();
    exact_doc.sketch_mut(3).unwrap().links[0].face_name = Some("F2:side:c1".into());
    let mut exact_model = crate::Model::new();
    exact_model.evaluate(&exact_doc);
    let exact_state = exact_model.state();
    let exact = &exact_state.sketch(3).unwrap().sketch;
    assert_eq!(result.warning, exact_model.result(3).unwrap().warning);
    assert_eq!(projected.links[0].lost, exact.links[0].lost);
    assert_eq!(projected.curves, exact.curves);
    assert_eq!(projected.points, exact.points);

    let saved = model.saved(&doc).expect("the healthy projection should save its model cache");
    let mut reopened = crate::format::read(&doc.to_json()).unwrap();
    let mut reopened_model = crate::Model::adopt(&reopened, &saved).unwrap();
    assert!(reopened_model.state().body("Body1").unwrap().names.origin.is_none());
    assert_eq!(reopened_model.state().body("Body1").unwrap().names.cached_construction, Some(2));
    let crate::FeatureKind::Sketch { plane, .. } = &mut reopened.feature_mut(3).unwrap().kind else { panic!("projection sketch") };
    *plane = crate::PlaneRef::Custom { plane: Plane::YZ.offset(0.1) };
    reopened_model.evaluate(&reopened);
    assert!(reopened_model.result(3).unwrap().error.is_none(), "{:?}", reopened_model.result(3));
    let pick = Vec3::new(-19.87, -2.3, 6.);
    assert!(crate::naming::face_on_body(body, "F2:side:c1#1", pick).is_none(), "ordinary lookup stays strict");
    let mut split = body.clone();
    let mut names = crate::naming::face_names(body).as_ref().clone();
    *names.iter_mut().find(|name| name.as_str() != "F2:side:c1").unwrap() = "F2:side:c1#2#[wall]".into();
    let origin = split.names.origin.clone();
    split.names = crate::naming::NameCell::known(Arc::new(names));
    split.names.origin = origin;
    assert!(crate::naming::face_on_body(&split, "F2:side:c1#1", pick).is_none());
    assert!(crate::name_lookup::locate_projected_face(&split, "F2:side:c1#1", pick).is_none(), "another piece still exists");

    let mut outside = doc.sketch(3).unwrap().clone();
    outside.links[0].source = LinkSource::Face { body: "Body1".into(), at: Vec3::new(-19.87, -2.3, 12.) };
    let error = crate::project::refresh_links(&doc, &BTreeMap::new(), &state, &Plane::YZ, &mut outside).unwrap_err();
    assert!(matches!(&error, crate::DocError::Reselect { reference, .. } if reference == "F2:side:c1#1"), "{error}");
}

#[test]
fn numbered_projection_refuses_modified_or_moved_cached_sources() {
    for (kind, pick) in [
        (serde_json::json!({"type": "shell", "body": "Source", "faces": [[10, 10, 10]], "thickness": "1"}), Vec3::new(0., 5., 5.)),
        (serde_json::json!({"type": "move", "bodies": ["Source"], "translate": ["1", "0", "0"]}), Vec3::new(1., 5., 5.)),
    ] {
        let mut doc = Document::default();
        let source = doc
            .add_feature(
                serde_json::from_value(serde_json::json!({
                    "type": "box", "corner": [0, 0, 0], "length": "20", "width": "20", "height": "10"
                }))
                .unwrap(),
                None,
            )
            .unwrap();
        doc.feature_mut(source).unwrap().body_names = vec!["Source".into()];
        doc.add_feature(serde_json::from_value(kind).unwrap(), None).unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|result| result.error.is_none()));
        let saved = model.saved(&doc).unwrap();
        let reopened = crate::format::read(&doc.to_json()).unwrap();
        let cached = crate::Model::adopt(&reopened, &saved).unwrap();
        let state = cached.state();
        let body = state.body("Source").unwrap();
        let whole = format!("F{source}:-x");
        let numbered = format!("{whole}#1");
        assert!(crate::naming::face_on_body(body, &whole, pick).is_some());
        assert!(crate::naming::face_on_body(body, &numbered, pick).is_none());
        assert!(body.names.origin.is_none());
        assert_eq!(body.names.cached_construction, None);
        assert!(crate::name_lookup::locate_projected_face(body, &numbered, pick).is_none());
    }
}

#[test]
fn numbered_projection_refuses_a_pick_inside_a_hole_in_the_named_face() {
    let mut doc = Document::default();
    let mut sketch = Sketch::new();
    sketch.add_circle(Vec2::ZERO, 10., Some(0), Some("c1")).unwrap();
    sketch.add_circle(Vec2::ZERO, 2., Some(0), Some("c2")).unwrap();
    let profile = doc.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
    let kind = serde_json::from_value(serde_json::json!({
        "type": "extrude", "sketch": profile, "profiles": {"type": "curves", "loops": [["c1"]]}, "extent": {"distance": "5"}
    }))
    .unwrap();
    let feature = doc.add_feature(kind, None).unwrap();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let body = state.body("Body1").unwrap();
    let whole = format!("F{feature}:top");
    let top = crate::naming::face_names(body).iter().position(|name| name == &whole).unwrap();
    let numbered = format!("{whole}#1");

    let pick = |x| crate::name_lookup::locate_projected_face(body, &numbered, Vec3::new(x, 0., 5.));
    assert_eq!(pick(5.).map(|(index, _)| index), Some(top));
    assert!(pick(0.).is_none(), "the pick is on the plane but inside the hole");
}

/// A tangent plane on a cylinder saved by 0.30.1, whose kernel split the round side into numbered
/// pieces, names it `F2:side:c1#2`; the side is one face now (#13252). The plane, and the sketch
/// on it, are found again when the cylinder is still the body its feature made; a piece that
/// another face of the surface could be (a split side) or a pick off the face is not.
#[test]
fn a_tangent_plane_on_a_numbered_piece_of_a_round_face_is_found_again() {
    let doc = |name: &str, at: [f64; 3]| Document {
        features: serde_json::from_value(serde_json::json!([
            {
                "id": 1, "name": "Sketch1", "type": "sketch", "plane": {"name": "XY", "type": "origin"},
                "sketch": {
                    "points": [{"fixed": true, "id": "origin", "pos": [0., 0.]}, {"fixed": false, "id": "c1.center", "pos": [0., 0.]}],
                    "curves": [{"c": 1, "construction": false, "id": "c1", "r": 10., "type": "circle"}],
                    "constraints": [{"id": "k1", "p": 0, "q": 1, "type": "coincident"}],
                    "counters": {"k": 1, "l": 0, "p": 1}
                }
            },
            {
                "id": 2, "name": "Extrude1", "type": "extrude", "body_names": ["Body1"],
                "extent": {"convention": "total_distance", "direction": "positive", "distance": "20"},
                "operation": "new_body", "profiles": {"loops": [["c1"]], "type": "curves"}, "sketch": 1
            },
            {
                "id": 3, "name": "Plane1", "type": "construction_plane", "suppressed": false,
                "plane": {"type": "tangent", "at": at, "face": {"type": "face", "at": at, "name": name}}
            },
            {
                "id": 4, "name": "Sketch2", "type": "sketch", "plane": {"feature": 3, "type": "construction"},
                "sketch": {"points": [{"fixed": true, "id": "origin", "pos": [0., 0.]}], "curves": [], "constraints": [], "counters": {}}
            }
        ]))
        .unwrap(),
        ..Document::default()
    };
    let run = |doc: Document| {
        let doc = crate::format::read(&doc.to_json()).unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        model
    };
    let whole = run(doc("F2:side:c1", [10., 0., 10.]));
    assert!(whole.result(3).unwrap().error.is_none(), "{:?}", whole.result(3));
    for piece in ["F2:side:c1#1", "F2:side:c1#2"] {
        let model = run(doc(piece, [10., 0., 10.]));
        assert!(model.result(3).unwrap().error.is_none(), "{piece}: {:?}", model.result(3));
        assert!(model.result(4).unwrap().error.is_none(), "{piece}: {:?}", model.result(4));
    }
    // A pick that is not on the face is still lost.
    let off = run(doc("F2:side:c1#2", [10., 0., 30.]));
    assert!(off.result(3).unwrap().error.is_some());
}

/// An edge picked when its side face was split in two keeps the piece in its name
/// (`F2:side:l1#[F2:start]`); once the side is one face again the edge is still found (#11731).
#[test]
fn an_edge_named_by_a_piece_of_a_face_that_is_whole_again_is_found() {
    let doc = Document {
        features: serde_json::from_value(serde_json::json!([
            {
                "id": 1, "name": "Sketch1", "type": "sketch", "plane": {"name": "XY", "type": "origin"},
                "sketch": {
                    "points": [
                        {"fixed": true, "id": "origin", "pos": [0., 0.]},
                        {"fixed": false, "id": "p1", "pos": [0., 0.]},
                        {"fixed": false, "id": "p2", "pos": [10., 0.]},
                        {"fixed": false, "id": "p3", "pos": [10., 16.1]},
                        {"fixed": false, "id": "p4", "pos": [0., 16.1]}
                    ],
                    "curves": [
                        {"a": 1, "b": 2, "construction": false, "id": "l1", "type": "line"},
                        {"a": 2, "b": 3, "construction": false, "id": "l2", "type": "line"},
                        {"a": 3, "b": 4, "construction": false, "id": "l3", "type": "line"},
                        {"a": 4, "b": 1, "construction": false, "id": "l4", "type": "line"}
                    ],
                    "constraints": [],
                    "counters": {"k": 0, "l": 4, "p": 4}
                }
            },
            {
                "id": 2, "name": "Extrude1", "type": "extrude", "body_names": ["Body1"],
                "extent": {"convention": "total_distance", "direction": "positive", "distance": "5"},
                "operation": "new_body", "profiles": {"loops": [["l1", "l2", "l3", "l4"]], "type": "curves"}, "sketch": 1
            }
        ]))
        .unwrap(),
        ..Document::default()
    };
    let doc = crate::format::read(&doc.to_json()).unwrap();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let body = state.body("Body1").unwrap();
    // The side along x = 0 meets the start cap in the edge under the pick.
    let near = Vec3::new(0., 8.05, 0.);
    let names = crate::naming::edge_names(body);
    let whole = names.iter().find(|n| n.contains("F2:start") && n.contains("F2:side:l4")).expect("the side's edge on the start cap");
    let piece = whole.replace("F2:side:l4", "F2:side:l4#[F2:start]");
    let hit = crate::name_lookup::locate_edge(body, &piece, near).unwrap_or_else(|e| panic!("{piece}: {e}"));
    assert!(hit.on_point, "the edge under the saved pick");
    assert_eq!(hit.point, near);
    assert_eq!(names.get(hit.edge), Some(whole));
    // A piece told apart by a face the side never touches (the opposite side) is still refused.
    let gone = whole.replace("F2:side:l4", "F2:side:l4#[F2:side:l2]");
    assert!(crate::name_lookup::locate_edge(body, &gone, near).is_err(), "{gone}");
}

/// #13464: a mesh's flat patch outline drops a corner part way along a side, even when single
/// precision has moved it off the line (as an STL's corners are), and when the loop starts there.
#[test]
fn a_mesh_outline_drops_a_rounded_corner_part_way_along_a_side() {
    let single = |x: f64| f64::from(x as f32);
    // a, c and d make a triangle; b lies on its side a–c up to single-precision rounding.
    let a = Vec3::new(0.0, 0.0, 0.0);
    let b = Vec3::new(single(0.3), single(0.33), 0.0);
    let c = Vec3::new(1.0, single(1.1), 0.0);
    let d = Vec3::new(0.0, 2.0, 0.0);
    let off = (b - a).cross(c - a).len() / (c - a).len();
    assert!(off > 0.0 && off < 1e-7, "b is off the line only by rounding: {off}");
    // b first, so the outline's loop starts at it.
    let mesh =
        solidcraft_geom::Mesh { positions: vec![b, c, d, a], triangles: vec![[0, 1, 2], [0, 2, 3]], tri_face: vec![0, 0], ..Default::default() };
    let loops = crate::project3d::flat_outline(&mesh, Vec3::new(0.3, 0.8, 0.0));
    assert_eq!(loops.len(), 1, "{loops:?}");
    let corners = loops.first().unwrap();
    assert_eq!(corners.len(), 4, "three corners and the first again: {corners:?}");
    assert!(!corners.iter().any(|p| p.dist(b) < 1e-12), "the corner on the side is gone: {corners:?}");
    for p in [a, c, d] {
        assert!(corners.iter().any(|q| q.dist(p) < 1e-12), "{p:?} is a corner: {corners:?}");
    }
}

/// #13464: far from the origin, a small flat patch keeps its corners. A tolerance that grew with
/// the distance from the origin alone took every corner of a 1 × 1 square at x = 1 000 000 for one
/// on a straight side, and the face projected to nothing.
#[test]
fn a_small_mesh_patch_far_from_the_origin_keeps_its_corners() {
    let x = 1_000_000.0;
    let corners = [Vec3::new(x, 0.0, 0.0), Vec3::new(x + 1.0, 0.0, 0.0), Vec3::new(x + 1.0, 1.0, 0.0), Vec3::new(x, 1.0, 0.0)];
    let mesh =
        solidcraft_geom::Mesh { positions: corners.to_vec(), triangles: vec![[0, 1, 2], [0, 2, 3]], tri_face: vec![0, 0], ..Default::default() };
    let loops = crate::project3d::flat_outline(&mesh, Vec3::new(x + 0.5, 0.5, 0.0));
    assert_eq!(loops.len(), 1, "{loops:?}");
    assert_eq!(loops.first().map(Vec::len), Some(5), "four corners and the first again: {loops:?}");
}

/// A ring turned from a rectangle on YZ about Z: each side of the rectangle makes one round face.
fn ring() -> Document {
    let doc = Document {
        features: serde_json::from_value(serde_json::json!([
            {
                "id": 1, "name": "Sketch1", "type": "sketch", "plane": {"name": "YZ", "type": "origin"},
                "sketch": {
                    "points": [
                        {"fixed": true, "id": "origin", "pos": [0., 0.]},
                        {"fixed": false, "id": "p1", "pos": [10., 0.]},
                        {"fixed": false, "id": "p2", "pos": [20., 0.]},
                        {"fixed": false, "id": "p3", "pos": [20., 5.]},
                        {"fixed": false, "id": "p4", "pos": [10., 5.]}
                    ],
                    "curves": [
                        {"a": 1, "b": 2, "construction": false, "id": "l1", "type": "line"},
                        {"a": 2, "b": 3, "construction": false, "id": "l2", "type": "line"},
                        {"a": 3, "b": 4, "construction": false, "id": "l3", "type": "line"},
                        {"a": 4, "b": 1, "construction": false, "id": "l4", "type": "line"}
                    ],
                    "constraints": [],
                    "counters": {"l": 4, "p": 4}
                }
            },
            {
                "id": 2, "name": "Revolve1", "type": "revolve", "body_names": ["Ring"], "angle": "360 deg",
                "axis": {"axis": "Z", "type": "world"}, "convention": "sketch_normal",
                "operation": "new_body", "profiles": {"type": "all"}, "sketch": 1
            }
        ]))
        .unwrap(),
        ..Document::default()
    };
    crate::format::read(&doc.to_json()).unwrap()
}

#[test]
fn numbered_edge_projection_recovers_the_edge_between_whole_faces() {
    // 0.30.1 split each round face of a revolve into numbered pieces; the edge between two of
    // them is now the edge between the two whole faces, at the same place.
    let doc = ring();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let at = Vec3::new(0., -20., 5.);
    let source = LinkSource::Edge { body: "Ring".into(), at };
    let resolved = crate::project::resolve(&doc, &BTreeMap::new(), &state, &Plane::XY, LinkKind::Project, &source).unwrap();
    let mut sketch = Sketch::default();
    let link = sketch.add_link(LinkKind::Project, source, &resolved.geom).unwrap();
    let numbered = "F2:side:l2#2|F2:side:l3#2";
    sketch.links[0].edge_name = Some(numbered.into());
    let curves = sketch.link_curves(&link).len();
    let saved = sketch.clone();

    let warnings = crate::project::refresh_links(&doc, &BTreeMap::new(), &state, &Plane::XY, &mut sketch).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!sketch.links[0].lost);
    assert_eq!(sketch.links[0].edge_name.as_deref(), Some(numbered));
    assert_eq!(sketch.link_curves(&link).len(), curves);

    // Edges picked by fillets, holes and joints keep the name as picked.
    let body = state.body("Ring").unwrap();
    assert!(crate::naming::locate_edge(body, numbered, at).is_err(), "ordinary lookup stays strict");

    // The saved point on another edge (the inner rim): not this edge, so it asks to reselect.
    let mut elsewhere = saved;
    elsewhere.links[0].source = LinkSource::Edge { body: "Ring".into(), at: Vec3::new(0., -10., 5.) };
    let error = crate::project::refresh_links(&doc, &BTreeMap::new(), &state, &Plane::XY, &mut elsewhere).unwrap_err();
    assert!(matches!(&error, crate::DocError::Reselect { reference, .. } if reference == numbered), "{error}");
}

/// A drafted face's edge is a circle seen at a slant: an ellipse. The sketch takes it as the
/// ellipse it is, not as the point set a spline of its own would be (#15061).
#[test]
fn a_drafted_face_projects_its_edge_as_one_ellipse() {
    let mut initial = ModelState::default();
    // A cylinder drafted three degrees: its top rim is a circle seen at a slant, an ellipse of
    // axis ratio 1/cos(3 deg).
    let cylinder = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 20.0, 30.0).unwrap();
    let drafted = solidcraft_kernel::draft(&cylinder, &[Vec3::new(0.0, 0.0, 30.0)], &Plane::XZ, Vec3::Y, 3f64.to_radians()).unwrap();
    let faces = drafted.faces(0.01).unwrap();
    // The drafted face leans off the base plane; the sketch lies in the face's own plane, so
    // the edge comes to the sketch as the ellipse it is.
    let lean = faces.iter().filter_map(|f| f.plane_normal).find(|n| n.z > 0.9).unwrap();
    let angle = 3f64.to_radians();
    let lean_sign = if lean.y >= 0.0 { 1.0 } else { -1.0 };
    let normal = Vec3::new(0.0, lean_sign * angle.sin(), angle.cos());
    let plane = Plane::from_normal(Vec3::new(0.0, 0.0, 30.0), normal).unwrap();
    let mut body = ModelBody::new("Drafted".into(), drafted, 1);
    body.names = crate::naming::NameCell::known(Arc::new(vec![String::new(); faces.len()]));
    initial.bodies.extend([body]);
    let doc = Document::default();
    let source = LinkSource::Face { body: "Drafted".into(), at: Vec3::new(0.0, 0.0, 30.0) };
    let resolved = crate::project::resolve(&doc, &BTreeMap::new(), &initial, &plane, LinkKind::Project, &source).unwrap();
    // The edge comes to the sketch as one ellipse, not as the point set of a spline.
    let [LinkGeom::Ellipse { c, major, minor }] = resolved.geom.as_slice() else { panic!("{:?}", resolved.geom) };
    assert!(c.len() < 1e-10);
    assert!((major.dist(*c) - 20.0 / angle.cos()).abs() < 1e-10);
    assert!((minor - 20.0).abs() < 1e-10);
    assert!(
        resolved.geom.iter().any(|g| matches!(g, LinkGeom::Ellipse { .. })),
        "a drafted face's edge is an ellipse, not a point set: {:?}",
        resolved
            .geom
            .iter()
            .map(|g| match g {
                LinkGeom::Ellipse { .. } => "ellipse".to_string(),
                LinkGeom::Spline { pts, .. } => format!("spline with {} points", pts.len()),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
    );
    assert!(
        !resolved.geom.iter().any(|g| matches!(g, LinkGeom::Spline { pts, .. } if pts.len() > 10)),
        "no projected spline may carry a point set: {:?}",
        resolved.geom
    );
}

fn lens_body(radius: f64, height: f64) -> ModelBody {
    // Two equal arcs meeting at two sharp corners (x = ±half_w, y = 0), so both top vertices
    // share the exact same three incident faces: bottom/top and the two arc walls.
    let half_w = 10.0;
    let d = (radius * radius - half_w * half_w).sqrt();
    let doc = Document {
        features: serde_json::from_value(serde_json::json!([
            {
                "id": 1, "name": "Sketch1", "type": "sketch", "plane": {"name": "XY", "type": "origin"},
                "sketch": {
                    "points": [
                        {"fixed": true, "id": "origin", "pos": [0., 0.]},
                        {"fixed": false, "id": "p1", "pos": [-half_w, 0.]},
                        {"fixed": false, "id": "p2", "pos": [half_w, 0.]},
                        {"fixed": false, "id": "c1", "pos": [0., -d]},
                        {"fixed": false, "id": "c2", "pos": [0., d]}
                    ],
                    "curves": [
                        {"type": "arc", "id": "a1", "a": 2, "b": 1, "c": 3, "reversed": false, "construction": false},
                        {"type": "arc", "id": "a2", "a": 1, "b": 2, "c": 4, "reversed": false, "construction": false}
                    ],
                    "constraints": [],
                    "counters": {"a": 2, "p": 4}
                }
            },
            {
                "id": 2, "name": "Extrude1", "type": "extrude", "body_names": ["Lens"],
                "extent": {"convention": "total_distance", "direction": "positive", "distance": format!("{height} mm")},
                "operation": "new_body", "profiles": {"type": "all"}, "sketch": 1
            }
        ]))
        .unwrap(),
        ..Document::default()
    };
    let doc = crate::format::read(&doc.to_json()).unwrap();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    model.state().body("Lens").cloned().unwrap()
}

#[test]
fn vertex_names_disambiguate_shared_incident_faces_with_tilde() {
    let body = lens_body(12.0, 5.0);
    let names = crate::naming::vertex_names(&body);
    let record = body.body.record();
    let numbered: Vec<(usize, &str)> = names.iter().enumerate().filter(|(_, n)| n.contains('~')).map(|(i, n)| (i, n.as_str())).collect();
    assert_eq!(numbered.len(), 4, "two top and two bottom vertices share identical face sets: {names:?}");
    for &(i, name) in &numbered {
        let p = record.vertices[i].point;
        let pt = Vec3::new(p[0], p[1], p[2]);
        let hit = crate::naming::locate_vertex(&body, name, pt).unwrap();
        assert_eq!(hit.vertex, i);
        assert!(hit.point.dist(pt) < 1e-6);
    }
}

#[test]
fn named_vertex_projection_moves_past_closer_neighbor_and_fails_when_removed() {
    let at_height = |h: f64| {
        // Right trapezoid whose bottom-right corner at x = 10 sits near x = 10, y = 0, z = h:
        // when z increases by 20, the other corner at x = 0, y = 0, z = h - 18 would be closer to
        // saved `at` if resolved by distance, while topological naming follows the picked corner.
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 4.0, h)).unwrap();
        let mut names = vec![String::new(); body.faces(0.01).unwrap().len()];
        for f in body.faces(0.01).unwrap() {
            let n = f.plane_normal.unwrap();
            names[f.index] = if n.z > 0.9 {
                "F2:top"
            } else if n.z < -0.9 {
                "F2:bottom"
            } else if n.x > 0.9 {
                "F2:side:right"
            } else if n.x < -0.9 {
                "F2:side:left"
            } else if n.y > 0.9 {
                "F2:side:back"
            } else {
                "F2:side:front"
            }
            .into();
        }
        let mut mb = ModelBody::new("Block".into(), body, 2);
        mb.names = crate::naming::NameCell::known(Arc::new(names));
        mb
    };

    let doc = Document::default();
    let mut initial = ModelState::default();
    initial.bodies.push(at_height(5.0));
    let plane = Plane::XZ;
    let source = LinkSource::Vertex { body: "Block".into(), at: Vec3::new(10.0, 0.0, 5.0) };
    let resolved = crate::project::resolve(&doc, &BTreeMap::new(), &initial, &plane, LinkKind::Project, &source).unwrap();
    let vertex_name = crate::project::resolved_vertex_name(&initial, &resolved).unwrap();
    assert_eq!(vertex_name, "F2:side:front|F2:side:right|F2:top");

    let mut sketch = Sketch::default();
    let link_id = sketch.add_link(LinkKind::Project, resolved.source, &resolved.geom).unwrap();
    sketch.links[0].vertex_name = Some(vertex_name.clone());

    // Grow height from 5 to 40 (well past 0.25 * diagonal and past closer neighbors).
    let mut taller = ModelState::default();
    taller.bodies.push(at_height(40.0));
    let warnings = crate::project::refresh_links(&doc, &BTreeMap::new(), &taller, &plane, &mut sketch).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!sketch.links[0].lost);
    let p_idx = sketch.link_points(&link_id)[0];
    let pt = sketch.points[p_idx].pos;
    // Plane::XZ has local y_dir = -Z, so z = 40 projects to local y = -40.
    assert!((pt.x - 10.0).abs() < 1e-6 && (pt.y + 40.0).abs() < 1e-6, "vertex followed to height 40: {pt:?}");

    // If the top face is gone, the named vertex fails with Reselect instead of snapping elsewhere.
    let mut renamed = taller;
    renamed.bodies[0].names = crate::naming::NameCell::known(Arc::new(vec!["other".into(); 6]));
    let error = crate::project::refresh_links(&doc, &BTreeMap::new(), &renamed, &plane, &mut sketch).unwrap_err();
    assert!(matches!(&error, crate::DocError::Reselect { reference, .. } if reference == &vertex_name), "{error}");
}

#[test]
fn mesh_body_has_no_vertex_names() {
    let mb = ModelBody::new(
        "MeshPart".into(),
        solidcraft_kernel::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], &[[0, 1, 2], [0, 1, 3], [1, 2, 3], [0, 2, 3]]).unwrap(),
        1,
    );
    assert!(crate::naming::vertex_names(&mb).is_empty());
}
