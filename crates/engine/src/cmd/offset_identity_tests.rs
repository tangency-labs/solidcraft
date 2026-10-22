#![allow(clippy::unwrap_used, clippy::panic)]
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind, ProfileSel};

use crate::Session;

fn run(session: &mut Session, command: &str, params: Value) -> Value {
    session.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

fn rounded_offset(radius: f64) -> (Session, u64, String, String) {
    let mut session = Session::default();
    run(&mut session, "sketch.create", json!({"plane":"XY","name":"Rounded"}));
    let sketch = session.active_sketch.unwrap();
    run(&mut session, "sketch.rectangle.two_point", json!({"p0":[-37.-radius,-32.-radius],"p1":[37.+radius,32.+radius]}));
    let fillet = run(&mut session, "sketch.fillet", json!({"points":["l1.start","l1.end","l2.end","l3.end"],"radius":radius}));
    // The radius is the only intended source freedom: keep the four exact arc
    // centres at (+/-37,+/-32), independently of the corner radius.
    let centers = fillet["curves"].as_array().unwrap().iter().map(|id| format!("{}.center", id.as_str().unwrap())).collect::<Vec<_>>();
    run(&mut session, "sketch.constraint.fix", json!({"entities":centers,"fixed":true}));
    let offset = run(&mut session, "sketch.offset", json!({"curves":["l1"],"distance":1.4,"side":[0,0]}));
    run(&mut session, "sketch.finish", json!({}));
    (session, sketch, fillet["param"].as_str().unwrap().into(), offset["association"].as_str().unwrap().into())
}

fn volume(session: &Session, feature: u64) -> f64 {
    let state = session.model.state();
    let body = state.bodies.iter().find(|body| body.feature == feature).unwrap();
    solidcraft_kernel::measure(&body.body).unwrap().volume
}

fn healthy(session: &Session) {
    assert!(session.model.results.iter().all(|result| result.error.is_none()), "{:?}", session.model.results);
}

#[test]
fn legacy_offset_capture_persists_reserved_curves_without_a_transient_association() {
    let (session, sketch, radius, _) = rounded_offset(2.);
    let mut document = (*session.doc).clone();
    let raw = document.sketch_mut(sketch).unwrap();
    let reserved = raw.offsets[0].outputs.clone();
    assert_eq!(reserved.len(), 8);
    raw.offsets[0].topology = None;
    document.format = "solidcraft/11".into();
    let mut session = Session::new(Document::from_json(&document.to_json()).unwrap());
    assert!(session.doc.sketch(sketch).unwrap().offsets[0].topology.is_none());
    let state = session.model.state();
    let solved = state.sketch(sketch).unwrap();
    assert!(solved.sketch.offsets[0].topology.is_some(), "legacy upgrade belongs only to the evaluated copy");
    let index = solidcraft_doc::profile_selection_indices(solved, &ProfileSel::Curves { loops: vec![reserved.clone()] }).unwrap()[0];
    let extrusion = run(&mut session, "solid.extrude", json!({"sketch":sketch,"profiles":[index],"distance":2,"name":"Legacy inner"}))["feature"]
        .as_u64()
        .unwrap();
    assert!(
        matches!(&session.doc.feature(extrusion).unwrap().kind,FeatureKind::Extrude {profiles:ProfileSel::Curves {loops},..} if loops == &vec![reserved.clone()])
    );
    assert!(session.doc.sketch(sketch).unwrap().offsets[0].topology.is_none());
    run(&mut session, "parameters.change", json!({"name":radius,"expression":"1 mm"}));
    healthy(&session);
    assert_eq!(session.model.state().sketch(sketch).unwrap().sketch.offsets[0].outputs.len(), 4);
    assert!(
        (volume(&session, extrusion) - 73.2 * 63.2 * 2.).abs() < 1e-6,
        "legacy sharp volume {} bounds {:?}",
        volume(&session, extrusion),
        session.model.state().bodies.last().unwrap().mesh().bounds()
    );
    let saved = session.doc.to_json();
    let mut reopened = Session::new(Document::from_json(&saved).unwrap());
    healthy(&reopened);
    assert_eq!(reopened.doc.to_json(), saved);
    assert!(reopened.doc.sketch(sketch).unwrap().offsets[0].topology.is_none());
    assert!((volume(&reopened, extrusion) - 73.2 * 63.2 * 2.).abs() < 1e-6);
    run(&mut reopened, "parameters.change", json!({"name":radius,"expression":"2 mm"}));
    healthy(&reopened);
    let state = reopened.model.state();
    assert_eq!(state.sketch(sketch).unwrap().sketch.offsets[0].outputs, reserved);
    let expected = 2. * (75.2 * 65.2 - (4. - std::f64::consts::PI) * 0.6 * 0.6);
    assert!((volume(&reopened, extrusion) - expected).abs() < 0.002);
}

#[test]
fn pasted_offset_boundary_has_scoped_identity_and_independent_dimensions_and_material() {
    let (mut session, sketch, radius, association) = rounded_offset(1.);
    let extrusion = run(
        &mut session,
        "solid.extrude",
        json!({"sketch":sketch,"profiles":{"offset_boundaries":[association]},"distance":2,"name":"Inner"}),
    )["feature"]
        .as_u64()
        .unwrap();
    let source_body = session.model.state().bodies[0].name.clone();
    run(&mut session, "material.assign", json!({"bodies":[source_body],"material":"Brass"}));
    run(&mut session, "timeline.copy", json!({"features":[sketch,extrusion]}));
    let pasted = run(&mut session, "timeline.paste", json!({"translate":[100,0,0]}));
    assert_eq!(pasted["errors"], json!([]));
    let copied_sketch = pasted["pasted"][0]["id"].as_u64().unwrap();
    let copied_extrusion = pasted["pasted"][1]["id"].as_u64().unwrap();
    assert_ne!(copied_sketch, sketch);
    assert!(
        matches!(&session.doc.feature(copied_extrusion).unwrap().kind,FeatureKind::Extrude {sketch:source,profiles:ProfileSel::OffsetBoundaries {associations},..} if *source==copied_sketch && associations == &vec![association.clone()])
    );
    let source = session.doc.sketch(sketch).unwrap();
    let copy = session.doc.sketch(copied_sketch).unwrap();
    assert_eq!(source.offsets[0].topology.as_ref().unwrap().id, copy.offsets[0].topology.as_ref().unwrap().id);
    let dimension = source.constraints.iter().position(|constraint| constraint.param.as_deref() == Some(&radius)).unwrap();
    let copied_radius = copy.constraints[dimension].param.clone().unwrap();
    assert_ne!(copied_radius, radius);
    let copied_body = session.model.state().bodies.iter().find(|body| body.feature == copied_extrusion).unwrap().name.clone();
    run(&mut session, "material.assign", json!({"bodies":[copied_body],"material":"Aluminum"}));
    let sharp = 73.2 * 63.2 * 2.;
    assert!((volume(&session, extrusion) - sharp).abs() < 1e-6);
    assert!((volume(&session, copied_extrusion) - sharp).abs() < 1e-6);
    run(&mut session, "parameters.change", json!({"name":copied_radius,"expression":"2 mm"}));
    healthy(&session);
    let state = session.model.state();
    assert_eq!(state.sketch(sketch).unwrap().sketch.offsets[0].outputs.len(), 4);
    assert_eq!(state.sketch(copied_sketch).unwrap().sketch.offsets[0].outputs.len(), 8);
    assert!((volume(&session, extrusion) - sharp).abs() < 1e-6);
    let rounded = 2. * (75.2 * 65.2 - (4. - std::f64::consts::PI) * 0.6 * 0.6);
    assert!(
        (volume(&session, copied_extrusion) - rounded).abs() < 0.002,
        "copied rounded volume {} expected {rounded} bounds {:?}",
        volume(&session, copied_extrusion),
        session.model.state().bodies.last().unwrap().mesh().bounds()
    );
    assert_eq!(session.doc.materials.get(&source_body).map(String::as_str), Some("Brass"));
    assert_eq!(session.doc.materials.get(&copied_body).map(String::as_str), Some("Aluminum"));
    let reopened = Session::new(Document::from_json(&session.doc.to_json()).unwrap());
    healthy(&reopened);
    assert!((volume(&reopened, extrusion) - sharp).abs() < 1e-6);
    assert!((volume(&reopened, copied_extrusion) - rounded).abs() < 0.002);
    let state = reopened.world_state().unwrap();
    let bounds = state.body(&copied_body).unwrap().mesh().bounds();
    assert!((bounds.min.x - (100. - 37.6)).abs() < 1e-6);
    assert!((bounds.max.x - (100. + 37.6)).abs() < 1e-6);
}
