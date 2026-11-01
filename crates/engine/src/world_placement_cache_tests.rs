//! Placement-cache changes must retain physical picking, pending positions and named joints.
use crate::{Session, geom::Vec3};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(session: &mut Session, command: &str, params: Value) -> Value {
    session.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

#[test]
fn geometry_independent_moves_do_not_evaluate_the_timeline() {
    let mut session = Session::default();
    let created = run(&mut session, "component.create", json!({"name":"Part","grounded":false}));
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block"}));
    let definitions = session.model.state();
    let before = session.evaluations;
    run(&mut session, "occurrence.move", json!({"occurrence":occurrence,"translate":[10,0,0],"capture":true}));
    assert_eq!(session.evaluations - before, 0, "a placement-independent model must not enter timeline evaluation");
    let world = session.world_state().unwrap();
    let block = world.bodies.iter().find(|b| b.source_name.as_deref() == Some("Block")).unwrap();
    assert!(block.mesh().bounds().min.dist(Vec3::new(10., 0., 0.)) < 1e-9);
    assert!(Arc::ptr_eq(&definitions, &session.model.state()));
    assert_fresh_world(&session);
    for command in ["edit.undo", "edit.redo"] {
        let before = session.evaluations;
        run(&mut session, command, json!({}));
        assert_eq!(session.evaluations, before, "{command} must share the placement reuse decision");
        assert!(Arc::ptr_eq(&definitions, &session.model.state()));
        assert_fresh_world(&session);
    }
    let mut scratch = session.scratch();
    run(&mut scratch, "occurrence.move", json!({"occurrence":occurrence,"translate":[5,0,0],"capture":true}));
    assert_eq!(scratch.evaluations, 0);
    assert!(Arc::ptr_eq(&definitions, &scratch.model.state()));
    assert_fresh_world(&scratch);
    assert!(session.world_state().unwrap().bodies[0].mesh().bounds().min.dist(Vec3::new(10., 0., 0.)) < 1e-9);
}

fn assert_fresh_world(session: &Session) {
    let mut fresh = crate::doc::Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&session.doc);
    assert!(fresh.results.iter().all(|r| r.error.is_none()));
    let expected = crate::doc::world_state(&session.doc, &fresh.state()).unwrap();
    let actual = session.world_state().unwrap();
    assert_eq!(actual.bodies.len(), expected.bodies.len());
    for (a, b) in actual.bodies.iter().zip(&expected.bodies) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.component, b.component);
        let (a, b) = (a.mesh(), b.mesh());
        assert_eq!(a.triangles, b.triangles);
        assert_eq!(a.tri_face, b.tri_face);
        assert_eq!(a.edge_faces, b.edge_faces);
        assert_eq!(a.positions.len(), b.positions.len());
        assert_eq!(a.normals.len(), b.normals.len());
        for (a, b) in a.positions.iter().zip(&b.positions) {
            assert!(a.dist(*b) < 1e-8);
        }
        for (a, b) in a.normals.iter().zip(&b.normals) {
            assert!(a.dist(*b) < 1e-8);
        }
    }
}

#[test]
fn geometry_independent_joint_drive_and_reach_keep_the_evaluated_model() {
    let mut session = Session::default();
    let created = run(&mut session, "component.create", json!({"name":"Rotor","grounded":false}));
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block"}));
    run(&mut session, "joint.as_built", json!({"type":"revolute","name":"Turn","a":0,"b":"Rotor","at":{"occurrence":0,"point":[0,0,0],"z":[0,0,1]}}));
    let definitions = session.model.state();
    for angle in [30_f64, -45., 90.] {
        let before = session.evaluations;
        let answer = run(&mut session, "joint.drive", json!({"joint":"Turn","value":angle,"quiet":true}));
        assert_eq!(answer["errors"], json!([]));
        assert_eq!(answer["conflicts"], json!([]));
        assert_eq!(session.evaluations, before);
        assert!(Arc::ptr_eq(&definitions, &session.model.state()));
        let m = crate::doc::joints::occurrence_world(&session.doc, occurrence);
        let expected = Vec3::new(2. * angle.to_radians().cos(), 2. * angle.to_radians().sin(), 0.);
        assert!(crate::doc::apply_point(&m, Vec3::new(2., 0., 0.)).dist(expected) < 1e-8);
        assert_fresh_world(&session);
    }
    let at = crate::doc::apply_point(&crate::doc::joints::occurrence_world(&session.doc, occurrence), Vec3::new(2., 0., 0.));
    let before = session.evaluations;
    let answer =
        run(&mut session, "joint.reach", json!({"occurrence":occurrence,"at":[at.x,at.y,at.z],"point":[2.,0.,0.],"quiet":true,"continuous":true}));
    assert_eq!(answer["reached"], true, "{answer}");
    assert_eq!(session.evaluations, before);
    assert!(Arc::ptr_eq(&definitions, &session.model.state()));
    assert_fresh_world(&session);
    for command in ["edit.undo", "edit.redo"] {
        let before = session.evaluations;
        run(&mut session, command, json!({}));
        assert_eq!(session.evaluations, before);
        assert_fresh_world(&session);
    }
}

#[test]
fn geometry_independent_fixed_sketch_extrusions_reuse_only_their_geometry() {
    let mut session = Session::default();
    let created = run(&mut session, "component.create", json!({"name":"Part","grounded":false}));
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut session, "sketch.create", json!({"plane":"XY"}));
    run(&mut session, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[2,3]}));
    run(&mut session, "sketch.finish", json!({}));
    run(&mut session, "solid.extrude", json!({"distance":4,"operation":"new"}));
    let definitions = session.model.state();
    let before = session.evaluations;
    run(&mut session, "occurrence.move", json!({"occurrence":occurrence,"translate":[10,0,0],"capture":true}));
    assert_eq!(session.evaluations, before);
    assert!(Arc::ptr_eq(&definitions, &session.model.state()));
    assert_fresh_world(&session);
    // An edit made directly before the move is not the model's evaluated input, even though
    // the command itself changes only placement. The proof must name the evaluated document.
    let sketch = session.doc.features.iter().find(|f| matches!(f.kind, crate::doc::FeatureKind::Sketch { .. })).unwrap().id;
    session.doc_mut().sketch_mut(sketch).unwrap().points[1].pos.x += 1.;
    let before = session.evaluations;
    run(&mut session, "occurrence.move", json!({"occurrence":occurrence,"translate":[1,0,0],"capture":true}));
    assert!(session.evaluations > before);
    assert_fresh_world(&session);
}

#[test]
fn placement_dependent_cross_component_cuts_still_evaluate_and_undo() {
    use crate::doc::{Document, FeatureKind, Model, Operation, point_expr};
    let mut document = Document::new("cross-component placement dependency");
    let plate = document.add_component(Some("Plate"), 0).unwrap();
    let tools = document.add_component(Some("Tools"), 0).unwrap();
    for occurrence in &mut document.occurrences {
        occurrence.grounded = occurrence.component == tools;
    }
    let feature = document
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::ZERO),
                length: "10".into(),
                width: "10".into(),
                height: "4".into(),
                operation: Operation::NewBody,
            },
            None,
        )
        .unwrap();
    document.feature_mut(feature).unwrap().component = plate;
    let mut model = Model::new();
    model.evaluate(&document);
    let plate_body = model.state().bodies[0].name.clone();
    let feature = document
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::new(4., 4., -1.)),
                length: "2".into(),
                width: "2".into(),
                height: "6".into(),
                operation: Operation::Cut,
            },
            None,
        )
        .unwrap();
    let feature = document.feature_mut(feature).unwrap();
    feature.component = tools;
    feature.participants = vec![plate_body.clone()];
    let mut session = Session::new(document);
    run(&mut session, "joint.as_built", json!({"type":"slider","name":"Slide","a":0,"b":"Plate","at":{"occurrence":0,"point":[0,0,0],"z":[1,0,0]}}));
    assert!((session.model.state().body(&plate_body).unwrap().measure().unwrap().volume - 384.).abs() < 1e-7);
    for (command, params, volume) in
        [("joint.drive", json!({"joint":"Slide","value":5,"quiet":true}), 392.), ("edit.undo", json!({}), 384.), ("edit.redo", json!({}), 392.)]
    {
        let before = session.evaluations;
        run(&mut session, command, params);
        assert!(session.evaluations > before, "{command} must update the cut");
        assert!((session.model.state().body(&plate_body).unwrap().measure().unwrap().volume - volume).abs() < 1e-7);
        assert_fresh_world(&session);
    }
}

#[test]
fn world_placement_cache_preserves_pending_picks_materials_scratch_and_named_joint_frames() {
    let mut session = Session::default();
    let created = run(&mut session, "component.create", json!({"name":"Part"}));
    let component = created["component"].as_u64().unwrap();
    let first = created["occurrence"].as_u64().unwrap();
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block","operation":"new"}));
    run(&mut session, "material.assign", json!({"bodies":["Block"],"material":"Brass"}));
    let second = run(&mut session, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    run(&mut session, "occurrence.move", json!({"occurrence":second,"translate":[10,20,0],"axis":[0,0,1],"angle":"90 deg","capture":false}));
    let pending = session.pending_moves.clone();
    let path = vec![second];
    let placement = session.occurrence_placement(&path).unwrap();
    let name = placement.bodies[0].world.clone();
    let world = session.world_state().unwrap();
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    let source_state = session.model.state();
    let source = source_state.body("Block").unwrap();
    let placed = world.body(&name).unwrap();
    assert_eq!(placed.source_name.as_deref(), Some("Block"));
    assert_eq!(placed.component, component);
    let bounds = placed.mesh().bounds();
    assert!(bounds.min.dist(Vec3::new(7., 20., 0.)) < 1e-9);
    assert!(bounds.max.dist(Vec3::new(10., 22., 4.)) < 1e-9);
    assert_eq!(session.doc.model_body_look(placed), session.doc.model_body_look(source));
    let source_names = crate::doc::naming::face_names(source);
    assert_eq!(source_names, crate::doc::naming::face_names(placed));
    let pick = Vec3::new(8.5, 21., 4.);
    let top = placed.body.faces(0.001).unwrap().into_iter().find(|face| face.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap().index;
    run(&mut session, "select.set", json!({"items":[{"type":"face","body":name,"index":top,"point":[pick.x,pick.y,pick.z]}]}));
    assert!(matches!(&session.selection[0], crate::Sel::Face { body, index, .. } if body == &name && *index == top));
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()), "selection changes must not discard prepared placements");
    let scratch = session.scratch();
    assert_eq!(scratch.pending_moves, pending);
    let scratch_world = scratch.world_state().unwrap();
    assert!(Arc::ptr_eq(&world, &scratch_world));
    let scratch_body = scratch_world.body(&name).unwrap();
    assert!(scratch_body.mesh().bounds().min.dist(bounds.min) < 1e-9);
    assert_eq!(crate::doc::naming::face_names(scratch_body), source_names);
    run(&mut session, "component.capture_position", json!({}));
    run(&mut session, "joint.origin", json!({"name":"Top joint","occurrence":second,"face":[pick.x,pick.y,pick.z]}));
    let joint = session.doc.assembly.origins[0].origin.clone();
    let local_frame = crate::doc::joints::resolve_origin(&session.doc, &session.model.state(), &joint).unwrap();
    let before_joint = crate::doc::mat_mul(&crate::doc::joints::occurrence_world(&session.doc, second), &local_frame);
    assert!(crate::doc::apply_point(&before_joint, Vec3::ZERO).dist(pick) < 1e-9);
    run(&mut session, "occurrence.move", json!({"occurrence":second,"translate":[5,0,0],"capture":true}));
    let moved = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&world, &moved), "a changed transform must invalidate placed meshes");
    assert!(moved.body(&name).unwrap().mesh().bounds().min.dist(bounds.min + Vec3::new(5., 0., 0.)) < 1e-9);
    assert!(world.body(&name).unwrap().mesh().bounds().min.dist(bounds.min) < 1e-9, "prior snapshots must remain immutable");
    assert_eq!(crate::doc::naming::face_names(moved.body(&name).unwrap()), source_names);
    let local_frame = crate::doc::joints::resolve_origin(&session.doc, &session.model.state(), &joint).unwrap();
    let after_joint = crate::doc::mat_mul(&crate::doc::joints::occurrence_world(&session.doc, second), &local_frame);
    assert!(crate::doc::apply_point(&after_joint, Vec3::ZERO).dist(pick + Vec3::new(5., 0., 0.)) < 1e-9);
    let reopened = Session::new(crate::doc::Document::from_json(&session.doc.to_json()).unwrap());
    assert!(reopened.world_state().unwrap().body(&name).unwrap().mesh().bounds().min.dist(bounds.min + Vec3::new(5., 0., 0.)) < 1e-9);
    assert_eq!(reopened.model.results.iter().filter(|r| r.error.is_some()).count(), 0);
}

#[test]
fn selection_visibility_section_and_view_revisions_reuse_world_geometry() {
    let mut session = Session::default();
    run(&mut session, "component.create", json!({"name":"Part"}));
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block","operation":"new"}));
    let world = session.world_state().unwrap();
    let revision = session.revision;
    run(&mut session, "select.set", json!({"items":[{"type":"body","name":"Block"}]}));
    assert!(session.revision > revision);
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    run(&mut session, "select.clear", json!({}));
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    run(&mut session, "browser.visibility", json!({"items":["Block"],"visible":false}));
    assert!(!session.body_visible("Block"));
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    run(&mut session, "inspect.section", json!({"plane":{"origin":[0,0,2],"normal":[0,0,1]}}));
    assert!(session.section.is_some());
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    session.invalidate_view(session.revision + 10);
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    assert!(Arc::ptr_eq(&world, &session.scratch().world_state().unwrap()));
}

#[test]
fn world_cache_requires_exact_design_model_pending_and_author_context() {
    let mut session = Session::default();
    let created = run(&mut session, "component.create", json!({"name":"Part"}));
    let component = created["component"].as_u64().unwrap();
    let first = created["occurrence"].as_u64().unwrap();
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block","operation":"new"}));
    let sketch = run(&mut session, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut session, "sketch.finish", json!({}));
    let second = run(&mut session, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    let initial = session.world_state().unwrap();
    run(&mut session, "occurrence.move", json!({"occurrence":second,"translate":[10,20,0],"capture":false}));
    let pending = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&initial, &pending));
    let name = session.occurrence_placement(&[second]).unwrap().bodies[0].world.clone();
    assert!(pending.body(&name).unwrap().mesh().bounds().min.dist(Vec3::new(10., 20., 0.)) < 1e-9);
    run(&mut session, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    let authored = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&pending, &authored));
    assert!(authored.sketch(sketch).unwrap().plane.origin.dist(Vec3::new(10., 20., 0.)) < 1e-9);
    let mut scratch = session.scratch();
    assert!(Arc::ptr_eq(&authored, &scratch.world_state().unwrap()));
    scratch.active_occurrence_path = Some(vec![first]);
    let scratch_world = scratch.world_state().unwrap();
    assert!(!Arc::ptr_eq(&authored, &scratch_world));
    assert!(scratch_world.sketch(sketch).unwrap().plane.origin.dist(Vec3::ZERO) < 1e-9);
    assert!(Arc::ptr_eq(&authored, &session.world_state().unwrap()), "scratch context must not replace the source cache");
    // Evaluation and placement never read appearances: an appearance edit, its undo and its redo
    // keep the evaluated model and the placed world (the cache follows the new document).
    let model = session.model.state();
    run(&mut session, "appearance.assign", json!({"bodies":["Block"],"color":"#ff0000"}));
    let painted = session.world_state().unwrap();
    assert!(Arc::ptr_eq(&authored, &painted), "an appearance edit keeps the placed world");
    assert!(Arc::ptr_eq(&model, &session.model.state()), "and the evaluated model");
    assert_ne!(session.doc.model_body_look(painted.body("Block").unwrap()), None);
    run(&mut session, "edit.undo", json!({}));
    let undone = session.world_state().unwrap();
    assert!(Arc::ptr_eq(&painted, &undone), "undoing it too");
    assert_eq!(session.doc.model_body_look(undone.body("Block").unwrap()), None);
    run(&mut session, "edit.redo", json!({}));
    let redone = session.world_state().unwrap();
    assert!(Arc::ptr_eq(&undone, &redone), "and redoing it");
    assert!(session.doc.model_body_look(redone.body("Block").unwrap()).is_some());
    let before_edit = session.world_state().unwrap();
    run(&mut session, "solid.box", json!({"length":5,"width":6,"height":7,"body_name":"Other","operation":"new"}));
    let edited = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&before_edit, &edited));
    assert!(edited.body("Other").is_some());
    session.doc = Arc::new(crate::doc::Document::from_json(&session.doc.to_json()).unwrap());
    let replaced = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&edited, &replaced), "equal contents in a different document cannot share the cache key");
    session.model = crate::doc::Model::default();
    session.model.evaluate(&session.doc);
    assert!(
        !Arc::ptr_eq(&replaced, &session.world_state().unwrap()),
        "a new evaluated model must invalidate even when the document Arc is unchanged"
    );
}

#[test]
fn adopted_scratch_keeps_its_warm_geometry_and_history_stages_invalidate_it() {
    let mut session = Session::default();
    run(&mut session, "component.create", json!({"name":"Part"}));
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block","operation":"new"}));
    let source = session.world_state().unwrap();
    let source_json = session.doc.to_json();
    let mut scratch = session.scratch();
    run(&mut scratch, "solid.box", json!({"length":5,"width":6,"height":7,"body_name":"Draft","operation":"new"}));
    let warmed = scratch.world_state().unwrap();
    assert!(!Arc::ptr_eq(&source, &warmed));
    assert_eq!(session.doc.to_json(), source_json);
    assert!(Arc::ptr_eq(&source, &session.world_state().unwrap()));
    assert!(source.body("Draft").is_none());
    let feature = warmed.body("Draft").unwrap().feature;
    session.accept_computed(scratch);
    assert!(Arc::ptr_eq(&warmed, &session.world_state().unwrap()), "adoption/view revision must retain the worker's prepared geometry");
    session.invalidate_view(session.revision + 20);
    assert!(Arc::ptr_eq(&warmed, &session.world_state().unwrap()));
    run(&mut session, "timeline.rollback", json!({"position":1}));
    let rolled = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&warmed, &rolled));
    assert!(rolled.body("Block").is_some());
    assert!(rolled.body("Draft").is_none());
    run(&mut session, "timeline.rollback", json!({}));
    let restored = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&rolled, &restored));
    assert!(restored.body("Draft").is_some());
    run(&mut session, "timeline.suppress", json!({"feature":feature,"suppressed":true}));
    let suppressed = session.world_state().unwrap();
    assert!(!Arc::ptr_eq(&restored, &suppressed));
    assert!(suppressed.body("Draft").is_none());
    run(&mut session, "timeline.suppress", json!({"feature":feature,"suppressed":false}));
    assert!(session.world_state().unwrap().body("Draft").is_some());
}

/// A moved component's bodies keep their placed geometry and display mesh while the design
/// changes elsewhere: each new body used to transform and mesh every body of every moved
/// component again. A body that changes, or a placement that moves, is placed afresh.
#[test]
fn moved_component_bodies_are_not_placed_again_when_something_else_changes() {
    let mut session = Session::default();
    let created = run(&mut session, "component.create", json!({"name":"Wheel","grounded":false}));
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut session, "solid.box", json!({"length":60,"width":60,"height":5,"body_name":"Rim"}));
    run(&mut session, "component.activate", json!({"component":"root"}));
    run(&mut session, "occurrence.move", json!({"occurrence":occurrence,"translate":[100,0,0]}));
    let placed = session.world_state().unwrap();
    let rim = placed.bodies.iter().find(|b| b.source_name.as_deref() == Some("Rim")).unwrap().clone();
    let mesh = rim.mesh();
    assert!(mesh.bounds().min.dist(Vec3::new(100., 0., 0.)) < 1e-6);
    // A body added elsewhere: the rim is the same placed body, already meshed.
    run(&mut session, "solid.box", json!({"length":5,"width":6,"height":7,"body_name":"Frame"}));
    let after = session.world_state().unwrap();
    let again = after.body(&rim.name).unwrap();
    assert!(Arc::ptr_eq(&mesh, &again.mesh()), "an unchanged placed body keeps its mesh");
    assert!(again.body.same(&rim.body));
    // The placement moves: new world coordinates, reusing the definition's triangulation.
    run(&mut session, "occurrence.move", json!({"occurrence":occurrence,"translate":[0,50,0]}));
    let moved = session.world_state().unwrap();
    let moved_rim = moved.bodies.iter().find(|b| b.source_name.as_deref() == Some("Rim")).unwrap();
    assert!(!Arc::ptr_eq(&mesh, &moved_rim.mesh()));
    assert!(moved_rim.mesh().bounds().min.dist(Vec3::new(100., 50., 0.)) < 1e-6);
    // The body itself changes: placed again too, and the old snapshot is untouched.
    run(&mut session, "body.rename", json!({"body":"Frame","name":"Frame tube"}));
    let source = session.model.state().body("Rim").unwrap().clone();
    let world = session.world_state().unwrap();
    let kept = world.bodies.iter().find(|b| b.source_name.as_deref() == Some("Rim")).unwrap();
    assert!(Arc::ptr_eq(&moved_rim.mesh(), &kept.mesh()));
    assert_eq!(kept.source_transform, moved_rim.source_transform);
    assert!(source.mesh().bounds().min.dist(Vec3::ZERO) < 1e-6);
    assert!(mesh.bounds().min.dist(Vec3::new(100., 0., 0.)) < 1e-6);
}

/// Driving an unchanged definition must map its warm reference triangles, not re-tessellate
/// the placed B-rep on the first mesh read of every new pose (viewport bounds does that read).
#[test]
fn joint_drive_reuses_reference_tessellation_across_rigid_poses() {
    let mut session = Session::default();
    run(&mut session, "solid.cylinder", json!({"radius":3,"height":8,"base":[4,0,0],"body_name":"Crank"}));
    run(&mut session, "component.create", json!({"name":"Crank","parent":"root","activate":false,"grounded":false}));
    run(&mut session, "component.move_bodies", json!({"bodies":["Crank"],"component":"Crank"}));
    run(
        &mut session,
        "joint.as_built",
        json!({"type":"revolute","name":"Crank","a":0,"b":"Crank","at":{"occurrence":0,"point":[0,0,0],"z":[0,0,1]}}),
    );
    let definitions = session.model.state();
    let original = session.world_state().unwrap();
    let local = original.body("Crank").unwrap().mesh();
    let bounds = local.bounds();
    for angle in [37., 91., 37., 0.] {
        let driven = run(&mut session, "joint.drive", json!({"joint":"Crank","value":angle}));
        assert_eq!(driven["moved"], true);
        assert!(Arc::ptr_eq(&definitions, &session.model.state()));
        let world = session.world_state().unwrap();
        let body = world.body("Crank").unwrap();
        let (mesh, work) = crate::kernel::profile::capture(|| body.mesh());
        assert_eq!(work.tessellate_ms, 0., "a warm definition must not enter the kernel mesher at a new pose: {work:?}");
        assert_eq!(mesh.positions.len(), local.positions.len());
        assert_eq!(mesh.normals.len(), local.normals.len());
        assert_eq!(mesh.edges.iter().map(Vec::len).collect::<Vec<_>>(), local.edges.iter().map(Vec::len).collect::<Vec<_>>());
        assert_eq!(mesh.triangles, local.triangles);
        assert_eq!(mesh.tri_face, local.tri_face);
        assert_eq!(mesh.edge_faces, local.edge_faces);
        assert_eq!(mesh.seams, local.seams);
        assert_eq!(mesh.smooth_edges, local.smooth_edges);
        assert_eq!(mesh.sharp_edges, local.sharp_edges);
        for (p, q) in local.positions.iter().zip(&mesh.positions) {
            assert!(crate::doc::apply_point(&body.source_transform, *p).dist(*q) < 1e-8);
        }
        for (n, q) in local.normals.iter().zip(&mesh.normals) {
            assert!(crate::doc::apply_vector(&body.source_transform, *n).dist(*q) < 1e-8);
        }
        for (edge, placed) in local.edges.iter().zip(&mesh.edges) {
            for (p, q) in edge.iter().zip(placed) {
                assert!(crate::doc::apply_point(&body.source_transform, *p).dist(*q) < 1e-8);
            }
        }
    }
    assert_eq!(local.bounds(), bounds, "old snapshots stay immutable");
}

/// Commands map world body names through every body's frame: worked out once while the world
/// state is current, and again when a placement moves or a body is added.
#[test]
fn body_frames_are_kept_with_the_world_state() {
    let mut session = Session::default();
    let first = run(&mut session, "component.create", json!({"name":"Part"}))["occurrence"].as_u64().unwrap();
    run(&mut session, "solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Block","operation":"new"}));
    let second = run(&mut session, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    let _ = session.world_state().unwrap();
    let frames = session.body_frames();
    assert!(Arc::ptr_eq(&frames, &session.body_frames()), "kept while the world state is current");
    let copy = session.occurrence_placement(&[second]).unwrap().bodies[0].world.clone();
    assert!(frames.iter().any(|(world, own, _)| *world == copy && own == "Block"), "a copy maps back to its definition");
    run(&mut session, "occurrence.move", json!({"occurrence":second,"translate":[10,0,0],"capture":false}));
    let _ = session.world_state().unwrap();
    let moved = session.body_frames();
    assert!(!Arc::ptr_eq(&frames, &moved), "a moved placement works them out again");
    let at = |frames: &crate::BodyFrames| frames.iter().find(|(world, ..)| *world == copy).map(|(_, _, m)| m[3][0]);
    assert_eq!(at(&moved), Some(10.0), "with the placement's new frame");
    assert_eq!(at(&frames), Some(0.0));
    run(&mut session, "solid.box", json!({"length":1,"width":1,"height":1,"body_name":"Other","operation":"new"}));
    let _ = session.world_state().unwrap();
    assert!(session.body_frames().iter().any(|(world, ..)| world.starts_with("Other")), "a new body is in them");
}
