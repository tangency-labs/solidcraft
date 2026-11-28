//! Each ribbon feature form keeps its parameter builder while sharing the dense layout.
#[path = "../../../solidcore/crates/solidcore/tests/support/annular_samples.rs"]
mod annular_samples;

use super::*;
use crate::scenario::{self, Harness};

fn repeated_profile_fixture() -> (Harness, u64, u64) {
    let mut h = Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    for x in [0, 4] {
        h.app.run("sketch.rectangle.two_point", json!({"p0":[x,0],"p1":[x+2,2]})).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    let path = h.app.run("sketch.create", json!({"plane":"XZ"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,10]]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    (h, sketch, path)
}

/// Twins picked by their numbers are saved by the sides of their curves (#22548). After a change
/// of the sketch's regions the dialog refills with the regions the feature builds, not none: two
/// circles of radius `r` and a fixed line through both centres, the halves of the lens and of
/// the lower crescent picked on r = 3.04 mm; on 3.45 the line no longer cuts the lens.
#[test]
fn profile_refill_of_sided_twins_follows_a_change_of_the_regions() {
    use solidcraft_engine::doc::FeatureKind;
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name": "r", "expression": "3.04 mm"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY", "name": "s"})).unwrap()["sketch"].as_u64().unwrap();
    for (id, y) in [("c1", 19.4), ("c2", 20.0)] {
        h.app.run("sketch.circle.center", json!({"center": [0, y], "radius": 3.04})).unwrap();
        h.app.run("sketch.dimension", json!({"entities": [id], "type": "diameter", "value": "2*r"})).unwrap();
        h.app.run("sketch.constraint.fix", json!({"entity": format!("{id}.center"), "fixed": true})).unwrap();
    }
    h.app.run("sketch.line", json!({"points": [[0, 1.75], [0, 22.5]]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity": "l1", "fixed": true})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let picks = [(-1.26, 19.7), (1.26, 19.7), (-0.5, 16.5), (0.5, 16.5)].map(|(x, y)| solidcore_record::Vec2::new(x, y));
    let at = |h: &Harness, points: &[solidcore_record::Vec2]| {
        let state = h.app.session.world_state().unwrap();
        let ss = state.sketch(sketch).unwrap();
        let mut found: Vec<usize> = points.iter().map(|q| solidcraft_engine::doc::sketch::profile_at_point(&ss.profiles, *q).unwrap()).collect();
        found.sort_unstable();
        found.dedup();
        found
    };
    let indices = at(&h, &picks);
    assert_eq!(indices.len(), 4);
    let feature = h.app.run("solid.extrude", json!({"sketch": sketch, "profiles": indices, "distance": 2})).unwrap()["feature"].as_u64().unwrap();
    h.app.run("parameters.change", json!({"name": "r", "expression": "3.45 mm"})).unwrap();
    let Some(FeatureKind::Extrude { profiles, .. }) = h.app.session.doc.feature(feature).map(|f| f.kind.clone()) else { panic!("an extrude") };
    assert!(matches!(profiles, ProfileSel::SidedCurves { .. }), "{profiles:?}");
    let state = h.app.session.world_state().unwrap();
    let mut refilled = profile_indices(state.sketch(sketch).unwrap(), &profiles);
    refilled.sort_unstable();
    assert_eq!(refilled, at(&h, &picks[1..]), "the lens and the crescent's two halves");
}

/// A region picked by a point is saved by its curves and the sides it lies on (#23291). When a
/// change makes a twin bounded by the same curves, the dialog refills with the region the feature
/// builds: a circle of radius 10, a fixed line through it, and a circle of radius r2 at (3, 10)
/// that crosses the line on r2 = 4 mm.
#[test]
fn profile_refill_of_a_curve_pick_follows_its_sides_when_a_change_makes_a_twin() {
    use solidcraft_engine::doc::FeatureKind;
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name": "r2", "expression": "2 mm"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY", "name": "s"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 10})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity": "c1", "fixed": true})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, -15], [0, 15]]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity": "l1", "fixed": true})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [3, 10], "radius": 2})).unwrap();
    h.app.run("sketch.dimension", json!({"entities": ["c2"], "type": "diameter", "value": "2*r2"})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity": "c2.center", "fixed": true})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let made = h.app.run("solid.extrude", json!({"sketch": sketch, "profiles": [{"point": [5, 0]}], "distance": 2})).unwrap();
    let feature = made["feature"].as_u64().unwrap();
    h.app.run("parameters.change", json!({"name": "r2", "expression": "4 mm"})).unwrap();
    let Some(FeatureKind::Extrude { profiles, .. }) = h.app.session.doc.feature(feature).map(|f| f.kind.clone()) else { panic!("an extrude") };
    assert!(matches!(&profiles, ProfileSel::Curves { sides, .. } if !sides.is_empty()), "{profiles:?}");
    let state = h.app.session.world_state().unwrap();
    let ss = state.sketch(sketch).unwrap();
    let here = solidcraft_engine::doc::sketch::profile_at_point(&ss.profiles, solidcore_record::Vec2::new(5.0, 0.0)).unwrap();
    assert_eq!(profile_indices(ss, &profiles), vec![here]);
}

#[test]
fn profile_refill_resolves_each_region_once_for_extrude_and_sweep() {
    for command in ["solid.extrude", "solid.sweep"] {
        let (mut h, sketch, path) = repeated_profile_fixture();
        let state = h.app.session.world_state().unwrap();
        let ss = state.sketch(sketch).unwrap();
        assert_eq!(ss.profiles.len(), 2);
        // Distinct saved places can name the same region after a divider disappears: two places
        // in the square at x 4..6, two in the square at x 0..2, alternating.
        let points: Vec<_> = [(5.0, 1.0), (1.01, 1.0), (5.02, 1.0), (1.03, 1.0)].map(|(x, y)| solidcore_record::Vec2::new(x, y)).to_vec();
        let order: Vec<usize> = points.iter().map(|q| solidcraft_engine::doc::sketch::profile_at_point(&ss.profiles, *q).unwrap()).collect();
        let (right, left) = (order[0], order[1]);
        assert_ne!(right, left);
        let loops = order.iter().map(|&i| ss.profiles[i].outer_curves.clone()).collect();
        let curve = &state.sketch(path).unwrap().sketch.curves[0].id;
        let made = h.app.run(command, json!({"sketch":sketch,"profiles":[1,0],"distance":10,"path_sketch":path,"path":[curve]})).unwrap();
        let feature = made["feature"].as_u64().unwrap();
        for selection in
            [ProfileSel::Points { points }, ProfileSel::Curves { loops, sides: Vec::new() }, ProfileSel::Indices { indices: order.clone() }]
        {
            let kind = &mut h.app.session.doc_mut().feature_mut(feature).unwrap().kind;
            match kind {
                FeatureKind::Extrude { profiles, .. } | FeatureKind::Sweep { profiles, .. } => *profiles = selection,
                _ => panic!("a profile feature"),
            }
            let d = for_feature(&h.app, feature, None).unwrap();
            assert_eq!(d.inputs[0].items, vec![Sel::Profile { sketch, index: right }, Sel::Profile { sketch, index: left }], "{command}");
            let commands = apply_commands(&h.app, &d).unwrap();
            assert_eq!(commands[0].1["params"]["profiles"], json!([right, left]), "{command}");
        }
    }
}

#[test]
fn profile_commands_send_each_region_once_for_extrude_and_sweep() {
    for command in ["solid.extrude", "solid.sweep"] {
        // Repeated picks can also arrive from older saved selections or external callers.
        for (order, expected) in [(vec![1, 0, 1, 0], vec![1, 0]), (vec![0; 6], vec![0])] {
            let (mut h, sketch, path) = repeated_profile_fixture();
            let state = h.app.session.world_state().unwrap();
            let curve = &state.sketch(path).unwrap().sketch.curves[0].id;
            let mut d = Dialog::for_command(&h.app, command).unwrap();
            d.inputs[0].items = order.into_iter().map(|index| Sel::Profile { sketch, index }).collect();
            if command == "solid.sweep" {
                d.inputs[1].items = vec![Sel::SketchCurve { sketch: Some(path), id: curve.clone() }];
            }
            let commands = apply_commands(&h.app, &d).unwrap();
            assert_eq!(commands[0].1["profiles"], json!(expected), "{command}");
            assert_eq!(commands[0].1["exact_regions"], json!(true), "{command}");
            h.app.run(&commands[0].0, commands[0].1.clone()).unwrap();
            // Each 2 x 2 mm square travels 10 mm: 40 mm³ per selected region.
            let volume = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
            assert!((volume - 40.0 * expected.len() as f64).abs() < 1e-6, "{command}: {volume}");
        }
    }
}

#[test]
fn profile_commands_keep_loft_section_order_when_removing_repeats() {
    let (mut h, sketch, _) = repeated_profile_fixture();
    let other = h.app.run("sketch.create", json!({"plane":"XY","offset":20})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[2,2]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.loft").unwrap();
    d.inputs[0].items = [(sketch, 1), (sketch, 1), (sketch, 0), (other, 0), (sketch, 1), (sketch, 1)]
        .into_iter()
        .map(|(sketch, index)| Sel::Profile { sketch, index })
        .collect();
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(
        commands[0].1["sections"],
        json!([
            {"sketch":sketch,"profiles":[1,0]},
            {"sketch":other,"profiles":[0]},
            {"sketch":sketch,"profiles":[1]}
        ])
    );
}

#[test]
fn profile_batch_picks_keep_one_sketch_for_extrude_and_sweep() {
    let (mut h, first, path) = repeated_profile_fixture();
    let mut sketches = vec![first];
    for z in 1..6 {
        let sketch = h.app.run("sketch.create", json!({"plane":"XY","offset":z*20})).unwrap()["sketch"].as_u64().unwrap();
        for x in [0, 4] {
            h.app.run("sketch.rectangle.two_point", json!({"p0":[x,0],"p1":[x+2,2]})).unwrap();
        }
        h.app.run("sketch.finish", json!({})).unwrap();
        sketches.push(sketch);
    }
    let last = *sketches.last().unwrap();
    let state = h.app.session.world_state().unwrap();
    let curve = &state.sketch(path).unwrap().sketch.curves[0].id;
    for command in ["solid.extrude", "solid.sweep"] {
        for indices in [vec![1, 0], vec![0]] {
            let picks: Vec<_> = sketches.iter().flat_map(|&sketch| indices.iter().map(move |&index| Sel::Profile { sketch, index })).collect();
            let expected: Vec<_> = indices.iter().map(|&index| Sel::Profile { sketch: last, index }).collect();
            for source in ["preselection", "box", "add_box", "click"] {
                h.app.session.selection = if source == "preselection" { picks.clone() } else { vec![] };
                let mut d = Dialog::for_command(&h.app, command).unwrap();
                match source {
                    "box" => d.take_box(&h.app.session, picks.clone(), false),
                    "add_box" => {
                        d.pick(&h.app.session, picks[0].clone());
                        d.take_box(&h.app.session, picks.clone(), true);
                    }
                    "click" => {
                        for pick in &picks {
                            d.pick(&h.app.session, pick.clone());
                        }
                    }
                    _ => {}
                }
                assert_eq!(d.inputs[0].items, expected, "{command}: {source}");
                if command == "solid.sweep" {
                    d.inputs[1].items = vec![Sel::SketchCurve { sketch: Some(path), id: curve.clone() }];
                }
                let commands = apply_commands(&h.app, &d).unwrap();
                assert_eq!(commands[0].1["sketch"], json!(last), "{command}: {source}");
                assert_eq!(commands[0].1["profiles"], json!(indices), "{command}: {source}");
                assert_eq!(commands[0].1["exact_regions"], json!(true));
            }
        }
    }
}

#[test]
fn measure_input_fits_external_sketch_geometry() {
    let mut h = Harness::new();
    let curve = Sel::ExternalSketchCurve { sketch: 17, id: "line1".into() };
    let point = Sel::ExternalSketchPoint { sketch: 17, id: "p1".into() };
    h.app.session.selection = vec![curve.clone(), point.clone()];
    let mut d = Dialog::for_command(&h.app, "inspect.measure").unwrap();
    let accept = d.inputs.first().unwrap().accept;
    assert!(fits(accept, &curve));
    assert!(fits(accept, &point));
    assert_eq!(d.items(), vec![curve.clone(), point.clone()]);
    assert_eq!(
        d.candidate(&h.app.session, &Hit::SketchCurve { sketch: 17, id: "line1".into(), straight: true, point: solidcore_record::Vec3::ZERO }),
        Some(curve.clone())
    );
    assert_eq!(
        d.candidate(&h.app.session, &Hit::SketchPoint { sketch: 17, id: "p1".into(), at: solidcore_record::Vec2::new(0.0, 0.0) }),
        Some(point.clone())
    );
    d.take_box(&h.app.session, vec![curve.clone(), point.clone()], false);
    assert_eq!(d.items(), vec![curve, point]);
}

#[test]
fn feature_inputs_reject_external_sketch_geometry() {
    let h = Harness::new();
    for (command, sel) in [
        ("solid.pipe", Sel::ExternalSketchCurve { sketch: 17, id: "line1".into() }),
        ("solid.hole", Sel::ExternalSketchPoint { sketch: 17, id: "p1".into() }),
    ] {
        let d = Dialog::for_command(&h.app, command).unwrap();
        assert!(!fits(d.inputs.first().unwrap().accept, &sel), "{command}");
    }
}

/// Boundary Fill's Tools also take planes, and still start from the body of a selected vertex.
#[test]
fn boundary_fill_tools_keep_the_body_of_a_preselected_vertex() {
    let mut h = Harness::new();
    h.app.session.selection = vec![Sel::Vertex { body: "Box".into(), point: Vec3::ZERO }];
    let d = Dialog::for_command(&h.app, "solid.boundary_fill").unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Box".into() }]);
}

#[test]
fn point_at_edge_and_plane_accepts_preselected_axes() {
    for (name, expected) in [("TestAxis", Vec3::new(20.0, 10.0, 0.0)), ("Z", Vec3::ZERO)] {
        let mut h = Harness::new();
        h.app.run("construct.axis.two_points", json!({"a":{"point":[20,10,-20]},"b":{"point":[20,10,20]},"name":"TestAxis"})).unwrap();
        let axis = Sel::Axis { name: name.into() };
        h.app.session.selection = vec![axis.clone(), Sel::Plane { name: "XY".into() }];
        let d = Dialog::for_command(&h.app, "construct.point.edge_plane").unwrap();
        assert_eq!(d.inputs[0].items, vec![axis], "{name}: the preselected axis fills Edge");
        let commands = apply_commands(&h.app, &d).unwrap();
        assert_eq!(commands[0].1["edge"], json!({"axis":name}));
        for (command, params) in commands {
            h.app.run(&command, params).unwrap();
        }
        let points = solidcraft_engine::view::construction_points(&h.app.session);
        assert_eq!(points.len(), 1);
        assert!(points[0].2.dist(expected) < 1e-9, "{name}: {:?}", points[0].2);
    }
}

#[test]
fn point_at_edge_and_plane_picks_a_construction_axis() {
    for browser in [true, false] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.tree.collapsed.insert("c0/features".into());
        h.app.run("construct.axis.two_points", json!({"a":{"point":[20,10,-20]},"b":{"point":[20,10,20]},"name":"TestAxis"})).unwrap();
        h.app.session.selection = vec![Sel::Plane { name: "XY".into() }];
        h.app.start("construct.point.edge_plane");
        h.frames(4);
        let axis = Sel::Axis { name: "TestAxis".into() };
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.candidate(&h.app.session, &Hit::Axis { name: "TestAxis".into() }), Some(axis.clone()));
        assert!(fits(d.inputs[0].accept, &Sel::Edge { body: "Body1".into(), index: 0, point: Vec3::ZERO }));
        assert!(fits(d.inputs[0].accept, &Sel::SketchCurve { id: "l1".into(), sketch: None }));
        let target = if browser { json!({"handle":"row:TestAxis"}) } else { json!({"world":[20,10,-20]}) };
        h.step(&json!({"click":target})).unwrap();
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![axis], "browser: {browser}");
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        let points = solidcraft_engine::view::construction_points(&h.app.session);
        assert_eq!(points.len(), 1);
        assert!(points[0].2.dist(Vec3::new(20.0, 10.0, 0.0)) < 1e-9, "{:?}", points[0].2);
    }
}

#[test]
fn command_dialog_remembers_values_for_the_same_command_only_in_this_session() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut pattern = Dialog::for_command(&h.app, "solid.pattern.rectangular").unwrap();
    if let Kind::PatternRect { count, spacing, count2, spacing2, .. } = &mut pattern.kind {
        *count = "7".into();
        *spacing = "12 mm".into();
        *count2 = "3".into();
        *spacing2 = "8 mm".into();
    } else {
        panic!("rectangular pattern dialog has the wrong kind");
    }
    remember(&mut h.app, &pattern);

    let recalled = Dialog::for_command(&h.app, "solid.pattern.rectangular").unwrap();
    assert!(matches!(
        &recalled.kind,
        Kind::PatternRect { count, spacing, count2, spacing2, .. }
            if count == "7" && spacing == "12 mm" && count2 == "3" && spacing2 == "8 mm"
    ));

    let fresh_app = Harness::new();
    let fresh = Dialog::for_command(&fresh_app.app, "solid.pattern.rectangular").unwrap();
    assert!(matches!(
        &fresh.kind,
        Kind::PatternRect { count, count2, .. } if count == "3" && count2 == "1"
    ));
}

#[test]
fn extrude_recall_keeps_a_from_object_offset_visible_without_reusing_its_pick() {
    for (value, shown) in [("", false), ("0 mm", true), ("5 mm", true)] {
        let mut h = Harness::new();
        h.app.preferences.remember_dialog_values = true;
        let mut extrude = Dialog::for_command(&h.app, "solid.extrude").unwrap();
        let Kind::Extrude { start, offset_start, .. } = &mut extrude.kind else { panic!("extrude") };
        *start = value.into();
        *offset_start = false;
        set_object(&mut extrude.inputs, &mut extrude.active, FROM_OBJECT, true);
        remember(&mut h.app, &extrude);

        let recalled = Dialog::for_command(&h.app, "solid.extrude").unwrap();
        assert!(!object_on(&recalled.inputs, FROM_OBJECT));
        assert!(matches!(&recalled.kind, Kind::Extrude { start, .. } if start == value));
        h.app.dialog = Some(recalled);
        h.frames(4);
        assert_eq!(scenario::handle_at("dialog:Offset").is_some(), shown, "offset {value:?}");
    }
}

#[test]
fn primitive_dialog_remembers_values_without_reusing_its_picked_placement() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut box_dialog = Dialog::for_command(&h.app, "solid.box").unwrap();
    if let Kind::Primitive { fields, operation, at, .. } = &mut box_dialog.kind {
        fields[0].1 = "42 mm".into();
        *operation = 2;
        *at = Some((Vec3::ZERO, None));
    } else {
        panic!("box dialog has the wrong kind");
    }
    remember(&mut h.app, &box_dialog);

    let recalled = Dialog::for_command(&h.app, "solid.box").unwrap();
    assert!(matches!(
        &recalled.kind,
        Kind::Primitive { fields, operation: 2, at: None, .. } if fields[0].1 == "42 mm"
    ));

    let different_command = Dialog::for_command(&h.app, "solid.cylinder").unwrap();
    assert!(matches!(
        &different_command.kind,
        Kind::Primitive { fields, operation: 0, .. } if fields[0].1 == "20 mm"
    ));
}

#[test]
fn end_cap_dialog_remembers_its_sizes() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut cap = Dialog::for_command(&h.app, "frame.end_cap").unwrap();
    let Kind::EndCap(c) = &mut cap.kind else { panic!("end cap dialog has the wrong kind") };
    c.thickness = "8 mm".into();
    c.direction = 1;
    c.use_offset = true;
    c.inset_kind = 1;
    c.offset = "2 mm".into();
    c.chamfer = "1 mm".into();
    c.unresolved = vec![2];
    remember(&mut h.app, &cap);
    let recalled = Dialog::for_command(&h.app, "frame.end_cap").unwrap();
    assert!(
        matches!(
            &recalled.kind,
            Kind::EndCap(c) if c.thickness == "8 mm" && c.direction == 1 && c.use_offset && c.inset_kind == 1
                && c.offset == "2 mm" && c.chamfer == "1 mm" && c.unresolved.is_empty()
        ),
        "{:?}",
        recalled.kind
    );
}

/// Frame Member remembers its values, but opens with Ghost Frame Members unticked and no tubes
/// picked for Remove Tube (#15705).
#[test]
fn frame_member_dialog_recalls_values_but_not_ghosting_or_picked_tubes() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut d = Dialog::for_command(&h.app, "frame.member").unwrap();
    let Kind::Frame(m) = &mut d.kind else { panic!("not the Frame Member dialog") };
    m.mirror = true;
    m.ghost = true;
    m.picked_tubes = vec!["pair1".into()];
    remember(&mut h.app, &d);
    let recalled = Dialog::for_command(&h.app, "frame.member").unwrap();
    let Kind::Frame(m) = &recalled.kind else { panic!("not the Frame Member dialog") };
    assert!(m.mirror, "values are remembered");
    assert!(!m.ghost, "unticked whenever the dialog opens");
    assert!(m.picked_tubes.is_empty());
}

#[test]
fn command_dialog_remembers_option_fields() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut shell = Dialog::for_command(&h.app, "solid.shell").unwrap();
    shell.extra.insert("direction".into(), json!("outside"));
    shell.extra.insert("tangent_chain".into(), json!(false));
    remember(&mut h.app, &shell);

    let recalled = Dialog::for_command(&h.app, "solid.shell").unwrap();
    assert_eq!(recalled.extra.get("direction"), Some(&json!("outside")));
    assert_eq!(recalled.extra.get("tangent_chain"), Some(&json!(false)));
}

#[test]
fn thread_recall_drops_a_remembered_suggestion_but_keeps_an_authored_size() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut thread = Dialog::for_command(&h.app, "solid.thread").unwrap();
    if let Kind::Thread { designation, suggested, .. } = &mut thread.kind {
        *designation = "M6x1".into();
        *suggested = Some("M6x1".into());
    }
    remember(&mut h.app, &thread);
    let recalled = Dialog::for_command(&h.app, "solid.thread").unwrap();
    assert!(matches!(&recalled.kind, Kind::Thread { designation, .. } if designation.is_empty()), "{:?}", recalled.kind);

    if let Kind::Thread { designation, .. } = &mut thread.kind {
        *designation = "M8x1.25".into();
    }
    remember(&mut h.app, &thread);
    let recalled = Dialog::for_command(&h.app, "solid.thread").unwrap();
    assert!(matches!(&recalled.kind, Kind::Thread { designation, .. } if designation == "M8x1.25"));
}

#[test]
fn move_and_hole_recall_rebuild_the_inputs_of_the_remembered_mode() {
    let mut h = Harness::new();
    h.app.preferences.remember_dialog_values = true;
    let mut rotate = Dialog::for_command(&h.app, "solid.move").unwrap();
    rotate.kind = crate::dialogs_move::kind(crate::dialogs_move::ROTATE, &h.app.session.doc);
    rotate.inputs = crate::dialogs_move::inputs(crate::dialogs_move::ROTATE);
    remember(&mut h.app, &rotate);
    let recalled = Dialog::for_command(&h.app, "solid.move").unwrap();
    let labels: Vec<&str> = recalled.inputs.iter().map(|i| i.label).collect();
    assert!(labels.contains(&"Axis") && !labels.contains(&"Pivot"), "{labels:?}");

    let mut hole = Dialog::for_command(&h.app, "solid.hole").unwrap();
    if let Kind::Hole { opts, .. } = &mut hole.kind {
        opts.multiple = true;
    }
    remember(&mut h.app, &hole);
    let recalled = Dialog::for_command(&h.app, "solid.hole").unwrap();
    assert_eq!(recalled.inputs.len(), 1);
    assert_eq!(recalled.inputs[0].label, "Points");
}

#[test]
fn command_dialog_value_memory_is_off_by_default_and_resets_each_time() {
    let mut h = Harness::new();
    assert!(!h.app.preferences.remember_dialog_values);
    let mut pattern = Dialog::for_command(&h.app, "solid.pattern.rectangular").unwrap();
    if let Kind::PatternRect { count, .. } = &mut pattern.kind {
        *count = "7".into();
    } else {
        panic!("rectangular pattern dialog has the wrong kind");
    }
    remember(&mut h.app, &pattern);

    let recalled = Dialog::for_command(&h.app, "solid.pattern.rectangular").unwrap();
    assert!(matches!(&recalled.kind, Kind::PatternRect { count, .. } if count == "3"));
}

#[test]
fn new_fillet_opens_at_the_last_applied_radius_without_remember_dialog_values() {
    let mut h = Harness::new();
    assert!(!h.app.preferences.remember_dialog_values);
    let first = Dialog::for_command(&h.app, "solid.fillet").unwrap();
    let Kind::Fillet { radius, chamfer: false, .. } = &first.kind else { panic!("a fillet dialog") };
    let shipped = radius.clone();
    h.app.last_fillet_radius = "5 mm".into();

    let next = Dialog::for_command(&h.app, "solid.fillet").unwrap();
    assert!(matches!(&next.kind, Kind::Fillet { radius, chamfer: false, .. } if radius == "5 mm"));
    assert_ne!(shipped, "5 mm");
}

fn fixture() -> Harness {
    let mut h = Harness::new();
    for (name, x) in [("A", 0), ("B", 30), ("C", 60), ("D", 90)] {
        h.app.run("component.activate", json!({"component":"root"})).unwrap();
        h.app.run("component.create", json!({"name":name})).unwrap();
        h.app.run("solid.cylinder", json!({"base":[x,0,0],"radius":10,"height":5})).unwrap();
    }
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    h.app.run("occurrence.ground", json!({"occurrence":"A:1","grounded":true})).unwrap();
    for (name, kind) in [("B", "revolute"), ("C", "cylindrical"), ("D", "slider")] {
        h.app.run("joint.as_built", json!({"name":name,"type":kind,"a":"A","b":name})).unwrap();
    }
    for (name, offset) in [("Base", 0), ("Top", 10)] {
        h.app.run("sketch.create", json!({"plane":"XY","offset":offset,"name":name})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[12,0],"p1":[16,4]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.app.run("sketch.create", json!({"plane":"XZ","name":"Path"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20]]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h
}

fn representative(h: &Harness, command: &str) -> Dialog {
    let mut d = Dialog::for_command(&h.app, command).unwrap();
    let s = &h.app.session;
    let profiles: Vec<_> = s.world_state().unwrap().sketches.iter().take(2).map(|sk| Sel::Profile { sketch: sk.feature, index: 0 }).collect();
    let path = s.world_state().unwrap().sketches.last().unwrap().sketch.curves.first().unwrap().id.clone();
    for (index, input) in d.inputs.iter_mut().enumerate() {
        if command == "joint.gear" {
            continue;
        }
        if command == "solid.coil" {
            continue; // an empty Profile input keeps the primitive section, which this form checks
        }
        if command == "solid.cylinder" {
            continue; // its face and reference edges place it (cylinder reference tests); this form checks its fields
        }
        if matches!(input.label, "Rail" | "Second Rail" | "Pass through" | "Direction" | "Roller (optional)" | "Tip (optional)") {
            continue;
        }
        let name = if index == 1 && command != "surface.ruled" { "Body2" } else { "Body1" };
        let point = if index == 1 && command != "surface.ruled" { Vec3::new(30.0, 0.0, 5.0) } else { Vec3::new(0.0, 0.0, 5.0) };
        input.items = if command == "construct.point.along_path" {
            vec![edge_sel(s, Vec3::new(10.0, 0.0, 5.0)).unwrap()]
        } else if input.accept & PROFILES != 0 {
            if matches!(d.kind, Kind::Loft { .. } | Kind::Surface(crate::dialogs_surface::Surf::Loft { .. })) {
                profiles.clone()
            } else {
                vec![profiles[0].clone()]
            }
        } else if input.accept & BODIES != 0 {
            if matches!(command, "joint.as_built" | "joint.rigid_group") {
                vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }]
            } else {
                vec![Sel::Body { name: name.into() }]
            }
        } else if input.accept == crate::dialogs_assembly::SNAPS {
            // A joint origin takes datums too now (PLANES, AXES), but a joint's two sides must
            // sit on different components: face snaps keep each side on its own component.
            vec![face_sel(s, point).unwrap()]
        } else if input.accept & PLANES != 0 {
            vec![Sel::Plane { name: if index == 1 { "XZ" } else { "XY" }.into() }]
        } else if input.accept & crate::selection::SKETCH_LINES != 0 {
            vec![Sel::ExternalSketchCurve { sketch: crate::open_meshing::session_world(s).sketches.last().unwrap().feature, id: path.clone() }]
        } else if input.accept & CURVES != 0 || command == "sheet.fold" && index == 0 {
            vec![Sel::SketchCurve { id: path.clone(), sketch: None }]
        } else if input.accept & AXES != 0 {
            // The Revolve fixture's profile is in XY; its preview needs an axis in that plane.
            let axis = if command == "solid.revolve" { "Y" } else { "Z" };
            vec![Sel::Axis { name: axis.into() }]
        } else if input.accept & (FACES | PLANAR_FACES) != 0 {
            vec![face_sel(s, point).unwrap()]
        } else if command == "surface.match" && index == 2 {
            vec![edge_sel(s, Vec3::new(40.0, 0.0, 5.0)).unwrap()]
        } else if input.accept & EDGES != 0 {
            vec![edge_sel(s, Vec3::new(10.0, 0.0, 5.0)).unwrap()]
        } else {
            Vec::new()
        };
    }
    if let Kind::PathPlane { sketch, .. } = &mut d.kind {
        *sketch = Some(s.world_state().unwrap().sketches.last().unwrap().feature);
    }
    if let Kind::Mirror = &d.kind {
        d.extra.insert("combine".into(), json!(false));
    }
    if let Kind::BoundaryFill { cells, .. } = &mut d.kind {
        cells.push((Vec3::new(0.0, 0.0, 2.0), "Cell".into(), true));
    }
    if let Kind::Thread { designation, .. } = &mut d.kind {
        *designation = "M20x2.5".into();
    }
    if let Kind::Assembly(crate::dialogs_assembly::Asm::Cam(f)) = &mut d.kind {
        f.b = s.doc.assembly.joints[2].id;
    }
    d
}

fn check_form(command: &str, labels: &[&str]) {
    check_form_with(command, labels, &[], |_| {});
}

/// [`check_form`] on the representative dialog after `set` changed it; the `absent` rows are not drawn.
fn check_form_with(command: &str, labels: &[&str], absent: &[&str], set: impl FnOnce(&mut Dialog)) {
    let mut h = fixture();
    let mut d = representative(&h, command);
    set(&mut d);
    assert_eq!(super::command(&d.kind), Some(command));
    let spec = solidcraft_engine::find_command(command).unwrap();
    assert!(!spec.icon.is_empty());
    let expected = apply_commands(&h.app, &d).unwrap_or_else(|e| panic!("{command}: representative parameters: {e}"));
    // Rendering must never discard an optional/default value or add a new parameter.
    for _ in 0..3 {
        scenario::clear_handles();
        let mut out = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 1600.0))), ..Default::default() },
            |ui| {
                ui.set_width(264.0);
                ui_kit::scope(ui, |ui| {
                    ui_kit::header(ui, spec.icon, tr(spec.label), || {});
                    rows(&mut h.app, &h.ctx, &mut d, ui);
                    ui_kit::footer(ui, "OK", "Cancel", true, ui_kit::Footer::default());
                });
            },
        );
        out.textures_delta.clear();
        h.output = out.platform_output;
    }
    let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
    for (i, input) in d.inputs.iter().enumerate() {
        // Cell Grid's cell profile shows with the Sketch shape only.
        if matches!(&d.kind, Kind::CellGrid(c) if !crate::dialogs_cell_grid::shown_picks(c).contains(&i)) {
            assert!(scenario::rect_of(&format!("dialog:field:{}", input.label)).is_none(), "{command}: hidden selection field {}", input.label);
            continue;
        }
        assert!(scenario::rect_of(&format!("dialog:field:{}", input.label)).is_some(), "{command}: selection field {}", input.label);
        assert!(
            nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(solidcraft_ui_text::tr_label(input.label))),
            "{command}: picker {} is a button",
            input.label
        );
    }
    for label in labels {
        if let Some(role) = control_role(command, label) {
            if *label == "Animate" || *label == "Radius" && command == "joint.rack_pinion" {
                let control = scenario::rect_of(&format!("dialog:control:{label}")).unwrap();
                assert!(
                    nodes.iter().any(|(_, n)| n.role() == role
                        && n.bounds().is_some_and(|b| control.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))),
                    "{command}: {label} uses {role:?}"
                );
                continue;
            }
            assert!(nodes.iter().any(|(_, n)| n.role() == role && n.label() == Some(tr(label))), "{command}: {label} uses {role:?}");
        }
    }
    let checks: &[&str] = match command {
        "solid.shell" => &["Tangent Chain"],
        "solid.rib" | "solid.web" | "solid.align" | "sheet.flange" | "sheet.hem" | "sheet.fold" | "joint.create" => &["Flip"],
        "inspect.section" => &["Flip", "Hatch"],
        "solid.move" => &["Create Copy"],
        "solid.combine" => &["Keep Tools", "New Component"],
        "solid.thread" => &["Modeled", "Automatic size"],
        "solid.boundary_fill" => &["Remove Tools"],
        "surface.offset" => &["Tangent chain"],
        "surface.fair" => &["Keep edges"],
        "surface.trim" => &["Extend to meet", "Stitch the kept pieces"],
        "surface.ruled" => &["Flip"],
        "surface.untrim" => &["Restore outer boundary", "In place (surfaces)"],
        "component.create" => &["Activate"],
        "joint.gear" => &["Reverse", "Internal gear"],
        "joint.rack_pinion" | "joint.screw" => &["Reverse"],
        "joint.cam" => &["Other side"],
        "frame.member" => &["All Lines", "Mirror"],
        "solid.pattern.path" => &["Flip direction"],
        "construct.plane.at_angle" => &["Through the axis", "Use axis direction only"],
        _ => &[],
    };
    for label in checks {
        assert!(nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::CheckBox && n.label() == Some(*label)), "{command}: option {label}");
    }
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0), "{command}");
    assert_eq!(apply_commands(&h.app, &d).unwrap(), expected, "{command}: drawing is lossless");
    let mut label_width: Option<f32> = None;
    // Cell Grid's cell profile has a row only with the Sketch shape.
    let shown = |i: usize| !matches!(&d.kind, Kind::CellGrid(c) if !crate::dialogs_cell_grid::shown_picks(c).contains(&i));
    let rows = d.inputs.iter().enumerate().filter(|(i, _)| shown(*i)).map(|(_, i)| i.label);
    for label in rows.chain(labels.iter().copied()) {
        let row = scenario::rect_of(&format!("dialog:row:{label}")).unwrap_or_else(|| panic!("{command}: missing dense row {label}"));
        let label_rect = scenario::rect_of(&format!("dialog:label:{label}")).unwrap();
        let control = scenario::rect_of(&format!("dialog:control:{label}")).unwrap();
        assert_eq!(row.height(), Tokens::get().control_height, "{command}: {label}");
        // The label column is the design's width, widened to the longest label, equally for every row.
        let t = Tokens::get();
        assert!(label_rect.width() >= t.form_label_width && label_rect.width() <= t.form_label_width * 1.6, "{command}: {label}");
        if let Some(first) = label_width {
            assert_eq!(label_rect.width(), first, "{command}: {label} lines up with the other rows");
        }
        label_width = Some(label_rect.width());
        assert!(row.contains_rect(control), "{command}: {label}");
        assert!(scenario::handle_at(&format!("dialog:{label}")).is_some() || scenario::rect_of(&format!("dialog:control:{label}")).is_some());
    }
    for label in absent {
        assert!(scenario::rect_of(&format!("dialog:row:{label}")).is_none(), "{command}: {label} is not drawn");
    }
    assert!(scenario::rect_of("dialog:footer").is_some());
    assert!(scenario::handle_at("dialog:ok").is_some());
    assert!(scenario::handle_at("dialog:cancel").is_some());
    let mut out = h.ctx.run_ui(egui::RawInput::default(), |ui| {
        ui_kit::scope(ui, |ui| {
            assert_eq!(ui_kit::footer(ui, "OK", "Cancel", false, ui_kit::Footer::default()), ui_kit::Action::None);
        });
    });
    out.textures_delta.clear();
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "{command}");
}

fn control_role(command: &str, label: &str) -> Option<egui::accesskit::Role> {
    use egui::accesskit::Role;
    Some(match label {
        "Type" if matches!(command, "sheet.flange" | "sheet.hem" | "solid.revolve" | "solid.emboss" | "solid.scale") => return None,
        "Direction" | "pattern.start_point" => return None, // segmented buttons, labelled by each choice
        "Target" if command == "mesh.reduce" => return None, // segmented target modes
        "ui.fair_strength" | "ui.fair_iterations" => return None,
        "Size" if matches!(command, "solid.cylinder" | "solid.sphere") => return None, // Diameter | Radius
        "Continuity" | "Corners" | "Cells" | "Profile" => return None,
        "Operation" if command == "solid.mirror" => return None,
        "Operation" | "Type" | "Orientation" | "Move Type" | "Object Type" | "Rule" | "Rim Edge" | "Catch Direction" | "Shape" | "Holes" | "Size"
        | "Bend Line Position" | "From Snap" | "To Snap" | "Joint" | "Joint 1" | "Joint 2" | "Gear 1" | "Gear 2" | "Pinion" | "Rack" | "Screw"
        | "Nut" | "Cam" | "Follower" | "Anchor" | "Bolt Pattern" => Role::ComboBox,
        "Inset By" | "Extent Type" => Role::ComboBox,
        "Thickness Placement" if command == "frame.gusset" => return None, // icon buttons, checked by the gusset interaction test
        "Outline" => Role::ComboBox,
        "Style" if command == "solid.dogbone" => Role::ComboBox,
        "Radius" if command == "joint.rack_pinion" => Role::Label,
        "Pitch" | "Form" | "Hand" | "Starts" | "Extent" | "Start" | "End" if command == "solid.thread" => Role::ComboBox,
        "Automatic size" | "Reverse" | "Other side" | "All Lines" | "Mirror" => Role::CheckBox,
        "Flip alternate cells" if command == "solid.cell_grid" => Role::CheckBox,
        "Offset" if command == "frame.end_cap" => Role::CheckBox,
        "Through all" if command == "solid.cell_grid" => Role::CheckBox,
        "Mode" | "Clip" | "Keep-out clip" | "Dot grid" if command == "solid.cell_grid" => Role::ComboBox,
        "Animate" => Role::Button,
        "Ratio" if command.starts_with("joint.") && command != "joint.motion_link" => return None,
        _ => Role::TextInput,
    })
}

macro_rules! form_case {
    ($name:ident, $command:literal, [$($label:literal),*]) => {
        #[test] fn $name() { check_form($command, &[$($label),*]); }
    };
}
form_case!(revolve, "solid.revolve", ["Angle", "Direction", "Type"]);
form_case!(shell, "solid.shell", ["Thickness", "Direction"]);
form_case!(draft, "solid.draft", ["Angle"]);
form_case!(dogbone, "solid.dogbone", ["Bit diameter", "Style", "Largest angle"]);
form_case!(mirror, "solid.mirror", ["Operation"]);
form_case!(end_cap, "frame.end_cap", ["Thickness", "Direction", "Offset", "Corner Chamfer"]);
form_case!(gusset, "frame.gusset", ["Outline", "First Leg", "Second Leg", "Thickness", "Corner Relief", "Joint Offset", "Thickness Placement"]);

/// The Motion Study's and Motion Trace's hints read in the user's language: the dialogs drew them
/// as they got them, in English.
#[test]
fn motion_dialog_hints_are_translated() {
    fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
            _ => {}
        }
    }
    let mut h = fixture();
    h.app.preferences.language = "ja".into();
    let _japanese = solidcraft_ui_text::scope("ja");
    for (command, english) in [("motion.study", "add a key for a joint first"), ("motion.trace", "select a point on a component")] {
        let mut d = Dialog::for_command(&h.app, command).unwrap();
        let mut painted = Vec::new();
        for _ in 0..2 {
            let mut out = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 1600.0))), ..Default::default() },
                |ui| {
                    ui.set_width(264.0);
                    ui_kit::scope(ui, |ui| {
                        rows(&mut h.app, &h.ctx, &mut d, ui);
                    });
                },
            );
            out.textures_delta.clear();
            painted.clear();
            out.shapes.iter().for_each(|shape| texts(&shape.shape, &mut painted));
        }
        assert!(!painted.iter().any(|p| p == english), "{command} paints {english:?} in Japanese");
        assert!(painted.iter().any(|p| p == solidcraft_ui_text::tr_label(english)), "{command} paints {painted:?}");
    }
}

fn mirror_preselection() -> (Harness, Vec<Sel>) {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Sketch1"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[6,6]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":"Sketch1","distance":5})).unwrap();
    let id = h.app.session.doc.features.iter().find(|f| f.name == "Extrude1").unwrap().id;
    let picked = vec![Sel::Feature { id }];
    h.app.session.selection = picked.clone();
    h.app.start("solid.mirror");
    h.frames(4);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    (h, picked)
}

fn mirror_object_type(h: &mut Harness, choice: &str) {
    h.step(&json!({"click":{"handle":"dialog:Object Type"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:Object Type:{choice}")}})).unwrap();
}

#[test]
fn mirror_object_type_switch_back_restores_preselected_feature() {
    let (mut h, picked) = mirror_preselection();
    mirror_object_type(&mut h, "Bodies");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    mirror_object_type(&mut h, "Features");
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(d.inputs[0].items, picked);
    d.inputs[1].items = vec![Sel::Plane { name: "YZ".into() }];
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["features"], json!(["Extrude1"]));
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    assert_eq!(crate::open_meshing::session_world(&h.app.session).bodies.len(), 2);
}

#[test]
fn mirror_timeline_click_keeps_picked_feature() {
    for dock in ["bottom", "left", "under_browser"] {
        mirror_history_click_keeps_picked_feature(dock);
    }
}

fn mirror_history_click_keeps_picked_feature(dock: &str) {
    let (mut h, picked) = mirror_preselection();
    h.app.run("view.history_layout", json!({"dock": dock})).unwrap();
    h.frames(4);
    // Mirror moves on to the plane; the objects input takes timeline clicks once it is active.
    h.step(&json!({"click":{"handle":"dialog:Features"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    h.step(&json!({"click":{"handle":"timeline:Extrude1"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    // Wait out the double-click time: a double click opens the feature to edit.
    h.frames(30);
    h.step(&json!({"click":{"handle":"timeline:Extrude1"},"ctrl":true})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(d.kind, Kind::Mirror));
    assert!(d.inputs[0].items.is_empty());
}

#[test]
fn rectangular_pattern_takes_features_from_every_history_layout() {
    for dock in ["bottom", "left", "under_browser"] {
        let mut h = Harness::new();
        h.app.run("view.history_layout", json!({"dock": dock})).unwrap();
        h.app.run("solid.box", json!({"length": 80, "width": 60, "height": 20})).unwrap();
        h.app.run("solid.cylinder", json!({"base": [20, 20, 20], "radius": 5, "height": 10, "operation": "join"})).unwrap();
        let id = h.app.session.doc.find_feature("Cylinder1").unwrap().id;
        h.app.run("select.clear", json!({})).unwrap();
        h.app.start("solid.pattern.rectangular");
        h.frames(4);
        mirror_object_type(&mut h, "Features");
        h.step(&json!({"click": {"handle": "timeline:Cylinder1"}})).unwrap();

        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![Sel::Feature { id }], "{dock}: the dialog receives the History feature");
        assert_eq!(d.active, 0, "{dock}: Features stays active so more features can be picked");
        assert!(h.app.session.selection.is_empty(), "{dock}: a dialog pick does not replace the ordinary selection");
        h.app.dialog.as_mut().unwrap().inputs[1].items = vec![Sel::Axis { name: "X".into() }];
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands[0].1["features"], json!(["Cylinder1"]), "{dock}: the command receives the chosen feature");
    }
}

/// #13170: with the History in the Browser, a click on one of its feature rows goes to the open
/// Pattern or Mirror dialog like a click in the bottom or left History.
#[test]
fn a_dialog_takes_features_from_the_browser_history_rows() {
    let mut h = Harness::new();
    h.app.preferences.history_in_browser = true;
    h.app.run("solid.box", json!({"length": 80, "width": 60, "height": 20})).unwrap();
    h.app.run("solid.cylinder", json!({"base": [20, 20, 20], "radius": 5, "height": 10, "operation": "join"})).unwrap();
    let id = h.app.session.doc.find_feature("Cylinder1").unwrap().id;
    h.app.run("select.clear", json!({})).unwrap();
    h.app.start("solid.pattern.rectangular");
    h.frames(4);
    mirror_object_type(&mut h, "Features");
    h.frames(3);
    h.step(&json!({"click": {"handle": format!("row:browser-feature:c0:{id}")}})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Feature { id }], "the dialog receives the Browser History feature");
    assert!(h.app.session.selection.is_empty(), "a dialog pick does not replace the ordinary selection");
}

form_case!(loft, "solid.loft", ["Operation"]);
form_case!(sweep, "solid.sweep", ["Orientation"]);
form_case!(offset_plane, "construct.plane.offset", ["Distance"]);
form_case!(midplane, "construct.plane.midplane", []);
form_case!(mesh_convert, "mesh.convert", ["Tolerance"]);
form_case!(mesh_reduce, "mesh.reduce", ["Target", "Retained percentage"]);

#[test]
fn mesh_reduce_builds_exactly_one_valid_target() {
    let h = Harness::new();
    let mut d = Dialog::for_command(&h.app, "mesh.reduce").unwrap();
    d.inputs[0].items = vec![Sel::Body { name: "Mesh".into() }];
    assert_eq!(apply_commands(&h.app, &d).unwrap()[0].1, json!({"body":"Mesh","percentage":50.0}));
    if let Kind::MeshReduce { mode, count, percentage } = &mut d.kind {
        *percentage = "100".into();
        *count = "42".into();
        assert_eq!(*mode, 0);
    }
    assert!(apply_commands(&h.app, &d).is_err());
    if let Kind::MeshReduce { mode, .. } = &mut d.kind {
        *mode = 1;
    }
    assert_eq!(apply_commands(&h.app, &d).unwrap()[0].1, json!({"body":"Mesh","triangle_count":42}));
    if let Kind::MeshReduce { count, .. } = &mut d.kind {
        *count = "0".into();
    }
    assert!(apply_commands(&h.app, &d).is_err());
}
form_case!(axis_two_planes, "construct.axis.two_planes", []);
form_case!(point_along_path, "construct.point.along_path", ["Position (0 to 1)"]);
form_case!(angle_plane, "construct.plane.at_angle", ["Angle"]);
form_case!(path_plane, "construct.plane.along_path", ["Position (0 to 1)"]);
form_case!(split, "solid.split_body", []);
form_case!(scale, "solid.scale", ["Type", "Scale factor"]);

/// Scale per axis (#10782): the dialog's X, Y and Z rows build `factors`; a Scale made with
/// `factors` opens per axis with its three values and keeps them on OK, and switching it back to
/// Uniform scales the same in every direction again.
#[test]
fn scale_takes_a_factor_per_axis() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]})).unwrap();
    let body = h.app.session.model.state().bodies[0].name.clone();
    let mut d = Dialog::for_command(&h.app, "solid.scale").unwrap();
    d.inputs[0].items = vec![Sel::Body { name: body.clone() }];
    let Kind::Scale { factor, per_axis, factor_y, factor_z } = &mut d.kind else { panic!("not the Scale dialog") };
    assert!(!*per_axis, "Scale opens uniform");
    *per_axis = true;
    (*factor, *factor_y, *factor_z) = ("2".into(), "1".into(), "0.5".into());
    for (command, params) in apply_commands(&h.app, &d).unwrap() {
        assert!(params.get("factor").is_none(), "{params}");
        h.app.run(&command, params).unwrap();
    }
    let scale = h.app.session.doc.features.last().unwrap().id;
    let kind = |h: &Harness| serde_json::to_value(&h.app.session.doc.feature(scale).unwrap().kind).unwrap();
    assert_eq!(kind(&h)["factors"], json!(["2", "1", "0.5"]));
    let size = |h: &Harness| h.app.session.model.state().bodies[0].mesh().bounds().size();
    assert!((size(&h) - Vec3::new(20.0, 10.0, 5.0)).len() < 1e-6, "{:?}", size(&h));
    let mut d = crate::dialogs::for_feature(&h.app, scale, None).unwrap();
    assert!(
        matches!(&d.kind, Kind::Scale { factor, per_axis: true, factor_y, factor_z } if [factor, factor_y, factor_z] == ["2", "1", "0.5"]),
        "{:?}",
        d.kind
    );
    for (command, params) in apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    assert_eq!(kind(&h)["factors"], json!(["2", "1", "0.5"]), "OK keeps the three factors");
    if let Kind::Scale { per_axis, factor, .. } = &mut d.kind {
        (*per_axis, *factor) = (false, "3".into());
    }
    for (command, params) in apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    assert!(kind(&h).get("factors").is_none(), "{}", kind(&h));
    assert!((size(&h) - Vec3::new(30.0, 30.0, 30.0)).len() < 1e-6, "{:?}", size(&h));
}

/// Scale's base point: with none picked, a new Scale grows the bodies about their centre, not the
/// world origin; a picked vertex is the point that stays put; reopening the Scale keeps its point.
#[test]
fn scale_about_a_base_point_or_the_bodies_centre() {
    // `on_box`: the base point is one of the box's vertices; otherwise it is the design origin,
    // which the viewport hands over as a vertex of no body.
    let scaled = |base: Option<Vec3>, on_box: bool| -> (Harness, u64) {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [10, 0, 0]})).unwrap();
        let body = h.app.session.model.state().bodies[0].name.clone();
        let mut d = Dialog::for_command(&h.app, "solid.scale").unwrap();
        d.inputs[0].items = vec![Sel::Body { name: body.clone() }];
        if let Some(point) = base {
            d.inputs[1].items = vec![Sel::Vertex { body: if on_box { body } else { String::new() }, point }];
        }
        let Kind::Scale { factor, .. } = &mut d.kind else { panic!("not the Scale dialog") };
        *factor = "2".into();
        for (command, params) in apply_commands(&h.app, &d).unwrap() {
            h.app.run(&command, params).unwrap();
        }
        let id = h.app.session.doc.features.last().unwrap().id;
        (h, id)
    };
    let bounds = |h: &Harness| h.app.session.model.state().bodies[0].mesh().bounds();
    let near = |a: Vec3, b: Vec3| (a - b).len() < 1e-6;

    // The box spans (10,0,0)..(20,10,10), centre (15,5,5): doubled about it.
    let (h, _) = scaled(None, true);
    let b = bounds(&h);
    assert!(near(b.min, Vec3::new(5.0, -5.0, -5.0)) && near(b.max, Vec3::new(25.0, 15.0, 15.0)), "{b:?}");

    // About its corner (10,0,0): that corner stays where it is.
    let (h, id) = scaled(Some(Vec3::new(10.0, 0.0, 0.0)), true);
    let b = bounds(&h);
    assert!(near(b.min, Vec3::new(10.0, 0.0, 0.0)) && near(b.max, Vec3::new(30.0, 20.0, 20.0)), "{b:?}");

    // Reopened and applied again, the Scale keeps the corner, as the vertex it was.
    let d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    let (_, params) = apply_commands(&h.app, &d).unwrap().into_iter().next().unwrap();
    assert_eq!(params["params"]["about"]["type"], json!("vertex"), "{params}");
    assert!(params["params"].get("origin").is_none_or(serde_json::Value::is_null), "{params}");

    // About the design origin: the box doubles and moves away from it.
    let (h, _) = scaled(Some(Vec3::ZERO), false);
    let b = bounds(&h);
    assert!(near(b.min, Vec3::new(20.0, 0.0, 0.0)) && near(b.max, Vec3::new(40.0, 20.0, 20.0)), "{b:?}");

    // About the box's top face: the centre of that face, (15,5,10), stays put.
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [10, 0, 0]})).unwrap();
    let st = h.app.session.model.state();
    let body = st.bodies[0].name.clone();
    let b = &st.bodies[0].body;
    let top = b.faces((b.size() * 2e-3).max(1e-3)).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.scale").unwrap();
    d.inputs[0].items = vec![Sel::Body { name: body.clone() }];
    d.inputs[1].items = vec![Sel::Face { body, index: top.index, point: Vec3::new(12.0, 3.0, 10.0) }];
    let Kind::Scale { factor, .. } = &mut d.kind else { panic!("not the Scale dialog") };
    *factor = "2".into();
    for (command, params) in apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let b = bounds(&h);
    assert!(near(b.min, Vec3::new(5.0, -5.0, -10.0)) && near(b.max, Vec3::new(25.0, 15.0, 10.0)), "{b:?}");

    // A shaft's side face stands for the centre of its nearer round end: picked near the bottom
    // of a cylinder standing on (10,0,0), that end's centre is the base point.
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"base": [10, 0, 0], "radius": 5, "height": 10})).unwrap();
    let st = h.app.session.model.state();
    let body = st.bodies[0].name.clone();
    let b = &st.bodies[0].body;
    let side = b.faces((b.size() * 2e-3).max(1e-3)).unwrap().into_iter().find(|f| f.plane_normal.is_none()).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.scale").unwrap();
    d.inputs[0].items = vec![Sel::Body { name: body.clone() }];
    d.inputs[1].items = vec![Sel::Face { body, index: side.index, point: Vec3::new(15.0, 0.0, 2.0) }];
    let Kind::Scale { factor, .. } = &mut d.kind else { panic!("not the Scale dialog") };
    *factor = "2".into();
    for (command, params) in apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let b = bounds(&h);
    assert!((b.min.z - 0.0).abs() < 1e-6 && (b.max.z - 20.0).abs() < 1e-6, "the bottom end stays: {b:?}");
}

/// What a Scale grows its bodies about follows the features before it: the bodies' centre and a
/// picked vertex are found again when an earlier parameter changes, not kept as old coordinates.
#[test]
fn scale_about_follows_the_features_before_it() {
    let bounds = |h: &Harness| h.app.session.model.state().bodies[0].mesh().bounds();
    let near = |a: Vec3, b: Vec3| (a - b).len() < 1e-6;
    let scaled = |far_corner: bool| -> Harness {
        let mut h = Harness::new();
        h.app.run("parameters.add", json!({"name": "len", "expression": "10 mm"})).unwrap();
        h.app.run("solid.box", json!({"length": "len", "width": 10, "height": 10, "corner": [10, 0, 0]})).unwrap();
        let body = h.app.session.model.state().bodies[0].name.clone();
        let mut d = Dialog::for_command(&h.app, "solid.scale").unwrap();
        d.inputs[0].items = vec![Sel::Body { name: body.clone() }];
        if far_corner {
            d.inputs[1].items = vec![Sel::Vertex { body, point: Vec3::new(20.0, 10.0, 10.0) }];
        }
        let Kind::Scale { factor, .. } = &mut d.kind else { panic!("not the Scale dialog") };
        *factor = "2".into();
        for (command, params) in apply_commands(&h.app, &d).unwrap() {
            h.app.run(&command, params).unwrap();
        }
        h.app.run("parameters.change", json!({"name": "len", "expression": "20 mm"})).unwrap();
        h
    };
    // The box is now (10,0,0)..(30,10,10), centre (20,5,5): doubled about that, not the old (15,5,5).
    let b = bounds(&scaled(false));
    assert!(near(b.min, Vec3::new(0.0, -5.0, -5.0)) && near(b.max, Vec3::new(40.0, 15.0, 15.0)), "{b:?}");
    // The far corner moved to (30,10,10) and stays put there.
    let b = bounds(&scaled(true));
    assert!(near(b.min, Vec3::new(-10.0, -10.0, -10.0)) && near(b.max, Vec3::new(30.0, 10.0, 10.0)), "{b:?}");
}
form_case!(offset_faces, "solid.offset_face", ["Distance"]);
form_case!(section, "inspect.section", ["Distance"]);
form_case!(move_copy, "solid.move", ["Move Type", "X Distance", "Y Distance", "Z Distance", "X Angle", "Y Angle", "Z Angle"]);
form_case!(rib, "solid.rib", ["Direction", "Thickness", "Depth"]);

/// Extend ends is opt-in (#13019): a new Rib or Web starts with it off, so it builds as before
/// unless it is ticked.
#[test]
fn a_new_rib_starts_with_extend_ends_off() {
    let h = Harness::new();
    for command in ["solid.rib", "solid.web"] {
        let d = Dialog::for_command(&h.app, command).unwrap();
        assert!(matches!(&d.kind, Kind::Rib { extend: false, .. }), "{command}");
    }
}
form_case!(web, "solid.web", ["Direction", "Thickness", "Depth"]);
form_case!(emboss, "solid.emboss", ["Depth", "Type", "Offset X", "Offset Y", "Angle"]);
form_case!(
    cell_grid,
    "solid.cell_grid",
    ["Mode", "Shape", "Cell size", "Web", "Through all", "Depth", "Direction", "Clip", "Keep-out clip", "Inset rows", "Angle"]
);

/// The rows that come and go with the choices: Sides for polygon cells, Dot grid for circle and
/// polygon cells, Inset rows with snapped cells, Margin with whole cells at the field and the
/// keep-outs only.
#[test]
fn cell_grid_form_shows_the_rows_of_the_chosen_shape_and_clip() {
    // Indices into the dialog's shapes (hexagon, square, triangle, circle, polygon), clips (whole,
    // snap, free) and keep-out clips (same as clip, whole, snap, free).
    let cell = |shape: usize, clip: usize, keepout_clip: usize| {
        move |d: &mut Dialog| {
            if let Kind::CellGrid(c) = &mut d.kind {
                (c.shape, c.clip, c.keepout_clip) = (shape, clip, keepout_clip);
            }
        }
    };
    let always = ["Mode", "Shape", "Cell size", "Web", "Through all", "Depth", "Direction", "Clip", "Keep-out clip", "Angle"];
    check_form_with("solid.cell_grid", &[&always[..], &["Sides", "Dot grid", "Margin"]].concat(), &["Inset rows"], cell(4, 0, 0));
    check_form_with("solid.cell_grid", &[&always[..], &["Margin"]].concat(), &["Inset rows"], cell(0, 0, 1));
    check_form_with("solid.cell_grid", &always, &["Margin", "Inset rows"], cell(0, 0, 2));
    check_form_with("solid.cell_grid", &[&always[..], &["Dot grid", "Inset rows"]].concat(), &["Sides", "Margin"], cell(3, 1, 0));
    check_form_with("solid.cell_grid", &always, &["Sides", "Dot grid", "Margin", "Inset rows"], cell(0, 2, 3));
    // A sketched cell: its layout instead of the size, no Inset rows (no snap), a cell sketch pick.
    let layout = ["Pitch X", "Pitch Y", "Stagger", "Cell angle", "Flip alternate cells"];
    let sketched: Vec<&str> = always.iter().copied().filter(|l| *l != "Cell size").chain(layout).collect();
    check_form_with("solid.cell_grid", &[&sketched[..], &["Margin"]].concat(), &["Cell size", "Sides", "Dot grid", "Inset rows"], cell(5, 0, 0));
    check_form_with("solid.cell_grid", &sketched, &["Cell size", "Margin", "Inset rows"], cell(5, 2, 3));
    // A snap kept from another shape is whole cells for a sketched cell.
    check_form_with("solid.cell_grid", &[&sketched[..], &["Margin"]].concat(), &["Cell size", "Inset rows"], cell(5, 1, 2));
}

/// A sketched cell: the sketch of the picked profile is the cell sketch, the layout goes as typed
/// (defaults left out), Size is not sent and Snap never is; the cells cut the plate; and Edit
/// Feature refills every field, so OK makes the same feature.
#[test]
fn cell_grid_sketch_shape_sends_the_cell_sketch_and_layout_and_edits_back() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"})).unwrap();
    let field = h.app.run("sketch.create", json!({"plane": "XY", "name": "Field"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let cell = h.app.run("sketch.create", json!({"plane": "XY", "name": "Cell"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [4, 3]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch: field, index: 0 }];
    // No cell picked yet: refused with a message.
    if let Kind::CellGrid(c) = &mut d.kind {
        c.shape = 5;
    }
    assert_eq!(apply_commands(&h.app, &d).unwrap_err(), String::from(h.app.tr("error.cell_grid_cell_sketch")));
    d.inputs[3].items = vec![Sel::Profile { sketch: cell, index: 0 }];
    if let Kind::CellGrid(c) = &mut d.kind {
        // The default clip is snap: a sketched cell sends whole.
        (c.pitch_x, c.pitch_y, c.web, c.depth, c.through_all) = ("8 mm".into(), "6 mm".into(), "1 mm".into(), "1 mm".into(), false);
    }
    let p = apply_commands(&h.app, &d).unwrap()[0].1.clone();
    assert_eq!(p["shape"], json!("sketch"), "{p}");
    assert_eq!((&p["cell_sketch"], &p["pitch_x"], &p["pitch_y"], &p["clip"]), (&json!(cell), &json!("8 mm"), &json!("6 mm"), &json!("whole")), "{p}");
    for absent in ["size", "stagger", "cell_angle", "flip", "snap_inset", "sides", "dot_grid", "cell_origin"] {
        assert!(p.get(absent).is_none(), "{absent} in {p}");
    }
    if let Kind::CellGrid(c) = &mut d.kind {
        (c.stagger, c.cell_angle, c.flip, c.keepout_clip) = ("0.5".into(), "15 deg".into(), true, 2);
    }
    let p = apply_commands(&h.app, &d).unwrap()[0].1.clone();
    assert_eq!((&p["stagger"], &p["cell_angle"], &p["flip"]), (&json!("0.5"), &json!("15 deg"), &json!(true)), "{p}");
    assert!(p.get("keepout_clip").is_none(), "snap is not sent for the keep-outs: {p}");
    let (id, p) = apply_commands(&h.app, &d).unwrap()[0].clone();
    let volume = |h: &mut Harness| h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    let plate = volume(&mut h);
    h.app.run(&id, p).unwrap();
    assert!(volume(&mut h) < plate, "the sketched cells cut the plate");
    // Edit Feature refills the fields, the cell profile and the origin kept as typed.
    let kind = h.app.session.doc.find_feature("CellGrid1").unwrap().kind.clone();
    let (Kind::CellGrid(c), inputs, extra) = crate::dialogs_cell_grid::for_feature(&h.app, &kind).unwrap() else { panic!() };
    assert_eq!(c.shape, 5, "{c:?}");
    assert_eq!((c.pitch_x.as_str(), c.pitch_y.as_str(), c.flip, c.clip), ("8 mm", "6 mm", true, 0), "{c:?}");
    assert_eq!(inputs[3].items, [Sel::Profile { sketch: cell, index: 0 }], "{inputs:?}");
    let commands = crate::dialogs_cell_grid::commands(&h.app, &c, &inputs, &extra).unwrap();
    let [(id, p)] = commands.as_slice() else { panic!("{commands:?}") };
    h.app.run(id, p.clone()).unwrap();
    let again = h.app.session.doc.find_feature("CellGrid2").unwrap().kind.clone();
    let without_profiles = |k: &FeatureKind| {
        let mut v = serde_json::to_value(k).unwrap();
        if let Some(o) = v.as_object_mut() {
            o.remove("profiles");
        }
        v
    };
    assert_eq!(without_profiles(&again), without_profiles(&kind));
}

/// The keep-outs' clip is sent only when it differs from "same as clip", and the snap inset only
/// with snapped cells and when it is not the default 0.
#[test]
fn cell_grid_sends_the_keepout_clip_and_the_snap_inset() {
    let mut h = Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    let sent = |d: &Dialog| apply_commands(&h.app, d).unwrap()[0].1.clone();
    let p = sent(&d);
    assert!(p.get("keepout_clip").is_none() && p.get("snap_inset").is_none(), "{p}");
    if let Kind::CellGrid(c) = &mut d.kind {
        (c.clip, c.keepout_clip, c.snap_inset) = (1, 3, "2".into());
    }
    let p = sent(&d);
    assert_eq!((p.get("keepout_clip"), p.get("snap_inset")), (Some(&json!("free")), Some(&json!("2"))), "{p}");
    if let Kind::CellGrid(c) = &mut d.kind {
        c.clip = 2;
    }
    let p = sent(&d);
    assert!(p.get("snap_inset").is_none(), "{p}");
}

/// Mode is Cut unless Raised is chosen: Cut sends no mode, Raised sends `raised`, the body grows
/// by the cells (a cut shrinks it), and Through all is not offered, nor sent, with raised cells.
#[test]
fn cell_grid_mode_cut_by_default_and_raised_adds_the_cells() {
    let volume = |h: &mut Harness| h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let plate = volume(&mut h);
    let mut d = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    assert!(matches!(&d.kind, Kind::CellGrid(c) if c.mode == 0));
    let p = apply_commands(&h.app, &d).unwrap()[0].1.clone();
    assert!(p.get("mode").is_none(), "{p}");
    if let Kind::CellGrid(c) = &mut d.kind {
        (c.mode, c.through_all, c.direction) = (1, true, 1);
    }
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands.len(), 1, "one command");
    let (id, p) = commands[0].clone();
    assert_eq!((p.get("mode"), p.get("through_all")), (Some(&json!("raised")), None), "{p}");
    h.app.run(&id, p).unwrap();
    assert!(volume(&mut h) > plate, "raised cells add material");
    // A cut of the same field takes material away.
    let mut cut = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    cut.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    let before = volume(&mut h);
    let commands = apply_commands(&h.app, &cut).unwrap();
    assert_eq!(commands.len(), 1, "one command");
    let (id, p) = commands[0].clone();
    h.app.run(&id, p).unwrap();
    assert!(volume(&mut h) < before, "a cut removes material");
    // Edit Feature shows a raised feature as Raised.
    let kind = h.app.session.doc.find_feature("CellGrid1").unwrap().kind.clone();
    let Some((Kind::CellGrid(c), ..)) = crate::dialogs_cell_grid::for_feature(&h.app, &kind) else { panic!() };
    assert_eq!(c.mode, 1);
}

/// Cell Grid fills one sketch: a box over two sketches keeps the last sketch's profiles, as
/// Extrude does, and profiles of two sketches in the input are refused, not dropped.
#[test]
fn cell_grid_takes_the_profiles_of_one_sketch() {
    let (mut h, first, _) = repeated_profile_fixture();
    let second = h.app.run("sketch.create", json!({"plane":"XY","offset":20})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[2,2]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let picks = vec![Sel::Profile { sketch: first, index: 0 }, Sel::Profile { sketch: second, index: 0 }];
    let mut d = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    d.take_box(&h.app.session, picks.clone(), false);
    assert_eq!(d.inputs[0].items, [Sel::Profile { sketch: second, index: 0 }]);
    d.inputs[0].items = picks;
    assert_eq!(apply_commands(&h.app, &d).unwrap_err(), String::from(h.app.tr("error.cell_grid_one_sketch")));
}
/// The anchor takes a construction point as well as a vertex: the command gets its point; a
/// pick that names no point in the model is refused.
#[test]
fn cell_grid_anchor_takes_a_construction_point() {
    let mut h = Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("construct.point.vertex", json!({"point": {"point": [5, 7, 0]}, "name": "Peg"})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    d.inputs[2].items = vec![Sel::ConstructionPoint { name: "Peg".into() }];
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1.get("anchor"), Some(&crate::selection::point_value(Vec3::new(5.0, 7.0, 0.0))), "{commands:?}");
    d.inputs[2].items = vec![Sel::ConstructionPoint { name: "Nowhere".into() }];
    assert_eq!(apply_commands(&h.app, &d).unwrap_err(), String::from(h.app.tr("error.cell_grid_anchor")));
}

/// A construction point in a moved component is the anchor where the view shows it: the session's
/// world has it placed already, so it is not moved by the placement a second time.
#[test]
fn cell_grid_anchor_takes_a_construction_point_of_a_moved_component_where_it_shows() {
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name": "Bracket", "grounded": false})).unwrap();
    h.app.run("construct.point.vertex", json!({"point": {"point": [5, 7, 0]}, "name": "Peg"})).unwrap();
    let occurrence = h.app.session.doc.occurrences.last().map(|o| o.id).unwrap_or_default();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": occurrence, "translate": [20, 0, 0]})).unwrap();
    let shown = solidcraft_engine::view::construction_points(&h.app.session).into_iter().find(|(_, n, _)| n == "Peg").map(|(_, _, p)| p).unwrap();
    assert!((shown.x - 25.0).abs() < 1e-9, "the point is placed by the move: {shown:?}");
    let mut d = Dialog::for_command(&h.app, "solid.cell_grid").unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    d.inputs[2].items = vec![Sel::ConstructionPoint { name: "Peg".into() }];
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1.get("anchor"), Some(&crate::selection::point_value(shown)), "{commands:?}");
}

/// Editing a grid whose anchor is an expression: a parameter changed while the dialog is open
/// moves the anchor, and OK keeps the expression; a new pick still replaces it with numbers.
#[test]
fn cell_grid_edit_keeps_an_anchor_expression_after_a_parameter_change() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"})).unwrap();
    h.app.run("parameters.add", json!({"name": "Inset", "expression": "4 mm"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Field"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app
        .run(
            "solid.cell_grid",
            json!({"sketch": "Field", "shape": "hexagon", "size": 6, "web": 1.5, "anchor": ["Inset", "2 mm", "0 mm"], "through_all": true}),
        )
        .unwrap();
    let kind = h.app.session.doc.find_feature("CellGrid1").unwrap().kind.clone();
    let (Kind::CellGrid(c), inputs, extra) = crate::dialogs_cell_grid::for_feature(&h.app, &kind).unwrap() else { panic!() };
    let typed = extra.get("anchor").cloned().unwrap();
    assert_eq!(typed[0], json!("Inset"), "{typed}");
    h.app.run("parameters.change", json!({"name": "Inset", "expression": "9 mm"})).unwrap();
    let commands = crate::dialogs_cell_grid::commands(&h.app, &c, &inputs, &extra).unwrap();
    assert_eq!(commands[0].1.get("anchor"), Some(&typed), "{commands:?}");
    assert!(commands[0].1.get("anchor_shown").is_none(), "{commands:?}");
    let mut moved = inputs.clone();
    moved[2].items = vec![Sel::Vertex { body: String::new(), point: Vec3::new(9.0, 2.0, 0.0) }];
    let commands = crate::dialogs_cell_grid::commands(&h.app, &c, &moved, &extra).unwrap();
    assert_eq!(commands[0].1.get("anchor"), Some(&crate::selection::point_value(Vec3::new(9.0, 2.0, 0.0))), "{commands:?}");
}

/// The same grid, edited through the dialog: after a parameter change the user clears the anchor
/// and picks the vertex at the point the field first showed. That is a new anchor, so numbers go.
#[test]
fn cell_grid_edit_sends_a_re_picked_anchor_even_at_the_point_it_showed() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"})).unwrap();
    h.app.run("parameters.add", json!({"name": "Inset", "expression": "4 mm"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Field"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let feature = h
        .app
        .run(
            "solid.cell_grid",
            json!({"sketch": "Field", "shape": "hexagon", "size": 6, "web": 1.5, "anchor": ["Inset", "2 mm", "0 mm"], "through_all": true}),
        )
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let mut d = for_feature(&h.app, feature, None).unwrap();
    let anchor =
        |app: &SolveApp, d: &Dialog| crate::dialogs_cell_grid::commands(app, &cell_grid(d), &d.inputs, &d.extra).unwrap()[0].1["anchor"].clone();
    fn cell_grid(d: &Dialog) -> crate::dialogs_cell_grid::CellGrid {
        let Kind::CellGrid(c) = &d.kind else { panic!() };
        c.clone()
    }
    assert_eq!(anchor(&h.app, &d)[0], json!("Inset"), "untouched: the expressions stay");
    h.app.run("parameters.change", json!({"name": "Inset", "expression": "9 mm"})).unwrap();
    assert_eq!(anchor(&h.app, &d)[0], json!("Inset"), "a parameter change alone keeps them");
    let shown = Vec3::new(4.0, 2.0, 0.0);
    crate::dialogs::clear_input(&mut d, 2);
    d.pick(&h.app.session, Sel::Vertex { body: String::new(), point: shown });
    assert_eq!(anchor(&h.app, &d), crate::selection::point_value(shown), "{:?}", d.extra);
}

form_case!(replace_face, "solid.replace_face", []);
form_case!(boundary_fill, "solid.boundary_fill", ["Cells", "Operation"]);
form_case!(align, "solid.align", ["Object Type", "From Snap", "To Snap"]);

/// Edit Feature opens Cell Grid with every stored value, and OK without a change makes the same
/// feature again: the shape, the grid, the clip, the anchor and the targets.
#[test]
fn cell_grid_edit_refills_every_stored_value() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Field"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let stored = json!({
        "sketch": "Field", "shape": "polygon", "sides": "6", "dot_grid": "square", "size": "8 mm", "web": "2 mm",
        "clip": "whole", "margin": "1 mm", "through_all": true, "direction": "negative", "angle": "30 deg",
        "anchor": ["1 mm", "2 mm", "0 mm"], "targets": ["Plate"],
    });
    h.app.run("solid.cell_grid", stored).unwrap();
    let kind = h.app.session.doc.find_feature("CellGrid1").unwrap().kind.clone();
    let (Kind::CellGrid(c), inputs, extra) = crate::dialogs_cell_grid::for_feature(&h.app, &kind).unwrap() else { panic!() };
    assert_eq!((c.shape, c.dot_grid, c.clip, c.direction), (4, 1, 0, 1), "{c:?}");
    assert!(c.through_all, "{c:?}");
    assert!(!inputs[0].items.is_empty() && inputs[1].items == [Sel::Body { name: "Plate".into() }], "{inputs:?}");
    assert!(extra.contains_key("anchor"), "{extra:?}");
    // The field shows the stored anchor's point; emptied, it is the sketch origin: no anchor is sent.
    assert_eq!(inputs[2].items, [Sel::Vertex { body: String::new(), point: Vec3::new(1.0, 2.0, 0.0) }], "{inputs:?}");
    let mut cleared = inputs.clone();
    cleared[2].items.clear();
    let to_origin = crate::dialogs_cell_grid::commands(&h.app, &c, &cleared, &extra).unwrap();
    assert!(to_origin[0].1.get("anchor").is_none(), "{to_origin:?}");
    let commands = crate::dialogs_cell_grid::commands(&h.app, &c, &inputs, &extra).unwrap();
    let [(id, p)] = commands.as_slice() else { panic!("{commands:?}") };
    h.app.run(id, p.clone()).unwrap();
    let again = h.app.session.doc.find_feature("CellGrid2").unwrap().kind.clone();
    // The first feature took the sketch's profiles by default, the dialog names the picked one:
    // the region is the same, the way the record writes it may differ.
    let without_profiles = |k: &FeatureKind| {
        let mut v = serde_json::to_value(k).unwrap();
        if let Some(o) = v.as_object_mut() {
            o.remove("profiles");
        }
        v
    };
    assert_eq!(without_profiles(&again), without_profiles(&kind));
}

/// Edit Feature refills the keep-outs' clip and the snap inset, and OK makes the same feature.
/// The dialog always sends the angle (as the first-time form does), so the stored record names one.
#[test]
fn cell_grid_edit_refills_the_keepout_clip_and_the_snap_inset() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Field"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let stored = json!({
        "sketch": "Field", "size": "6 mm", "web": "1.5 mm", "depth": "2 mm", "clip": "snap", "keepout_clip": "whole",
        "snap_inset": "1", "angle": "15 deg", "targets": ["Plate"],
    });
    h.app.run("solid.cell_grid", stored).unwrap();
    let kind = h.app.session.doc.find_feature("CellGrid1").unwrap().kind.clone();
    let (Kind::CellGrid(c), inputs, extra) = crate::dialogs_cell_grid::for_feature(&h.app, &kind).unwrap() else { panic!() };
    assert_eq!((c.clip, c.keepout_clip, c.snap_inset.as_str()), (1, 1, "1"), "{c:?}");
    let commands = crate::dialogs_cell_grid::commands(&h.app, &c, &inputs, &extra).unwrap();
    let [(id, p)] = commands.as_slice() else { panic!("{commands:?}") };
    h.app.run(id, p.clone()).unwrap();
    let again = h.app.session.doc.find_feature("CellGrid2").unwrap().kind.clone();
    let without_profiles = |k: &FeatureKind| {
        let mut v = serde_json::to_value(k).unwrap();
        if let Some(o) = v.as_object_mut() {
            o.remove("profiles");
        }
        v
    };
    assert_eq!(without_profiles(&again), without_profiles(&kind));
}

#[test]
fn align_components_mode_switches_and_aligns_only_selected_instance() {
    let mut h = Harness::new();
    let comp = h.app.run("component.create", json!({"name": "Extrusion"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 60, "body_name": "Beam"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"component": comp, "translate": [40, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();

    // Preselecting an occurrence opens Align in Components mode and advances to From.
    let picked = vec![Sel::Occurrence { path: vec![second] }];
    h.app.session.selection = picked.clone();
    h.app.start("solid.align");
    h.frames(4);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].label, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);

    // Switching Object Type to Bodies and back restores the preselected occurrence.
    mirror_object_type(&mut h, "Bodies");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].label, "Bodies");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    mirror_object_type(&mut h, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].label, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);

    let from = face_sel(&h.app.session, Vec3::new(45.0, 5.0, 0.0)).unwrap();
    let to = face_sel(&h.app.session, Vec3::new(10.0, 5.0, 30.0)).unwrap();
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[1].items = vec![from];
        d.inputs[2].items = vec![to];
    }
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].0, "occurrence.align");
    assert_eq!(commands[0].1["paths"], json!([[second]]));
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }

    let st = crate::open_meshing::session_world(&h.app.session);
    let b1 = st.body("Beam").unwrap().mesh().bounds();
    assert!((b1.min.x - 0.0).abs() < 1e-5 && (b1.max.x - 10.0).abs() < 1e-5, "{b1:?}");
    let b2 = st.body("Beam (Extrusion:2)").unwrap().mesh().bounds();
    assert!((b2.min.x - 10.0).abs() < 1e-5 && (b2.max.x - 70.0).abs() < 1e-5, "{b2:?}");
}
form_case!(remove, "solid.remove", []);
form_case!(path_pattern, "solid.pattern.path", ["Type", "Object Type", "Quantity", "Spacing", "pattern.start_point", "Orientation"]);
form_case!(pipe, "solid.pipe", ["Diameter", "Type", "Wall", "Operation"]);

/// #18344: the Pipe window has a Type row, and the Wall row stays where it was: a thickness typed
/// in it makes a tube, as it always did, and the Type row says which one the pipe is.
#[test]
fn the_pipe_window_keeps_its_wall_row_and_says_which_pipe_it_makes() {
    let mut h = fixture();
    h.app.home.open = false;
    let d = representative(&h, "solid.pipe");
    h.app.dialog = Some(d);
    h.frames(2);
    // The Wall row is there for a solid pipe, as it was before the Type row.
    assert!(scenario::rect_of("dialog:row:Type").is_some(), "the pipe window has a type row");
    assert!(scenario::rect_of("dialog:row:Wall").is_some(), "a solid pipe keeps the wall row");
    {
        let Kind::Pipe { hollow, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("no pipe") };
        assert_eq!(*hollow, 0, "a pipe with no wall is a solid rod");
    }
    // A thickness typed in the wall row makes a tube, and the type row says so.
    h.step(&json!({"click": {"handle": "dialog:Wall"}})).unwrap();
    h.step(&json!({"text": "2 mm"})).unwrap();
    h.frames(2);
    {
        let Kind::Pipe { hollow, wall, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("no pipe") };
        assert_eq!(wall, "2 mm", "the wall row holds the thickness that was typed in it");
        assert_eq!(*hollow, 1, "a thickness in the wall row makes a tube");
    }
    // An empty wall row is a rod again.
    for _ in 0..4 {
        h.step(&json!({"key": "Backspace"})).unwrap();
    }
    h.frames(2);
    {
        let Kind::Pipe { hollow, wall, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("no pipe") };
        assert_eq!(wall, "", "the wall row is empty again");
        assert_eq!(*hollow, 0, "an empty wall row is a solid rod");
    }
}

/// #18344: a thickness typed in the Wall row is never dropped. Choosing Solid in the Type row
/// leaves the thickness where the user typed it, and the command carries it.
#[test]
fn the_pipe_window_keeps_the_typed_wall_when_the_type_becomes_solid() {
    let mut h = fixture();
    h.app.home.open = false;
    let d = representative(&h, "solid.pipe");
    h.app.dialog = Some(d);
    h.frames(2);
    // A thickness typed in the wall row.
    h.step(&json!({"click": {"handle": "dialog:Wall"}})).unwrap();
    h.step(&json!({"text": "2 mm"})).unwrap();
    h.frames(2);
    {
        let Kind::Pipe { hollow, wall, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("no pipe") };
        assert_eq!(wall, "2 mm", "the wall row holds the thickness that was typed in it");
        assert_eq!(*hollow, 1, "a thickness in the wall row makes a tube");
    }
    // Solid, in the Type row.
    h.step(&json!({"click": {"handle": "dialog:Type"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Type:Solid"}})).unwrap();
    h.frames(2);
    {
        let Kind::Pipe { hollow, wall, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("no pipe") };
        assert_eq!(*hollow, 0, "the type row says solid");
        assert_eq!(wall, "2 mm", "the thickness typed in the wall row is never dropped");
    }
    // The command carries the thickness the user typed: a wall in the window is the pipe's wall.
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["wall"], "2 mm", "{:?}", commands[0].1);
}
form_case!(stock, "solid.bounding_solid", ["Margin"]);
form_case!(frame_plate, "frame.plate", ["Shape", "Width", "Height", "Thickness", "Offset", "Bolt Pattern"]);
form_case!(frame_member, "frame.member", ["All Lines", "Anchor", "Angle", "Mirror", "Name"]);
form_case!(thread, "solid.thread", ["Form", "Size", "Pitch", "Designation", "Hand", "Starts", "Extent", "Start", "End"]);
form_case!(primitive_box, "solid.box", ["ui.length", "ui.width", "ui.height", "Operation"]);
form_case!(cylinder, "solid.cylinder", ["Size", "ui.diameter", "ui.height", "Operation"]);
form_case!(sphere, "solid.sphere", ["Size", "ui.diameter", "Operation"]);
form_case!(torus, "solid.torus", ["ui.major_radius", "ui.minor_radius", "Operation"]);
form_case!(cone, "solid.cone", ["ui.diameter", "ui.top_diameter", "ui.height", "Operation"]);
form_case!(coil, "solid.coil", ["ui.diameter", "ui.revolutions", "ui.pitch", "ui.section_size", "ui.direction", "Operation"]);
form_case!(combine, "solid.combine", ["Operation"]);
form_case!(surface_extrude, "surface.extrude", ["Distance", "Direction"]);
form_case!(surface_revolve, "surface.revolve", ["Angle", "Direction"]);
form_case!(surface_sweep, "surface.sweep", ["Orientation"]);
form_case!(surface_loft, "surface.loft", []);
form_case!(surface_patch, "surface.patch", ["Continuity"]);
form_case!(surface_unstitch, "surface.unstitch", []);
form_case!(surface_reverse, "surface.reverse_normal", []);
form_case!(surface_offset, "surface.offset", ["Distance"]);
form_case!(surface_ruled, "surface.ruled", ["Distance", "Type", "Corners"]);
form_case!(surface_stitch, "surface.stitch", ["Tolerance"]);
form_case!(surface_fair, "surface.fair", ["ui.fair_strength", "ui.fair_iterations", "Tolerance"]);
form_case!(surface_match, "surface.match", ["Continuity"]);
form_case!(thicken, "surface.thicken", ["Thickness", "Direction", "Operation"]);
form_case!(surface_extend, "surface.extend", ["Extent Type", "Distance"]);
form_case!(surface_trim, "surface.trim", []);
form_case!(surface_untrim, "surface.untrim", ["Holes"]);
form_case!(split_face, "solid.split_face", []);
form_case!(flange, "sheet.flange", ["Type", "Rule"]);
form_case!(path_flange, "sheet.path_flange", ["Bend Radius"]);
form_case!(tab, "sheet.tab", []);
form_case!(hem, "sheet.hem", ["Type", "Length", "ui.sheet_gap"]);
form_case!(fold, "sheet.fold", ["Bend Line Position", "Angle", "Bend Radius"]);
form_case!(unfold, "sheet.unfold", []);
form_case!(refold, "sheet.refold", []);
form_case!(sheet_convert, "sheet.convert", ["Rule"]);
form_case!(sheet_recognize, "sheet.recognize", ["Rule", "New Rule"]);
form_case!(boss, "plastic.boss", ["Diameter", "Height", "Hole Diameter", "Hole Depth"]);
form_case!(lip, "plastic.lip", ["Type", "Rim Edge", "Width", "Height"]);
form_case!(snap_fit, "plastic.snap_fit", ["Catch Direction", "Length", "Thickness", "Width", "Catch Depth", "Catch Length"]);
form_case!(rest, "plastic.rest", ["Shape", "Diameter", "Height"]);
form_case!(plastic_assign, "plastic.assign_rule", ["Rule"]);
form_case!(component, "component.create", ["Name"]);
form_case!(joint, "joint.create", ["Type", "Angle"]);
form_case!(as_built, "joint.as_built", ["Type"]);
form_case!(joint_origin, "joint.origin", ["Name"]);
form_case!(rigid_group, "joint.rigid_group", []);
form_case!(motion_link, "joint.motion_link", ["Joint 1", "Joint 2", "Ratio", "Offset"]);
form_case!(gear, "joint.gear", ["Gear 1", "Radius 1", "Gear 2", "Radius 2", "Ratio", "Animate"]);
form_case!(rack, "joint.rack_pinion", ["Pinion", "Radius", "Rack", "Ratio", "Animate"]);
form_case!(screw, "joint.screw", ["Screw", "Nut", "Pitch", "Ratio", "Animate"]);
form_case!(cam, "joint.cam", ["Cam", "Follower", "Type", "Animate"]);
form_case!(trace, "motion.trace", ["Joint", "From", "To", "Samples"]);

#[test]
fn registry_dialog_inventory_has_no_unclassified_feature_forms() {
    let h = fixture();
    for spec in solidcraft_engine::command_specs() {
        let Some(d) = Dialog::for_command(&h.app, spec.id) else { continue };
        assert!(super::command(&d.kind).is_some(), "{}: {:?}", spec.id, d.kind);
    }
}

/// Texts these dialogs draw themselves read in the user's language: the joints' Play button, the
/// Motion Study's Loop, the Stitch tolerance hint, the Flange type and the Emboss note.
#[test]
fn hand_drawn_dialog_texts_are_translated() {
    fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
            _ => {}
        }
    }
    let mut h = fixture();
    h.app.preferences.language = "ja".into();
    let _japanese = solidcraft_ui_text::scope("ja");
    for (command, english) in [
        ("joint.gear", "▶ Play"),
        ("motion.study", "Loop"),
        ("surface.stitch", "tight"),
        ("sheet.flange", "Base"),
        (
            "solid.emboss",
            "Leave Target Face empty to use the face the sketch lies on, or the cylinder it touches. Pick a face if Emboss chooses the wrong one.",
        ),
    ] {
        let mut d = Dialog::for_command(&h.app, command).unwrap();
        let mut painted = Vec::new();
        for _ in 0..2 {
            let mut out = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 1600.0))), ..Default::default() },
                |ui| {
                    ui.set_width(264.0);
                    ui_kit::scope(ui, |ui| {
                        rows(&mut h.app, &h.ctx, &mut d, ui);
                    });
                },
            );
            out.textures_delta.clear();
            painted.clear();
            out.shapes.iter().for_each(|shape| texts(&shape.shape, &mut painted));
        }
        assert!(!painted.iter().any(|p| p == english), "{command} paints {english:?} in Japanese: {painted:?}");
    }
}

#[test]
fn revolve_layout_fixture_builds_a_valid_solid() {
    let mut h = fixture();
    let d = representative(&h, "solid.revolve");
    let before = h.app.session.model.results.len();
    let commands = crate::dialogs::preview_commands(&h.app, &d).unwrap();
    assert!(!commands.is_empty());
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    assert!(h.app.session.model.results.len() > before);
    assert!(h.app.session.model.results.iter().all(|result| result.error.is_none()));
}

#[test]
fn new_cards_settle_before_the_first_click_in_both_layouts() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        for command in ["solid.box", "solid.revolve", "solid.shell", "joint.create"] {
            let mut h = fixture();
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.resize", json!({"width":1400,"height":950}));
            // The system's fallback faces are read on a thread (slow where large CJK faces are
            // installed) and the fonts are set up again once they are in, which can change the
            // card's text: on CI that landed between the two reads below (#21495). Read them now
            // (this waits for that thread's search) and let the app set its fonts up with them.
            crate::fonts::setup_fonts_for(&h.ctx, "en", &h.app.preferences.ui_font);
            h.frames(2);
            h.app.dialog = Some(representative(&h, command));
            // The card settles on its own; its preview (joint.create's is an error row here)
            // comes from a worker, which a loaded machine can finish a frame later.
            h.frames_after_preview(2);
            let first = scenario::rect_of("dialog:footer").unwrap();
            h.frames_after_preview(1);
            let settled = scenario::rect_of("dialog:footer").unwrap();
            assert_eq!(first, settled, "{command} {mode:?}: footer moved under the first click");
        }
    }
}

#[test]
fn the_loft_dialog_widens_its_label_column_for_the_current_language() {
    // #10620: German "Anpassung an Leitkurve" (Rail Fit) clipped to "Anpassung an Leit…" in
    // the fixed-width Loft card. The label column now measures the dialog's labels in the
    // active language and the card widens by the same amount, so the label fits and the
    // control column keeps its design width. English, whose labels fit the design, is unchanged.
    let t = Tokens::get();
    let shoot = |language: &str| {
        let mut h = fixture();
        h.call("ui.resize", json!({"width": 1400, "height": 950}));
        h.app.preferences.language = language.into();
        h.app.dialog = Some(representative(&h, "solid.loft"));
        h.frames(4);
        (h, scenario::rect_of("dialog:card").unwrap(), scenario::rect_of("dialog:label:Rail Fit").unwrap())
    };
    let (h, card, label) = shoot("de");
    let (_, english_card, english_label) = shoot("en");
    assert!(card.width() > english_card.width(), "the German card widened: {card:?} vs {english_card:?}");
    assert!(label.width() > t.form_label_width * 1.6, "past the ordinary cap: {label:?}");
    assert!(english_label.width() <= t.form_label_width * 1.6, "English keeps the design's label column: {english_label:?}");
    // The drawn galley holds the whole translation: no ellipsis.
    let drawn = h
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("Anpassung") => Some(text.galley.job.text.to_string()),
            _ => None,
        })
        .expect("the Rail Fit row drew its German label");
    assert!(drawn.contains("Leitkurve") && !drawn.contains('…'), "the label is not ellipsized: {drawn:?}");
}

#[test]
fn remaining_dialogs_keep_kit_chrome_and_cancel_restores_preview() {
    for command in [
        "contact.create",
        "motion.study",
        "explode.create",
        "parts.insert",
        "joint.drive",
        "inspect.interference",
        "file.save_mesh",
        "file.export_view_dxf",
        "sheet.flat_pattern",
    ] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        let design = h.app.session.doc.to_json();
        let moves = h.app.session.pending_moves.clone();
        h.app.start(command);
        h.frames(4);
        for key in ["dialog:header", "dialog:footer", "dialog:outer"] {
            let rect = scenario::rect_of(key).unwrap_or_else(|| panic!("{command}: missing {key}"));
            assert!(rect.is_finite() && rect.is_positive() && h.ctx.content_rect().contains_rect(rect), "{command}: {key} {rect:?}");
        }
        let footer = scenario::rect_of("dialog:footer").unwrap();
        assert!(footer.contains(scenario::handle_at("dialog:cancel").unwrap()), "{command}: cancel remains in the pinned footer");
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        assert!(h.app.dialog.is_none(), "{command}");
        assert_eq!(h.app.session.doc.to_json(), design, "{command}: opening/cancelling keeps the design");
        assert_eq!(h.app.session.pending_moves, moves, "{command}: cancel restores display-only moves");
    }
}

/// Interface › Dialog buttons › Top (#6708): every card made by the shared panel moves its
/// actions into the header row, with the footer's enabled states, handles and close behaviour.
#[test]
fn top_dialog_buttons_keep_every_cards_states_handles_and_close() {
    for command in [
        "solid.revolve",
        "solid.shell",
        "solid.box",
        "joint.create",
        "solid.thread",
        "inspect.measure",
        "contact.create",
        "motion.study",
        "explode.create",
        "parts.insert",
        "joint.drive",
        "inspect.interference",
        "file.save_mesh",
        "file.export_view_dxf",
        "sheet.flat_pattern",
    ] {
        let mut seen = Vec::new();
        for placement in ["bottom", "top"] {
            let mut h = fixture();
            h.call("ui.resize", json!({"width":1600,"height":1200}));
            h.call("ui.prefs", json!({"dialog_buttons": placement}));
            let design = h.app.session.doc.to_json();
            h.app.start(command);
            h.frames(4);
            let header = scenario::rect_of("dialog:header").unwrap_or_else(|| panic!("{command} {placement}: no header"));
            let cancel = scenario::handle_at("dialog:cancel").unwrap_or_else(|| panic!("{command} {placement}: no cancel"));
            seen.push((
                scenario::count_of("dialog:ok:enabled"),
                scenario::rect_of("dialog:ok").is_some(),
                scenario::rect_of("dialog:apply").is_some(),
            ));
            if placement == "top" {
                assert!(scenario::rect_of("dialog:footer").is_none(), "{command}: no footer row");
                assert!(header.contains(cancel), "{command}: cancel in the header {header:?}");
                assert!(scenario::rect_of("dialog:ok").is_none_or(|ok| header.contains_rect(ok)), "{command}: OK in the header");
                assert_eq!(scenario::handle_at("dialog:close"), Some(cancel), "{command}: Cancel is the close");
            } else {
                assert!(scenario::rect_of("dialog:footer").unwrap().contains(cancel), "{command}: cancel in the footer");
            }
            h.step(&json!({"click":{"handle":"dialog:close"}})).unwrap();
            assert!(h.app.dialog.is_none(), "{command} {placement}: closed");
            assert_eq!(h.app.session.doc.to_json(), design, "{command} {placement}: closing keeps the design");
        }
        assert_eq!(seen.first(), seen.get(1), "{command}: OK and Apply as at the bottom");
    }
}

/// With the buttons at the top, OK, Enter and Esc do what they do with them at the bottom.
#[test]
fn top_dialog_buttons_apply_and_cancel_like_the_footer() {
    for how in ["click", "enter", "escape"] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"dialog_buttons": "top"}));
        h.app.dialog = Some(representative(&h, "solid.revolve"));
        h.settle();
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0));
        let (design, bodies) = (h.app.session.doc.to_json(), h.app.session.world_state().unwrap().bodies.len());
        let step = match how {
            "click" => json!({"click":{"handle":"dialog:OK"}}),
            "enter" => json!({"key":"Enter"}),
            _ => json!({"key":"Escape"}),
        };
        h.step(&step).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{how}: the dialog closes");
        if how == "escape" {
            assert_eq!(h.app.session.doc.to_json(), design, "Esc leaves the design as it was");
        } else {
            assert_eq!(h.app.session.world_state().unwrap().bodies.len(), bodies + 1, "{how}: the revolve is made");
        }
    }
}

/// OK and Cancel in the view's corner (#6709) do what the dialog's own do: the check applies and
/// closes, the cross cancels without a change, and the check is grey (and does nothing) whenever
/// the dialog's OK is.
#[test]
fn view_corner_ok_and_cancel_answer_the_dialog_like_its_own_buttons() {
    for how in ["ok", "cancel"] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"view_confirm_corner": true}));
        h.app.dialog = Some(representative(&h, "solid.revolve"));
        h.settle();
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), scenario::count_of("dialog:ok:enabled"));
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(1.0));
        let (design, bodies) = (h.app.session.doc.to_json(), crate::open_meshing::session_world(&h.app.session).bodies.len());
        h.step(&json!({"click":{"handle":format!("view:confirm:{how}")}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{how}: the dialog closes");
        if how == "ok" {
            assert_eq!(crate::open_meshing::session_world(&h.app.session).bodies.len(), bodies + 1, "the check made the revolve");
        } else {
            assert_eq!(h.app.session.doc.to_json(), design, "the cross leaves the design as it was");
        }
        assert!(scenario::rect_of("view:confirm").is_none(), "{how}: no dialog and no sketch, no pair");
    }

    // A dialog whose OK is grey: so is the check, and clicking it changes nothing.
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    h.call("ui.prefs", json!({"view_confirm_corner": true}));
    h.app.start("solid.shell");
    h.settle();
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0));
    assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(0.0));
    let design = h.app.session.doc.to_json();
    h.step(&json!({"click":{"handle":"view:confirm:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_some(), "a grey check does nothing");
    assert_eq!(h.app.session.doc.to_json(), design);
    // Measure only closes: its check stays grey and the cross is its Close.
    h.app.start("inspect.measure");
    h.settle();
    assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(0.0));
    h.step(&json!({"click":{"handle":"view:confirm:cancel"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "the cross closes Measure");
}

/// Both options at once (#6708 with #6709): the check in the view's corner greys out with the OK
/// in the title row, the pair keeps clear of the dialog, and either OK applies it.
#[test]
fn top_dialog_buttons_and_the_view_corner_agree() {
    let open = |command: &str| {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"dialog_buttons": "top", "view_confirm_corner": true}));
        if command == "solid.revolve" {
            h.app.dialog = Some(representative(&h, command));
        } else {
            h.app.start(command);
        }
        h.settle();
        h
    };
    for (command, enabled) in [("solid.revolve", 1.0), ("solid.shell", 0.0)] {
        let _h = open(command);
        let header = scenario::rect_of("dialog:header").unwrap();
        assert!(scenario::rect_of("dialog:footer").is_none(), "{command}");
        assert!(header.contains_rect(scenario::rect_of("dialog:ok").unwrap()), "{command}: OK in the title row");
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(enabled), "{command}");
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(enabled), "{command}: the check follows the title row's OK");
        assert!(!scenario::rect_of("view:confirm").unwrap().intersects(header), "{command}: the pair keeps clear of the dialog");
    }
    for handle in ["dialog:ok", "view:confirm:ok"] {
        let mut h = open("solid.revolve");
        let bodies = h.app.session.world_state().unwrap().bodies.len();
        h.step(&json!({"click":{"handle":handle}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{handle}: the dialog closes");
        assert_eq!(h.app.session.world_state().unwrap().bodies.len(), bodies + 1, "{handle}: the revolve is made");
    }
}

fn resized_box_editor(command: &str, params: Value) -> (Harness, u64) {
    let mut h = Harness::new();
    for (id, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]})),
        ("sketch.dimension", json!({"entities":["l1"],"value":40})),
        ("sketch.dimension", json!({"entities":["l2"],"value":20})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":20})),
        (command, params),
        ("parameters.change", json!({"name":"d2","expression":"30"})),
    ] {
        h.app.run(id, params).unwrap();
    }
    let id = h.app.session.doc.features.last().unwrap().id;
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    h.app.edit_feature(id);
    (h, id)
}

#[test]
fn blend_editor_refills_named_edges_after_sketch_resize() {
    for command in ["solid.fillet", "solid.chamfer"] {
        let (mut h, id) = resized_box_editor(command, json!({"edges":[[40,10,20],[20,20,20],[20,0,20],[0,10,20]],"radius":4,"distance":4}));
        let d = h.app.dialog.as_ref().unwrap();
        let picks = &d.inputs[0].items;
        assert_eq!(picks.len(), 4);
        assert_eq!(&d.picked_edges, picks);
        let s = &h.app.session;
        let body = s.model.state().bodies.first().unwrap().clone();
        let names = &s.doc.feature(id).unwrap().edge_names;
        for (pick, name) in picks.iter().zip(names) {
            let Sel::Edge { point, index, .. } = pick else { panic!("{pick:?}") };
            let hit = solidcraft_engine::doc::naming::locate_edge(&body, name, *point).unwrap();
            assert_eq!(*index, hit.edge);
            assert!(hit.on_point, "{command}: {point:?} must lie on the resized edge");
        }
        let commands = apply_commands(&h.app, d).unwrap();
        crate::dialogs::roll_marker(&mut h.app, None).unwrap();
        for (command, params) in commands {
            h.app.run(&command, params).unwrap();
        }
        assert_eq!(h.app.session.doc.features.last().unwrap().id, id);
        assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    }
}

#[test]
fn face_editor_refills_named_faces_after_sketch_resize() {
    for (command, params) in
        [("solid.shell", json!({"faces":[[20,20,10]],"thickness":2})), ("solid.draft", json!({"faces":[[20,20,10]],"neutral":"XY","angle":3}))]
    {
        let (mut h, id) = resized_box_editor(command, params);
        let d = h.app.dialog.as_ref().unwrap();
        let picks = &d.inputs[0].items;
        assert_eq!(picks.len(), 1, "{command}: the moved face must refill");
        let Sel::Face { point, .. } = picks.first().unwrap() else { panic!("{picks:?}") };
        assert!((point.y - 30.0).abs() < 1e-6, "{command}: {point:?}");
        let commands = apply_commands(&h.app, d).unwrap();
        crate::dialogs::roll_marker(&mut h.app, None).unwrap();
        for (command, params) in commands {
            h.app.run(&command, params).unwrap();
        }
        assert_eq!(h.app.session.doc.features.last().unwrap().id, id);
        assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    }
}

/// A 40 x 20 x 20 block with a pocket 10 deep from x 10 to 30 and y 5 to `d2 - 5`, relieved by
/// `params`; then `d2` goes from 20 to 30, so the pocket's far wall moves from y 15 to y 25, and
/// the Dogbone feature is opened for editing.
fn resized_pocket_dogbone_editor(params: Value) -> (Harness, u64) {
    let mut h = Harness::new();
    for (id, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]})),
        ("sketch.dimension", json!({"entities":["l1"],"value":40})),
        ("sketch.dimension", json!({"entities":["l2"],"value":20})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":20})),
        ("solid.box", json!({"corner":[10,5,10],"length":20,"width":"d2 - 10","height":10,"operation":"cut"})),
        ("solid.dogbone", params),
        ("parameters.change", json!({"name":"d2","expression":"30"})),
    ] {
        h.app.run(id, params).unwrap();
    }
    let id = h.app.session.doc.features.last().unwrap().id;
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    h.app.edit_feature(id);
    (h, id)
}

fn apply_dogbone_editor(mut h: Harness, id: u64) {
    let d = h.app.dialog.as_ref().unwrap();
    let commands = apply_commands(&h.app, d).unwrap();
    crate::dialogs::roll_marker(&mut h.app, None).unwrap();
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    assert_eq!(h.app.session.doc.features.last().unwrap().id, id);
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
}

/// The stored bare diameter 3 (millimetres) refills as "3 mm" in an inch design, not "3", which OK
/// would read as 3 in. A bit diameter and gap typed as bare numbers there are inches: OK hands the
/// command 3.175 mm and 0.254 mm, not 0.125 mm and 0.01 mm.
#[test]
fn dogbone_editor_reads_bare_lengths_in_the_design_unit() {
    let (mut h, id) = resized_pocket_dogbone_editor(json!({"faces":[[20,10,10]],"diameter":3}));
    crate::dialogs::cancel(&mut h.app);
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.app.edit_feature(id);
    let Some(crate::dialogs::Kind::Dogbone(k)) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("no Dogbone editor") };
    assert_eq!(k.diameter, "3 mm");
    k.diameter = "0.125".into();
    // Minimal, the style with a gap.
    k.style = 1;
    k.gap = "0.01".into();
    // An edit is applied as a redefine of the feature, with the command's parameters inside.
    let (command, redefine) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().into_iter().next().unwrap();
    assert_eq!(command, "timeline.redefine", "{redefine}");
    let params = &redefine["params"];
    for (key, mm) in [("diameter", 3.175), ("gap", 0.254)] {
        let text = params[key].as_str().unwrap_or_default();
        let got = h.app.session.doc.eval(text, solidcraft_engine::doc::expr::Kind::Length).unwrap();
        assert!(text.ends_with(" in") && (got - mm).abs() < 1e-9, "{key}: {text} is {got} mm");
    }
}

#[test]
fn dogbone_editor_refills_named_edges_after_sketch_resize() {
    let (h, id) = resized_pocket_dogbone_editor(json!({"edges":[[10,5,15],[30,5,15],[30,15,15],[10,15,15]],"diameter":3}));
    let d = h.app.dialog.as_ref().unwrap();
    let picks = &d.inputs[0].items;
    assert_eq!(picks.len(), 4);
    let s = &h.app.session;
    let body = s.model.state().bodies.first().unwrap().clone();
    let names = &s.doc.feature(id).unwrap().edge_names;
    assert_eq!(names.len(), 4);
    for (pick, name) in picks.iter().zip(names) {
        let Sel::Edge { point, index, .. } = pick else { panic!("{pick:?}") };
        let hit = solidcraft_engine::doc::naming::locate_edge(&body, name, *point).unwrap();
        assert_eq!(*index, hit.edge);
        assert!(hit.on_point, "{point:?} must lie on the moved corner edge");
    }
    let far: Vec<f64> = picks[2..].iter().map(|p| if let Sel::Edge { point, .. } = p { point.y } else { f64::NAN }).collect();
    assert!(far.iter().all(|y| (y - 25.0).abs() < 1e-6), "{far:?}");
    apply_dogbone_editor(h, id);
}

#[test]
fn dogbone_editor_refills_named_faces_after_sketch_resize() {
    let (h, id) = resized_pocket_dogbone_editor(json!({"faces":[[20,10,10]],"diameter":3}));
    let d = h.app.dialog.as_ref().unwrap();
    let picks = &d.inputs[0].items;
    assert_eq!(picks.len(), 1, "the pocket's floor must refill");
    let Sel::Face { point, .. } = picks.first().unwrap() else { panic!("{picks:?}") };
    assert!((point.z - 10.0).abs() < 1e-6, "{point:?}");
    apply_dogbone_editor(h, id);
}

/// A face pick Edit Feature cannot bring back (no stored name, and its point 2 mm above the
/// pocket's floor, on no face) is not dropped: OK hands it to the command as stored, beside the
/// edge pick that did come back, and it counts as a pick, so there is no "no corners" refusal.
#[test]
fn dogbone_editor_keeps_a_pick_it_cannot_bring_back() {
    use solidcraft_engine::doc::FeatureKind;

    let (h, id) = resized_pocket_dogbone_editor(json!({"edges":[[10,5,15]],"faces":[[20,10,10]],"diameter":3}));
    let mut f = h.app.session.doc.feature(id).unwrap().clone();
    let FeatureKind::Dogbone { faces, .. } = &mut f.kind else { panic!("{:?}", f.kind) };
    faces[0].0 = Vec3::new(20.0, 10.0, 12.0);
    f.face_names.clear();
    let (crate::dialogs::Kind::Dogbone(k), inputs, extra) = crate::dialogs_dogbone::for_feature(&h.app, &f).unwrap() else {
        panic!("no Dogbone editor")
    };
    assert!(inputs[0].items.iter().all(|p| matches!(p, Sel::Edge { .. })), "{:?}", inputs[0].items);
    assert_eq!(inputs[0].items.len(), 1, "the edge comes back, the face does not");
    let commands = crate::dialogs_dogbone::commands(&h.app, &k, &inputs, &extra).unwrap();
    let [(command, params)] = commands.as_slice() else { panic!("{commands:?}") };
    assert_eq!(command, "solid.dogbone");
    assert_eq!(params["faces"], json!([[20.0, 10.0, 12.0]]), "{params}");
    assert_eq!(params["edges"].as_array().map(Vec::len), Some(1), "{params}");
    for key in ["kept_edges", "kept_faces", "kept_body"] {
        assert!(params.get(key).is_none(), "{key} must not reach the command: {params}");
    }
}

/// The dialog shows a kept pick with a button that drops it (a kept point the command refuses
/// would fail every OK): after the click, OK sends the edge that came back and no kept face, and
/// the button is gone.
#[test]
fn dogbone_editor_drops_a_kept_pick_on_request() {
    use solidcraft_engine::doc::FeatureKind;

    let (mut h, id) = resized_pocket_dogbone_editor(json!({"edges":[[10,5,15]],"faces":[[20,10,10]],"diameter":3}));
    let mut f = h.app.session.doc.feature(id).unwrap().clone();
    let FeatureKind::Dogbone { faces, .. } = &mut f.kind else { panic!("{:?}", f.kind) };
    faces[0].0 = Vec3::new(20.0, 10.0, 12.0);
    f.face_names.clear();
    let (kind, inputs, extra) = crate::dialogs_dogbone::for_feature(&h.app, &f).unwrap();
    let d = h.app.dialog.as_mut().unwrap();
    (d.kind, d.inputs, d.extra) = (kind, inputs, extra);
    h.frames(2);
    assert!(h.widget("Remove stored picks").is_some(), "the kept pick must show with its button");
    h.step(&json!({"widget":"Remove stored picks"})).unwrap();
    h.frames(2);
    assert!(h.widget("Remove stored picks").is_none(), "nothing kept is left to remove");
    let d = h.app.dialog.as_ref().unwrap();
    for key in ["kept_edges", "kept_faces", "kept_body"] {
        assert!(!d.extra.contains_key(key), "{key} must be dropped: {:?}", d.extra);
    }
    // An edit is applied as a redefine of the feature, with the command's parameters inside.
    let commands = apply_commands(&h.app, d).unwrap();
    let [(command, redefine)] = commands.as_slice() else { panic!("{commands:?}") };
    assert_eq!(command, "timeline.redefine", "{redefine}");
    let params = &redefine["params"];
    assert!(params.get("faces").is_none(), "the kept face must not reach the command: {params}");
    assert_eq!(params["edges"].as_array().map(Vec::len), Some(1), "{params}");
}

#[test]
fn draft_uses_the_picked_solid_beside_a_hidden_overlapping_mesh() {
    use solidcraft_engine::doc::{Document, MeshData, naming};

    let mesh = solidcraft_engine::kernel::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 10.0))
        .unwrap()
        .mesh(&solidcraft_engine::kernel::MeshTolerance::chord(0.01))
        .unwrap();
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "overlapping-box.stl".into(),
            meshes: vec![MeshData {
                name: "Hidden mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    let mut h = Harness::new();
    h.app.session = Session::new(doc);
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10})).unwrap();
    h.app.run("browser.visibility", json!({"items":["Hidden mesh"],"visible":false})).unwrap();
    let state = crate::open_meshing::session_world(&h.app.session);
    let hidden_mesh = state.body("Hidden mesh").unwrap().mesh();
    let body = state.bodies.iter().find(|body| !body.body.is_mesh()).unwrap();
    let target = body.name.clone();
    let point = Vec3::new(20.0, 10.0, 5.0);
    let name = naming::face_name_at_body(body, point).unwrap();
    let (index, _) = naming::face_on_body(body, &name, point).unwrap();
    h.app.session.selection = vec![Sel::Face { body: target.clone(), index, point }];
    let mut d = Dialog::for_command(&h.app, "solid.draft").unwrap();
    d.inputs[1].items = vec![Sel::Plane { name: "XY".into() }];
    d.kind = Kind::Draft { angle: "5 deg".into() };
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(preview_commands(&h.app, &d).unwrap(), commands);
    let (command, params) = &commands[0];
    h.app.run(command, params.clone()).unwrap();
    assert_eq!(params["body"], target);
    let state = h.app.session.model.state();
    assert_eq!(state.bodies.len(), 2);
    assert!(state.body("Hidden mesh").unwrap().body.is_mesh());
    assert_eq!(state.body("Hidden mesh").unwrap().mesh().positions, hidden_mesh.positions);
    assert_eq!(state.body("Hidden mesh").unwrap().mesh().triangles, hidden_mesh.triangles);
    let volume = state.body(&target).unwrap().body.volume_at(0.01).unwrap();
    assert!((volume - 4000.0).abs() > 50.0, "the picked wall should taper: {volume}");
    assert!(matches!(
        &h.app.session.doc.features.last().unwrap().kind,
        FeatureKind::Draft { body: Some(body), .. } if *body == target
    ));
}

/// The Offset Face dialog sends the body clicked where two bodies touch flush, so that body moves.
#[test]
fn offset_face_dialog_moves_the_body_clicked_where_two_bodies_touch_flush() {
    use solidcraft_engine::doc::naming;
    let mut h = Harness::new();
    // Big (40 × 30 × 20, made first) and Small (10 × 10 × 5) touch flush at Y=0.
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "operation": "new", "body_name": "Big"})).unwrap();
    h.app
        .run("solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [10, -10, 5], "operation": "new", "body_name": "Small"}))
        .unwrap();
    let state = h.app.session.model.state();
    let small = state.body("Small").unwrap();
    let point = Vec3::new(15.0, 0.0, 7.5);
    let name = naming::face_name_at_body(small, point).unwrap();
    let (index, _) = naming::face_on_body(small, &name, point).unwrap();
    h.app.session.selection = vec![Sel::Face { body: "Small".into(), index, point }];
    let mut d = Dialog::for_command(&h.app, "solid.offset_face").unwrap();
    d.kind = Kind::OffsetFaces { distance: "2".into() };
    let commands = apply_commands(&h.app, &d).unwrap();
    let (command, params) = &commands[0];
    assert_eq!(params["body"], "Small");
    h.app.run(command, params.clone()).unwrap();
    let state = h.app.session.model.state();
    let volume = |n: &str| solidcraft_engine::kernel::measure(&state.body(n).unwrap().body).unwrap().volume;
    // Small grows to 10 × 12 × 5; Big keeps 40 × 30 × 20.
    assert!((volume("Small") - 600.0).abs() < 1e-6, "{}", volume("Small"));
    assert!((volume("Big") - 24000.0).abs() < 1e-6, "{}", volume("Big"));
}

/// Mouse hits on curved faces are triangle points. The dialog keeps their face identity so
/// neither name capture nor evaluation asks the approximate point to pick another face.
#[test]
fn offset_face_dialog_accepts_a_curved_display_triangle_hit() {
    use solidcraft_engine::doc::naming;
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Cylinder"})).unwrap();
    let state = crate::open_meshing::session_world(&h.app.session);
    let body = state.body("Cylinder").unwrap();
    let index = body.body.face_at_point(Vec3::new(10.0, 0.0, 10.0)).unwrap().unwrap();
    let name = naming::face_names(body)[index].clone();
    let mesh = body.mesh();
    let triangle = mesh.triangles.iter().zip(&mesh.tri_face).find_map(|(t, f)| if *f as usize == index { mesh.tri(t) } else { None }).unwrap();
    let point = Vec3::from(annular_samples::triangle_centroid(triangle.map(|p| [p.x, p.y, p.z])));
    assert_eq!(body.body.face_at_point(point).unwrap(), None, "the display hit is beyond modelling tolerance");
    h.app.session.selection = vec![Sel::Face { body: "Cylinder".into(), index, point }];
    let mut d = Dialog::for_command(&h.app, "solid.offset_face").unwrap();
    d.kind = Kind::OffsetFaces { distance: "2".into() };
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(preview_commands(&h.app, &d).unwrap(), commands);
    let (command, params) = &commands[0];
    assert_eq!(params["faces"], json!([name]));
    assert_eq!(params["body"], "Cylinder");
    h.app.run(command, params.clone()).unwrap();
    let state = h.app.session.model.state();
    let body = &state.body("Cylinder").unwrap().body;
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    assert!((0..body.face_count()).any(|f| matches!(
        solidcraft_engine::kernel::face_surface(body, f),
        Some(solidcraft_engine::kernel::FaceSurface::Cylinder { radius, .. }) if (radius - 12.0).abs() < 1e-6
    )));
}

#[test]
fn transform_face_accepts_a_curved_display_pick_and_disables_ok_for_invalid_fields() {
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius":10,"height":20,"body_name":"Cylinder"})).unwrap();
    h.app.start("solid.transform_face");
    h.frames(3);
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "a face is required");
    let state = crate::open_meshing::session_world(&h.app.session);
    let body = state.body("Cylinder").unwrap();
    let index = body.body.face_at_point(Vec3::new(10.0, 0.0, 10.0)).unwrap().unwrap();
    let mesh = body.mesh();
    let triangle = mesh.triangles.iter().zip(&mesh.tri_face).find_map(|(t, f)| if *f as usize == index { mesh.tri(t) } else { None }).unwrap();
    let point = Vec3::from(annular_samples::triangle_centroid(triangle.map(|p| [p.x, p.y, p.z])));
    assert_eq!(body.body.face_at_point(point).unwrap(), None);
    let d = h.app.dialog.as_mut().unwrap();
    d.pick(&h.app.session, Sel::Face { body: "Cylinder".into(), index, point });
    h.frames(3);
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0), "a curved pick and explicit zero motion are valid");
    let base = h.app.dialog.as_ref().unwrap().clone();
    let cmds = apply_commands(&h.app, &base).unwrap();
    assert_eq!(cmds[0].1["body"], "Cylinder");
    assert!(cmds[0].1.get("origin").is_none(), "the default pivot follows the picked face");
    h.app.session.preview(&cmds).unwrap();
    for invalid in ["empty length", "angle dimension", "zero axis", "pivot length"] {
        let mut d = base.clone();
        let Kind::TransformFace { translate, angle, axis, custom_axis, explicit_pivot, origin } = &mut d.kind else { panic!("expected face form") };
        match invalid {
            "empty length" => translate[1].clear(),
            "angle dimension" => *angle = "5 mm".into(),
            "zero axis" => {
                *axis = 3;
                *custom_axis = ["0"; 3].map(String::from);
            }
            _ => {
                *explicit_pivot = true;
                origin[0] = "bad_length".into();
            }
        }
        h.app.dialog = Some(d);
        h.frames(3);
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "{invalid}");
    }
}

#[test]
fn draft_rejects_faces_from_different_bodies() {
    let h = Harness::new();
    let mut d = Dialog::for_command(&h.app, "solid.draft").unwrap();
    d.inputs[0].items =
        vec![Sel::Face { body: "First".into(), index: 0, point: Vec3::ZERO }, Sel::Face { body: "Second".into(), index: 0, point: Vec3::X }];
    d.inputs[1].items = vec![Sel::Plane { name: "XY".into() }];
    assert_eq!(apply_commands(&h.app, &d).unwrap_err(), "pick faces of one body");
}

#[test]
fn solid_pattern_quantity_arrows_step_the_count_the_command_receives() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"copies","expression":"3","unit":""})).unwrap();
    for (command, key, label) in [
        ("solid.pattern.circular", "count", "Quantity"),
        ("solid.pattern.rectangular", "count1", "Quantity"),
        ("solid.pattern.path", "count", "Quantity"),
    ] {
        let mut d = representative(&h, command);
        if command == "solid.pattern.rectangular" {
            d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
        }
        h.app.dialog = Some(d);
        h.settle();
        let sent = |h: &Harness| {
            let cmds = crate::dialogs::preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_or_else(|e| panic!("{command}: {e}"));
            cmds.iter().find(|(id, _)| id == command).map(|(_, p)| p[key].clone()).unwrap_or_else(|| panic!("{command}: {cmds:?}"))
        };
        let before = sent(&h).as_str().unwrap().parse::<i64>().unwrap();
        h.step(&json!({"click":{"handle":format!("dialog:{label} up")}})).unwrap();
        assert_eq!(sent(&h), json!((before + 1).to_string()), "{command}");
        let set = |h: &mut Harness, text: &str| {
            if let Some(Kind::PatternRect { count, .. } | Kind::PatternCirc { count, .. } | Kind::PathPattern { count, .. }) =
                h.app.dialog.as_mut().map(|d| &mut d.kind)
            {
                *count = text.into();
            }
            h.frames(2);
        };
        // From 1 to the document's 1000 instances.
        set(&mut h, "1");
        h.step(&json!({"click":{"handle":format!("dialog:{label} down")}})).unwrap();
        assert_eq!(sent(&h), json!("1"), "{command}");
        set(&mut h, "1000");
        h.step(&json!({"click":{"handle":format!("dialog:{label} up")}})).unwrap();
        assert_eq!(sent(&h), json!("1000"), "{command}");
        set(&mut h, "copies");
        h.step(&json!({"click":{"handle":format!("dialog:{label} up")}})).unwrap();
        assert_eq!(sent(&h), json!("copies + 1"), "{command}");
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.settle();
    }
}

/// #6054: the Pattern on Path dialog sends its start point (a distance, or a fraction of the
/// path's length), Orientation and Flip, and editing the pattern opens it with them again.
#[test]
fn path_pattern_dialog_sends_the_start_point_orientation_and_flip_and_reopens_with_them() {
    let mut h = fixture();
    let mut d = representative(&h, "solid.pattern.path");
    // The pattern's parameters, also when an edit sends them through timeline.redefine.
    fn sent(app: &SolveApp, d: &Dialog) -> Value {
        let commands = apply_commands(app, d).unwrap();
        let (id, p) = commands.first().cloned().unwrap();
        if id == "timeline.redefine" { p["params"].clone() } else { p }
    }
    fn set(d: &mut Dialog, to: (&str, &str, &str, bool)) {
        if let Kind::PathPattern { count, spacing, start, fraction } = &mut d.kind {
            (*count, *spacing, *start, *fraction) = (to.0.into(), to.1.into(), to.2.into(), to.3);
        }
    }
    // By default none of them is sent: the command's defaults keep today's result.
    let p = sent(&h.app, &d);
    for key in ["start", "start_fraction", "orientation", "flip"] {
        assert!(p.get(key).is_none(), "{key}: {p}");
    }
    set(&mut d, ("2", "5 mm", "10 mm", false));
    d.extra.insert("orientation".into(), json!("path"));
    d.extra.insert("flip".into(), json!(true));
    let p = sent(&h.app, &d);
    assert_eq!((&p["start"], &p["orientation"], &p["flip"]), (&json!("10 mm"), &json!("path"), &json!(true)), "{p}");
    assert!(p.get("start_fraction").is_none(), "{p}");
    set(&mut d, ("2", "5 mm", "0.25", true));
    let p = sent(&h.app, &d);
    assert_eq!(p["start_fraction"], json!("0.25"), "{p}");
    assert!(p.get("start").is_none(), "{p}");

    h.app.run("solid.pattern.path", p.clone()).unwrap();
    let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
    let reopened = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    assert!(
        matches!(&reopened.kind, Kind::PathPattern { count, spacing, start, fraction: true } if count == "2" && spacing == "5 mm" && start == "0.25"),
        "{:?}",
        reopened.kind
    );
    let again = sent(&h.app, &reopened);
    for key in ["path", "path_sketch", "count", "spacing", "start_fraction", "orientation", "flip"] {
        assert_eq!(again[key], p[key], "{key}: {again} vs {p}");
    }
}

/// A Pattern on Path saved before the rotation-minimising frame (no `follow_frame`), opened in
/// its dialog and OK'd unchanged, keeps building what it built: the edit does not give it the
/// frame. Choosing Identical there is a new choice, and gives it.
#[test]
fn path_pattern_dialog_edit_keeps_an_older_designs_orientation() {
    let mut h = fixture();
    let mut d = representative(&h, "solid.pattern.path");
    if let Kind::PathPattern { count, spacing, .. } = &mut d.kind {
        (*count, *spacing) = ("2".into(), "5 mm".into());
    }
    d.extra.insert("orientation".into(), json!("path"));
    let (_, p) = apply_commands(&h.app, &d).unwrap().first().cloned().unwrap();
    h.app.run("solid.pattern.path", p).unwrap();
    let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
    let follows = |app: &SolveApp| match app.session.doc.feature(id).map(|f| &f.kind) {
        Some(FeatureKind::Pattern { pattern: PatternKind::Path { follow_frame, .. }, .. }) => *follow_frame,
        other => panic!("not a path pattern: {other:?}"),
    };
    assert!(follows(&h.app), "a pattern made now follows the frame");
    if let Some(FeatureKind::Pattern { pattern: PatternKind::Path { follow_frame, .. }, .. }) =
        h.app.session.doc_mut().feature_mut(id).map(|f| &mut f.kind)
    {
        *follow_frame = false;
    }
    h.app.session.refresh();
    let ok = |app: &mut SolveApp, orientation: &str| {
        let mut d = crate::dialogs::for_feature(app, id, None).unwrap();
        d.extra.insert("orientation".into(), json!(orientation));
        for (command, p) in apply_commands(app, &d).unwrap() {
            assert_eq!(command, "timeline.redefine");
            app.run(&command, p).unwrap();
        }
    };
    ok(&mut h.app, "path");
    assert!(!follows(&h.app), "OK on the older pattern kept its orientation");
    ok(&mut h.app, "identical");
    assert!(follows(&h.app), "a newly chosen orientation is the one patterns get now");
}

/// The Rectangular Pattern dialog shows each direction as its own group, with its pick, Quantity,
/// Spacing and (for features) Alignment in the same order for both directions, under a heading
/// that separates the two (#9634).
#[test]
fn rectangular_pattern_groups_each_direction_in_the_same_order() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10})).unwrap();
    for features in [false, true] {
        h.app.start("solid.pattern.rectangular");
        h.frames(4);
        if features {
            mirror_object_type(&mut h, "Features");
            h.frames(2);
        }
        let objects = h.app.dialog.as_ref().unwrap().inputs[0].label;
        let group = |n: &str, rows: [&str; 3]| {
            let mut keys = vec![format!("dialog:section:Direction {n}")];
            keys.extend(rows.iter().map(|row| format!("dialog:row:{row}")));
            if features {
                keys.push(format!("dialog:row:{}", if n == "1" { "Alignment" } else { "Alignment 2" }));
            }
            keys
        };
        let mut order = vec![format!("dialog:row:{objects}")];
        order.extend(group("1", ["Direction", "Quantity", "Spacing"]));
        order.extend(group("2", ["Direction 2", "Quantity 2", "Spacing 2"]));
        let tops: Vec<f32> =
            order.iter().map(|key| scenario::rect_of(key).unwrap_or_else(|| panic!("features {features}: {key} is shown")).top()).collect();
        for (pair, keys) in tops.windows(2).zip(order.windows(2)) {
            assert!(pair[0] < pair[1], "features {features}: {} shows above {}", keys[0], keys[1]);
        }
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.settle();
    }
}

fn coil_plane_coordinates(h: &Harness, point: &Value) -> [f64; 3] {
    std::array::from_fn(|k| {
        let value = point.get(k).or_else(|| point.get(["x", "y", "z"][k])).expect("a point or direction coordinate");
        value.as_f64().unwrap_or_else(|| {
            h.app.session.doc.eval(value.as_str().expect("a coordinate expression"), solidcraft_engine::doc::expr::Kind::Length).unwrap()
        })
    })
}

/// Plane picks place a primitive Coil; they are not sketch profiles. Both Browser rows and
/// viewport picks must reach the same preview and the command run by the dialog's OK button.
fn assert_coil_plane_pick(plane: &str, z: f64) {
    for browser in [true, false] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.set_origin_shown(true);
        if plane == "Plane1" {
            h.app.run("construct.plane.offset", json!({"base":"XY", "offset":z, "name":plane})).unwrap();
        }
        h.call("ui.camera", json!({"target":[0,0,z], "distance":150}));
        h.step(&json!({"start":"solid.coil"})).unwrap();
        h.frames(4);
        if browser {
            let row = format!("row:{plane}");
            if scenario::handle_at(&row).is_none() {
                let folder = if plane == "Plane1" { "Construction" } else { "Origin" };
                h.step(&json!({"click":{"handle":format!("fold:{folder}")}})).unwrap();
                h.frames(2);
            }
            h.step(&json!({"click":{"handle":row}})).unwrap();
        } else {
            let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.expect("a visible viewport"));
            let target = if plane == "XY" {
                crate::viewport::origin_plane_target(&h.app, &proj, plane).expect("the visible XY origin plane")
            } else {
                let (_, _, quad) =
                    crate::viewport::construction_quads(&h.app).into_iter().find(|(name, _, _)| name == plane).expect("the construction plane quad");
                let middle = (quad[0] + quad[1] + quad[2] + quad[3]) / 4.0;
                // Quarter centres stay well inside the rendered quad and clear of its axes.
                quad.into_iter()
                    .map(|corner| (middle + corner) / 2.0)
                    .find(|point| {
                        proj.to_screen(*point).is_some_and(|pixel| {
                            matches!(crate::viewport::pick(&h.app, &proj, pixel).first(), Some(crate::viewport::Hit::Plane { name, .. }) if name == plane)
                        })
                    })
                    .expect("an unobscured interior of the construction plane")
            };
            let pixel = proj.to_screen(target).expect("the plane target projects into the view");
            assert!(
                matches!(crate::viewport::pick(&h.app, &proj, pixel).first(), Some(crate::viewport::Hit::Plane { name, .. }) if name == plane),
                "the viewport click must hit {plane} before testing Coil placement"
            );
            h.step(&json!({"click":{"world":[target.x,target.y,target.z]}})).unwrap();
        }
        h.settle();
        let dialog = h.app.dialog.as_ref().expect("Coil remains open after the plane pick");
        let commands = crate::dialogs::preview_commands(&h.app, dialog).unwrap();
        let (command, params) = commands.iter().find(|(id, _)| id == "solid.coil").expect("Coil preview command");
        assert_eq!(command, "solid.coil");
        let base = coil_plane_coordinates(&h, params.get("base").expect("the picked plane places the Coil base"));
        for (value, expected) in base.into_iter().zip([0.0, 0.0, z]) {
            assert!((value - expected).abs() < 1e-9, "{plane}, Browser={browser}: {params}");
        }
        let axis = coil_plane_coordinates(&h, params.get("axis").expect("the picked plane supplies the Coil axis"));
        assert_eq!(axis, [0.0, 0.0, 1.0], "{plane}, Browser={browser}: {params}");
        assert!(params.get("sketch").is_none(), "a placement plane must not become a profile: {params}");
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{plane}, Browser={browser}: OK applies the Coil");
        let measured = h.app.run("inspect.measure", json!({})).unwrap();
        assert_eq!(measured["body_count"], json!(1));
        let body = &measured["bodies"][0];
        assert_eq!(body["closed"], json!(true));
        // Default four turns at pitch 8, centred circular section of diameter 3: the
        // coil extends 1.5 below the base and 32 + 1.5 above it, translated by the pick.
        for (side, expected) in [("min", z - 1.5), ("max", z + 33.5)] {
            let actual = body["bbox"][side][2].as_f64().unwrap();
            assert!((actual - expected).abs() < 1e-6, "{plane}, Browser={browser}: {measured}");
        }
    }
}

#[test]
fn coil_plane_pick_uses_the_xy_origin_plane() {
    assert_coil_plane_pick("XY", 0.0);
}

#[test]
fn coil_plane_pick_uses_an_offset_construction_plane() {
    assert_coil_plane_pick("Plane1", 50.0);
}

/// A stale plane pick is refused by the real Coil command builder in the interface language.
#[test]
fn coil_missing_plane_error_uses_the_interface_language() {
    let h = Harness::new();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.inputs[0].items = vec![Sel::Plane { name: "RemovedCoilPlane".into() }];
    for (language, expected) in
        [("en", "The selected plane is unavailable. Pick a plane again."), ("ja", "選択した平面は使用できません。平面を選び直してください。")]
    {
        let _language = solidcraft_ui_text::scope(language);
        let error = apply_commands(&h.app, &d).unwrap_err();
        assert_eq!(error, expected, "{language}: the Coil command builder localizes its refusal");
        assert_eq!(crate::dialogs::shown_error(&error), expected, "{language}: the dialog keeps the localized refusal");
    }
}

/// Plane placement supplies the Direction shown in the existing field. A later typed direction
/// overrides that normal; clearing the field restores it, for each shared selection entry point.
#[test]
fn coil_plane_direction_shows_its_normal_and_keeps_a_later_typed_direction() {
    for selection in 0..3 {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("construct.plane.offset", json!({"base":"YZ", "offset":50, "name":"Plane1"})).unwrap();
        let plane = Sel::Plane { name: "Plane1".into() };
        let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
        d.extra.insert("axis".into(), json!("Y"));
        match selection {
            0 => d.pick(&h.app.session, plane),
            1 => d.take_box(&h.app.session, vec![plane], false),
            _ => {
                h.app.session.selection = vec![plane];
                d = Dialog::for_command(&h.app, "solid.coil").unwrap();
            }
        }
        assert!(d.extra.get("axis").is_none(), "a new plane replaces the saved direction, selection={selection}");
        h.app.dialog = Some(d);
        h.frames(4);
        let params = |h: &Harness| apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.clone();
        let original = params(&h);
        assert_eq!(coil_plane_coordinates(&h, &original["base"]), [50.0, 0.0, 0.0], "{original}");
        assert_eq!(coil_plane_coordinates(&h, &original["axis"]), [1.0, 0.0, 0.0], "{original}");
        h.step(&json!({"expect":{"shown":json!([1.0,0.0,0.0]).to_string()}})).unwrap();
        let design = h.app.session.doc.to_json();
        h.step(&json!({"click":{"handle":"dialog:ui.direction"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"Z"})).unwrap();
        h.frames(2);
        let typed = params(&h);
        assert_eq!(typed["axis"], json!("Z"), "typing Direction must affect the command: {typed}");
        assert_eq!(coil_plane_coordinates(&h, &typed["base"]), [50.0, 0.0, 0.0], "{typed}");
        h.step(&json!({"click":{"handle":"dialog:ui.direction"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"key":"Backspace"})).unwrap();
        h.frames(2);
        let cleared = params(&h);
        assert_eq!(coil_plane_coordinates(&h, &cleared["axis"]), [1.0, 0.0, 0.0], "clearing restores the plane normal: {cleared}");
        assert_eq!(coil_plane_coordinates(&h, &cleared["base"]), [50.0, 0.0, 0.0], "{cleared}");
        assert_eq!(h.app.session.doc.to_json(), design, "Direction edits change only the preview");
    }
}

/// An explicit Axis pick keeps supplying its own line when the placement input holds a plane.
#[test]
fn coil_axis_pick_keeps_its_origin_and_direction_with_a_plane_selected() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("construct.plane.offset", json!({"base":"XY", "offset":50, "name":"Plane1"})).unwrap();
    h.app.run("construct.axis.two_points", json!({"a":{"point":[20,10,-20]},"b":{"point":[40,10,-20]},"name":"CoilAxis"})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.inputs[0].items = vec![Sel::Plane { name: "Plane1".into() }];
    d.active = 1;
    d.pick(&h.app.session, Sel::Axis { name: "CoilAxis".into() });
    assert_eq!(d.inputs[0].items, vec![Sel::Plane { name: "Plane1".into() }]);
    for commands in [crate::dialogs::preview_commands(&h.app, &d).unwrap(), apply_commands(&h.app, &d).unwrap()] {
        let (_, p) = commands.iter().find(|(id, _)| id == "solid.coil").expect("the Coil command");
        assert_eq!(p["axis"], json!("CoilAxis"), "the explicit axis stays linked: {p}");
        assert_eq!(coil_plane_coordinates(&h, &p["base"]), [20.0, 10.0, -20.0], "{p}");
        assert!(p.get("sketch").is_none(), "the placement plane is not a profile: {p}");
    }
}

/// A newly picked world plane is distinct from an unchanged saved component-local placement,
/// even when both serialise to the same numbers. Preview and OK must use the picked plane.
#[test]
fn editing_a_coil_in_a_moved_component_uses_the_picked_world_plane() {
    use solidcraft_engine::doc::{FeatureKind, apply_point, apply_vector};

    for turned in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        let owner = h.app.run("component.create", json!({"name":"Owner", "grounded":false})).unwrap();
        let component = owner["component"].as_u64().unwrap();
        h.app
            .run(
                "solid.coil",
                json!({"name":"Coil", "body_name":"Spring", "base":[0,0,0], "axis":[0,0,1],
                   "diameter":40, "revolutions":4, "pitch":8, "section_size":3}),
            )
            .unwrap();
        let id = h.app.session.doc.find_feature("Coil").unwrap().id;
        assert_eq!(h.app.session.doc.feature(id).unwrap().component, component, "the Coil belongs to the component being moved");
        h.app.run("component.activate", json!({"component":"root"})).unwrap();
        h.app
            .run(
                "occurrence.move",
                json!({"occurrence":owner["occurrence"], "translate":[10,0,0], "axis":[0,1,0],
                   "angle":if turned { "90 deg" } else { "0 deg" }}),
            )
            .unwrap();
        let frame = h.app.session.placed_document().component_transform(component);
        assert!((apply_point(&frame, Vec3::ZERO) - Vec3::new(10.0, 0.0, 0.0)).len() < 1e-9);
        if turned {
            assert!((apply_vector(&frame, Vec3::Z) - Vec3::X).len() < 1e-9);
        }
        h.app.edit_feature(id);
        h.frames(3);
        let plane = crate::dialogs::plane_by_name(&h.app.session, "XY").expect("the picked world XY plane");
        assert_eq!(plane.origin, Vec3::ZERO, "Edit Feature resolves XY at the world origin");
        assert_eq!(plane.normal(), Vec3::Z, "the picked plane has the world Z normal");
        let saved = h.app.dialog.as_ref().expect("the Coil editor");
        assert_eq!(coil_plane_coordinates(&h, &saved.extra["base"]), [0.0, 0.0, 0.0]);
        assert_eq!(coil_plane_coordinates(&h, &saved.extra["axis"]), [0.0, 0.0, 1.0]);
        {
            let dialog = h.app.dialog.as_mut().expect("the Coil editor");
            assert!(dialog.editing.is_some_and(|(edited, _)| edited == id));
            assert_eq!(dialog.extra["base"].as_array().map(Vec::len), Some(3), "saved placement uses its existing coordinate-list form");
            assert_eq!(dialog.extra["axis"].as_array().map(Vec::len), Some(3));
            dialog.pick(&h.app.session, Sel::Plane { name: "XY".into() });
            assert_eq!(dialog.inputs[0].items, vec![Sel::Plane { name: "XY".into() }]);
        }
        let commands = preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].0, "timeline.redefine");
        let params = &commands[0].1["params"];
        assert_eq!(coil_plane_coordinates(&h, &params["base"]), [0.0, 0.0, 0.0]);
        assert_eq!(coil_plane_coordinates(&h, &params["axis"]), [0.0, 0.0, 1.0]);
        let before = h.app.session.doc.to_json();
        let preview = h.app.session.preview_world(&commands).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before, "preview preserves the design");
        assert_eq!(preview.after.bodies.len(), 1);
        let bounds = preview.after.bodies[0].mesh().bounds();
        assert!(bounds.center().x.abs() < 1e-5 && bounds.center().y.abs() < 1e-5, "world-plane preview, turned={turned}: {bounds:?}");
        assert!((bounds.min.z + 1.5).abs() < 1e-5 && (bounds.max.z - 33.5).abs() < 1e-5, "world Z preview, turned={turned}: {bounds:?}");
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "OK applies the Coil edit");
        let FeatureKind::Coil { base, axis, .. } = &h.app.session.doc.feature(id).unwrap().kind else { panic!("the edited Coil") };
        let local = h.app.session.doc.eval_point(base).unwrap();
        let frame = h.app.session.placed_document().component_transform(component);
        assert!(apply_point(&frame, local).len() < 1e-9, "the applied base lies at world XY, turned={turned}");
        let local_axis = axis.each_ref().map(|component| h.app.session.doc.eval(component, solidcraft_engine::doc::expr::Kind::Unitless).unwrap());
        let local_axis = solidcraft_engine::doc::unit_direction(Vec3::from(local_axis)).unwrap();
        assert!((apply_vector(&frame, local_axis) - Vec3::Z).len() < 1e-9, "the applied axis follows world XY, turned={turned}");
        assert_eq!(h.app.session.world_state().unwrap().bodies[0].mesh().bounds(), bounds, "OK matches the actual world preview");
    }
}

/// Each direction's Distance Type (#8523): the dialog shows the rows that type takes and sends
/// only the values it uses (Quantity and Extent: Quantity and Extent; Spacing and Extent:
/// Spacing, Extent and Copy at the end), and a pattern made with them opens with them again.
#[test]
fn rectangular_pattern_distance_type_sends_what_it_uses_and_reopens() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Block"})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.pattern.rectangular").unwrap();
    d.inputs[0].items = vec![Sel::Body { name: "Block".into() }];
    d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
    d.inputs[2].items = vec![Sel::Axis { name: "Y".into() }];
    d.extra.insert("distance1".into(), json!("count_extent"));
    d.extra.insert("distance2".into(), json!("spacing_extent"));
    d.extra.insert("copy_at_end2".into(), json!(true));
    if let Kind::PatternRect { count, extent, spacing2, extent2, .. } = &mut d.kind {
        *count = "5".into();
        *extent = "80 mm".into();
        *spacing2 = "15 mm".into();
        *extent2 = "40 mm".into();
    }
    let commands = apply_commands(&h.app, &d).unwrap();
    let (id, p) = &commands[0];
    assert_eq!(id, "solid.pattern.rectangular");
    assert_eq!((&p["distance1"], &p["count1"], &p["extent1"]), (&json!("count_extent"), &json!("5"), &json!("80 mm")), "{p}");
    assert_eq!((&p["distance2"], &p["spacing2"], &p["extent2"]), (&json!("spacing_extent"), &json!("15 mm"), &json!("40 mm")), "{p}");
    assert_eq!(p["copy_at_end2"], json!(true), "{p}");
    assert!(p.get("spacing1").is_none() && p.get("count2").is_none(), "only the values each type uses: {p}");
    h.app.dialog = Some(d);
    h.settle();
    for row in ["ui.pattern_distance_type", "ui.pattern_extent", "ui.pattern_distance_type_2", "ui.pattern_extent_2", "ui.pattern_copy_at_end_2"] {
        assert!(scenario::rect_of(&format!("dialog:row:{row}")).is_some(), "{row} is shown");
    }
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    h.settle();
    // Made and opened again: the same distance types, values and Copy at the end.
    h.app.run(id, p.clone()).unwrap();
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.call("ui.editFeature", json!({"feature": name}));
    h.settle();
    let d = h.app.dialog.as_ref().expect("the pattern opens to edit");
    assert_eq!(d.extra.get("distance1"), Some(&json!("count_extent")));
    assert_eq!(d.extra.get("distance2"), Some(&json!("spacing_extent")));
    assert_eq!(d.extra.get("copy_at_end2"), Some(&json!(true)));
    assert!(
        matches!(&d.kind, Kind::PatternRect { count, extent, spacing2, extent2, .. }
            if count == "5" && extent == "80 mm" && spacing2 == "15 mm" && extent2 == "40 mm"),
        "{:?}",
        d.kind
    );
}

/// Quantity and List: the dialog shows a Spacing List row in place of Spacing and sends
/// the gaps typed between semicolons as a list, each with its unit; a pattern made with them
/// opens with them again, and with no canvas value (a list is no one length to drag).
#[test]
fn rectangular_pattern_spacing_list_sends_its_gaps_and_reopens() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Block"})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.pattern.rectangular").unwrap();
    d.inputs[0].items = vec![Sel::Body { name: "Block".into() }];
    d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
    d.extra.insert("distance1".into(), json!("count_list"));
    if let Kind::PatternRect { count, list, .. } = &mut d.kind {
        *count = "5".into();
        *list = " 50 mm;60;; 70,5 mm ".into();
    }
    assert!(d.primary().is_none(), "no canvas value for a spacing list");
    let commands = apply_commands(&h.app, &d).unwrap();
    let (id, p) = &commands[0];
    assert_eq!(id, "solid.pattern.rectangular");
    assert_eq!(p["distance1"], json!("count_list"), "{p}");
    assert_eq!(p["count1"], json!("5"), "{p}");
    let gaps = p["spacing_list1"].as_array().cloned().unwrap_or_default();
    assert_eq!(gaps.len(), 3, "empty entries are dropped: {p}");
    assert_eq!(gaps[0], json!("50 mm"), "{p}");
    assert!(p.get("spacing1").is_none() && p.get("extent1").is_none(), "only the values the type uses: {p}");
    h.app.dialog = Some(d);
    h.settle();
    assert!(scenario::rect_of("dialog:row:ui.pattern_spacing_list").is_some(), "the Spacing List row is shown");
    assert!(scenario::rect_of("dialog:row:Spacing").is_none(), "no Spacing row");
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    h.settle();
    h.app.run(id, p.clone()).unwrap();
    assert_eq!(h.app.session.model.state().bodies.len(), 5);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.call("ui.editFeature", json!({"feature": name}));
    h.settle();
    let d = h.app.dialog.as_ref().expect("the pattern opens to edit");
    assert_eq!(d.extra.get("distance1"), Some(&json!("count_list")));
    let Kind::PatternRect { list, .. } = &d.kind else { panic!("{:?}", d.kind) };
    assert_eq!(list.split(';').count(), 3, "{list}");
    assert!(list.starts_with("50 mm; "), "{list}");
}

#[test]
fn coil_axis_field_passes_named_and_custom_directions_with_a_profile() {
    let mut h = fixture();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    let sketch = crate::open_meshing::session_world(&h.app.session).sketches[0].feature;
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    h.app.dialog = Some(d);
    h.frames(3);
    let params = |h: &Harness| apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.clone();
    assert!(params(&h).get("axis").is_none(), "the command defaults to Z");
    for (text, expected) in [("X", json!("X")), ("Y", json!("Y")), ("Z", json!("Z")), ("[1, 2, 3]", json!([1, 2, 3]))] {
        h.step(&json!({"click":{"handle":"dialog:ui.direction"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":text})).unwrap();
        h.frames(2);
        let p = params(&h);
        assert_eq!(p["axis"], expected);
        assert_eq!(p["sketch"], json!(sketch));
        assert_eq!(p["profiles"], json!([0]));
    }
}

#[test]
fn coil_profile_stays_active_after_a_profile_pick() {
    let h = fixture();
    let sketch = crate::open_meshing::session_world(&h.app.session).sketches[0].feature;
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.active = 0;
    d.pick(&h.app.session, Sel::Profile { sketch, index: 0 });
    assert_eq!(d.active, 0, "the optional Axis is not visited, so the next profile click replaces this one");
    d.pick(&h.app.session, Sel::Profile { sketch, index: 0 });
    assert_eq!(d.inputs[0].items, vec![Sel::Profile { sketch, index: 0 }]);
    assert!(d.inputs[1].items.is_empty());
}

#[test]
fn coil_picks_reference_axes_in_the_browser_and_viewport() {
    for browser in [true, false] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.placement_handles = true;
        h.app.tree.collapsed.insert("c0/features".into());
        h.app.run("construct.axis.two_points", json!({"a":{"point":[20,10,-20]},"b":{"point":[20,10,20]},"name":"CoilAxis"})).unwrap();
        h.app.start("solid.coil");
        h.call("ui.dialogInput", json!({"index":1}));
        h.frames(4);
        let target = if browser { json!({"handle":"row:CoilAxis"}) } else { json!({"world":[20,10,-20]}) };
        h.step(&json!({"click":target})).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[1].items, vec![Sel::Axis { name: "CoilAxis".into() }], "browser: {browser}");
        let commands = apply_commands(&h.app, d).unwrap();
        assert_eq!(commands[0].1["axis"], "CoilAxis");
        assert_eq!(commands[0].1["base"], json!([20.0, 10.0, -20.0]));
        h.call("ui.dialogInput", json!({"index":0}));
        h.frames(2);
        assert!(crate::scenario::handle_at("primitive:position").is_some(), "an axis pick is not a sketch profile");
    }
}

#[test]
fn coil_keeps_the_selected_edge_and_occurrence_when_the_occurrence_moves() {
    let mut h = Harness::new();
    let component = h.app.run("component.create", json!({"name":"Block"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Box"})).unwrap();
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    let mut frame = solidcraft_engine::doc::IDENTITY;
    frame[3][0] = 150.0;
    let occurrence = h.app.session.doc_mut().add_occurrence(component, 0, frame).unwrap();
    let state = h.app.session.model.state();
    let world_body = h.app.session.doc.world_body_name(&state, "Box", component, &[occurrence]);
    let body = state.body("Box").unwrap();
    // The corner also lies on a horizontal edge that appears earlier than the named vertical edge.
    let mesh = body.mesh();
    let mut incident = mesh.edges.iter().enumerate().filter(|(_, points)| points.contains(&Vec3::ZERO));
    let (first, _) = incident.next().unwrap();
    let (index, _) = incident.find(|(_, points)| points.contains(&Vec3::new(0., 0., 10.))).unwrap();
    assert!(index > first);
    let axial_span = |body: &solidcraft_engine::doc::ModelBody| {
        let bounds = body.body.exact_bounds().unwrap();
        bounds.max.z - bounds.min.z
    };
    for (point, coil_name) in [(Vec3::new(150., 0., 5.), "Middle Coil"), (Vec3::new(150., 0., 0.), "Corner Coil")] {
        let edge = Sel::Edge { body: world_body.clone(), index, point };
        let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
        d.active = 1;
        d.pick(&h.app.session, edge);
        let mut commands = apply_commands(&h.app, &d).unwrap();
        let (command, params) = &mut commands[0];
        let axis = params["axis"].clone();
        assert_eq!(axis["type"], "placed_edge");
        assert_eq!(axis["body"], "Box");
        assert_eq!(axis["path"], json!([occurrence]));
        assert!(axis["name"].as_str().is_some_and(|name| !name.is_empty()), "{axis}");
        params["body_name"] = json!(coil_name);
        params["diameter"] = json!(40);
        params["section_size"] = json!(2);
        params["pitch"] = json!(8);
        params["revolutions"] = json!(2);
        let coil = h.app.run(command, params.clone()).unwrap()["feature"].as_u64().unwrap();

        let state = h.app.session.model.state();
        let body = state.body(coil_name).unwrap();
        assert!((axial_span(body) - 18.).abs() < 1e-6);
        assert!(body.body.exact_bounds().unwrap().min.x > 100.);
        let d = for_feature(&h.app, coil, None).unwrap();
        assert!(matches!(d.inputs[1].items.as_slice(), [Sel::Edge { body, index: i, .. }] if body == &world_body && *i == index));
        assert_eq!(apply_commands(&h.app, &d).unwrap()[0].1["params"]["axis"], axis);
    }

    let name = solidcraft_engine::doc::naming::edge_names(body)[index].clone();
    let legacy = h
        .app
        .run(
            "solid.coil",
            json!({
                "axis": {"type":"edge","at":[150,0,0],"name":name},
                "diameter":40,"section_size":2,"pitch":8,"revolutions":2,"body_name":"Legacy Coil"
            }),
        )
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let state = h.app.session.model.state();
    let legacy_body = state.body("Legacy Coil").unwrap();
    assert!((axial_span(legacy_body) - 18.).abs() < 1e-6);
    assert!(legacy_body.body.exact_bounds().unwrap().min.x > 100.);
    let d = for_feature(&h.app, legacy, None).unwrap();
    assert!(matches!(d.inputs[1].items.as_slice(), [Sel::Edge { body, index: i, .. }] if body == &world_body && *i == index));

    frame[3][0] += 100.0;
    h.app.session.doc_mut().occurrences.iter_mut().find(|item| item.id == occurrence).unwrap().transform = frame;
    h.app.session.refresh();
    let state = h.app.session.model.state();
    for name in ["Middle Coil", "Corner Coil"] {
        let body = state.body(name).unwrap();
        assert!((axial_span(body) - 18.).abs() < 1e-6);
        assert!(body.body.exact_bounds().unwrap().min.x > 200.);
    }

    h.app.run("component.activate", json!({"component":component,"occurrence":occurrence,"path":[occurrence]})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.active = 1;
    d.pick(&h.app.session, Sel::Edge { body: world_body, index, point: Vec3::new(250., 0., 0.) });
    let mut commands = apply_commands(&h.app, &d).unwrap();
    let (command, params) = &mut commands[0];
    assert_eq!(params["axis"]["author_path"], json!([occurrence]));
    params["body_name"] = json!("Component Coil");
    h.app.run(command, params.clone()).unwrap();
    let state = h.app.session.model.state();
    let coil = state.body("Component Coil").unwrap();
    assert_eq!(coil.component, component);
    assert!(coil.body.exact_bounds().unwrap().min.x.abs() < 80.);
}

#[test]
fn coil_uses_the_world_line_of_a_straight_3d_wire() {
    let mut h = Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    let wire = h.app.run("sketch.line3d", json!({"points":[[5,7,3],[5,7,13]]})).unwrap()["curves"][0].as_str().unwrap().to_owned();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.active = 1;
    d.pick(&h.app.session, Sel::SketchCurve { id: wire, sketch: Some(sketch) });
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1["axis"], json!({"origin":[5.0,7.0,3.0],"dir":[0.0,0.0,1.0]}));
    h.app.run(&commands[0].0, commands[0].1.clone()).unwrap();
}

#[test]
fn coil_requires_an_explicit_profile_when_the_sketch_has_several_regions() {
    let (mut h, sketch, _) = repeated_profile_fixture();
    h.app.session.selection = vec![Sel::Feature { id: sketch }];
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    assert!(!d.pick_sketch(&h.app.session, sketch));
    d.take_sketch_profile(&h.app.session, sketch);
    assert!(d.inputs[0].items.is_empty());

    d.pick(&h.app.session, Sel::Profile { sketch, index: 1 });
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1["sketch"], sketch);
    assert_eq!(commands[0].1["profiles"], json!([1]));
}

#[test]
fn coil_edits_keep_whole_sketch_profiles_until_the_user_picks_them_explicitly() {
    let mut h = Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[9,-0.5],"p1":[10,0.5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let feature =
        h.app.run("solid.coil", json!({"sketch":sketch,"profiles":"all","axis":"Y","pitch":8,"revolutions":2})).unwrap()["feature"].as_u64().unwrap();
    let s = &h.app.session;
    let profile = Sel::Profile { sketch, index: 0 };
    let edit_profiles = |d: &Dialog| apply_commands(&h.app, d).unwrap()[0].1["params"]["profiles"].clone();
    let mut d = for_feature(&h.app, feature, None).unwrap();
    assert_eq!(edit_profiles(&d), json!("all"));

    d.active = 0;
    d.pick(s, profile.clone());
    assert_eq!(edit_profiles(&d), json!([0]));

    let mut d = for_feature(&h.app, feature, None).unwrap();
    d.active = 0;
    d.take_box(s, vec![profile.clone()], false);
    assert_eq!(edit_profiles(&d), json!([0]));

    let mut d = for_feature(&h.app, feature, None).unwrap();
    d.remove_listed(s, 0, 0, &profile);
    assert!(d.profile_selection_changed);
}

#[test]
fn coil_edits_keep_the_expressions_of_an_axis_origin() {
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name":"axis_offset","expression":"20 mm"})).unwrap();
    let axis = json!({"origin":["axis_offset", 0, 0], "dir":[0, 0, 1]});
    let feature =
        h.app.run("solid.coil", json!({"axis":axis,"diameter":10,"pitch":8,"revolutions":2,"section_size":2})).unwrap()["feature"].as_u64().unwrap();
    let d = for_feature(&h.app, feature, None).unwrap();
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1["params"]["axis"]["origin"][0], json!("axis_offset"), "{}", commands[0].1);
    for (cmd, params) in commands {
        h.app.run(&cmd, params).unwrap();
    }
    h.app.run("parameters.change", json!({"name":"axis_offset","expression":"30 mm"})).unwrap();
    let f = h.app.session.doc.feature(feature).unwrap();
    let FeatureKind::Coil { axis_ref: Some(AxisRef::Line { origin, .. }), .. } = &f.kind else { panic!("a Coil about a line: {:?}", f.kind) };
    assert_eq!(h.app.session.doc.eval_point(origin).unwrap(), Vec3::new(30.0, 0.0, 0.0));
}

#[test]
fn coil_keeps_face_placement_until_the_user_picks_an_axis() {
    use solidcraft_engine::doc::naming;
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius": 5, "height": 10, "body_name": "Pin"})).unwrap();
    let point = Vec3::new(5.0, 0.0, 4.0);
    let world = crate::open_meshing::session_world(&h.app.session);
    let body = world.body("Pin").unwrap();
    let name = naming::face_name_at_body(body, point).unwrap();
    let (index, _) = naming::face_on_body(body, &name, point).unwrap();
    let pick = Sel::Face { body: "Pin".into(), index, point };
    let (origin, _) = axis_of(&h.app, &pick).unwrap();
    assert_ne!(origin, point);
    h.app.session.selection = vec![pick.clone()];
    let d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    assert!(d.inputs[1].items.is_empty());
    assert_eq!(apply_commands(&h.app, &d).unwrap()[0].1["base"], pt(point));

    let mut d = d;
    d.active = 1;
    d.pick(&h.app.session, pick.clone());
    assert_eq!(d.inputs[1].items, vec![pick]);
    assert_eq!(apply_commands(&h.app, &d).unwrap()[0].1["base"], pt(origin));
}

/// Coil from an end face: with the Axis field active, a click on a shaft's flat top takes the face
/// itself (not a placement on it), fills Diameter with its round edge's, and the coil cut from
/// there grooves the shaft's wall with the triangle section.
#[test]
fn coil_starts_from_a_picked_end_face_and_takes_its_diameter() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.cylinder", json!({"radius": 20, "height": 40, "body_name": "Shaft"})).unwrap();
    h.app.start("solid.coil");
    h.call("ui.dialogInput", json!({"index":1}));
    h.frames(4);
    h.step(&json!({"click":{"world":[5, 5, 40]}})).unwrap();
    let d = h.app.dialog.as_mut().unwrap();
    assert!(matches!(d.inputs[1].items.as_slice(), [Sel::Face { body, .. }] if body == "Shaft"), "{:?}", d.inputs[1].items);
    let Kind::Primitive { fields, .. } = &d.kind else { panic!("a coil dialog") };
    assert_eq!(fields.iter().find(|(k, _)| *k == "diameter").map(|(_, v)| v.as_str()), Some("40 mm"));
    d.extra.insert("section".into(), json!("triangle"));
    d.extra.insert("reversed".into(), json!(true));
    if let Kind::Primitive { operation, .. } = &mut d.kind {
        *operation = OPS.iter().position(|o| *o == "cut").unwrap();
    }
    let d = h.app.dialog.as_ref().unwrap();
    let commands = apply_commands(&h.app, d).unwrap();
    let axis = &commands[0].1["axis"];
    assert!(axis.get("face").is_some() || axis["type"] == "face", "the face is the reference: {axis}");
    let before = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    h.app.run(&commands[0].0, commands[0].1.clone()).unwrap();
    let after = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!(after < before, "the triangle groove cuts the shaft: {before} -> {after}");
}

#[test]
fn coil_picks_origin_axes_and_reverses_without_changing_clockwise() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.start("solid.coil");
    h.app.set_origin_shown(true);
    h.call("ui.dialogInput", json!({"index":1}));
    h.frames(4);
    assert!(h.app.origin_visible());
    h.step(&json!({"click":{"axis":"X"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items, vec![Sel::Axis { name: "X".into() }]);
    h.step(&json!({"click":{"handle":"dialog:Reverse Direction"}})).unwrap();
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["axis"], json!({"type":"world","axis":"X"}));
    assert_eq!(commands[0].1["reversed"], true);
    assert_ne!(commands[0].1.get("clockwise"), Some(&json!(true)));
}

#[test]
fn coil_and_revolve_share_axis_pick_filters_and_origin_visibility() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for command in ["solid.revolve", "solid.coil"] {
        h.app.dialog = Some(Dialog::for_command(&h.app, command).unwrap());
        h.app.dialog.as_mut().unwrap().active = 1;
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].accept, AXIS_PICKS);
        h.app.set_origin_shown(false);
        assert!(!h.app.origin_visible(), "{command} respects a hidden origin");
        h.app.set_origin_shown(true);
        assert!(h.app.origin_visible(), "{command} respects a shown origin");
    }
}

#[test]
fn coil_and_revolve_keep_the_owner_of_a_picked_sketch_line() {
    let h = fixture();
    let state = h.app.session.world_state().unwrap();
    let profile = state.sketches[0].feature;
    let owner = state.sketches.last().unwrap();
    let line = owner.sketch.curves[0].id.clone();
    for command in ["solid.revolve", "solid.coil"] {
        let mut d = Dialog::for_command(&h.app, command).unwrap();
        d.inputs[0].items = vec![Sel::Profile { sketch: profile, index: 0 }];
        d.inputs[1].items = vec![Sel::SketchCurve { sketch: Some(owner.feature), id: line.clone() }];
        let commands = apply_commands(&h.app, &d).unwrap();
        let params = &commands[0].1;
        assert_eq!(params["axis"], json!({"sketch":owner.feature,"curve":line}), "{command}");
    }
}

#[test]
fn coil_and_revolve_keep_the_owner_of_a_collinear_axis() {
    let mut h = Harness::new();
    let owner = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.line", json!({"points":[[0,10],[0,0]],"construction":true})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let profile = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,10]],"construction":true})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[4,4]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    for command in ["solid.coil", "solid.revolve"] {
        let mut d = Dialog::for_command(&h.app, command).unwrap();
        d.inputs[0].items = vec![Sel::Profile { sketch: profile, index: 0 }];
        for (sketch, expected) in [(owner, json!({"sketch":owner,"curve":"l1"})), (profile, json!("l1"))] {
            d.inputs[1].items = vec![Sel::SketchCurve { sketch: Some(sketch), id: "l1".into() }];
            let commands = apply_commands(&h.app, &d).unwrap();
            assert_eq!(commands[0].1["axis"], expected, "{command}");
        }
    }
}

/// Two cylinders touch along a line. A round face picked on that line is the face that was
/// clicked: the point alone names the first body's face there (#20586).
#[test]
fn a_round_face_picked_as_an_axis_where_two_faces_touch_is_the_clicked_face() {
    use solidcraft_engine::doc::naming;
    let mut h = Harness::new();
    for (name, x) in [("First", 0), ("Second", 10)] {
        h.app.run("solid.cylinder", json!({"base":[x,0,0],"radius":5,"height":10,"operation":"new","body_name":name})).unwrap();
    }
    // Far from both cylinders, so the revolve makes a body of its own about either axis.
    let profile = h.app.run("sketch.create", json!({"plane":"XZ"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[40,1],"p1":[42,3]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let touch = Vec3::new(5.0, 0.0, 5.0);
    let state = crate::open_meshing::session_world(&h.app.session);
    let second = state.body("Second").unwrap();
    let index = second.body.face_at_point(touch).unwrap().unwrap();
    let name = naming::face_names(second)[index].clone();
    assert!(state.body("First").unwrap().body.face_at_point(touch).unwrap().is_some(), "both side faces hold the pick");
    let mut d = Dialog::for_command(&h.app, "solid.revolve").unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch: profile, index: 0 }];
    d.inputs[1].items = vec![Sel::Face { body: "Second".into(), index, point: touch }];
    let commands = apply_commands(&h.app, &d).unwrap();
    let axis = commands[0].1["axis"].clone();
    let bodies = h.app.session.model.state().bodies.len();
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    let state = h.app.session.model.state();
    assert_eq!(state.bodies.len(), bodies + 1, "the revolve is a body of its own");
    let ring = state.bodies.iter().find(|b| !["First", "Second"].contains(&b.name.as_str())).unwrap();
    let bounds = ring.body.exact_bounds().unwrap();
    let centre = (bounds.min + bounds.max) * 0.5;
    assert!((centre.x - 10.0).abs() < 1e-6 && centre.y.abs() < 1e-6, "turned about the second cylinder's axis: {centre:?}");
    assert_eq!(axis["name"], json!(name), "the axis names the clicked face: {axis}");
}

#[test]
fn coil_from_a_profile_hides_the_primitive_section() {
    let mut h = fixture();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    let sketch = crate::open_meshing::session_world(&h.app.session).sketches[0].feature;
    let primitive_options = [("section", json!("square")), ("section_position", json!("outside")), ("start_angle", json!("30 deg"))];
    for (key, value) in &primitive_options {
        d.extra.insert((*key).into(), value.clone());
    }
    d.extra.insert("clockwise".into(), json!(true));
    d.extra.insert("reversed".into(), json!(true));
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    let commands = apply_commands(&h.app, &d).unwrap();
    let (id, p) = &commands[0];
    assert_eq!(id, "solid.coil");
    assert_eq!(p["sketch"], json!(sketch));
    assert_eq!(p["profiles"], json!([0]));
    for key in ["diameter", "section_size", "section", "section_position", "start_angle"] {
        assert!(p.get(key).is_none(), "{key}: {p}");
    }
    assert!(p.get("revolutions").is_some() && p.get("pitch").is_some(), "{p}");
    assert!(p.get("operation").is_some(), "{p}");
    assert_eq!((p["clockwise"].as_bool(), p["reversed"].as_bool()), (Some(true), Some(true)));
    // The rows that would change nothing are not shown; the axis and both directions are.
    let section_rows = ["Section", "Section position", "Start angle"];
    h.app.dialog = Some(d);
    h.frames(2);
    for row in section_rows {
        assert!(scenario::rect_of(&format!("dialog:row:{row}")).is_none(), "{row} is hidden with a profile");
    }
    assert!(scenario::rect_of("dialog:row:ui.direction").is_some());
    for option in ["Clockwise", "Reverse Direction"] {
        assert!(scenario::handle_at(&format!("dialog:{option}")).is_some(), "{option} stays with a profile");
    }

    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    h.frames(2);
    for row in section_rows {
        assert!(scenario::rect_of(&format!("dialog:row:{row}")).is_some(), "{row} is back without a profile");
    }
    let d = h.app.dialog.as_ref().unwrap();
    let commands = apply_commands(&h.app, d).unwrap();
    let p = &commands[0].1;
    assert!(p.get("sketch").is_none(), "{p}");
    assert!(p.get("diameter").is_some() && p.get("section_size").is_some(), "{p}");
    for (key, value) in primitive_options {
        assert_eq!(p[key], value);
    }
}

/// A coil from a sketch profile takes its section from the sketch, so the dialog shows neither the
/// Section rows nor Start angle and never sends them, even remembered from an earlier coil
/// (#17870): `solid.coil` refuses them with `sketch`.
#[test]
fn coil_from_a_profile_hides_and_never_sends_the_section_options() {
    let mut h = fixture();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.extra.insert("section".into(), json!("square"));
    d.extra.insert("section_position".into(), json!("outside"));
    d.extra.insert("start_angle".into(), json!("30 deg"));
    // The profile's sketch is on XY, so the axis lies in its plane (the default axis is across it).
    d.extra.insert("axis".into(), json!([0.0, 1.0, 0.0]));
    h.app.dialog = Some(d.clone());
    h.frames(2);
    for label in ["Section position", "Start angle"] {
        h.step(&json!({"expect":{"shown": label}})).unwrap_or_else(|e| panic!("without a profile: {e}"));
    }
    let sketch = crate::open_meshing::session_world(&h.app.session).sketches[0].feature;
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    h.app.dialog = Some(d.clone());
    h.frames(2);
    for label in ["Section position", "Start angle"] {
        h.step(&json!({"expect":{"not_shown": label}})).unwrap_or_else(|e| panic!("with a profile: {e}"));
    }
    h.step(&json!({"expect":{"shown": "Clockwise"}})).unwrap();
    let commands = apply_commands(&h.app, &d).unwrap();
    let (id, p) = &commands[0];
    assert_eq!(id, "solid.coil");
    for key in ["section", "section_position", "start_angle"] {
        assert!(p.get(key).is_none(), "{key} sent with a sketch profile: {p}");
    }
    h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{e}: {p}"));
}

/// Reverse Direction winds the coil the other way along its axis (#17870), down from a sketch
/// profile too. The axis is sent as written with `reversed`, so a picked axis stays linked.
#[test]
fn coil_reverse_direction_winds_the_coil_down_its_axis() {
    use solidcraft_engine::doc::FeatureKind;
    let mut h = fixture();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.extra.insert("reversed".into(), json!(true));
    d.extra.insert("axis".into(), json!([0.0, 1.0, 0.0]));
    let (_, p) = apply_commands(&h.app, &d).unwrap().remove(0);
    assert_eq!((&p["axis"], &p["reversed"]), (&json!([0.0, 1.0, 0.0]), &json!(true)), "{p}");
    // The profile's sketch is on XY, so the axis [0, 1, 0] lies in its plane, clear of the profile; it stays set.
    let sketch = crate::open_meshing::session_world(&h.app.session).sketches[0].feature;
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    h.app.dialog = Some(d.clone());
    h.frames(2);
    h.step(&json!({"expect":{"shown": "Reverse Direction"}})).unwrap();
    let (id, p) = apply_commands(&h.app, &d).unwrap().remove(0);
    h.app.run(&id, p.clone()).unwrap_or_else(|e| panic!("{e}: {p}"));
    let reversed = h.app.session.doc.features.iter().rev().find_map(|f| match &f.kind {
        FeatureKind::Coil { reversed, .. } => Some(*reversed),
        _ => None,
    });
    assert_eq!(reversed, Some(true), "the coil winds down");
}

/// Reverse Direction keeps an axis of expressions (#21932): the axis is sent as written with
/// `reversed`, so the reversed coil still follows its parameter.
#[test]
fn coil_reverse_direction_keeps_an_axis_of_expressions() {
    use solidcraft_engine::doc::FeatureKind;
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name": "t", "expression": "45 deg"})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.extra.insert("reversed".into(), json!(true));
    d.extra.insert("axis".into(), json!([0.0, "-sin(t)", "-cos(t)"]));
    let (id, p) = apply_commands(&h.app, &d).unwrap().remove(0);
    assert_eq!((&p["axis"], &p["reversed"]), (&json!([0.0, "-sin(t)", "-cos(t)"]), &json!(true)), "{p}");
    h.app.run(&id, p.clone()).unwrap_or_else(|e| panic!("{e}: {p}"));
    let axis = h.app.session.doc.features.iter().rev().find_map(|f| match &f.kind {
        FeatureKind::Coil { axis, .. } => Some(axis.clone()),
        _ => None,
    });
    assert!(axis.as_ref().is_some_and(|a| solidcraft_engine::doc::literal_dir(a).is_none()), "still expressions: {axis:?}");
}

/// Picking a plane supplies the base and normal before Reverse Direction grows against it. A
/// remembered axis must not replace the picked normal or lose the offset plane's origin.
#[test]
fn coil_reverse_direction_keeps_the_picked_plane_normal_and_origin() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("construct.plane.offset", json!({"base":"XY", "offset":50, "name":"Plane1"})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.extra.insert("axis".into(), json!([1.0, 0.0, 0.0]));
    d.pick(&h.app.session, Sel::Plane { name: "Plane1".into() });
    assert!(d.extra.get("axis").is_none(), "the new plane replaces the saved direction");
    d.extra.insert("reversed".into(), json!(true));
    for commands in [crate::dialogs::preview_commands(&h.app, &d).unwrap(), apply_commands(&h.app, &d).unwrap()] {
        let (_, p) = commands.iter().find(|(id, _)| id == "solid.coil").expect("the Coil command");
        assert_eq!(coil_plane_coordinates(&h, p.get("base").unwrap()), [0.0, 0.0, 50.0], "{p}");
        assert_eq!(coil_plane_coordinates(&h, p.get("axis").unwrap()), [0.0, 0.0, 1.0], "{p}");
        assert_eq!(p["reversed"], json!(true), "{p}");
        assert!(p.get("sketch").is_none() && p.get("flip").is_none(), "{p}");
    }
    h.app.dialog = Some(d);
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK applies the reversed Coil");
    let measured = h.app.run("inspect.measure", json!({})).unwrap();
    assert_eq!(measured["body_count"], json!(1));
    let body = &measured["bodies"][0];
    assert_eq!(body["closed"], json!(true));
    // Four turns at pitch 8 extend down 32 from z=50; a centred diameter-3 section
    // adds 1.5 at each end, giving z=16.5..51.5 independently of the dialog code.
    for (side, expected) in [("min", 16.5), ("max", 51.5)] {
        let actual = body["bbox"][side][2].as_f64().unwrap();
        assert!((actual - expected).abs() < 1e-6, "{measured}");
    }
}

/// A square-ended coil edited onto a sketch profile is accepted: the profile's plane is the
/// section, so the end cut it kept from the coil is not sent (#20737).
#[test]
fn a_square_ended_coil_edited_onto_a_profile_is_accepted() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "YZ", "name": "Section"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let sketch = crate::open_meshing::session_world(&h.app.session).sketches[0].feature;
    let made = h.app.run("solid.coil", json!({"diameter": 40, "revolutions": 3, "pitch": 8, "section_size": 2, "end_cut": "square"})).unwrap();
    let feature = made["feature"].as_u64().unwrap();
    let mut d = for_feature(&h.app, feature, None).unwrap();
    assert_eq!(d.extra.get("end_cut"), Some(&json!("square")), "the edit keeps the end cut");
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    for (command, params) in apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params.clone()).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
    }
    let kind = &h.app.session.doc.feature(feature).unwrap().kind;
    assert!(matches!(kind, FeatureKind::Coil { sketch: Some(_), .. }), "{kind:?}");
}

/// The Revolutions field steps by whole turns like the other counts (#7481).
#[test]
fn coil_revolutions_arrows_step_whole_turns() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"turns","expression":"3","unit":""})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    d.preview_enabled = false; // Count arrows exercise command parameters, not preview geometry.
    h.app.dialog = Some(d);
    h.settle();
    let sent = |h: &Harness| {
        let cmds = crate::dialogs::preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        cmds.iter().find(|(id, _)| id == "solid.coil").map(|(_, p)| p["revolutions"].clone()).unwrap_or_else(|| panic!("{cmds:?}"))
    };
    let set = |h: &mut Harness, text: &str| {
        if let Some(Kind::Primitive { fields, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind)
            && let Some((_, value)) = fields.iter_mut().find(|(k, _)| *k == "revolutions")
        {
            *value = text.into();
        }
        h.frames(2);
    };
    // Before each click: the preview has finished and the arrows have stopped moving. A preview
    // the kernel refuses (1500 turns at this size: "too many turns for the tolerance") shows its
    // error and scrolls the panel to it, moving the arrows up by about a field: a click aimed
    // before that scroll ended on the down arrow, stepping to 1499 (#23506).
    let still = |h: &mut Harness| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let (mut last, mut same) = (None, 0);
        while same < 3 {
            assert!(std::time::Instant::now() < deadline, "the Coil preview did not settle");
            if h.app.preview.busy {
                h.app.preview.wait_for_worker();
            }
            h.frames(1);
            let at = solidcraft_ui_registry::handle_at("dialog:ui.revolutions up");
            same = if !h.app.preview.busy && at.is_some() && at == last { same + 1 } else { 0 };
            last = at;
        }
    };
    let click = |h: &mut Harness, arrow: &str| {
        still(h);
        h.step(&json!({"click":{"handle":format!("dialog:ui.revolutions {arrow}")}})).unwrap();
    };
    assert_eq!(sent(&h), json!("4"));
    click(&mut h, "up");
    assert_eq!(sent(&h), json!("5"));
    click(&mut h, "down");
    click(&mut h, "down");
    assert_eq!(sent(&h), json!("3"));
    // A fraction steps to the next whole turn either way.
    set(&mut h, "2.5");
    click(&mut h, "up");
    assert_eq!(sent(&h), json!("3"));
    set(&mut h, "2.5");
    click(&mut h, "down");
    assert_eq!(sent(&h), json!("2"));
    // From 1 to the helix's 1500 turns (the kernel's span bound; #22577: no longer 200).
    set(&mut h, "1");
    click(&mut h, "down");
    assert_eq!(sent(&h), json!("1"));
    set(&mut h, "202");
    click(&mut h, "down");
    assert_eq!(sent(&h), json!("201"));
    set(&mut h, "1500");
    click(&mut h, "up");
    assert_eq!(sent(&h), json!("1500"));
    set(&mut h, "turns");
    click(&mut h, "up");
    assert_eq!(sent(&h), json!("turns + 1"));
}

fn chamfer_paste_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.nav = "blender".into();
    h.call("ui.view", json!({"view":"front"}));
    h.step(&json!({"start":"solid.chamfer"})).unwrap();
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:Distance"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"12 mm"})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    assert!(h.ctx.text_edit_focused(), "the distance editor has keyboard focus");
    assert_eq!(chamfer_distance(&h), "12 mm");
    h
}

fn chamfer_distance(h: &Harness) -> &str {
    let Kind::Fillet { radius, chamfer: true, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("Chamfer dialog") };
    radius
}

fn middle_at(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: egui::Modifiers::NONE }
}

fn chamfer_viewport_point(h: &Harness) -> egui::Pos2 {
    let at = h.app.viewport.layout.as_ref().unwrap().drawable.center() - egui::vec2(80.0, 80.0);
    assert!(!crate::shell::covered(&h.app, at));
    at
}

/// #20299: a browser Paste targets the focused field; native PRIMARY paste carries its position.
/// Both belong to navigation when the middle press starts in the viewport, even if it ends on a field.
#[test]
fn viewport_middle_orbit_does_not_paste_into_chamfer_distance() {
    for native in [false, true] {
        for release_on_field in [false, true] {
            let mut h = chamfer_paste_fixture();
            let at = chamfer_viewport_point(&h);
            let paste = |pos| {
                if native { egui::Event::MiddleClickPaste { pos, text: "731 mm".into() } } else { egui::Event::Paste("731 mm".into()) }
            };
            let before = h.app.cam;
            h.frame_with(vec![egui::Event::PointerMoved(at), middle_at(at, true), paste(at)]);
            assert_eq!(chamfer_distance(&h), "12 mm", "press must not paste");
            let moved = at + egui::vec2(40.0, 20.0);
            let mut events = vec![egui::Event::PointerMoved(moved)];
            if native {
                events.push(paste(moved));
            }
            h.frame_with(events);
            assert_eq!(chamfer_distance(&h), "12 mm", "drag must not paste");
            let mut expected = before;
            expected.orbit(40.0, 20.0);
            assert!((h.app.cam.yaw - expected.yaw).abs() < 1e-6, "the middle drag orbits");
            assert!((h.app.cam.pitch - expected.pitch).abs() < 1e-6, "the middle drag orbits");
            let end = if release_on_field { scenario::handle_at("dialog:Distance").unwrap() } else { moved };
            h.frame_with(vec![egui::Event::PointerMoved(end), middle_at(end, false), paste(end)]);
            assert_eq!(chamfer_distance(&h), "12 mm", "release must not paste, including over the field");
            // Once navigation ends, ordinary keyboard paste still replaces selected text.
            h.step(&json!({"click":{"handle":"dialog:Distance"}})).unwrap();
            h.step(&json!({"key":"A","cmd":true})).unwrap();
            h.frame_with(vec![egui::Event::Paste("27 mm".into())]);
            assert_eq!(chamfer_distance(&h), "27 mm");
        }
    }
}

#[test]
fn viewport_middle_click_does_not_paste_into_chamfer_distance() {
    for single_frame in [false, true] {
        let mut h = chamfer_paste_fixture();
        let at = chamfer_viewport_point(&h);
        let mut events = vec![egui::Event::PointerMoved(at), middle_at(at, true)];
        if !single_frame {
            h.frame_with(std::mem::take(&mut events));
        }
        events.extend([middle_at(at, false), egui::Event::Paste("731 mm".into())]);
        // A modifier pressed after PRIMARY paste must not reclassify that earlier paste.
        events.push(egui::Event::ModifiersChanged(egui::Modifiers::CTRL | egui::Modifiers::COMMAND));
        h.frame_with(events);
        assert_eq!(chamfer_distance(&h), "12 mm", "a stationary click also belongs to navigation");
    }
}

#[test]
fn middle_click_on_chamfer_distance_keeps_primary_selection_paste() {
    let mut h = chamfer_paste_fixture();
    let editor = h.ctx.memory(|m| m.focused()).unwrap();
    // Click beyond the short value's text so PRIMARY insertion has a known cursor position.
    let at = h.ctx.read_response(editor).unwrap().rect.right_center() - egui::vec2(4.0, 0.0);
    let viewport = chamfer_viewport_point(&h);
    let before = h.app.cam;
    // Two clicks can arrive in one slow frame. Only the viewport click's paste is consumed.
    h.frame_with(vec![
        egui::Event::PointerMoved(viewport),
        middle_at(viewport, true),
        middle_at(viewport, false),
        egui::Event::Paste("999".into()),
        egui::Event::PointerMoved(at),
        middle_at(at, true),
        egui::Event::MiddleClickPaste { pos: at, text: "731".into() },
    ]);
    h.frame_with(vec![middle_at(at, false)]);
    assert_eq!(chamfer_distance(&h), "12 mm731", "only PRIMARY text is inserted at the end of the field");
    assert_eq!(h.app.cam.yaw, before.yaw);
    assert_eq!(h.app.cam.pitch, before.pitch);
    assert_eq!(h.app.cam.target, before.target);
}

#[test]
fn keyboard_paste_into_chamfer_distance_works_during_middle_orbit() {
    for modifiers in [egui::Modifiers::CTRL | egui::Modifiers::COMMAND, egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND] {
        for browser in [false, true] {
            for release in [false, true] {
                let mut h = chamfer_paste_fixture();
                let at = chamfer_viewport_point(&h);
                let before = h.app.cam;
                h.frame_with(vec![egui::Event::PointerMoved(at), middle_at(at, true)]);
                h.frame_with(vec![egui::Event::PointerMoved(at + egui::vec2(40.0, 20.0))]);
                assert!(h.ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle)));
                assert!(h.ctx.text_edit_focused());
                let before_paste = h.app.cam;
                if !browser {
                    // Ctrl/Cmd can have been held since an earlier frame. Native sends no V key.
                    h.frame_with(vec![egui::Event::ModifiersChanged(modifiers)]);
                }
                let next = at + egui::vec2(50.0, 20.0);
                let mut events = vec![egui::Event::PointerMoved(next)];
                if release {
                    events.push(middle_at(next, false));
                }
                if browser {
                    // Browser keydown precedes the DOM paste event, possibly with keyup between.
                    events.push(egui::Event::ModifiersChanged(modifiers));
                    events.push(egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: true, repeat: false, modifiers });
                    events.push(egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers });
                    events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
                }
                // Native Ctrl/Cmd+V delivers only Paste, with no V key event.
                events.push(egui::Event::Paste("27 mm".into()));
                events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
                h.frame_with(events);
                assert_eq!(chamfer_distance(&h), "27 mm", "keyboard paste works (browser: {browser}, release: {release})");
                assert_ne!(h.app.cam.yaw, before.yaw, "the middle drag still orbits");
                assert_ne!(h.app.cam.pitch, before.pitch, "the middle drag still orbits");
                if !release {
                    assert!(h.ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle)));
                    assert_ne!(h.app.cam.yaw, before_paste.yaw, "the orbit continues in the keyboard-paste frame");
                    h.frame_with(vec![middle_at(next, false)]);
                }
            }
        }
    }
}

fn extrude_focus_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[50,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.step(&json!({"start":"solid.extrude"})).unwrap();
    h.frames(8);
    h
}

fn extrude_control_focused(h: &Harness, target: &str) -> bool {
    let focused = h.ctx.memory(|m| m.focused()).and_then(|id| h.ctx.read_response(id));
    focused.zip(scenario::handle_at(target)).is_some_and(|(response, at)| response.interact_rect.contains(at))
}

/// Move focus through the real dialog controls, as a keyboard user does.
fn tab_to_extrude_control(h: &mut Harness, target: &str, shift: bool) {
    for _ in 0..32 {
        h.step(&json!({"key": "Tab", "shift": shift})).unwrap();
        if extrude_control_focused(h, target) {
            return;
        }
    }
    panic!("{}Tab did not reach {target}; focus is {:?}", if shift { "Shift+" } else { "" }, h.ctx.memory(|m| m.focused()));
}

fn extrude_dialog_with_typed_distance() -> Harness {
    let mut h = extrude_focus_fixture();
    assert_eq!(h.call("ui.prefs", json!({"value_boxes_in_view": false}))["ok"], true);
    if h.app.dialog.as_ref().unwrap().items().is_empty() {
        h.step(&json!({"click": {"world": [15, 10, 0]}})).unwrap();
    }
    assert_eq!(h.app.dialog.as_ref().unwrap().items().len(), 1);
    crate::test_support::click(&mut h, "dialog:Distance");
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "30 mm"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, direction: 0, .. } if distance == "30 mm"));
    h
}

#[test]
fn extrude_dialog_keyboard_selects_symmetric_and_applies() {
    let mut h = extrude_dialog_with_typed_distance();
    let direction = "dialog:Direction";
    let options = ["dialog:Direction:One Side", "dialog:Direction:Two Sides", "dialog:Direction:Symmetric"];
    tab_to_extrude_control(&mut h, direction, true);
    h.step(&json!({"key": "Space"})).unwrap();
    assert!(scenario::handle_at(options[0]).is_some(), "Space on Direction opens its choices");

    h.step(&json!({"key": "ArrowDown"})).unwrap();
    assert!(extrude_control_focused(&h, options[0]), "Arrow Down enters the open menu");
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_some(), "Enter on a menu choice must leave Extrude open");
    assert!(scenario::handle_at(options[0]).is_none(), "Reselecting the current choice closes the popup");
    assert!(extrude_control_focused(&h, direction), "Direction regains focus after Enter");

    h.step(&json!({"key": "Space"})).unwrap();
    for option in options {
        h.step(&json!({"key": "ArrowDown"})).unwrap();
        assert!(extrude_control_focused(&h, option), "Arrow Down should focus {option}");
    }
    h.step(&json!({"key": "Space"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, direction: 2, .. } if distance == "30 mm"));
    assert!(scenario::handle_at(options[2]).is_none(), "Selecting Symmetric closes the popup");
    assert!(extrude_control_focused(&h, direction), "Direction regains focus after Space");

    tab_to_extrude_control(&mut h, "dialog:Distance", false);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "Enter from Distance applies Extrude");
    let bounds = solidcraft_engine::view::bounds(&h.app.session).unwrap();
    assert!((bounds.min.z + 15.0).abs() < 1e-7 && (bounds.max.z - 15.0).abs() < 1e-7, "{bounds:?}");
}

#[test]
fn extrude_dialog_keyboard_reverses_and_applies() {
    let mut h = extrude_dialog_with_typed_distance();
    tab_to_extrude_control(&mut h, "dialog:Reverse Direction", false);
    h.step(&json!({"key": "Space"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, direction: 1, .. } if distance == "30 mm"));

    tab_to_extrude_control(&mut h, "dialog:Distance", true);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "Enter from Distance applies Extrude");
    let bounds = solidcraft_engine::view::bounds(&h.app.session).unwrap();
    assert!((bounds.min.z + 30.0).abs() < 1e-7 && bounds.max.z.abs() < 1e-7, "{bounds:?}");
}

#[test]
fn viewport_profile_click_returns_extrude_value_focus() {
    let mut h = extrude_focus_fixture();
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, .. } if distance == "30"));
    // Clicking an already selected profile removes it; pick it again before building.
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    let bounds = crate::open_meshing::session_world(&h.app.session).bodies.first().unwrap().mesh().bounds();
    assert!((bounds.max.z - bounds.min.z - 30.0).abs() < 1e-7);
}

/// Typing `depth = 30` in the extrude dialog's distance defines the user parameter `depth` and
/// the extrude uses it, in the dialog's one undo step; the preview made nothing (#4071).
#[test]
fn a_dialog_field_typed_as_a_definition_defines_the_parameter() {
    let mut h = extrude_focus_fixture();
    let undo = h.app.session.undo.len();
    // As above: the first click returns focus to the distance, the second picks the profile.
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"depth = 30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, .. } if distance == "depth = 30"));
    h.settle();
    assert!(h.app.session.doc.param("depth").is_none(), "the preview defined nothing");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "{:?} {:?}", h.app.status, h.app.dialog.as_ref().map(|d| (d.error.clone(), format!("{:?}", d.kind))));
    let doc = &h.app.session.doc;
    let depth = doc.param("depth").expect("the user parameter was added");
    assert!(!depth.model && depth.expr == "30" && depth.unit == "mm", "{depth:?}");
    let extrude = doc.features.iter().rev().find(|f| f.name.starts_with("Extrude")).unwrap();
    assert!(extrude.kind.inputs().iter().any(|(label, e, _)| *label == "Distance" && e == "depth"), "{:?}", extrude.kind.inputs());
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let bounds = crate::open_meshing::session_world(&h.app.session).bodies.first().unwrap().mesh().bounds();
    assert!((bounds.max.z - bounds.min.z - 30.0).abs() < 1e-7);
}

#[test]
fn viewport_empty_click_falls_back_to_first_numeric_dialog_field() {
    let mut h = extrude_focus_fixture();
    if let Some(id) = h.ctx.memory(|m| m.focused()) {
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.frames(2);
    let viewport = h.app.viewport.rect.unwrap();
    h.step(&json!({"click":[viewport.left()+80.0, viewport.bottom()-80.0]})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, .. } if distance == "30"));
}

#[test]
fn viewport_click_preserves_active_secondary_dialog_value() {
    let mut h = extrude_focus_fixture();
    if let Kind::Extrude { direction, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *direction = crate::dialogs::TWO_SIDES;
    }
    h.frames(3);
    crate::test_support::click(&mut h, "dialog:Distance 2");
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"7"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance2, .. } if distance2 == "7"));
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, distance2, .. } if distance == "10 mm" && distance2 == "30"));
}

#[test]
fn extrude_two_sides_tab_moves_directly_from_distance_to_distance_2() {
    let mut h = extrude_dialog_with_typed_distance();
    // Switch to Two Sides + Cut so Target Bodies is present in the dialog above Distance.
    if let Kind::Extrude { direction, operation, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *direction = crate::dialogs::TWO_SIDES;
        *operation = 2; // Cut
    }
    h.frames(3);
    crate::test_support::click(&mut h, "dialog:Distance");
    assert!(extrude_control_focused(&h, "dialog:Distance"));

    // Tab moves directly through Extent Type 2 to Distance 2 (never landing on an invisible scrub
    // label that drops editor focus and highlights Target Bodies).
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:ui.extrude_extent_type_2"), "Tab from Distance should focus Extent Type 2 directly");
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Distance 2"), "Tab from Extent Type 2 should focus Distance 2 directly");
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "25"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, distance2, .. } if distance == "30 mm" && distance2 == "25"));

    // Shift+Tab must move directly back through Extent Type 2, Distance, Extent Type, and Direction.
    h.step(&json!({"key": "Tab", "shift": true})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:ui.extrude_extent_type_2"), "Shift+Tab from Distance 2 should focus Extent Type 2 directly");
    h.step(&json!({"key": "Tab", "shift": true})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Distance"), "Shift+Tab from Extent Type 2 should focus Distance directly");
    h.step(&json!({"key": "Tab", "shift": true})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Extent Type"), "Shift+Tab from Distance should focus Extent Type directly");
    h.step(&json!({"key": "Tab", "shift": true})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Direction"), "Shift+Tab from Extent Type should focus Direction directly");

    // Forward Tab from Direction must visit Extent Type -> Distance -> Extent Type 2 -> Distance 2 -> Operation in order.
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Extent Type"), "Tab from Direction should focus Extent Type directly");
    h.frames(1);
    assert_eq!(
        crate::scenario::count_of("dialog:selection:Profiles:active"),
        Some(0.0),
        "Profiles must not stay highlighted while Extent Type holds focus"
    );
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Distance"), "Tab from Extent Type should focus Distance directly");
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:ui.extrude_extent_type_2"), "Tab from Distance should focus Extent Type 2 directly");
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Distance 2"), "Tab from Extent Type 2 should focus Distance 2 directly");
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Operation"), "Tab from Distance 2 should focus Operation directly");

    // Clicking the Profiles pick field activates it; pressing Tab from there must enter the first
    // control after the pick inputs (Name, then Direction) instead of jumping to the app bar, and
    // drop the active highlight from Profiles.
    let profiles = crate::scenario::rect_of("dialog:field:Profiles").expect("Profiles field rect").center();
    h.step(&json!({"click": [profiles.x, profiles.y]})).unwrap();
    h.frames(2);
    assert_eq!(crate::scenario::count_of("dialog:selection:Profiles:active"), Some(1.0));
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Name"), "Tab from Profiles should focus Name directly");
    h.frames(1);
    assert_eq!(crate::scenario::count_of("dialog:selection:Profiles:active"), Some(0.0), "Profiles must not stay highlighted while Name holds focus");
    h.step(&json!({"key": "Tab"})).unwrap();
    assert!(extrude_control_focused(&h, "dialog:Direction"), "Tab from Name should focus Direction directly");
    h.frames(1);
    assert_eq!(
        crate::scenario::count_of("dialog:selection:Profiles:active"),
        Some(0.0),
        "Profiles must not stay highlighted while Direction holds focus"
    );
}

#[test]
fn extrude_two_sides_canvas_tab_moves_directly_from_distance_to_distance_2() {
    let mut h = extrude_focus_fixture();
    if let Kind::Extrude { direction, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *direction = crate::dialogs::TWO_SIDES;
    }
    h.frames(3);
    let first = egui::Id::unique("sc_canvas_value");
    let second = egui::Id::unique("sc_canvas_value2");
    h.ctx.memory_mut(|m| m.request_focus(first));
    h.frames(1);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(first));

    h.step(&json!({"key": "Tab"})).unwrap();
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(second), "Tab from canvas Distance should focus canvas Distance 2");
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "25"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance2, .. } if distance2 == "25"));
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "Enter from canvas Distance 2 applies Extrude");
}

/// A circle drawn partly past the top face its sketch is on is cut by the face's projected outline
/// into a lens on the face and a crescent past it. Clicking one of them in Extrude builds that
/// region alone, the one highlighted and counted, not the whole circle (#6724).
#[test]
fn extrude_builds_the_one_region_clicked_where_the_face_outline_cuts_a_profile() {
    for x in [7.0, 14.0] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 10})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance": 10, "body_name": "Base"})).unwrap();
        let sketch = h.app.run("sketch.create", json!({"plane": {"face": [0, 0, 10]}})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.circle.center", json!({"center": [10, 0], "radius": 6})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let state = h.app.session.world_state().unwrap();
        let ss = state.sketch(sketch).unwrap();
        assert_eq!(ss.profiles.len(), 3, "the lens, the crescent past the face and the rest of the face");
        let index = ss.profiles.iter().position(|p| p.region.contains(solidcore_record::Vec2::new(x, 0.0))).unwrap();
        let area = ss.profiles[index].area;
        let mut d = Dialog::for_command(&h.app, "solid.extrude").unwrap();
        if let Kind::Extrude { distance, .. } = &mut d.kind {
            *distance = "5".into();
        }
        d.pick(&h.app.session, Sel::Profile { sketch, index });
        assert_eq!(d.items(), vec![Sel::Profile { sketch, index }], "one region picked at x = {x}");
        let commands = apply_commands(&h.app, &d).unwrap();
        assert_eq!(commands.len(), 1, "{commands:?}");
        let (id, p) = &commands[0];
        assert_eq!(p["exact_regions"], json!(true), "{p}");
        let before = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        let made = h.app.run(id, p.clone()).unwrap()["feature"].as_u64().unwrap();
        let Some(FeatureKind::Extrude { profiles, .. }) = h.app.session.doc.feature(made).map(|f| f.kind.clone()) else { panic!("an extrude") };
        let after = h.app.session.world_state().unwrap();
        let used = solidcraft_engine::doc::profile_selection_indices(after.sketch(sketch).unwrap(), &profiles).unwrap();
        assert_eq!(used, vec![index], "the region clicked at x = {x}, and only it");
        let volume = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        assert!(((volume - before) - area * 5.0).abs() < 5e-3 * area * 5.0, "x = {x}: {volume} - {before} vs {}", area * 5.0);
    }
}

/// Every selection field of every command's dialog is labelled with a message the catalogs carry,
/// so it reads in the user's language ("First edge", "Second point" read English everywhere when
/// they were missing).
#[test]
fn every_selection_field_label_is_translated() {
    let h = fixture();
    let _japanese = solidcraft_ui_text::scope("ja");
    let mut missing = std::collections::BTreeSet::new();
    for spec in solidcraft_engine::command_specs() {
        let Some(d) = Dialog::for_command(&h.app, spec.id) else { continue };
        for input in &d.inputs {
            let label = input.label.trim();
            if label.chars().any(|c| c.is_ascii_alphabetic()) && solidcraft_ui_text::tr_label(label) == label {
                missing.insert(format!("{}: {label}", spec.id));
            }
        }
    }
    assert!(missing.is_empty(), "selection fields without a message: {missing:#?}");
}

#[test]
fn align_components_picks_the_placement_under_the_active_component_and_warns_when_shared() {
    let mut h = Harness::new();
    let made = h.app.run("component.create", json!({"name": "Frame", "grounded": false})).unwrap();
    let (frame, a) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
    let bracket = h.app.run("component.create", json!({"name": "Bracket", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "Plate"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let plate = crate::open_meshing::session_world(&h.app.session).bodies.first().map(|b| b.name.clone()).unwrap();

    // With the design active, a click on the part inside Frame picks the Frame placement.
    assert_eq!(occurrence_path_of_body(&h.app.session, &plate), Some(vec![a]));
    // With that Frame placement active, the same click picks the part inside it.
    h.app.run("component.activate", json!({"component": frame, "occurrence": a, "path": [a]})).unwrap();
    assert_eq!(occurrence_path_of_body(&h.app.session, &plate), Some(vec![a, bracket]));
    assert!(!align_moves_shared(&h.app.session, &[Sel::Occurrence { path: vec![a, bracket] }]), "Frame is placed once");

    // Once Frame is placed twice, the part's placement inside it is shared: the dialog says so.
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.run("occurrence.copy", json!({"component": frame, "translate": [0, 50, 0]})).unwrap();
    assert!(align_moves_shared(&h.app.session, &[Sel::Occurrence { path: vec![a, bracket] }]));
    assert!(!align_moves_shared(&h.app.session, &[Sel::Occurrence { path: vec![a] }]), "a top-level placement is its own");

    h.app.session.selection = vec![Sel::Occurrence { path: vec![a] }];
    h.app.start("solid.align");
    h.frames(4);
    assert!(h.widget(ALIGN_SHARED_WARNING).is_none(), "no warning for a top-level pick");
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Occurrence { path: vec![a, bracket] }];
    h.frames(4);
    assert!(h.widget(ALIGN_SHARED_WARNING).is_some(), "the shared pick is called out");
}

/// OK on the editor of a pattern behind the history marker leaves it rolled back, so the view
/// does not change: the status line says why, where the edit used to look as if it had silently
/// failed (#8100).
#[test]
fn ok_on_a_rolled_back_pattern_editor_says_it_is_not_built() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, params) in [
        ("solid.box", json!({"length":40,"width":30,"height":10,"corner":[0,0,0],"body_name":"Plate"})),
        ("solid.cylinder", json!({"radius":2,"height":20,"base":[10,15,-5],"operation":"cut","targets":["Plate"],"name":"Cut"})),
        ("solid.pattern.circular", json!({"features":["Cut"],"axis":{"origin":[20,15,0],"dir":[0,0,1]},"count":2,"name":"Pattern"})),
        ("timeline.roll_to", json!({"position":2})),
    ] {
        h.app.run(id, params).unwrap();
    }
    let pattern = h.app.session.doc.features.last().unwrap().id;
    h.app.edit_feature(pattern);
    h.settle();
    assert!(h.app.dialog.is_some(), "the pattern's editor opens");
    h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
    for _ in 0..500 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.app.dialog.is_none(), "the edit is applied");
    assert_eq!(h.app.session.doc.marker, Some(2), "the pattern stays behind the marker");
    let (text, _, error) = h.app.status.clone().unwrap();
    assert!(!error, "{text}");
    assert!(text.starts_with("Pattern changed, but it is after the history marker"), "{text}");
}

/// #10852: the editor of a circular pattern whose axis origin is length expressions sends them
/// back as they are, so applying it unchanged keeps the pattern following its parameters.
#[test]
fn editing_a_circular_pattern_keeps_its_origin_expressions() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, params) in [
        ("parameters.add", json!({"name":"cx","expression":"20 mm"})),
        ("solid.box", json!({"length":40,"width":30,"height":10,"corner":[0,0,0],"body_name":"Plate"})),
        ("solid.cylinder", json!({"radius":2,"height":20,"base":["cx - 10 mm",15,-5],"operation":"cut","targets":["Plate"],"name":"Cut"})),
        ("solid.pattern.circular", json!({"features":["Cut"],"axis":{"origin":["cx",15,0],"dir":[0,0,1]},"count":2,"name":"Pattern"})),
    ] {
        h.app.run(id, params).unwrap();
    }
    let pattern = h.app.session.doc.features.last().unwrap().id;
    let origin = |h: &Harness| serde_json::to_value(&h.app.session.doc.feature(pattern).unwrap().kind).unwrap()["pattern"]["origin"].clone();
    assert_eq!(origin(&h), json!(["cx", 15.0, 0.0]));
    let d = crate::dialogs::for_feature(&h.app, pattern, None).unwrap();
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    assert_eq!(origin(&h), json!(["cx", 15.0, 0.0]), "the expression is kept, not its value now");
}

/// The Circular Pattern dialog must show only one field active at a time (#10066): the pick
/// field's accent ring yields while a text field holds the keyboard, and clicking a pick field
/// takes the keyboard back.
#[test]
fn circular_pattern_dialog_shows_one_active_field_at_a_time() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
    let mut d = Dialog::for_command(&h.app, "solid.pattern.circular").unwrap();
    d.inputs[1].items = vec![Sel::Axis { name: "Z".into() }];
    h.app.dialog = Some(d);
    h.settle();
    let active = |label: &str| crate::scenario::count_of(&format!("dialog:selection:{label}:active")) == Some(1.0);
    let keyboard = |h: &Harness| h.ctx.memory(|m| m.focused()).is_some();
    let click_field = |h: &mut Harness, rect: &str| {
        let at = crate::scenario::rect_of(rect).unwrap_or_else(|| panic!("{rect} is not drawn")).center();
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
    };
    // A fresh dialog: the Objects pick field is the one active field, no text field has the keyboard.
    assert!(!keyboard(&h), "nothing holds the keyboard when the dialog opens");
    assert_eq!((active("Objects"), active("Axis")), (true, false), "only Objects looks active");
    // Clicking Quantity: the text field takes the keyboard and no pick field keeps its ring.
    h.step(&json!({"click": {"handle": "dialog:Quantity"}})).unwrap();
    h.frames(2);
    assert!(keyboard(&h), "Quantity holds the keyboard");
    assert_eq!((active("Objects"), active("Axis")), (false, false), "only the typed field looks active");
    // Clicking the Axis pick field: it becomes the one active field and takes the keyboard back.
    click_field(&mut h, "dialog:field:Axis");
    assert!(!keyboard(&h), "the pick field takes the keyboard from Quantity");
    assert_eq!((active("Objects"), active("Axis")), (false, true), "only Axis looks active");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1, "picks still go to Axis");
}

/// #10057: a sketch clicked for a pattern's objects is not taken in silence. In the view, the
/// Browser and the History, with either Object Type, the dialog says a sketch is neither a body
/// nor a feature and what to do instead, as a hint rather than an error; nothing is picked and
/// the selection stays as it was.
#[test]
fn pattern_says_why_a_clicked_sketch_is_not_taken() {
    for (dock, ty) in [("bottom", "Bodies"), ("left", "Features"), ("under_browser", "Bodies")] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock": dock})).unwrap();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [20, 0], "radius": 5})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.app.start("solid.pattern.circular");
        h.frames(4);
        mirror_object_type(&mut h, ty);
        for click in [json!({"browser": "Sketch1"}), json!({"handle": "timeline:Sketch1"}), json!({"world": [25, 0, 0]})] {
            h.app.dialog.as_mut().unwrap().error = None;
            // Out of double-click time: a double click opens the sketch to edit.
            h.frames(30);
            h.step(&json!({ "click": click })).unwrap();
            h.frames(2);
            let d = h.app.dialog.as_ref().expect("the pattern stays open");
            assert!(matches!(d.kind, Kind::PatternCirc { .. }), "{dock} {ty} {click}");
            assert_eq!(d.error.as_deref(), Some(crate::dialogs::SKETCH_NOT_OBJECT), "{dock} {ty} {click}");
            assert!(d.inputs[0].items.is_empty(), "{dock} {ty} {click}: the sketch is not picked");
            assert!(h.app.session.selection.is_empty(), "{dock} {ty} {click}: the selection stays as it was");
        }
    }
    assert!(crate::dialogs::missing_pick(crate::dialogs::SKETCH_NOT_OBJECT), "shown as what to pick, with no Create Issue");
    let _japanese = solidcraft_ui_text::scope("ja");
    assert!(!solidcraft_ui_text::tr_label(crate::dialogs::SKETCH_NOT_OBJECT).is_ascii(), "the hint reads in the user's language");
    assert!(!solidcraft_ui_text::tr("ui.pattern_object_type_tip").is_ascii(), "so does the Object Type tooltip");
}

/// Features stored with bare numbers edit with their units written out, as the Plastic ones do
/// (#1936): Fillet, Chamfer, Shell, Draft, Offset Face, Stock, Offset Plane and Revolve among them.
/// OK in the editor keeps the feature in place.
#[test]
fn stored_bare_numbers_edit_with_their_units() {
    use serde_json::{Value, json};
    let profile = [
        ("sketch.create", json!({"plane": "XZ", "name": "Profile"})),
        ("sketch.rectangle.two_point", json!({"p0": [50, 0], "p1": [60, 10]})),
        ("sketch.finish", json!({})),
    ];
    let one = |command: &'static str, params: Value| vec![(command, params)];
    let cases: Vec<(Vec<(&str, Value)>, &[&str])> = vec![
        (one("solid.fillet", json!({"edges": [[20, 0, 20]], "radius": 2})), &["2 mm"]),
        (one("solid.chamfer", json!({"edges": [[20, 0, 20]], "distance": 1, "distance2": 1.5})), &["1 mm", "1.5 mm"]),
        (one("solid.chamfer", json!({"edges": [[20, 0, 20]], "distance": 1, "angle": 30})), &["1 mm", "30 deg"]),
        (one("solid.shell", json!({"faces": [[20, 15, 20]], "thickness": 2})), &["2 mm"]),
        (one("solid.draft", json!({"faces": [[0, 15, 10]], "angle": 3, "neutral": "XY"})), &["3 deg"]),
        (one("solid.offset_face", json!({"faces": [[20, 15, 20]], "distance": 2})), &["2 mm"]),
        (one("solid.bounding_solid", json!({"margin": 2})), &["2 mm"]),
        (one("construct.plane.offset", json!({"base": "XY", "offset": 5})), &["5 mm"]),
        (profile.iter().cloned().chain([("solid.revolve", json!({"sketch": "Profile", "axis": "Z", "angle": 90}))]).collect(), &["90 deg"]),
    ];
    for (cmds, expected) in cases {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let command = cmds.last().map(|(c, _)| *c).unwrap_or_default();
        for (c, p) in cmds {
            h.app.run(c, p).unwrap();
        }
        h.frames(2);
        let id = h.app.session.doc.features.last().unwrap().id;
        let count = h.app.session.doc.features.len();
        h.app.edit_feature(id);
        h.frames(4);
        let shown = match &h.app.dialog.as_ref().unwrap().kind {
            Kind::Fillet { radius, ctype: 0, .. } => vec![radius.clone()],
            Kind::Fillet { radius, distance2, ctype: 1, .. } => vec![radius.clone(), distance2.clone()],
            Kind::Fillet { radius, angle, ctype: 2, .. } => vec![radius.clone(), angle.clone()],
            Kind::Shell { thickness } => vec![thickness.clone()],
            Kind::Draft { angle } => vec![angle.clone()],
            Kind::OffsetFaces { distance } => vec![distance.clone()],
            Kind::Stock { margin } => vec![margin.clone()],
            Kind::OffsetPlane { offset } => vec![offset.clone()],
            Kind::Revolve { angle, .. } => vec![angle.clone()],
            other => panic!("{command}: {other:?}"),
        };
        assert_eq!(shown, expected, "{command}");
        h.step(&json!({"widget": "OK"})).unwrap();
        h.frames(4);
        assert!(h.app.dialog.is_none(), "{command}: OK applies the unchanged editor");
        assert_eq!(h.app.session.doc.features.len(), count, "{command}: the feature is redefined in place");
    }
}

#[test]
fn dialog_profile_labels_name_the_sketch_in_coil_and_mirror() {
    for command in ["solid.coil", "sketch.mirror"] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Panel"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        if command == "solid.coil" {
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.call("ui.view", json!({"view": "top"}));
        h.call("ui.camera", json!({"target": [20, 15, 0], "distance": 100}));
        h.step(&json!({"click": {"world": [20, 15, 0]}})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::Profile { sketch, index: 0 }]);
        h.app.start(command);
        h.app.dialog.as_mut().unwrap().preview_enabled = false;
        h.frames(3);
        // A field that takes several picks lists them, labels and all, behind its arrow (#3683).
        let (multi, label) = h.app.dialog.as_ref().map(|d| (d.inputs[0].multi, d.inputs[0].label)).unwrap();
        if multi {
            h.step(&json!({"click": {"handle": format!("dialog:list_toggle:{label}")}})).unwrap();
            h.frames(2);
        }
        let d = h.app.dialog.as_ref().unwrap();
        let input = &d.inputs[0];
        assert_eq!(input.items, vec![Sel::Profile { sketch, index: 0 }]);
        assert_eq!(selection_label(&h.app.session, h.app.preferences.origin_plane_names, d, input, &input.items[0]), "Panel · profile 1");
        let mut painted = false;
        for clipped in &h.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Text(text) = shape {
                    painted |= text.galley.text() == "Panel · profile 1";
                }
            });
        }
        assert!(painted, "{command}: the painted selection label");
    }
}

#[test]
fn dialog_sketch_selection_labels_translate_and_keep_identifiers() {
    let mut h = Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY", "name": "Panel"})).unwrap()["sketch"].as_u64().unwrap();
    let d = Dialog::for_command(&h.app, "sketch.mirror").unwrap();
    for (sel, english, german) in [
        (Sel::Profile { sketch, index: 1 }, "Panel · profile 2", "Panel · Profil 2"),
        (Sel::SketchMidpoint { id: "l7".into() }, "Midpoint · l7", "Mittelpunkt · l7"),
        (Sel::ExternalSketchCurve { sketch, id: "l7".into() }, "Panel · curve l7", "Panel · Kurve l7"),
        (Sel::SketchConstraint { id: "k3".into() }, "Constraint k3", "Bedingung k3"),
    ] {
        for (language, expected) in [("en", english), ("de", german)] {
            let _language = solidcraft_ui_text::scope(language);
            assert_eq!(selection_label(&h.app.session, h.app.preferences.origin_plane_names, &d, &d.inputs[0], &sel), expected);
        }
    }
    let missing = Sel::Profile { sketch: 999, index: 0 };
    assert_eq!(selection_label(&h.app.session, h.app.preferences.origin_plane_names, &d, &d.inputs[0], &missing), "Sketch999 · profile 1");
}

/// A pattern of every body of a component placed twice: the dialog sees that the bodies make up
/// both placements whole, and offers them as Components. Switched, the pattern places each again
/// (`occurrences`) instead of copying every body. A selection missing one body of a placement
/// gets no offer.
#[test]
fn a_pattern_of_whole_components_bodies_offers_and_places_the_components() {
    let mut h = Harness::new();
    let comp = h.app.run("component.create", json!({"name": "Bracket"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 30, "width": 20, "height": 4, "body_name": "Base"})).unwrap();
    h.app.run("solid.box", json!({"length": 4, "width": 20, "height": 20, "body_name": "Upright"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let first = h.app.session.doc.occurrence_of(comp).unwrap().id;
    let second = h.app.run("occurrence.copy", json!({"component": comp, "translate": [50, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    let world: Vec<String> = crate::open_meshing::session_world(&h.app.session).bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(world.len(), 4, "{world:?}");
    let bodies: Vec<Sel> = world.iter().map(|name| Sel::Body { name: name.clone() }).collect();

    let whole = crate::pattern_components::whole_placements(&h.app.session, &bodies);
    assert_eq!(whole, vec![vec![first], vec![second]]);
    assert!(crate::pattern_components::whole_placements(&h.app.session, &bodies[1..]).is_empty(), "a placement missing a body is not whole");

    h.app.session.selection = bodies.clone();
    h.app.start("solid.pattern.rectangular");
    h.frames(4);
    let offered = |h: &Harness| h.app.dialog.as_ref().unwrap().whole_components.as_ref().map(|c| (c.revision, c.placements.clone()));
    assert_eq!(offered(&h), Some((h.app.session.revision, whole.clone())));
    // Activating a component changes what a pick takes (the design is the same), so the offer is
    // worked out again for it.
    for active in [json!({"component": comp, "occurrence": first}), json!({"component": "root"})] {
        let doc = h.app.session.doc.clone();
        h.app.run("component.activate", active).unwrap();
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        h.frames(2);
        let now = crate::pattern_components::whole_placements(&h.app.session, &bodies);
        assert_eq!(offered(&h), Some((h.app.session.revision, now)));
    }
    // Each Object Type keeps its own objects: Bodies by way of Features to Components gives the
    // bodies' placements, not the bodies, and Features picked are kept past Bodies and Components.
    let placements = vec![Sel::Occurrence { path: vec![first] }, Sel::Occurrence { path: vec![second] }];
    let feature = vec![Sel::Feature { id: 1 }];
    mirror_object_type(&mut h, "Features");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    mirror_object_type(&mut h, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, placements);
    mirror_object_type(&mut h, "Features");
    h.app.dialog.as_mut().unwrap().inputs[0].items.clone_from(&feature);
    mirror_object_type(&mut h, "Bodies");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, bodies);
    mirror_object_type(&mut h, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, placements);
    mirror_object_type(&mut h, "Features");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, feature);
    // Components picked by hand outlast a trip to Bodies that leaves the bodies as they came
    // back, or empties them; bodies picked anew there become their placements.
    let items = |h: &Harness| h.app.dialog.as_ref().unwrap().inputs[0].items.clone();
    let narrowed = vec![Sel::Occurrence { path: vec![first] }];
    mirror_object_type(&mut h, "Components");
    h.app.dialog.as_mut().unwrap().inputs[0].items.clone_from(&narrowed);
    mirror_object_type(&mut h, "Bodies");
    assert_eq!(items(&h), bodies);
    mirror_object_type(&mut h, "Components");
    assert_eq!(items(&h), narrowed, "Bodies left as they came back");
    mirror_object_type(&mut h, "Bodies");
    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    mirror_object_type(&mut h, "Components");
    assert_eq!(items(&h), narrowed, "Bodies emptied");
    mirror_object_type(&mut h, "Bodies");
    assert!(items(&h).is_empty());
    let picked = vec![bodies[3].clone()];
    h.app.dialog.as_mut().unwrap().inputs[0].items.clone_from(&picked);
    mirror_object_type(&mut h, "Components");
    let placed = crate::pattern_components::as_placements(&h.app.session, &picked);
    assert_eq!(placed.len(), 1);
    assert_ne!(placed, narrowed, "the body picked is in the other placement");
    assert_eq!(items(&h), placed, "Bodies picked anew");
    h.app.dialog.as_mut().unwrap().inputs[0].items.clone_from(&placements);
    mirror_object_type(&mut h, "Features");
    assert_eq!(items(&h), feature);
    mirror_object_type(&mut h, "Components");
    {
        let d = h.app.dialog.as_mut().unwrap();
        assert_eq!(d.inputs[0].label, "Components");
        assert_eq!(d.inputs[0].items, vec![Sel::Occurrence { path: vec![first] }, Sel::Occurrence { path: vec![second] }]);
        d.inputs[1].items = vec![Sel::Axis { name: "Y".into() }];
    }
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].0, "solid.pattern.rectangular");
    assert_eq!(commands[0].1["occurrences"], json!([[first], [second]]));
    let occurrences = h.app.session.doc.occurrences.len();
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    // Two more placements of each (a quantity of 3), and no new bodies.
    assert_eq!(h.app.session.doc.occurrences.len(), occurrences + 4);
    assert_eq!(h.app.session.model.state().bodies.len(), 2);
}

/// Legacy construction names keep selecting that exact plane rather than an origin alias.
#[test]
fn coil_plane_pick_keeps_a_construction_plane_named_like_an_origin_alias() {
    for (alias, base, point, normal) in [
        ("Top", "YZ", [50.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ("Front", "XY", [0.0, 0.0, 50.0], [0.0, 0.0, 1.0]),
        ("Right", "XY", [0.0, 0.0, 50.0], [0.0, 0.0, 1.0]),
    ] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("construct.plane.offset", json!({"base":base,"offset":50,"name":"Old plane"})).unwrap();
        // The command disallows these reserved names today; older designs can contain them.
        let id = h.app.session.doc.features.iter().find(|f| f.name == "Old plane").unwrap().id;
        h.app.session.doc_mut().feature_mut(id).unwrap().name = alias.into();
        assert!(h.app.session.model.rename_feature(&h.app.session.doc, id));
        let authored = h.app.session.doc.to_json();
        let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
        d.preview_enabled = false;
        d.pick(&h.app.session, Sel::Plane { name: alias.into() });
        h.app.dialog = Some(d);
        h.frames(3);
        let d = h.app.dialog.as_ref().unwrap();
        for commands in [crate::dialogs::preview_commands(&h.app, d).unwrap(), apply_commands(&h.app, d).unwrap()] {
            let (_, p) = commands.iter().find(|(id, _)| id == "solid.coil").unwrap();
            assert_eq!(coil_plane_coordinates(&h, &p["base"]), point, "{alias}: {p}");
            assert_eq!(coil_plane_coordinates(&h, &p["axis"]), normal, "{alias}: {p}");
        }
        h.step(&json!({"expect":{"shown":json!(normal).to_string()}})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), authored);
    }
}

/// Once a plane places Coil, its chip is the placement source. Clearing it must not revive an
/// earlier point or leave a misleading Position row that offers to clear a different source.
#[test]
fn coil_plane_pick_replaces_the_previous_point_placement_and_clears_to_origin() {
    for selection in 0..3 {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("construct.plane.offset", json!({"base":"XY","offset":50,"name":"Plane1"})).unwrap();
        let vertex = Sel::Vertex { body: String::new(), point: Vec3::new(7.0, 8.0, 9.0) };
        h.app.session.selection = vec![vertex.clone()];
        let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
        assert!(matches!(&d.kind, Kind::Primitive { at: Some((point, _)), .. } if *point == Vec3::new(7.0, 8.0, 9.0)));
        let plane = Sel::Plane { name: "Plane1".into() };
        match selection {
            0 => d.pick(&h.app.session, plane),
            1 => d.take_box(&h.app.session, vec![plane], false),
            _ => {
                h.app.session.selection = vec![vertex, plane];
                d = Dialog::for_command(&h.app, "solid.coil").unwrap();
            }
        }
        assert!(matches!(&d.kind, Kind::Primitive { at: None, .. }), "the plane replaces the old point, entry={selection}");
        d.preview_enabled = false;
        d.extra.insert("axis".into(), json!("Y"));
        h.app.dialog = Some(d);
        let authored = h.app.session.doc.to_json();
        h.frames(3);
        h.step(&json!({"expect":{"not_shown":"7.00, 8.00, 9.00"}})).unwrap();
        let params = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.clone();
        assert_eq!(coil_plane_coordinates(&h, &params["base"]), [0.0, 0.0, 50.0]);
        assert_eq!(params["axis"], json!("Y"), "later typed Direction remains authoritative");
        h.step(&json!({"click":{"handle":"dialog:clear:Profile"}})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs[0].items.is_empty());
        assert!(matches!(&d.kind, Kind::Primitive { at: None, .. }));
        for commands in [crate::dialogs::preview_commands(&h.app, d).unwrap(), apply_commands(&h.app, d).unwrap()] {
            let p = &commands.iter().find(|(id, _)| id == "solid.coil").unwrap().1;
            assert!(p.get("base").is_none(), "clearing the sole placement source restores the default origin: {p}");
            assert_eq!(p["axis"], json!("Y"), "clearing a placement does not erase typed Direction");
        }
        assert_eq!(h.app.session.doc.to_json(), authored);
    }
}

/// Replacing a saved Coil placement must not leave an invisible base to restore on Clear.
#[test]
fn editing_a_coil_plane_then_clearing_profile_uses_the_origin() {
    for boxed in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("construct.plane.offset", json!({"base":"XY","offset":50,"name":"Plane1"})).unwrap();
        h.app
            .run("solid.coil", json!({"name":"Coil","base":[7,8,9],"axis":[0,0,1],"diameter":40,"revolutions":2,"pitch":8,"section_size":3}))
            .unwrap();
        let feature = h.app.session.doc.find_feature("Coil").unwrap().id;
        let authored = h.app.session.doc.to_json();
        let mut d = for_feature(&h.app, feature, None).unwrap();
        d.preview_enabled = false;
        assert_eq!(coil_plane_coordinates(&h, &d.extra["base"]), [7.0, 8.0, 9.0]);
        assert_eq!(coil_plane_coordinates(&h, &d.extra["axis"]), [0.0, 0.0, 1.0]);
        for commands in [preview_commands(&h.app, &d).unwrap(), apply_commands(&h.app, &d).unwrap()] {
            assert_eq!(commands[0].0, "timeline.redefine");
            let p = &commands[0].1["params"];
            assert_eq!(coil_plane_coordinates(&h, &p["base"]), [7.0, 8.0, 9.0], "an unchanged edit retains saved placement");
            assert_eq!(coil_plane_coordinates(&h, &p["axis"]), [0.0, 0.0, 1.0]);
        }
        d.active = 0;
        let plane = Sel::Plane { name: "Plane1".into() };
        if boxed {
            d.take_box(&h.app.session, vec![plane], false);
        } else {
            d.pick(&h.app.session, plane);
        }
        d.extra.insert("axis".into(), json!("Y"));
        for commands in [preview_commands(&h.app, &d).unwrap(), apply_commands(&h.app, &d).unwrap()] {
            let p = &commands[0].1["params"];
            assert_eq!(coil_plane_coordinates(&h, &p["base"]), [0.0, 0.0, 50.0]);
            assert_eq!(p["axis"], json!("Y"));
        }
        h.app.dialog = Some(d);
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:clear:Profile"}})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs[0].items.is_empty());
        for commands in [preview_commands(&h.app, d).unwrap(), apply_commands(&h.app, d).unwrap()] {
            assert_eq!(commands[0].0, "timeline.redefine");
            let p = &commands[0].1["params"];
            assert!(p.get("base").is_none(), "clearing the replacement plane must not resurrect the saved base, boxed={boxed}: {p}");
            assert_eq!(p["axis"], json!("Y"), "typed Direction survives placement clearing");
        }
        assert_eq!(h.app.session.doc.to_json(), authored, "building preview and OK commands does not author a change");
    }
}

/// An explicit Axis continues to supply its own origin after the edited Profile is cleared.
#[test]
fn editing_a_coil_plane_then_clearing_profile_keeps_the_explicit_axis() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":50,"name":"Plane1"})).unwrap();
    h.app.run("construct.axis.two_points", json!({"a":{"point":[20,10,-20]},"b":{"point":[40,10,-20]},"name":"CoilAxis"})).unwrap();
    h.app.run("solid.coil", json!({"name":"Coil","base":[7,8,9],"axis":[0,0,1],"diameter":40,"revolutions":2,"pitch":8,"section_size":3})).unwrap();
    let feature = h.app.session.doc.find_feature("Coil").unwrap().id;
    let authored = h.app.session.doc.to_json();
    let mut d = for_feature(&h.app, feature, None).unwrap();
    d.preview_enabled = false;
    assert_eq!(coil_plane_coordinates(&h, &d.extra["base"]), [7.0, 8.0, 9.0]);
    assert_eq!(coil_plane_coordinates(&h, &d.extra["axis"]), [0.0, 0.0, 1.0]);
    d.active = 0;
    d.pick(&h.app.session, Sel::Plane { name: "Plane1".into() });
    d.active = 1;
    d.pick(&h.app.session, Sel::Axis { name: "CoilAxis".into() });
    h.app.dialog = Some(d);
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:clear:Profile"}})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(d.inputs[0].items.is_empty());
    assert_eq!(d.inputs[1].items, vec![Sel::Axis { name: "CoilAxis".into() }]);
    for commands in [preview_commands(&h.app, d).unwrap(), apply_commands(&h.app, d).unwrap()] {
        assert_eq!(commands[0].0, "timeline.redefine");
        let p = &commands[0].1["params"];
        assert_eq!(p["axis"], json!("CoilAxis"), "the explicit Axis remains linked");
        assert_eq!(coil_plane_coordinates(&h, &p["base"]), [20.0, 10.0, -20.0]);
    }
    assert_eq!(h.app.session.doc.to_json(), authored);
}
