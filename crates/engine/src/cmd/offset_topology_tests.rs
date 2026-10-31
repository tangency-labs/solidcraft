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
        let before_volume = session.world_state().bodies.last().unwrap().mesh().measure().volume.abs();
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
        assert!((session.world_state().bodies.last().unwrap().mesh().measure().volume.abs() - before_volume).abs() < 1e-6);
        run(&mut session, "edit.undo", json!({}));
        assert_eq!(session.doc.to_json(), before);
        run(&mut session, "edit.redo", json!({}));
        assert_eq!(session.doc.to_json(), frozen);
        run(&mut session, "parameters.change", json!({"name":radius,"expression":"2 mm"}));
        assert!(session.model.results.iter().all(|result| result.error.is_none()));
        assert!((session.world_state().bodies.last().unwrap().mesh().measure().volume.abs() - before_volume).abs() < 1e-6);
        let reopened = Session::new(solidcraft_doc::Document::from_json(&session.doc.to_json()).unwrap());
        assert!(reopened.model.results.iter().all(|result| result.error.is_none()));
        assert!((reopened.world_state().bodies.last().unwrap().mesh().measure().volume.abs() - before_volume).abs() < 1e-6);
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
        let state = session.world_state();
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

/// #7798: a bone-shaped outline, two r10 arcs meeting in a notch at each end and joined by two
/// lines. Its inward offset is the set of points `d` from the outline, so every point of every
/// offset curve lies exactly `d` from it. At a notch that takes a round of radius `d` about the
/// notch point: the shrunken arcs alone meet farther in (1.88 mm from the outline at 1 mm), and
/// past 2 mm they no longer meet at all.
#[test]
fn inset_of_a_notched_outline_stays_its_distance_from_the_outline() {
    use solidcraft_geom::{Seg2, Vec2};
    fn distance(p: Vec2, seg: &Seg2) -> f64 {
        match *seg {
            Seg2::Line { a, b } => {
                let t = ((p - a).dot(b - a) / (b - a).dot(b - a)).clamp(0.0, 1.0);
                p.dist(a + (b - a) * t)
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let phi = (p.y - center.y).atan2(p.x - center.x);
                let into =
                    if sweep >= 0.0 { (phi - start).rem_euclid(std::f64::consts::TAU) } else { (start - phi).rem_euclid(std::f64::consts::TAU) };
                if into <= sweep.abs() { (p.dist(center) - radius).abs() } else { p.dist(seg.start()).min(p.dist(seg.end())) }
            }
            _ => panic!("the outline is lines and arcs"),
        }
    }
    let sources = ["l1", "a1", "a2", "l2", "a3", "a4"];
    for d in [1.0, 1.9, 3.0, 5.0] {
        let mut session = Session::default();
        run(&mut session, "sketch.create", json!({"plane":"XY","name":"Bone"}));
        let id = session.active_sketch.unwrap();
        {
            let sk = session.doc_mut().sketch_mut(id).unwrap();
            let v = Vec2::new;
            let p: Vec<usize> = [v(30., 18.), v(-30., 18.), v(-36., 0.), v(-30., -18.), v(30., -18.), v(36., 0.)]
                .iter()
                .map(|q| sk.add_point(*q, None).unwrap())
                .collect();
            sk.add_line_pts(p[0], p[1], Some("l1")).unwrap();
            sk.add_arc(v(-30., 8.), v(-30., 18.), v(-36., 0.), [None, Some(p[1]), Some(p[2])], Some("a1")).unwrap();
            sk.add_arc(v(-30., -8.), v(-36., 0.), v(-30., -18.), [None, Some(p[2]), Some(p[3])], Some("a2")).unwrap();
            sk.add_line_pts(p[3], p[4], Some("l2")).unwrap();
            sk.add_arc(v(30., -8.), v(30., -18.), v(36., 0.), [None, Some(p[4]), Some(p[5])], Some("a3")).unwrap();
            sk.add_arc(v(30., 8.), v(36., 0.), v(30., 18.), [None, Some(p[5]), Some(p[0])], Some("a4")).unwrap();
        }
        let offset = session
            .execute("sketch.offset", &json!({"curves":sources,"distance":d,"side":[0,0]}))
            .unwrap_or_else(|error| panic!("an inset of {d} mm: {error}"));
        let state = session.model.state();
        let sk = &state.sketch(id).unwrap().sketch;
        let segs = |id: &str| {
            let i = sk.curve_index(id).unwrap();
            sk.exact_segs(i).unwrap_or_else(|| sk.segs(i))
        };
        let outline: Vec<Seg2> = sources.iter().flat_map(|s| segs(s)).collect();
        for curve in offset["curves"].as_array().unwrap() {
            for seg in segs(curve.as_str().unwrap()) {
                for k in 0..=20 {
                    let p = seg.point_at(f64::from(k) / 20.0);
                    let off = outline.iter().map(|s| distance(p, s)).fold(f64::INFINITY, f64::min);
                    assert!((off - d).abs() < 1e-6, "an inset of {d} mm: {curve} passes {p:?} at {off} mm from the outline");
                }
            }
        }
    }
}
