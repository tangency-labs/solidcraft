use crate::{Document, ModelBody, ModelState};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{LinkKind, LinkSource, Sketch};
use std::collections::BTreeMap;
use std::sync::Arc;

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
