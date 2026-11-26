#![allow(clippy::unwrap_used, clippy::panic)]
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind, ProfileSel};
use solidcraft_geom::{MeshGeom, Vec2Geom};

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

/// A design saved by the released SolidCraft 0.46.2, before picks that close a shape offset as one
/// joined outline (#14788): `sketch-offsets-0.46.2.sc`, with the face and edge names 0.46.2 gave
/// its bodies in `sketch-offsets-0.46.2.names.txt`. Sketch Arch holds a 20 × 20 mm rectangle and an
/// arc across its top line, offset inwards by `gap` with the arc and the three lines below it
/// picked; Sides holds the same arch, offset with its left and top lines picked; Box holds a plain
/// rectangle, offset with one line picked. Frame (the outer ring of Arch) has a round on the corner
/// between two of its offset lines, stored by name; Cap is bounded by the offset arc; Ring is Box's.
#[test]
fn offsets_of_a_design_saved_before_closed_picks_joined_replay_as_saved() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures");
    let path = dir.join("sketch-offsets-0.46.2.sc");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut session = Session::default();
    run(&mut session, "doc.open", json!({"path": path.to_string_lossy()}));
    healthy(&session);

    // Every sketch solves to what the file holds: each curve and point where it was, each offset
    // from the curves it was made from and with the curves it made, none joined to another.
    let replays_as_saved = |session: &Session| {
        let state = session.model.state();
        let mut chains = Vec::new();
        for feature in saved["features"].as_array().unwrap().iter().filter(|f| f["type"] == "sketch") {
            let name = feature["name"].as_str().unwrap();
            let mut was: solidcraft_sketch::Sketch = serde_json::from_value(feature["sketch"].clone()).unwrap();
            let mut now = state.sketch(feature["id"].as_u64().unwrap()).unwrap().sketch.clone();
            assert_eq!(now.points.len(), was.points.len(), "{name}");
            for (p, q) in now.points.iter_mut().zip(&mut was.points) {
                assert!(p.pos.dist(q.pos) < 1e-9, "{name}: {} at {:?}, saved at {:?}", p.id, p.pos, q.pos);
                p.pos = q.pos;
            }
            assert_eq!(now, was, "{name}");
            chains.push((name.to_string(), now.offsets.iter().map(|o| (o.sources.len(), o.closed)).collect::<Vec<_>>()));
        }
        chains
    };
    let unjoined = vec![
        ("Arch".to_string(), vec![(1, false), (3, false)]),
        ("Sides".to_string(), vec![(3, false), (1, false)]),
        ("Box".to_string(), vec![(4, true)]),
    ];
    assert_eq!(replays_as_saved(&session), unjoined);

    // Every face and edge has the name it had, so the stored round finds its edge.
    let names = |session: &Session| {
        let state = session.model.state();
        let mut names: Vec<String> = state
            .bodies
            .iter()
            .flat_map(|b| {
                let faces = solidcraft_doc::naming::face_names(b).iter().map(|n| format!("{} face {n}", b.name)).collect::<Vec<_>>();
                let edges = solidcraft_doc::naming::edge_names(b).iter().map(|n| format!("{} edge {n}", b.name)).collect::<Vec<_>>();
                [faces, edges].concat()
            })
            .collect();
        names.sort();
        names
    };
    let recorded = std::fs::read_to_string(dir.join("sketch-offsets-0.46.2.names.txt")).unwrap();
    let mut recorded: Vec<String> = recorded.lines().map(str::to_string).collect();
    recorded.sort();
    assert_eq!(names(&session), recorded);

    // The bodies are the ones the saved offsets bound, at the saved distance and at another: the
    // offsets follow `gap` without being made again. All are 10 mm high. Frame is the rectangle
    // less the 20 - 2 gap wide, 20 - gap high space between the offset lines, plus what the 0.5 mm
    // round adds in a corner. Cap is the part above the top line of the circle of the arc's centre
    // and radius less gap. Ring is the rectangle less the one offset inwards all round.
    let solids = |session: &Session, gap: f64| {
        let radius = (10f64.powi(2) + 6.2f64.powi(2)) / (2. * 6.2);
        let below = radius - 6.2;
        let r = radius - gap;
        let frame = 400. - (20. - 2. * gap) * (20. - gap) + (1. - std::f64::consts::FRAC_PI_4) * 0.25;
        let cap = r * r * (below / r).acos() - below * (r * r - below * below).sqrt();
        let ring = 400. - (20. - 2. * gap).powi(2);
        for (name, area) in [("Frame", frame), ("Cap", cap), ("Ring", ring)] {
            let body = session.model.state().bodies.iter().find(|b| b.name == name).unwrap().clone();
            let volume = solidcraft_kernel::measure(&body.body).unwrap().volume;
            assert!((volume - 10. * area).abs() < 1e-6, "{name}: {volume}, want {}", 10. * area);
        }
    };
    solids(&session, 1.);
    run(&mut session, "parameters.change", json!({"name": "gap", "expression": "2 mm"}));
    healthy(&session);
    solids(&session, 2.);
    run(&mut session, "parameters.change", json!({"name": "gap", "expression": "1 mm"}));
    healthy(&session);
    assert_eq!(replays_as_saved(&session), unjoined);
    assert_eq!(names(&session), recorded);

    // Saved and opened again, or a sketch opened for editing and finished, it is the same.
    let mut reopened = Session::new(Document::from_json(&session.doc.to_json()).unwrap());
    healthy(&reopened);
    assert_eq!(replays_as_saved(&reopened), unjoined);
    assert_eq!(names(&reopened), recorded);
    for sketch in ["Arch", "Sides", "Box"] {
        run(&mut reopened, "sketch.edit", json!({"sketch": sketch}));
        run(&mut reopened, "sketch.finish", json!({}));
    }
    healthy(&reopened);
    assert_eq!(replays_as_saved(&reopened), unjoined);
    assert_eq!(names(&reopened), recorded);
}
