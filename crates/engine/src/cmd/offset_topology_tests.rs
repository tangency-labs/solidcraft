#![allow(clippy::unwrap_used, clippy::panic)]
use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, ProfileSel};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(session: &mut Session, command: &str, params: Value) -> Value {
    session.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

#[test]
fn breaking_offset_preserves_downstream_material_history_and_source_independence() {
    for distance in [0.2, 1.4] {
        let (mut session, sketch, radius) = rounded_source();
        let offset = run(&mut session, "sketch.offset", json!({"curves":["l1"],"distance":distance,"side":[0,0]}));
        let association = offset["association"].as_str().unwrap().to_string();
        let curves = session.model.state().sketch(sketch).unwrap().sketch.offset_boundary(&association).unwrap();
        run(&mut session, "sketch.finish", json!({}));
        run(&mut session, "solid.extrude", json!({"sketch":sketch,"profiles":{"offset_boundaries":[association]},"distance":2,"operation":"new"}));
        let before_volume = session.world_state().unwrap().bodies.last().unwrap().mesh().measure().volume.abs();
        run(&mut session, "sketch.edit", json!({"sketch":sketch}));
        let before = session.doc.to_json();
        let history = session.undo.len();
        run(&mut session, "sketch.offset.break", json!({"curve":association}));
        assert_eq!(session.undo.len(), history + 1);
        assert!(session.model.results.iter().all(|result| result.error.is_none()));
        assert!(
            matches!(&session.doc.features.last().unwrap().kind, FeatureKind::Extrude { profiles: ProfileSel::Curves { loops }, .. } if loops == &vec![curves.clone()])
        );
        let frozen = session.doc.to_json();
        assert!((session.world_state().unwrap().bodies.last().unwrap().mesh().measure().volume.abs() - before_volume).abs() < 1e-6);
        run(&mut session, "edit.undo", json!({}));
        assert_eq!(session.doc.to_json(), before);
        run(&mut session, "edit.redo", json!({}));
        assert_eq!(session.doc.to_json(), frozen);
        run(&mut session, "parameters.change", json!({"name":radius,"expression":"2 mm"}));
        assert!(session.model.results.iter().all(|result| result.error.is_none()));
        assert!((session.world_state().unwrap().bodies.last().unwrap().mesh().measure().volume.abs() - before_volume).abs() < 1e-6);
        let reopened = Session::new(solidcraft_doc::Document::from_json(&session.doc.to_json()).unwrap());
        assert!(reopened.model.results.iter().all(|result| result.error.is_none()));
        assert!((reopened.world_state().unwrap().bodies.last().unwrap().mesh().measure().volume.abs() - before_volume).abs() < 1e-6);
    }
}

fn rounded_source() -> (Session, u64, String) {
    let mut session = Session::default();
    run(&mut session, "sketch.create", json!({"plane":"XY","name":"Rounded"}));
    let sketch = session.active_sketch.unwrap();
    run(&mut session, "sketch.rectangle.two_point", json!({"p0":[-38,-33],"p1":[38,33]}));
    let fillet = run(&mut session, "sketch.fillet", json!({"points":["l1.start","l1.end","l2.end","l3.end"],"radius":"1 mm"}));
    (session, sketch, fillet["param"].as_str().unwrap().to_string())
}

#[test]
fn inset_past_corner_radius_creates_a_real_boundary_and_saved_profile_identity() {
    let (mut session, sketch, _) = rounded_source();
    let offset = run(&mut session, "sketch.offset", json!({"curves":["l1"],"distance":1.4,"side":[0,0]}));
    assert_eq!(offset["curves"].as_array().unwrap().len(), 4);
    let state = session.model.state();
    let solved = state.sketch(sketch).unwrap();
    let record = solved.sketch.offsets.last().unwrap();
    let topology = record.topology.as_ref().unwrap();
    let association = topology.id.clone();
    assert_eq!(topology.roles.len(), 8);
    assert_eq!(topology.roles.iter().filter(|role| role.retired).count(), 4);
    assert_eq!(solved.sketch.offset_boundary(&association).unwrap(), record.outputs);
    assert!(topology.roles.iter().filter(|role| role.retired).all(|role| solved.sketch.curve_index(&role.output).is_none()));
    run(&mut session, "sketch.finish", json!({}));
    run(
        &mut session,
        "solid.extrude",
        json!({"sketch":sketch,"profiles":{"offset_boundaries":[association]},"distance":2,"operation":"new","name":"Inner"}),
    );
    let feature = session.doc.features.last().unwrap();
    assert!(matches!(&feature.kind, FeatureKind::Extrude { profiles: ProfileSel::OffsetBoundaries { associations }, .. } if associations.len() == 1));
    let check = |session: &Session| {
        assert!(session.model.results.iter().all(|result| result.error.is_none()));
        let state = session.world_state().unwrap();
        let body = state.bodies.last().unwrap();
        // The exact inward parallel set is a sharp 73.2 by 63.2 rectangle.
        let mesh = body.mesh();
        assert!((mesh.measure().volume.abs() - 73.2 * 63.2 * 2.0).abs() < 1e-6);
        assert!(mesh.contains(Vec3::new(36.5, 31.5, 1.0)));
        assert!(!mesh.contains(Vec3::new(36.7, 31.5, 1.0)));
        assert!(!mesh.contains(Vec3::new(36.5, 31.7, 1.0)));
    };
    check(&session);
    let saved = session.doc.to_json();
    let reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    check(&reopened);
    assert_eq!(reopened.doc.to_json(), saved);
    let undo = session.undo.len();
    let redo = session.redo.len();
    for params in [
        json!({"profiles":{"offset_boundaries":[]}}),
        json!({"profiles":{"offset_boundaries":["missing"]}}),
        json!({"profiles":{"offset_boundaries":[association,association]}}),
        json!({"profiles":[[record.outputs[0],record.outputs[1],record.outputs[2]]]}),
    ] {
        let mut params = params;
        params["sketch"] = json!(sketch);
        params["distance"] = json!(1);
        assert!(session.execute("solid.extrude", &params).is_err());
        assert_eq!(session.doc.to_json(), saved);
        assert_eq!(session.undo.len(), undo);
        assert_eq!(session.redo.len(), redo);
    }
}
