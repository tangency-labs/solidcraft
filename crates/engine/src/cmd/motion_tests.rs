use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn occ(s: &Session, component: &str) -> u64 {
    let c = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0)
}

#[test]
fn deleted_motion_item_ids_are_not_reused_after_reopening() {
    for (create, delete, key, params) in [
        ("contact.create", "contact.edit", "set", json!({"occurrences": ["A", "B"]})),
        ("motion.study", "motion.delete", "study", json!({"name": "Travel", "steps": 10})),
        ("explode.create", "explode.delete", "view", json!({"moves": [{"occurrence": "B", "translate": [20, 0, 0]}]})),
    ] {
        let mut s = Session::default();
        for name in ["A", "B"] {
            run(&mut s, "component.activate", json!({"component": "root"}));
            run(&mut s, "component.create", json!({"name": name}));
        }
        let first = run(&mut s, create, params.clone())[key].as_u64().unwrap();
        let deletion = json!({key: first, "delete": true});
        run(&mut s, delete, deletion.clone());
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        *s.doc_mut() = doc;
        let second = run(&mut s, create, params)[key].as_u64().unwrap();
        assert!(second > first, "{create}: deleted item {first} was reused as {second}");
        assert!(s.execute(delete, &deletion).is_err());
    }
}

/// Where B's corner (20, 0, 0) is now.
fn b_x(s: &Session) -> f64 {
    super::world_point(s, occ(s, "B"), Vec3::new(20.0, 0.0, 0.0)).x
}

#[derive(Clone, Copy)]
enum PartAuthoring {
    RootThenTransfer,
    RootThenNestedTransfer,
    InComponent,
}

#[derive(Clone, Copy)]
enum JoinSelection {
    Named,
    Implicit,
    All,
}

/// A boolean authored before a later transfer still reads the bodies in its timeline input,
/// not their final owners. Explicit targets must not turn every occurrence into an input.
fn joined_motion_part(authoring: PartAuthoring, selection: JoinSelection) -> Session {
    let mut s = Session::default();
    if matches!(authoring, PartAuthoring::InComponent) {
        run(&mut s, "component.create", json!({"name":"Rotor","grounded":false}));
    }
    run(&mut s, "solid.box", json!({"length":10,"width":8,"height":6,"body_name":"Part","name":"Base"}));
    run(&mut s, "select.set", json!({"items":[]}));
    let mut p = json!({"length":6,"width":4,"height":4,"corner":[8,2,1],"operation":"join","name":"Boss"});
    match selection {
        JoinSelection::Named => p["targets"] = json!(["Part"]),
        JoinSelection::All => p["targets"] = json!(["*"]),
        JoinSelection::Implicit => {}
    }
    run(&mut s, "solid.box", p);
    if !matches!(authoring, PartAuthoring::InComponent) {
        let parent = if matches!(authoring, PartAuthoring::RootThenNestedTransfer) {
            run(&mut s, "component.create", json!({"name":"Group","activate":false,"grounded":false}));
            "Group"
        } else {
            "root"
        };
        run(&mut s, "component.create", json!({"name":"Rotor","parent":parent,"activate":false,"grounded":false}));
        run(&mut s, "component.move_bodies", json!({"bodies":["Part"],"component":"Rotor"}));
    }
    let driven = if matches!(authoring, PartAuthoring::RootThenNestedTransfer) { "Group" } else { "Rotor" };
    attach_motion_rotor(&mut s, driven, "Turn");
    s
}

fn attach_motion_rotor(s: &mut Session, component: &str, joint: &str) {
    run(s, "component.activate", json!({"component":"root"}));
    run(s, "joint.as_built", json!({"type":"revolute","name":joint,"a":0,"b":component,"at":{"occurrence":0,"point":[0,0,0],"z":[0,0,1]}}));
}

fn unrelated_motion_rotor(s: &mut Session) {
    run(s, "component.activate", json!({"component":"root"}));
    run(s, "component.create", json!({"name":"Unrelated","grounded":false}));
    run(s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[100,100,100],"body_name":"Remote"}));
    attach_motion_rotor(s, "Unrelated", "Other turn");
}

/// Test work and identity, not elapsed time. Every first visit to an angle must preserve the
/// definitions and their warm meshes, even across full turns; returning to a cached pose alone
/// would miss the regression. The world mesh is still in the new physical position.
fn assert_joint_only_moves(s: &mut Session, joint: &str) {
    use std::sync::Arc;
    let definitions = s.model.state();
    let meshes: Vec<_> = definitions.bodies.iter().map(|b| b.mesh()).collect();
    for body in &s.world_state().bodies {
        std::hint::black_box(body.mesh());
        std::hint::black_box(body.display_plan());
    }
    let joints = run(s, "joint.list", json!({"quiet":true}));
    let base = joints["joints"].as_array().unwrap().iter().find(|j| j["name"] == joint).unwrap()["values"][0]["value"].as_f64().unwrap();
    for offset in [5., 10., 15., 37., 91., 365., 721., 12418., 12423., 37., 0.] {
        let angle = base + offset;
        let r = run(s, "joint.drive", json!({"joint":joint,"value":angle,"quiet":true}));
        assert_eq!(r["moved"], true, "{r}");
        assert!(r["stopped_by_loop"].is_null() && r["stopped_by_contact"].is_null(), "{r}");
        let joints = run(s, "joint.list", json!({"quiet":true}));
        let actual = joints["joints"].as_array().unwrap().iter().find(|j| j["name"] == joint).unwrap()["values"][0]["value"].as_f64().unwrap();
        assert!((actual - angle).abs() < 1e-8, "{joint} did not reach {angle}: {actual}");
        assert_eq!(s.model.last_recomputed, 0, "{joint} at {angle}: unchanged geometry was evaluated again");
        let current = s.model.state();
        assert!(Arc::ptr_eq(&definitions, &current), "{joint} at {angle}: definition state replaced");
        for (b, mesh) in current.bodies.iter().zip(&meshes) {
            assert!(Arc::ptr_eq(&b.mesh(), mesh), "{}: canonical mesh replaced", b.name);
        }
        let world = s.world_state();
        let (_, work) = crate::kernel::profile::capture(|| {
            for b in &world.bodies {
                let local = definitions.body(b.definition_name()).unwrap().mesh();
                let placed = b.mesh();
                assert_eq!(placed.triangles, local.triangles);
                assert_eq!(placed.tri_face, local.tri_face);
                assert_eq!(placed.positions.len(), local.positions.len());
                assert_eq!(placed.normals.len(), local.normals.len());
                for (p, q) in local.positions.iter().zip(&placed.positions) {
                    assert!(crate::doc::apply_point(&b.source_transform, *p).dist(*q) < 1e-8);
                }
                for (n, q) in local.normals.iter().zip(&placed.normals) {
                    assert!(crate::doc::apply_vector(&b.source_transform, *n).dist(*q) < 1e-8);
                }
            }
        });
        assert_eq!(work.tessellate_ms, 0., "{joint} at {angle}: reference tessellation ran");
    }
}

fn assert_motion_matches_full_rebuild(s: &Session) {
    let mut full = crate::doc::Model::default();
    full.no_cache = true;
    full.evaluate(&s.doc);
    assert!(full.results.iter().all(|r| r.error.is_none()), "{:?}", full.results.iter().filter_map(|r| r.error.as_ref()).collect::<Vec<_>>());
    let (cached, fresh) = (s.model.state(), full.state());
    assert_eq!(cached.bodies.len(), fresh.bodies.len());
    for (a, b) in cached.bodies.iter().zip(&fresh.bodies) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.body.face_count(), b.body.face_count(), "{}", a.name);
        let (a_bounds, b_bounds) = (a.mesh().bounds(), b.mesh().bounds());
        assert!(a_bounds.min.dist(b_bounds.min) < 1e-8 && a_bounds.max.dist(b_bounds.max) < 1e-8, "{}", a.name);
        let (a_mass, b_mass) = (crate::kernel::measure(&a.body).unwrap(), crate::kernel::measure(&b.body).unwrap());
        assert!((a_mass.volume - b_mass.volume).abs() < 1e-6, "{}: {} vs {}", a.name, a_mass.volume, b_mass.volume);
    }
}

#[test]
fn joint_pose_named_union_before_transfer_retains_definitions() {
    assert_joint_only_moves(&mut joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named), "Turn");
}

#[test]
fn joint_pose_joined_cylinders_keep_mesh_cells_at_unseen_angles() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":8.5,"height":8,"base":[0,32,0],"axis":[0,1,0],"operation":"new","body_name":"Crank"}));
    run(
        &mut s,
        "solid.cylinder",
        json!({"radius":2.8,"height":30,"base":[0,36,0],"axis":[0.981627183447664,0,0.1908089953765448],"operation":"join","targets":["Crank"]}),
    );
    run(&mut s, "component.create", json!({"name":"Crank","activate":false,"grounded":false}));
    run(&mut s, "component.move_bodies", json!({"bodies":["Crank"],"component":"Crank"}));
    run(&mut s, "joint.as_built", json!({"type":"revolute","name":"Crank","a":0,"b":"Crank","at":{"occurrence":0,"point":[0,0,0],"z":[0,1,0]}}));
    assert_joint_only_moves(&mut s, "Crank");
}

#[test]
fn joint_pose_named_union_in_its_component_retains_definitions() {
    assert_joint_only_moves(&mut joined_motion_part(PartAuthoring::InComponent, JoinSelection::Named), "Turn");
}

#[test]
fn joint_pose_nested_body_transfer_ignores_ancestor_motion() {
    assert_joint_only_moves(&mut joined_motion_part(PartAuthoring::RootThenNestedTransfer, JoinSelection::Named), "Turn");
}

#[test]
fn joint_pose_second_occurrence_shares_the_definition() {
    let mut s = joined_motion_part(PartAuthoring::InComponent, JoinSelection::Named);
    let second = run(&mut s, "occurrence.copy", json!({"component":"Rotor","translate":[40,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "joint.as_built", json!({"type":"revolute","name":"Copy turn","a":0,"b":second,"at":{"occurrence":0,"point":[40,0,0],"z":[0,0,1]}}));
    assert_eq!(s.world_state().bodies.len(), 2);
    assert_joint_only_moves(&mut s, "Copy turn");
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_implicit_union_in_its_component_retains_definitions() {
    assert_joint_only_moves(&mut joined_motion_part(PartAuthoring::InComponent, JoinSelection::Implicit), "Turn");
}

#[test]
fn joint_pose_unrelated_part_does_not_invalidate_named_union() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named);
    unrelated_motion_rotor(&mut s);
    assert_joint_only_moves(&mut s, "Other turn");
}

#[test]
fn joint_pose_future_body_is_not_an_input_of_an_earlier_wildcard_union() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::All);
    unrelated_motion_rotor(&mut s);
    assert_joint_only_moves(&mut s, "Other turn");
}

#[test]
fn joint_pose_empty_cache_keeps_unchanged_definition_results() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named);
    *s.model.cache.lock().unwrap() = Default::default();
    assert_joint_only_moves(&mut s, "Turn");
}

#[test]
fn joint_pose_explicit_extrude_targets_ignore_unrelated_motion() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Base"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"BossOutline"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[8,2],"p1":[14,6]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"BossOutline","distance":3,"operation":"join","targets":["Base"],"name":"Boss"}));
    unrelated_motion_rotor(&mut s);
    assert_joint_only_moves(&mut s, "Other turn");
}

#[test]
fn joint_pose_combine_ignores_unrelated_motion() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[4,4,9],"body_name":"Boss","operation":"new"}));
    run(&mut s, "solid.combine", json!({"target":"Base","tools":["Boss"],"operation":"join"}));
    unrelated_motion_rotor(&mut s);
    assert_joint_only_moves(&mut s, "Other turn");
}

/// A fixed tool removes the top 5 mm of the target where x=8..12. Sliding the target
/// relative to it changes the removed volume from 100 to 200 mm³. Moving the tool together
/// with the target preserves that volume and must preserve the computed definition too.
fn cross_component_motion_cut(move_tool_with_target: bool, kind: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"MovingTarget","grounded":false}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Target"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    if move_tool_with_target {
        run(&mut s, "component.create", json!({"name":"Tool"}));
    }
    run(&mut s, "solid.box", json!({"length":4,"width":12,"height":6,"corner":[8,-1,5],"operation":"cut","targets":["Target"],"name":"Notch"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    let axis = if kind == "slider" { json!([1, 0, 0]) } else { json!([0, 0, 1]) };
    run(&mut s, "joint.as_built", json!({"type":kind,"name":"Motion","a":0,"b":"MovingTarget","at":{"occurrence":0,"point":[0,0,0],"z":axis}}));
    if move_tool_with_target {
        run(&mut s, "joint.rigid_group", json!({"occurrences":["MovingTarget","Tool"]}));
    }
    s
}

fn motion_target_volume(s: &Session) -> f64 {
    crate::kernel::measure(&s.model.state().body("Target").unwrap().body).unwrap().volume
}

fn motion_combine_cut(kind: &str, together: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"MovingTarget","grounded":false}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Target"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "component.create", json!({"name":"Tool"}));
    run(&mut s, "solid.box", json!({"length":4,"width":12,"height":6,"corner":[8,-1,5],"body_name":"Cutter","operation":"new"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "solid.combine", json!({"target":"Target","tools":["Cutter"],"operation":"cut"}));
    let axis = if kind == "slider" { json!([1, 0, 0]) } else { json!([0, 0, 1]) };
    run(&mut s, "joint.as_built", json!({"type":kind,"name":"Motion","a":0,"b":"MovingTarget","at":{"occurrence":0,"point":[0,0,0],"z":axis}}));
    if together {
        run(&mut s, "joint.rigid_group", json!({"occurrences":["MovingTarget","Tool"]}));
    }
    s
}

#[test]
fn joint_pose_combine_common_motion_keeps_the_target_frame() {
    let mut s = motion_combine_cut("revolute", true);
    assert_joint_only_moves(&mut s, "Motion");
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_combine_relative_motion_rebuilds_the_cut() {
    let mut s = motion_combine_cut("slider", false);
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    run(&mut s, "joint.drive", json!({"joint":"Motion","value":5}));
    let volume = motion_target_volume(&s);
    assert!((volume - 800.).abs() < 1e-6, "Combine must follow relative motion: {volume}");
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_target_body_offset_rebuilds_the_cut() {
    let mut s = cross_component_motion_cut(false, "slider");
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    let mut offset = crate::doc::IDENTITY;
    offset[3][0] = 5.;
    s.doc_mut().body_offsets.insert("Target".into(), offset);
    s.refresh();
    let volume = motion_target_volume(&s);
    assert!((volume - 800.).abs() < 1e-6, "the input body's offset changes the tool frame: {volume}");
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_unrelated_body_offset_keeps_named_union() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named);
    unrelated_motion_rotor(&mut s);
    let before = s.model.state();
    let mut offset = crate::doc::IDENTITY;
    offset[3][0] = 5.;
    s.doc_mut().body_offsets.insert("Remote".into(), offset);
    s.refresh();
    assert_eq!(s.model.last_recomputed, 0);
    assert!(std::sync::Arc::ptr_eq(&before, &s.model.state()));
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_implicit_union_reads_its_target_body_offset() {
    let mut s = joined_motion_part(PartAuthoring::InComponent, JoinSelection::Implicit);
    let volume = |s: &Session| crate::kernel::measure(&s.model.state().body("Part").unwrap().body).unwrap().volume;
    assert!((volume(&s) - 544.).abs() < 1e-6);
    let mut offset = crate::doc::IDENTITY;
    offset[3][0] = 5.;
    s.doc_mut().body_offsets.insert("Part".into(), offset);
    s.refresh();
    assert!((volume(&s) - 480.).abs() < 1e-6, "the join's tool is now entirely inside its target: {}", volume(&s));
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_true_relative_target_motion_rebuilds_the_cut() {
    let mut s = cross_component_motion_cut(false, "slider");
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    run(&mut s, "joint.drive", json!({"joint":"Motion","value":5}));
    let volume = motion_target_volume(&s);
    assert!((volume - 800.).abs() < 1e-6, "relative motion should leave 800 mm³, got {volume}");
    assert!(s.model.last_recomputed > 0, "a target moved relative to its tool");
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_shared_rigid_motion_keeps_the_cut_definition() {
    let mut s = cross_component_motion_cut(true, "slider");
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    let before = s.model.state();
    run(&mut s, "joint.drive", json!({"joint":"Motion","value":5}));
    assert_eq!(s.model.last_recomputed, 0, "relative target/tool placement did not change");
    assert!(std::sync::Arc::ptr_eq(&before, &s.model.state()));
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_shared_rotation_preserves_relative_cut_geometry() {
    let mut s = cross_component_motion_cut(true, "revolute");
    assert_joint_only_moves(&mut s, "Motion");
    assert!((motion_target_volume(&s) - 900.).abs() < 1e-6);
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_primed_ahead_result_does_not_replace_unchanged_geometry() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named);
    let definitions = s.model.state();
    let mesh = definitions.body("Part").unwrap().mesh();
    let commands = [("joint.drive".into(), json!({"joint":"Turn","value":5,"quiet":true}))];
    let evaluated = s.ahead().evaluate(&commands).unwrap();
    *s.model.cache.lock().unwrap() = Default::default();
    s.prime(&evaluated);
    run(&mut s, &commands[0].0, commands[0].1.clone());
    s.unprime();
    assert!(std::sync::Arc::ptr_eq(&definitions, &s.model.state()), "computing ahead must not replace an unchanged definition");
    assert!(std::sync::Arc::ptr_eq(&mesh, &s.model.state().body("Part").unwrap().mesh()));
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_save_and_reopen_keep_definitions_at_new_angles() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named);
    run(&mut s, "joint.drive", json!({"joint":"Turn","value":5}));
    let directory = std::env::temp_dir().join(format!(
        "solidcraft-joint-pose-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("part.solidcraft");
    run(&mut s, "file.save", json!({"path":path,"cache":true,"preview":true}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path":path}));
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
    assert_motion_matches_full_rebuild(&opened);
    assert_joint_only_moves(&mut opened, "Turn");
}

#[test]
fn joint_pose_undo_and_redo_match_fresh_geometry() {
    let mut s = joined_motion_part(PartAuthoring::RootThenTransfer, JoinSelection::Named);
    for angle in [5, 10] {
        run(&mut s, "joint.drive", json!({"joint":"Turn","value":angle}));
    }
    run(&mut s, "edit.undo", json!({}));
    let joints = run(&mut s, "joint.list", json!({"quiet":true}));
    assert!((joints["joints"][0]["values"][0]["value"].as_f64().unwrap() - 5.).abs() < 1e-8);
    assert_motion_matches_full_rebuild(&s);
    run(&mut s, "edit.redo", json!({}));
    let joints = run(&mut s, "joint.list", json!({"quiet":true}));
    assert!((joints["joints"][0]["values"][0]["value"].as_f64().unwrap() - 10.).abs() < 1e-8);
    assert_motion_matches_full_rebuild(&s);
}

#[test]
fn joint_pose_geometry_edit_invalidates_the_warm_definition_mesh() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"span","expression":"10 mm"}));
    run(&mut s, "component.create", json!({"name":"Rotor","grounded":false}));
    run(&mut s, "solid.box", json!({"length":"span","width":8,"height":6,"body_name":"Part"}));
    attach_motion_rotor(&mut s, "Rotor", "Turn");
    run(&mut s, "joint.drive", json!({"joint":"Turn","value":5}));
    let before = s.model.state().body("Part").unwrap().mesh();
    run(&mut s, "parameters.change", json!({"name":"span","expression":"12 mm"}));
    let model = s.model.state();
    let body = model.body("Part").unwrap();
    assert!(!std::sync::Arc::ptr_eq(&before, &body.mesh()));
    assert!((crate::kernel::measure(&body.body).unwrap().volume - 576.).abs() < 1e-6);
    assert_motion_matches_full_rebuild(&s);
}

/// Cubes A (grounded, x 0…10) and B (x 20…30) on a slider along x: B's −x face on A's +x face
/// at value 0, B 10 mm away to start.
fn rail() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    assert!((b_x(&s) - 20.0).abs() < 1e-6, "{}", b_x(&s));
    s
}

#[test]
fn contact_sets_stop_a_driven_joint_at_touching() {
    let mut s = rail();
    // No contact: B passes into A.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 5.0).abs() < 1e-6);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    super::super::joints::solve_counts::take();
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    let (all, full) = super::super::joints::solve_counts::take();
    assert!(all > 4, "contact must still test intermediate placements");
    assert_eq!(full, 1, "contact trials must not repeat DOF analysis");
    assert!(r["stopped_by_contact"].is_object(), "{r}");
    // Stopped where the faces touch (to the search's resolution, 15 mm / 2^14).
    assert!((b_x(&s) - 10.0).abs() < 2e-3, "{}", b_x(&s));
    assert!(b_x(&s) >= 10.0 - 2e-3, "a touch, not a pass");
    assert_eq!(run(&mut s, "contact.check", json!({}))["collisions"].as_array().map(Vec::len), Some(0));
    // Away is free; contact off lets it through again.
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    assert!(r["stopped_by_contact"].is_null() && (b_x(&s) - 13.0).abs() < 1e-6, "{r}");
    run(&mut s, "contact.disable_all", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 5.0).abs() < 1e-6);
    // Suppressed sets don't count; Enable All Contact puts every occurrence in one.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.enable_sets", json!({}));
    run(&mut s, "contact.edit", json!({"set": "Contact Set1", "suppressed": true}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 5.0).abs() < 1e-6);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.enable_all", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!((b_x(&s) - 10.0).abs() < 2e-3);
    assert!(s.execute("contact.create", &json!({"occurrences": ["A"]})).is_err());
}

/// A base plate and a lid plate (both 40 × 40 × 3) hinged along their shared edge, driven
/// past flat by `angle` degrees.
fn hinged_plates(angle: f64) -> Session {
    let mut s = Session::default();
    run(
        &mut s,
        "solid.box",
        json!({"length": 40.0, "width": 40.0, "height": 3.0, "corner": [-20.0, -40.0, 0.0], "operation": "new", "body_name": "BasePlate"}),
    );
    run(
        &mut s,
        "solid.box",
        json!({"length": 40.0, "width": 40.0, "height": 3.0, "corner": [-20.0, 0.0, 0.0], "operation": "new", "body_name": "LidPlate"}),
    );
    run(&mut s, "component.create", json!({"name": "Base", "parent": "root", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["BasePlate"], "component": "Base"}));
    run(&mut s, "component.create", json!({"name": "Lid", "parent": "root", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["LidPlate"], "component": "Lid"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "Base:1", "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "Hinge", "flip": true, "limits": [[0.0, 270.0]],
        "a": {"occurrence": "Base", "point": [0.0, 0.0, 3.0], "z": [1.0, 0.0, 0.0]},
        "b": {"occurrence": "Lid", "point": [0.0, 0.0, 3.0], "z": [1.0, 0.0, 0.0]}}),
    );
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": angle, "quiet": true}));
    run(&mut s, "contact.create", json!({"occurrences": ["Base:1", "Lid:1"]}));
    s
}

/// Regression (#3323): past the angles where a corner lies inside the other plate, no vertex,
/// mid-edge or triangle centre of either plate is inside it, and `contact.check` reported no
/// collision while `inspect.interference` found the overlap. The faces crossing is the
/// collision; the check must see it.
#[test]
fn contact_check_finds_plates_crossing_face_to_face() {
    // Flat against the base: touching is not crossing.
    let mut s = hinged_plates(180.0);
    assert_eq!(run(&mut s, "contact.check", json!({}))["collisions"].as_array().map(Vec::len), Some(0), "flat");
    for angle in [210.0, 240.0, 270.0] {
        let mut s = hinged_plates(angle);
        let collisions = run(&mut s, "contact.check", json!({}))["collisions"].clone();
        let c = collisions.as_array().unwrap();
        assert_eq!(c.len(), 1, "{angle}°: {c:?}");
        assert_eq!(c[0]["a"], "Base:1");
        assert_eq!(c[0]["b"], "Lid:1");
        assert!(c[0]["depth_mm"].as_f64().unwrap() > 0.5, "{angle}°: {c:?}");
    }
}

#[test]
fn contact_uses_preserved_body_placements_after_component_reassignment() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Transferred"}));
    let component = run(&mut s, "component.create", json!({"name": "Carrier", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": component["occurrence"], "translate": [100, 0, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Transferred"], "component": component["component"]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let probe = run(&mut s, "component.create", json!({"name": "Probe"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 0, 0]}));
    run(&mut s, "contact.create", json!({"occurrences": [component["occurrence"], probe["occurrence"]]}));
    let collisions = run(&mut s, "contact.check", json!({}));
    assert_eq!(collisions["collisions"].as_array().map(Vec::len), Some(1), "body stayed at x=0: {collisions}");
    run(&mut s, "occurrence.move", json!({"occurrence": probe["occurrence"], "translate": [100, 0, 0]}));
    let collisions = run(&mut s, "contact.check", json!({}));
    assert_eq!(collisions["collisions"].as_array().map(Vec::len), Some(0), "probe moved away: {collisions}");
}

#[test]
fn automatic_explode_centres_follow_reassigned_bodies() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Transferred"}));
    let component = run(&mut s, "component.create", json!({"name": "Carrier", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": component["occurrence"], "translate": [100, 0, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Transferred"], "component": component["component"]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Neighbour"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    let exploded = run(&mut s, "explode.create", json!({"scale": 1}));
    let moves = exploded["moves"].as_array().unwrap();
    let carrier = moves.iter().find(|m| m["occurrence"] == component["occurrence"]).unwrap();
    assert!((carrier["translate"][0].as_f64().unwrap() + 10.0).abs() < 1e-6, "{exploded}");
}

#[test]
fn motion_study_plays_and_exports() {
    let mut s = rail();
    let r = run(&mut s, "motion.study", json!({"name": "Push", "steps": 100, "keys": [{"joint": "Rail", "points": [[0, 10], [100, -5]]}]}));
    assert_eq!(r["keys"], 1);
    run(&mut s, "motion.play", json!({"study": "Push", "step": 50}));
    assert!((b_x(&s) - 12.5).abs() < 1e-6, "{}", b_x(&s));
    // With contact, the end of the study stops at touching.
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    let r = run(&mut s, "motion.play", json!({"study": "Push", "step": 100}));
    assert!(r["stopped_by_contact"].is_object() && (b_x(&s) - 10.0).abs() < 2e-3, "{r}");
    // Export: positions per sample; the design keeps its joint value.
    run(&mut s, "motion.play", json!({"study": "Push", "step": 0}));
    let before = s.doc.clone();
    let e = run(&mut s, "motion.export", json!({"study": "Push", "samples": 5}));
    assert_eq!(*s.doc, *before);
    let rows = e["samples"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 5);
    let bx = |row: &Value| {
        let p = row["positions"].as_array().and_then(|a| a.iter().find(|p| p["occurrence"] == "B:1")).cloned().unwrap_or_default();
        p["pose"]["origin"][0].as_f64().unwrap_or(f64::NAN)
    };
    // The B occurrence's origin moves by the slider value (it starts at 0 offset with value 0 at 10 mm… so 10 → −5).
    assert!((bx(&rows[1]) - bx(&rows[0]) + 3.75).abs() < 1e-6, "{e}");
    assert!(rows[4]["stopped_by_contact"].is_object());
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-motion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("push.csv");
    run(&mut s, "motion.export", json!({"study": "Push", "samples": 3, "path": csv.to_string_lossy()}));
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with("step,occurrence,x,y,z") && text.lines().count() == 1 + 3 * 2, "{text}");
    // For an agent (#205) the export lands only inside its folders.
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.clone()])), || {
        run(&mut s, "motion.export", json!({"study": "Push", "samples": 2, "path": root.join("in.json").to_string_lossy()}));
        let e = s.execute("motion.export", &json!({"study": "Push", "samples": 2, "path": dir.join("out.json").to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("outside the folders this agent may use"), "{e}");
    });
    assert!(root.join("in.json").exists() && !dir.join("out.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
    assert!(s.execute("motion.study", &json!({"keys": [{"joint": "Rail", "index": 3, "points": [[0, 1]]}]})).is_err());
    run(&mut s, "motion.delete", json!({"study": "Push"}));
    assert_eq!(run(&mut s, "motion.list", json!({}))["studies"].as_array().map(Vec::len), Some(0));
}

#[test]
fn exploded_views_show_without_changing_the_design() {
    let mut s = rail();
    let before = s.doc.occurrences.clone();
    let r = run(&mut s, "explode.create", json!({"name": "Apart", "scale": 1}));
    assert_eq!(r["moves"].as_array().map(Vec::len), Some(2), "{r}");
    run(&mut s, "explode.show", json!({"view": "Apart"}));
    // Centres at x 5 and 25 → middle 15: each moves 10 mm further out.
    let bb = |s: &mut Session, b: &str| run(s, "inspect.measure", json!({"bodies": [b]}))["bodies"][0]["bbox"]["min"][0].as_f64().unwrap_or(f64::NAN);
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    assert!((bb(&mut s, &names[0]) + 10.0).abs() < 1e-6, "{names:?}");
    assert!((bb(&mut s, &names[1]) - 30.0).abs() < 1e-6);
    assert_eq!(s.doc.occurrences, before, "shown only");
    run(&mut s, "explode.show", json!({"view": null}));
    assert!(bb(&mut s, &names[0]).abs() < 1e-6);
    run(&mut s, "explode.create", json!({"name": "Hand", "moves": [{"occurrence": "B", "translate": [0, 0, 50]}]}));
    run(&mut s, "explode.show", json!({"view": "Hand"}));
    assert!(
        (run(&mut s, "inspect.measure", json!({"bodies": [names[1]]}))["bodies"][0]["bbox"]["min"][2].as_f64().unwrap_or(0.0) - 50.0).abs() < 1e-6
    );
    assert_eq!(run(&mut s, "explode.list", json!({}))["views"].as_array().map(Vec::len), Some(2));
}

/// Pictures drawn by the CPU rasterizer (the engine has no GPU): real views of the scene.
fn cpu(scene: &crate::render::Scene, cam: &crate::render::Camera, w: u32, h: u32) -> Result<crate::capture::Picture, String> {
    Ok(crate::capture::Picture { rgba: crate::render::render(scene, cam, w as usize, h as usize).rgba, renderer: "cpu" })
}

/// A folder of its own for one test's files.
fn out_dir(test: &str) -> std::path::PathBuf {
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-anim-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A GIF's logical screen size and the number of its frames (image descriptors).
fn gif_frames(gif: &[u8]) -> (usize, usize, usize) {
    let u16_at = |i: usize| usize::from(u16::from_le_bytes([gif[i], gif[i + 1]]));
    assert_eq!(&gif[..6], b"GIF89a");
    let mut at = 13 + if gif[10] & 0x80 != 0 { 3 << ((gif[10] & 7) + 1) } else { 0 };
    let mut frames = 0;
    // Each block: a size byte and that many bytes, up to a 0.
    let skip_blocks = |mut at: usize| {
        while gif[at] != 0 {
            at += usize::from(gif[at]) + 1;
        }
        at + 1
    };
    loop {
        match gif[at] {
            0x3B => return (u16_at(6), u16_at(8), frames),
            0x21 => at = skip_blocks(at + 2),
            0x2C => {
                frames += 1;
                at = skip_blocks(at + 11);
            }
            b => panic!("unexpected GIF block {b:#x}"),
        }
    }
}

/// A PNG's width and height (its IHDR).
fn png_size(png: &[u8]) -> (u32, u32) {
    let be = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
    (be(16), be(20))
}

#[test]
fn an_animation_has_the_frames_and_the_size_asked_for() {
    let mut s = rail();
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 10, "keys": [{"joint": "Rail", "points": [[0, 10], [10, 0]]}]}));
    let dir = out_dir("size");
    let before = s.doc.clone();
    let gif = dir.join("push.gif");
    let p = json!({"study": "Push", "path": gif.to_string_lossy(), "samples": 6, "width": 64, "height": 40, "fps": 25});
    let r = super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_eq!(
        (r["format"].as_str(), r["frames"].as_u64(), r["width"].as_u64(), r["height"].as_u64()),
        (Some("gif"), Some(6), Some(64), Some(40)),
        "{r}"
    );
    assert_eq!(r["fps"], 25.0, "{r}");
    let bytes = std::fs::read(&gif).unwrap();
    assert_eq!(r["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(gif_frames(&bytes), (64, 40, 6));
    // Without samples: the study's steps + 1.
    let r =
        super::animation::export_with(&s, &json!({"study": "Push", "path": gif.to_string_lossy(), "width": 32, "height": 32}), Some(&cpu)).unwrap();
    assert_eq!(gif_frames(&std::fs::read(&gif).unwrap()), (32, 32, 11), "{r}");
    // A folder of PNG frames, made where missing; a joint driven instead of a study.
    let frames = dir.join("frames").join("rail");
    let p = json!({"joint": "Rail", "from": 10, "to": 0, "path": frames.to_string_lossy(), "samples": 3, "width": 48, "height": 30});
    let r = super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_eq!((r["format"].as_str(), r["frames"].as_u64()), (Some("png"), Some(3)), "{r}");
    let mut names: Vec<String> = std::fs::read_dir(&frames).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["frame-0001.png", "frame-0002.png", "frame-0003.png"]);
    assert!(names.iter().all(|n| png_size(&std::fs::read(frames.join(n)).unwrap()) == (48, 30)));
    // A shorter export into the same folder leaves no frames of a longer one behind, past a gap
    // too; other files stay.
    std::fs::write(frames.join("notes.txt"), "mine").unwrap();
    std::fs::write(frames.join("frame-0005.png"), "older").unwrap();
    super::animation::export_with(&s, &json!({"joint": "Rail", "from": 10, "to": 0, "path": frames.to_string_lossy(), "samples": 2}), Some(&cpu))
        .unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&frames).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["frame-0001.png", "frame-0002.png", "notes.txt"]);
    assert_eq!(*s.doc, *before, "the design is not changed");
    // Through the registry, with the renderer the host installed.
    crate::capture::install_test_renderer();
    let r = run(&mut s, "motion.export_animation", json!({"study": "Push", "path": gif.to_string_lossy(), "samples": 2, "width": 16, "height": 16}));
    assert_eq!(r["frames"], 2, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_two_frame_drive_gives_two_different_frames() {
    let s = rail();
    let dir = out_dir("two");
    let frames = |name: &str, to: f64| {
        let folder = dir.join(name);
        let p = json!({"joint": "Rail", "from": 10, "to": to, "path": folder.to_string_lossy(), "samples": 2, "width": 120, "height": 80});
        super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
        (std::fs::read(folder.join("frame-0001.png")).unwrap(), std::fs::read(folder.join("frame-0002.png")).unwrap())
    };
    let (a, b) = frames("moved", -5.0);
    assert_ne!(a, b, "B moved 15 mm between the frames");
    // Held still, the two frames are the same picture: the difference is the motion.
    let (c, d) = frames("still", 10.0);
    assert_eq!(c, d);
    assert_eq!(a, c, "both start at 10 mm");
    // Without the ground grid and the axes, the same pose is another picture.
    let bare = dir.join("bare");
    let p = json!({"joint": "Rail", "from": 10, "to": 10, "path": bare.to_string_lossy(), "samples": 2, "width": 120, "height": 80, "grid": false, "axes": false});
    super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
    assert_ne!(std::fs::read(bare.join("frame-0001.png")).unwrap(), c);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_animation_is_drawn_from_the_camera_given() {
    let mut s = rail();
    let dir = out_dir("camera");
    // A renderer is 'static (hosts install it for the process): it shares what it saw.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = {
        let seen = seen.clone();
        move |scene: &crate::render::Scene, cam: &crate::render::Camera, w: u32, h: u32| {
            seen.lock().unwrap().push(*cam);
            cpu(scene, cam, w, h)
        }
    };
    let path = dir.join("cam.gif");
    let base = json!({"joint": "Rail", "from": 10, "to": 0, "path": path.to_string_lossy(), "samples": 3, "width": 40, "height": 30});
    let with = |camera: Value| {
        let mut p = base.clone();
        p["camera"] = camera;
        p
    };
    // ui.camera's reply, every field.
    let reply = json!({"target": [1.0, 2.0, 3.0], "yaw": 0.5, "pitch": -0.25, "roll": 0.1, "distance": 300.0, "fov": 0.6, "up": "y"});
    super::animation::export_with(&s, &with(reply.clone()), Some(&record)).unwrap();
    let cams = std::mem::take(&mut *seen.lock().unwrap());
    assert_eq!(cams.len(), 3);
    assert!(cams.iter().all(|c| json!(c) == reply), "{cams:?}");
    // Fields left out keep the default view: view.apply's camera.
    run(&mut s, "view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 1, "pitch": 0.2, "distance": 80}}));
    super::animation::export_with(&s, &with(json!({"yaw": 2.0})), Some(&record)).unwrap();
    let cams = std::mem::take(&mut *seen.lock().unwrap());
    assert!(cams.iter().all(|c| c.yaw == 2.0 && c.pitch == 0.2 && c.distance == 80.0), "{cams:?}");
    // Without one: view.apply's camera as it is; and another camera draws another picture.
    super::animation::export_with(&s, &base, Some(&record)).unwrap();
    assert!(seen.lock().unwrap().iter().all(|c| Some(*c) == s.camera));
    let pictures = |camera: Value| {
        let folder = dir.join(format!("p{}", camera["yaw"]));
        let mut p = with(camera);
        p["path"] = json!(folder.to_string_lossy());
        super::animation::export_with(&s, &p, Some(&cpu)).unwrap();
        std::fs::read(folder.join("frame-0001.png")).unwrap()
    };
    assert_ne!(pictures(json!({"yaw": 0.3})), pictures(json!({"yaw": 1.3})));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_animation_params_are_refused() {
    let mut s = rail();
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 10, "keys": [{"joint": "Rail", "points": [[0, 10], [10, 0]]}]}));
    let dir = out_dir("hostile");
    let gif = dir.join("x.gif").to_string_lossy().to_string();
    let base = json!({"study": "Push", "path": gif, "samples": 2, "width": 16, "height": 16});
    let refused = |change: Value| {
        let mut p = base.clone();
        if let (Some(p), Some(change)) = (p.as_object_mut(), change.as_object()) {
            for (k, v) in change {
                if v.is_null() {
                    p.remove(k);
                } else {
                    p.insert(k.clone(), v.clone());
                }
            }
        }
        super::animation::export_with(&s, &p, Some(&cpu)).err().map(|e| e.to_string())
    };
    for change in [
        json!({"samples": 0}),
        json!({"samples": 1}),
        json!({"samples": 1e9}),
        json!({"samples": "many"}),
        json!({"samples": 1001}),
        json!({"width": 1e9}),
        json!({"width": -16}),
        json!({"height": 3}),
        json!({"height": f64::MAX}),
        json!({"fps": 0}),
        json!({"fps": 1e9}),
        json!({"path": ""}),
        json!({"path": null}),
        json!({"path": "one.png"}),
        json!({"path": "a\u{0}b.gif"}),
        json!({"path": "x".repeat(5000)}),
        json!({"camera": "iso"}),
        json!({"camera": {"distance": -1}}),
        json!({"camera": {"distance": 1e300}}),
        json!({"camera": {"pitch": 4}}),
        json!({"camera": {"fov": 9}}),
        json!({"camera": {"target": [1, 2]}}),
        json!({"camera": {"fit": true}}),
        json!({"grid": "no"}),
        json!({"axes": 1}),
        json!({"study": null}),
        json!({"study": "Nope"}),
        json!({"study": null, "joint": "Rail"}),
        // 4096 × 4096 frames: a GIF holds four of them.
        json!({"width": 4096, "height": 4096, "samples": 5}),
    ] {
        assert!(refused(change.clone()).is_some(), "{change} was accepted");
    }
    let e = refused(json!({"width": 4096, "height": 4096, "samples": 5})).unwrap();
    assert!(e.contains("2…4 for 4096 × 4096"), "{e}");
    assert!(super::animation::export_with(&s, &base, None).unwrap_err().to_string().contains("no renderer"));
    assert!(!std::path::Path::new(&gif).exists(), "nothing refused was written");
    // An agent's export lands only inside its folders.
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.clone()])), || {
        let e = refused(json!({"path": dir.join("out").to_string_lossy()})).unwrap();
        assert!(e.contains("outside the folders this agent may use"), "{e}");
        assert!(refused(json!({"path": root.join("in.gif").to_string_lossy()})).is_none());
    });
    assert!(root.join("in.gif").exists() && !dir.join("out").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A grounded Base plate, an Arm turning on it about Z (joint `Pivot`) and a Head fixed to the
/// arm. The Arm's slot sketch projects the arm's own edge and its axis, the Head's bore sketch
/// an edge of the arm (they move together). With `in_context`, the slot sketch also projects an
/// edge of the Base, across the joint.
fn pivot(in_context: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "Plate"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[0, -20, 10]], "distance": 1}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Arm"}));
    run(&mut s, "solid.box", json!({"length": 50, "width": 8, "height": 5, "corner": [-4, -4, 10], "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 15], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Slot"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [20, 4, 15], "body": "Bar"}, "origin"]}));
    if in_context {
        run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, 20, 10], "body": "Plate"}]}));
    }
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, -2], "p1": [30, 2]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "distance": -2, "operation": "cut", "name": "SlotCut"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[10, -4, 15]], "distance": 0.5}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Head"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 12, "height": 6, "corner": [36, -6, 15], "body_name": "Cap"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 21], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Bore"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [40, 4, 15], "body": "Bar"}]}));
    run(&mut s, "sketch.circle.center", json!({"center": [41, 0], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Bore", "distance": -4, "operation": "cut", "name": "BoreCut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let (base, arm, head) = (occ(&s, "Base"), occ(&s, "Arm"), occ(&s, "Head"));
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": base, "point": [0, 0, 10]}, "b": {"occurrence": arm, "point": [0, 0, 10]}, "flip": true, "name": "Pivot"}),
    );
    run(&mut s, "joint.rigid_group", json!({"occurrences": [arm, head]}));
    s
}

/// Every body of `model` and `full` the same: names, faces and where their meshes lie.
fn same_bodies(model: &solidcraft_doc::ModelState, full: &solidcraft_doc::ModelState) {
    assert_eq!(model.bodies.len(), full.bodies.len());
    for (a, b) in model.bodies.iter().zip(&full.bodies) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.body.face_count(), b.body.face_count(), "{}", a.name);
        let (m, n) = (a.mesh().bounds(), b.mesh().bounds());
        assert!(m.min.dist(n.min) < 1e-9 && m.max.dist(n.max) < 1e-9, "{}: {m:?} vs {n:?}", a.name);
    }
}

/// The model `s` shows, evaluated again from nothing.
fn rebuilt(s: &Session) -> std::sync::Arc<solidcraft_doc::ModelState> {
    let mut full = solidcraft_doc::Model::default();
    full.no_cache = true;
    full.evaluate(&s.doc);
    full.state()
}

/// Driving a joint only moves parts: a part that projects its own geometry, or that of a part
/// moving with it, is not evaluated again (#1989's motion on a finished assembly evaluated the
/// whole timeline on every drive, and every new pose missed the cache).
#[test]
fn driving_a_joint_moves_parts_without_evaluating_them() {
    let mut s = pivot(false);
    let model = s.model.state();
    let head = occ(&s, "Head");
    let tip = |s: &Session| super::world_point(s, head, Vec3::new(46.0, 0.0, 15.0));
    let before = tip(&s);
    for (step, angle) in ["30 deg", "75 deg", "-40 deg", "75 deg"].into_iter().enumerate() {
        let r = run(&mut s, "joint.drive", json!({"joint": "Pivot", "value": angle}));
        assert_eq!(r["moved"], true, "{r}");
        assert_eq!((s.model.last_recomputed, s.model.last_restored), (0, 0), "step {step}: nothing evaluated, nothing taken from the cache");
        assert!(std::sync::Arc::ptr_eq(&model, &s.model.state()), "the parts' bodies are the ones they were");
    }
    // The head went round with the arm: 75° about Z through the origin.
    let (sin, cos) = 75f64.to_radians().sin_cos();
    let want = Vec3::new(before.x * cos - before.y * sin, before.x * sin + before.y * cos, before.z);
    assert!(tip(&s).dist(want) < 1e-6, "{:?} vs {want:?}", tip(&s));
    same_bodies(&s.model.state(), &rebuilt(&s));
}

/// A sketch that projects another part's edge is in-context design: it is evaluated again when
/// that part moves relative to it (and gives what a full rebuild gives), not when the two move
/// together.
#[test]
fn an_in_context_sketch_follows_only_the_parts_it_reads() {
    let mut s = pivot(true);
    let slot = s.doc.find_feature("Slot").map(|f| f.id).unwrap();
    let projected = |s: &Session| s.model.state().sketch(slot).map(|k| k.sketch.clone()).unwrap();
    let first = projected(&s);
    run(&mut s, "joint.drive", json!({"joint": "Pivot", "value": "30 deg"}));
    assert!(s.model.last_recomputed > 0, "the slot sketch reads the base, which turned relative to it");
    assert_ne!(projected(&s).links, first.links, "the base's edge is where the arm now sees it");
    same_bodies(&s.model.state(), &rebuilt(&s));
    // The whole assembly moved as one (both occurrences by the same turn and shift): nothing the
    // sketch reads moved relative to it.
    let model = s.model.state();
    let turn = solidcraft_doc::rigid(Vec3::new(5.0, -3.0, 2.0), Vec3::new(1.0, 2.0, 0.0), Vec3::new(0.3, 0.2, 1.0), 0.7);
    for o in &mut s.doc_mut().occurrences {
        o.transform = solidcraft_doc::mat_mul(&turn, &o.transform);
    }
    s.refresh();
    assert_eq!(s.model.last_recomputed, 0);
    assert!(std::sync::Arc::ptr_eq(&model, &s.model.state()));
    same_bodies(&s.model.state(), &rebuilt(&s));
}
