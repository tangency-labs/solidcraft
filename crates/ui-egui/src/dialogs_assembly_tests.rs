#[test]
fn secondary_alignment_panel_is_available_for_joint_creation() {
    let mut app = two_boxes();
    app.start("joint.create");
    let dialog = app.dialog.as_ref().expect("joint creation dialog");
    assert_eq!(dialog.inputs.len(), 4, "two primary origins and two optional secondary alignments");
}

use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, edge_sel, face_sel, preview_commands};

#[test]
fn coderabbit_review_alignment_accepts_circular_edges_without_a_cylindrical_wall() {
    let mut app = two_boxes();
    app.run("component.activate", json!({"component":"B"})).unwrap();
    app.run("solid.cone", json!({"radius":5,"top_radius":2,"height":10,"base":[40,10,20],"operation":"new"})).unwrap();
    app.run("component.activate", json!({"component":"root"})).unwrap();
    let state = app.session.model.state();
    let body = state.body("Body3").unwrap();
    let record = body.body.record();
    let index = record
        .edges
        .iter()
        .position(|edge| {
            matches!(edge.curve, solidcore_record::brep::Curve::Circle { .. })
                && edge.uses.iter().all(|u| {
                    !matches!(
                        solidcraft_engine::kernel::face_surface(&body.body, u.face as usize),
                        Some(solidcraft_engine::kernel::FaceSurface::Cylinder { .. })
                    )
                })
        })
        .expect("the cone has an exact circular edge without a cylindrical wall");
    let point = body.mesh().edges[index][0];
    let edge = Sel::Edge { body: "Body3".into(), index, point };
    let reference = alignment_pick(&app.session, &edge, true).expect("an exact circular edge remains selectable without a cylinder");
    assert_eq!(reference["reference"]["type"], "edge");
    app.start("joint.create");
    let mut dialog = app.dialog.take().unwrap();
    for point in [Vec3::new(20.0, 10.0, 5.0), Vec3::new(50.0, 10.0, 5.0)] {
        dialog.pick(&app.session, face_sel(&app.session, point).unwrap());
    }
    let Kind::Assembly(Asm::Joint(form)) = &dialog.kind else { panic!() };
    dialog.active = form.alignment_order.iter().position(|side| *side == 1).unwrap() + 2;
    assert!(alignment_accepts(&app.session, &dialog, &edge));
    dialog.pick(&app.session, edge);
    let commands = apply_commands(&app, &dialog).unwrap();
    assert_eq!(commands[0].1["b_alignment"]["reference"]["type"], "edge");
    app.session.scratch().execute(&commands[0].0, &commands[0].1).unwrap();
}

#[test]
fn coderabbit_review_alignment_resolves_component_origin_datums_and_rejects_nested_paths() {
    let mut app = two_boxes();
    let occurrence = app.session.doc.occurrences.iter().find(|o| o.name == "B:1").unwrap().id;
    for (selection, kind, field, item) in [
        (Sel::Axis { name: format!("o:{occurrence}/origin/X") }, "origin_axis", "axis", "X"),
        (Sel::Plane { name: format!("o:{occurrence}/origin/YZ") }, "origin_plane", "plane", "YZ"),
    ] {
        let reference =
            alignment_pick(&app.session, &selection, false).expect("a top-level component origin datum is a persistent alignment reference");
        assert_eq!(reference["occurrence"], occurrence);
        assert_eq!(reference["reference"]["type"], kind);
        assert_eq!(reference["reference"][field], item);
        app.start("joint.create");
        let mut dialog = app.dialog.take().unwrap();
        for point in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
            dialog.pick(&app.session, face_sel(&app.session, point).unwrap());
        }
        let Kind::Assembly(Asm::Joint(form)) = &dialog.kind else { panic!() };
        dialog.active = form.alignment_order.iter().position(|side| *side == 1).unwrap() + 2;
        assert!(alignment_accepts(&app.session, &dialog, &selection));
        dialog.pick(&app.session, selection);
        let commands = apply_commands(&app, &dialog).unwrap();
        assert_eq!(commands[0].1["b_alignment"]["reference"]["type"], kind);
        assert_eq!(commands[0].1["b_alignment"]["occurrence"], occurrence);
        app.session.scratch().execute(&commands[0].0, &commands[0].1).unwrap();
    }
    let parent = app.session.doc.occurrences.iter().find(|o| o.name == "A:1").unwrap().id;
    for selection in
        [Sel::Axis { name: format!("o:{parent}/{occurrence}/origin/X") }, Sel::Plane { name: format!("o:{parent}/{occurrence}/origin/YZ") }]
    {
        assert!(alignment_pick(&app.session, &selection, false).is_err(), "nested datums remain outside the supported scope");
    }
    assert_eq!(alignment_pick(&app.session, &Sel::Axis { name: "X".into() }, false).unwrap()["occurrence"], 0);
    assert_eq!(alignment_pick(&app.session, &Sel::Plane { name: "YZ".into() }, false).unwrap()["occurrence"], 0);
}

#[test]
fn coderabbit_review_no_cylinder_circle_marker_uses_edge_axis() {
    let mut app = two_boxes();
    app.run("component.activate", json!({"component":"B"})).unwrap();
    app.run("solid.cone", json!({"radius":5,"top_radius":2,"height":10,"base":[40,15,20],"operation":"new"})).unwrap();
    app.run("component.activate", json!({"component":"root"})).unwrap();
    let state = app.session.model.state();
    let body = state.body("Body3").unwrap();
    let record = body.body.record();
    let index = record
        .edges
        .iter()
        .position(|edge| {
            matches!(edge.curve, solidcore_record::brep::Curve::Circle { .. })
                && edge.uses.iter().all(|u| {
                    !matches!(
                        solidcraft_engine::kernel::face_surface(&body.body, u.face as usize),
                        Some(solidcraft_engine::kernel::FaceSurface::Cylinder { .. })
                    )
                })
        })
        .expect("the cone has an exact circular edge without a cylindrical wall");
    let point = body.mesh().edges[index][0];
    let edge = Sel::Edge { body: "Body3".into(), index, point };
    app.start("joint.create");
    let mut dialog = app.dialog.take().unwrap();
    for point in [Vec3::new(20.0, 10.0, 5.0), Vec3::new(50.0, 10.0, 5.0)] {
        dialog.pick(&app.session, face_sel(&app.session, point).unwrap());
    }
    let Kind::Assembly(Asm::Joint(form)) = &dialog.kind else { panic!() };
    dialog.active = form.alignment_order.iter().position(|side| *side == 1).unwrap() + 2;
    assert!(alignment_accepts(&app.session, &dialog, &edge));
    let mut confirmed = dialog.clone();
    confirmed.pick(&app.session, edge);
    let commands = apply_commands(&app, &confirmed).unwrap();
    assert_eq!(commands[0].1["b_alignment"]["reference"]["type"], "edge");
    app.session.scratch().execute(&commands[0].0, &commands[0].1).unwrap();
    app.dialog = Some(dialog);
    let marker = review_alignment_marker(&mut app, crate::viewport::Hit::Edge { body: "Body3".into(), index, mid: point, point })
        .expect("the accepted circular edge has graphical feedback");
    assert!(marker.z.x.abs() < 1e-6, "the cone's edge axis is along Z: {:?}", marker.z);
    assert!(marker.z.y.abs() < 1e-6, "the marker must not point toward the displaced circle centre: {:?}", marker.z);
    assert!((marker.z.z.abs() - 1.0).abs() < 1e-6, "the cone's edge axis is along Z: {:?}", marker.z);
}

#[test]
fn coderabbit_review_alignment_hover_cache_tracks_rollback() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.start("joint.create");
    h.app.dialog.as_mut().unwrap().active = 2;
    h.frames(2);
    let projection = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let pointer = egui::pos2(700.0, 400.0);
    let first = Some((
        crate::viewport::Hit::Face { body: "Body2".into(), index: 3, point: Vec3::ZERO },
        Some(Sel::Face { body: "Body2".into(), index: 3, point: Vec3::ZERO }),
    ));
    let authored = Some((
        crate::viewport::Hit::Face { body: "Body1".into(), index: 4, point: Vec3::ZERO },
        Some(Sel::Face { body: "Body1".into(), index: 4, point: Vec3::ZERO }),
    ));
    assert_eq!(crate::viewport::stable_alignment_hover(&mut h.app, &projection, Some(pointer), first.clone()), first);
    assert_eq!(
        crate::viewport::stable_alignment_hover(&mut h.app, &projection, Some(pointer), None),
        first,
        "the unchanged pose keeps the stable candidate"
    );
    h.app.preview.rolled_back = true;
    assert_eq!(
        crate::viewport::stable_alignment_hover(&mut h.app, &projection, Some(pointer), authored.clone()),
        authored,
        "rollback must discard the candidate from the preview pose"
    );
    assert_eq!(crate::viewport::stable_alignment_hover(&mut h.app, &projection, Some(pointer), None), authored);
    h.app.preview.rolled_back = false;
    assert!(
        crate::viewport::stable_alignment_hover(&mut h.app, &projection, Some(pointer), None).is_none(),
        "returning to the preview must discard an authored-pose candidate"
    );
}

#[test]
fn secondary_alignment_edit_primary_replacement_uses_the_new_occurrence() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    app.run("component.create", json!({"name":"C"})).unwrap();
    app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[80,0,0]})).unwrap();
    app.run("component.activate", json!({"component":"root"})).unwrap();
    let mut edit = edit_joint(&app, id).unwrap();
    assert_eq!(edit.inputs.len(), 4);
    assert!(alignment_side(&edit.kind, 0).is_none(), "primary rows stay primary while editing");
    let a = edit.inputs[0].items.clone();
    edit.active = 1;
    edit.pick(&app.session, face_sel(&app.session, Vec3::new(90.0, 10.0, 0.0)).unwrap());
    assert_eq!(edit.inputs[0].items, a);
    edit.active = 2;
    let candidate = face_sel(&app.session, Vec3::new(90.0, 20.0, 5.0)).unwrap();
    assert!(alignment_accepts(&app.session, &edit, &candidate));
    edit.pick(&app.session, candidate);
    let params = apply_commands(&app, &edit).unwrap()[0].1.clone();
    assert_eq!(params["b"]["occurrence"], params["b_alignment"]["occurrence"]);
    app.run("joint.edit", params).unwrap();
    let joint = &app.session.doc.assembly.joints[0];
    assert_ne!(joint.a.occurrence, joint.b.occurrence);
}

#[test]
fn secondary_alignment_edit_woken_face_point_maps_back_from_the_preview() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    let Sel::Face { body, index, point } = face_sel(&app.session, Vec3::new(10.0, 10.0, 20.0)).unwrap() else { panic!() };
    app.dialog = edit_joint(&app, id);
    if let Kind::Assembly(Asm::Joint(f)) = &mut app.dialog.as_mut().unwrap().kind {
        f.angle = "27 deg".into();
        f.flip = true;
    }
    settle_alignment_update(&mut app, &egui::Context::default());
    let interaction = app.preview.interaction.as_ref().unwrap();
    let back = interaction.to_document[&body];
    let shown = solidcraft_engine::doc::mat_inverse(&back).unwrap();
    let face = Sel::Face { body: body.clone(), index, point: solidcraft_engine::doc::apply_point(&shown, point) };
    let mut scratch = app.session.scratch();
    scratch.doc = interaction.document.clone();
    let (snap, source) = solidcraft_engine::snap::snap_points_from(&scratch, &face)
        .into_iter()
        .find(|(p, from)| p.kind == solidcraft_engine::snap::SnapKind::End && from.face.is_some())
        .unwrap();
    let expected = solidcraft_engine::doc::apply_point(&back, snap.at);
    assert!(expected.dist(snap.at) > 1e-3, "the preview actually moves the selected point");
    app.viewport.woken.item = Some(face);
    app.dialog.as_mut().unwrap().active = 1;
    let hit = crate::viewport::Hit::Vertex { body: body.clone(), point: snap.at };
    crate::viewport::dialog_pick(&mut app, &hit, Sel::Vertex { body, point: snap.at });
    let dialog = app.dialog.as_ref().unwrap();
    let Sel::Vertex { point, .. } = dialog.inputs[1].items[0] else { panic!() };
    assert!(point.dist(expected) < 1e-6, "primary picks are authored in the document pose while editing");
    let (_, _, from) = dialog.inputs[1].from_face.first().expect("preserve the face, corner and edge association");
    assert!(from.face.unwrap().dist(solidcraft_engine::doc::apply_point(&back, source.face.unwrap())) < 1e-6);
    assert!(from.edge.unwrap().dist(solidcraft_engine::doc::apply_point(&back, source.edge.unwrap())) < 1e-6);
}

/// Components A (grounded) and B: a 20 x 20 x 10 box each, B 30 mm along X.
fn two_boxes() -> SolveApp {
    let mut s = Session::default();
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 30)], |id, p| s.execute(id, &p).unwrap());
    SolveApp::new(s, Services::default())
}

fn round_plates() -> SolveApp {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 150)] {
        s.execute("component.activate", &json!({"component":"root"})).unwrap();
        s.execute("component.create", &json!({"name":name})).unwrap();
        s.execute("solid.box", &json!({"length":100,"width":80,"height":10,"corner":[x,0,0]})).unwrap();
        for (a, b) in [(20, 20), (80, 20), (20, 60), (80, 60)] {
            s.execute("solid.hole", &json!({"position":[x+a,b,10],"diameter":6})).unwrap();
        }
    }
    s.execute("component.activate", &json!({"component":"root"})).unwrap();
    s.execute("occurrence.ground", &json!({"occurrence":"A:1","grounded":true})).unwrap();
    SolveApp::new(s, Services::default())
}

fn rim(s: &Session, body: &str, center: Vec3) -> (crate::viewport::Hit, Sel) {
    let st = s.world_state().unwrap();
    let b = st.body(body).unwrap();
    let mesh = b.mesh();
    let index = mesh.edges.iter().position(|e| circle_of(e).is_some_and(|(c, _)| c.dist(center) < 1e-6)).unwrap();
    let point = mesh.edges[index][0];
    (crate::viewport::Hit::Edge { body: body.into(), index, mid: point, point }, Sel::Edge { body: body.into(), index, point })
}

#[test]
fn secondary_alignment_hover_preference_defaults_on_and_persists_transactions() {
    let old: crate::prefs::Prefs = serde_json::from_value(json!({})).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap()["joint_auto_alignment"], true);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let before = h.app.session.doc.to_json();
    h.call("ui.prefs", json!({"open":"Design"}));
    h.app.prefs_window.shell.filter = "joint alignment".into();
    h.frames(5);
    crate::prefs::reveal_preference(&mut h, "prefs:joint_auto_alignment");
    h.step(&json!({"click":{"handle":"prefs:joint_auto_alignment"}})).unwrap();
    assert_eq!(
        serde_json::to_value(&h.app.preferences).unwrap()["joint_auto_alignment"],
        false,
        "home={} window={:?} body={:?} control={:?}",
        h.app.home.open,
        crate::scenario::rect_of("prefs:window"),
        crate::scenario::rect_of("dialog:page_body"),
        crate::scenario::rect_of("prefs:joint_auto_alignment")
    );
    let mut restored = SolveApp::new(Session::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    // Unapplied preferences stay transactional, including what is saved to disk.
    assert_eq!(serde_json::to_value(&restored.preferences).unwrap()["joint_auto_alignment"], true);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_auto_alignment"], true);
    h.call("ui.prefs", json!({"open":"Design","joint_auto_alignment":false}));
    h.call("ui.prefs", json!({"button":"apply"}));
    restored.load_prefs(&h.app.prefs());
    assert_eq!(serde_json::to_value(&restored.preferences).unwrap()["joint_auto_alignment"], false);
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_auto_alignment"], true);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_auto_alignment"], false);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.session.undo.is_empty());
}

#[test]
fn secondary_alignment_hover_preference_disables_automatic_rim_alignment_but_keeps_explicit_picks() {
    let mut app = round_plates();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(23.0, 20.0, 9.0), Vec3::new(173.0, 20.0, 9.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    let (_, moving) = rim(&app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
    d.pick(&app.session, moving);
    let ctx = egui::Context::default();
    app.dialog = Some(d.clone());
    settle_alignment_update(&mut app, &ctx);
    let confirmed_pose = app.preview.interaction.as_ref().unwrap().to_document["Body2"];
    // Current main's default round frame may already match a particular counterpart.
    // Choose a different valid counterpart so this test measures an actual transient change.
    let alternatives: Vec<_> = [Vec3::new(80.0, 20.0, 10.0), Vec3::new(20.0, 60.0, 10.0), Vec3::new(80.0, 60.0, 10.0)]
        .into_iter()
        .map(|p| rim(&app.session, "Body1", p))
        .collect();
    let (hit, fixed) = alternatives
        .into_iter()
        .find(|(hit, _)| {
            app.viewport.hover = Some(hit.clone());
            alignment_hover_commands(&app, app.dialog.as_ref().unwrap())
                .and_then(|commands| world_preview(&app.session, &commands, Colors::from_tokens(&Tokens::get())))
                .and_then(Result::ok)
                .and_then(|b| b.interaction)
                .is_some_and(|i| i.to_document["Body2"] != confirmed_pose)
        })
        .expect("a counterpart that turns the part");
    app.viewport.hover = Some(hit);
    settle_alignment_update(&mut app, &ctx);
    assert_ne!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], confirmed_pose);
    let before = apply_commands(&app, &d).unwrap();
    let doc = app.session.doc.clone();
    assert!(alignment_hover_commands(&app, &d).is_some());
    let mut settings = serde_json::to_value(&app.preferences).unwrap();
    settings["joint_auto_alignment"] = json!(false);
    app.preferences = serde_json::from_value(settings).unwrap();
    assert!(alignment_hover_commands(&app, &d).is_none());
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], confirmed_pose);
    assert_eq!(apply_commands(&app, &d).unwrap(), before);
    assert!(d.inputs[3].items.is_empty());
    d.pick(&app.session, fixed);
    let confirmed = apply_commands(&app, &d).unwrap();
    assert_eq!(confirmed[0].1["a_alignment"]["reference"]["type"], "circle_center");
    app.session.scratch().execute(&confirmed[0].0, &confirmed[0].1).unwrap();
    assert_eq!(app.session.doc, doc);
}

#[test]
fn secondary_alignment_primary_pick_preference_defaults_off_and_persists_transactions() {
    let old: crate::prefs::Prefs = serde_json::from_value(json!({})).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap()["joint_moving_first"], false);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.call("ui.prefs", json!({"open":"Design"}));
    h.app.prefs_window.shell.filter = "selection order".into();
    h.frames(5);
    crate::prefs::reveal_preference(&mut h, "prefs:joint_moving_first");
    h.step(&json!({"click":{"handle":"prefs:joint_moving_first"}})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_moving_first"], true);
    let mut restored = SolveApp::new(Session::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(serde_json::to_value(&restored.preferences).unwrap()["joint_moving_first"], false);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_moving_first"], false);
    h.call("ui.prefs", json!({"open":"Design","joint_moving_first":true}));
    h.call("ui.prefs", json!({"button":"apply"}));
    restored.load_prefs(&h.app.prefs());
    assert_eq!(serde_json::to_value(&restored.preferences).unwrap()["joint_moving_first"], true);
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_moving_first"], false);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_moving_first"], true);
}

#[test]
fn secondary_alignment_primary_pick_preference_normalizes_both_sequences_and_snapshots_the_dialog() {
    for moving_first in [false, true] {
        let mut app = round_plates();
        let mut settings = serde_json::to_value(&app.preferences).unwrap();
        settings["joint_moving_first"] = json!(moving_first);
        app.preferences = serde_json::from_value(settings).unwrap();
        app.start("joint.create");
        let mut d = app.dialog.take().unwrap();
        assert_eq!(d.inputs[0].label, "Component 1");
        assert_eq!(d.inputs[1].label, "Component 2");
        let (_, a) = rim(&app.session, "Body1", Vec3::new(20.0, 20.0, 10.0));
        let (_, b) = rim(&app.session, "Body2", Vec3::new(170.0, 20.0, 0.0));
        let picks = if moving_first { [b, a] } else { [a, b] };
        d.pick(&app.session, picks[0].clone());
        assert_eq!(d.active, 1);
        let mut settings = serde_json::to_value(&app.preferences).unwrap();
        settings["joint_moving_first"] = json!(!moving_first);
        app.preferences = serde_json::from_value(settings).unwrap();
        d.pick(&app.session, picks[1].clone());
        assert_eq!(d.active, 2);
        let p = apply_commands(&app, &d).unwrap()[0].1.clone();
        assert_eq!(p["a"]["occurrence"], occ(&app, "A"));
        assert_eq!(p["b"]["occurrence"], occ(&app, "B"));
        let (_, moving) = rim(&app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
        d.pick(&app.session, moving);
        let p = apply_commands(&app, &d).unwrap()[0].1.clone();
        assert!(p["b_alignment"]["reference"]["name"].is_string());
        crate::dialogs::run_dialog(&mut app, &d).unwrap();
        let before = app.session.doc.clone();
        let edit = edit_joint(&app, before.assembly.joints[0].id).unwrap();
        let commands = apply_commands(&app, &edit).unwrap();
        let mut scratch = app.session.scratch();
        scratch.execute(&commands[0].0, &commands[0].1).unwrap();
        assert_eq!(scratch.doc.assembly.joints, before.assembly.joints);
    }
}

#[test]
fn secondary_alignment_primary_hole_rim_clicks_persist_round_origins_before_alignment() {
    for reverse in [false, true] {
        let mut app = round_plates();
        app.start("joint.create");
        let mut d = app.dialog.take().unwrap();
        let (_, a) = rim(&app.session, "Body1", Vec3::new(20.0, 20.0, 10.0));
        let (_, b) = rim(&app.session, "Body2", Vec3::new(170.0, 20.0, 0.0));
        let picks = if reverse { [b, a] } else { [a, b] };
        for pick in picks {
            d.pick(&app.session, pick);
        }
        let commands = apply_commands(&app, &d).unwrap();
        let mut base = app.session.scratch();
        base.execute(&commands[0].0, &commands[0].1).unwrap();
        let before = format!("{:?}", base.doc.assembly.joints[0]);
        let primary_picks = [d.inputs[0].items.clone(), d.inputs[1].items.clone()];
        let (_, moving) = rim(&app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
        d.pick(&app.session, moving);
        assert_eq!([d.inputs[0].items.clone(), d.inputs[1].items.clone()], primary_picks, "a secondary click must not replace either primary pick");
        let commands = apply_commands(&app, &d).unwrap();
        let mut preview = app.session.scratch();
        let result = preview.execute(&commands[0].0, &commands[0].1);
        assert!(result.is_ok(), "primary rim origins: {before}; alignment: {result:?}");
        let joint = &preview.doc.assembly.joints[0];
        for origin in [&joint.a, &joint.b] {
            assert!(matches!(&origin.snap, solidcraft_engine::doc::joints::Snap::CircleCenter { face: Some(_), end: Some(_), .. }), "{origin:?}");
        }
        let moving_side = usize::from(!reverse);
        assert_eq!(alignment_side(&d.kind, 2), Some(moving_side));
        assert!(commands[0].1[if reverse { "a" } else { "b" }].get("circle").is_some(), "the persisted round identity above is authoritative");
    }
}

#[test]
fn secondary_alignment_nonround_primary_edge_disables_alignment_before_secondary_picking() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    let st = app.session.world_state().unwrap();
    let mesh = st.body("Body1").unwrap().mesh();
    let index = mesh.edges.iter().position(|e| circle_of(e).is_none()).unwrap();
    let point = mesh.edges[index][0];
    d.pick(&app.session, Sel::Edge { body: "Body1".into(), index, point });
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let base = apply_commands(&app, &d).unwrap();
    app.session.scratch().execute(&base[0].0, &base[0].1).unwrap();
    assert!(!alignment_input_available(&d, 2));
    assert!(!alignment_input_available(&d, 3));
    let Kind::Assembly(Asm::Joint(f)) = &d.kind else { panic!() };
    assert!(f.alignment_origin_problem.is_some(), "explain the limitation before the first secondary pick");
    d.active = 2;
    let secondary = face_sel(&app.session, Vec3::new(40.0, 20.0, 5.0)).unwrap();
    assert!(!alignment_accepts(&app.session, &d, &secondary));
    d.pick(&app.session, secondary);
    assert!(d.inputs[2].items.is_empty());
}

#[test]
fn secondary_alignment_round_origins_start_with_the_actual_moving_occurrence_in_either_pick_order() {
    for reverse in [false, true] {
        let mut app = round_plates();
        app.start("joint.create");
        let mut d = app.dialog.take().unwrap();
        let mut picks = [Vec3::new(23.0, 20.0, 9.0), Vec3::new(173.0, 20.0, 9.0)];
        if reverse {
            picks.reverse();
        }
        for p in picks {
            d.pick(&app.session, face_sel(&app.session, p).unwrap());
        }
        assert_eq!(alignment_side(&d.kind, 2), Some(if reverse { 0 } else { 1 }));
        let (moving, _) = rim(&app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
        let (fixed, _) = rim(&app.session, "Body1", Vec3::new(80.0, 20.0, 10.0));
        assert!(d.candidate(&app.session, &moving).is_some());
        assert!(d.candidate(&app.session, &fixed).is_none());
        assert!(!alignment_input_available(&d, 3), "the fixed component is second");
        app.dialog = Some(d.clone());
        app.viewport.hover = Some(moving);
        assert!(alignment_hover_commands(&app, &d).is_none(), "no confirmed alignment: highlight only");
    }
}

#[test]
fn secondary_alignment_order_uses_measured_preview_motion_without_an_explicit_ground() {
    for reverse in [false, true] {
        let mut app = round_plates();
        app.run("occurrence.ground", json!({"occurrence":"A:1","grounded":false})).unwrap();
        app.start("joint.create");
        let mut d = app.dialog.take().unwrap();
        let mut picks = [Vec3::new(23.0, 20.0, 9.0), Vec3::new(173.0, 20.0, 9.0)];
        if reverse {
            picks.reverse();
        }
        for p in picks {
            d.pick(&app.session, face_sel(&app.session, p).unwrap());
        }
        app.dialog = Some(d);
        settle_alignment_update(&mut app, &egui::Context::default());
        let view = app.preview.interaction.as_ref().unwrap();
        let a = !solidcraft_engine::doc::is_identity(&view.to_document["Body1"]);
        let b = !solidcraft_engine::doc::is_identity(&view.to_document["Body2"]);
        assert_ne!(a, b, "this fixture has exactly one actual moving body");
        let expected = if a { usize::from(reverse) } else { usize::from(!reverse) };
        let d = app.dialog.as_ref().unwrap();
        assert_eq!(alignment_side(&d.kind, 2), Some(expected));
        assert!(alignment_input_available(d, 2));
        let d = app.dialog.take().unwrap();
        crate::dialogs::run_dialog(&mut app, &d).unwrap();
        app.dialog = edit_joint(&app, app.session.doc.assembly.joints[0].id);
        settle_alignment_update(&mut app, &egui::Context::default());
        let edit = app.dialog.as_ref().unwrap();
        assert_eq!(alignment_side(&edit.kind, 2), Some(expected));
        assert!(alignment_input_available(edit, 0), "an already solved floating joint reuses the solver's actual fixed root");
    }
}

#[test]
fn secondary_alignment_round_origins_zero_then_one_confirmed_hover_leave_and_click() {
    let mut app = round_plates();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(23.0, 20.0, 9.0), Vec3::new(173.0, 20.0, 9.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    let (moving, sel) = rim(&app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
    app.dialog = Some(d);
    let ctx = egui::Context::default();
    settle_alignment_update(&mut app, &ctx);
    let zero = app.preview.interaction.as_ref().unwrap().to_document["Body2"];
    app.viewport.hover = Some(moving.clone());
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], zero);
    assert!(app.dialog.as_ref().unwrap().inputs[2].items.is_empty());
    let mut d = app.dialog.take().unwrap();
    d.pick(&app.session, sel);
    let params = apply_commands(&app, &d).unwrap()[0].1.clone();
    assert_eq!(params["b_alignment"]["reference"]["type"], "circle_center");
    assert!(params.get("a_alignment").is_none());
    app.dialog = Some(d);
    app.viewport.hover = None;
    settle_alignment_update(&mut app, &ctx);
    let authored = app.preview.interaction.as_ref().unwrap().to_document["Body2"];
    let doc = app.session.doc.clone();
    let candidates: Vec<_> = [Vec3::new(80.0, 20.0, 10.0), Vec3::new(20.0, 60.0, 10.0)].into_iter().map(|p| rim(&app.session, "Body1", p)).collect();
    let (hit, sel) = candidates
        .into_iter()
        .find(|(hit, _)| {
            app.viewport.hover = Some(hit.clone());
            alignment_hover_commands(&app, app.dialog.as_ref().unwrap())
                .and_then(|c| world_preview(&app.session, &c, crate::preview::Colors::from_tokens(&solidcraft_theme::Tokens::get())))
                .and_then(Result::ok)
                .and_then(|b| b.interaction)
                .is_some_and(|i| i.to_document["Body2"] != authored)
        })
        .unwrap();
    app.viewport.hover = Some(hit.clone());
    settle_alignment_update(&mut app, &ctx);
    assert_ne!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], authored);
    assert!(app.dialog.as_ref().unwrap().inputs[3].items.is_empty());
    assert_eq!(app.session.doc, doc);
    app.viewport.hover = None;
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], authored);
    crate::viewport::dialog_pick(&mut app, &hit, sel);
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.dialog.as_ref().unwrap().inputs[3].items.len(), 1);
    assert_eq!(app.session.doc, doc);
    let d = app.dialog.take().unwrap();
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    let edit = edit_joint(&app, app.session.doc.assembly.joints[0].id).unwrap();
    assert_eq!(alignment_side(&edit.kind, 2), Some(1));
    assert_eq!(stored_alignment(&edit.kind, 2).unwrap()["reference"]["type"], "circle_center");
}

#[test]
fn secondary_alignment_circle_centers_are_automatic_without_technical_mode_widgets() {
    let mut h = crate::scenario::Harness::new();
    h.app = round_plates();
    h.app.start("joint.create");
    let mut d = h.app.dialog.take().unwrap();
    for p in [Vec3::new(23.0, 20.0, 9.0), Vec3::new(173.0, 20.0, 9.0)] {
        d.pick(&h.app.session, face_sel(&h.app.session, p).unwrap());
    }
    h.app.dialog = Some(d);
    h.frames(3);
    let (hit, _) = rim(&h.app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
    assert!(h.step(&json!({"click":{"handle":"dialog:circle-axis:1"}})).is_err(), "Axis mode must not be offered");
    assert!(h.step(&json!({"click":{"handle":"dialog:circle-center:1"}})).is_err(), "Center is automatic, not a mode control");
    assert!(h.app.dialog.as_ref().unwrap().candidate(&h.app.session, &hit).is_some());
    let (same, _) = rim(&h.app.session, "Body2", Vec3::new(170.0, 20.0, 10.0));
    assert!(h.app.dialog.as_ref().unwrap().candidate(&h.app.session, &same).is_none(), "coincident center cannot define clocking");
}

#[test]
fn secondary_alignment_cylinder_face_keeps_directional_reference_compatibility() {
    let mut app = two_boxes();
    app.run("component.activate", json!({"component":"A"})).unwrap();
    app.run("parameters.change", json!({"name":"holeDiameter","expression":"6 mm"})).unwrap();
    app.run("solid.hole", json!({"position":[10,10,10],"diameter":"holeDiameter"})).unwrap();
    app.run("component.activate", json!({"component":"root"})).unwrap();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(20.0, 10.0, 5.0), Vec3::new(30.0, 10.0, 5.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 20.0, 5.0)).unwrap());
    d.active = 3;
    let Sel::Face { body, index, point } = face_sel(&app.session, Vec3::new(13.0, 10.0, 9.0)).unwrap() else { panic!() };
    let hit = crate::viewport::Hit::Face { body, index, point };
    let sel = d.candidate(&app.session, &hit).unwrap();
    let snap = snap_of(&app.session, &sel).unwrap();
    assert_eq!(snap.kind, SnapKind::CircleCenter);
    assert!(snap.at.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-6);
    d.pick(&app.session, sel);
    let commands = apply_commands(&app, &d).unwrap();
    assert_eq!(commands[0].1["a_alignment"]["reference"]["type"], "cylinder");
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    app.run("parameters.change", json!({"name":"holeDiameter","expression":"8 mm"})).unwrap();
    let saved = serde_json::to_value(&*app.session.doc).unwrap();
    let loaded: solidcraft_engine::doc::Document = serde_json::from_value(saved).unwrap();
    assert_eq!(loaded.assembly.joints, app.session.doc.assembly.joints);
    let edit = edit_joint(&app, app.session.doc.assembly.joints[0].id).unwrap();
    assert_eq!(stored_alignment(&edit.kind, 3).unwrap()["reference"]["type"], "cylinder");
}

#[test]
fn secondary_alignment_wrong_occurrence_hover_and_click_are_ignored() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0), Vec3::new(40.0, 20.0, 5.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    d.active = 2;
    let before = apply_commands(&app, &d).unwrap();
    let wrong = face_sel(&app.session, Vec3::new(20.0, 10.0, 5.0)).unwrap();
    let Sel::Face { body, index, point } = &wrong else { panic!() };
    let hit = crate::viewport::Hit::Face { body: body.clone(), index: *index, point: *point };
    assert!(d.candidate(&app.session, &hit).is_none());
    d.pick(&app.session, wrong);
    assert_eq!(apply_commands(&app, &d).unwrap(), before);
    app.dialog = Some(d.clone());
    app.viewport.hover = Some(hit);
    assert!(alignment_hover_commands(&app, &d).is_none());
    app.session.scratch().execute(&before[0].0, &before[0].1).unwrap();
    // Same definition, different occurrence is still the wrong path.
    app.run("occurrence.copy", json!({"component":app.session.doc.find_component("B").unwrap(),"translate":[70,0,0]})).unwrap();
    let wrong = face_sel(&app.session, Vec3::new(120.0, 10.0, 5.0)).unwrap();
    let saved = d.inputs[2].items.clone();
    d.pick(&app.session, wrong);
    assert_eq!(d.inputs[2].items, saved);
}

#[test]
fn secondary_alignment_filter_covers_construction_sketch_and_nested_occurrences() {
    let mut app = two_boxes();
    let mut features = Vec::new();
    let mut curves = Vec::new();
    for component in ["A", "B"] {
        app.run("component.activate", json!({"component":component})).unwrap();
        app.run("construct.axis.two_points", json!({"a":{"point":[0,0,0]},"b":{"point":[0,1,0]},"name":format!("{component}Axis")})).unwrap();
        app.run(
            "construct.plane.three_points",
            json!({"a":{"point":[0,0,0]},"b":{"point":[0,1,0]},"c":{"point":[0,0,1]},"name":format!("{component}Plane")}),
        )
        .unwrap();
        app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        features.push(app.session.active_sketch.unwrap());
        app.run("sketch.line", json!({"points":[[0,0],[0,10]]})).unwrap();
        let sketch = *features.last().unwrap();
        curves.push(app.session.model.state().sketch(sketch).unwrap().sketch.curves[0].id.clone());
        app.run("sketch.finish", json!({})).unwrap();
    }
    app.run("component.activate", json!({"component":"root"})).unwrap();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 20.0, 5.0)).unwrap());
    for (side, good, bad) in [(2, "B", "A"), (3, "A", "B")] {
        d.active = side;
        for sel in [
            Sel::Axis { name: format!("{good}Axis") },
            Sel::Plane { name: format!("{good}Plane") },
            Sel::ExternalSketchCurve { sketch: features[3 - side], id: curves[3 - side].clone() },
        ] {
            assert!(
                alignment_accepts(&app.session, &d, &sel),
                "side {side}, selection {sel:?}, form {:?}, owner {:?}, frame {:?}, projection {}",
                d.kind,
                alignment_occurrence(&app.session, &sel),
                primary_alignment_frame(&app.session, &d, 3 - side),
                alignment_projection_valid(&app.session, &d, &sel, 3 - side)
            );
        }
        for sel in [
            Sel::Axis { name: format!("{bad}Axis") },
            Sel::Plane { name: format!("{bad}Plane") },
            Sel::ExternalSketchCurve { sketch: features[side - 2], id: curves[side - 2].clone() },
            Sel::Axis { name: "X".into() },
        ] {
            assert!(!alignment_accepts(&app.session, &d, &sel));
        }
    }
    d.active = 2;
    let a = occ(&app, "A");
    let b = occ(&app, "B");
    app.session.doc_mut().occurrences.iter_mut().find(|o| o.id == b).unwrap().parent = a;
    assert!(!alignment_accepts(&app.session, &d, &Sel::Axis { name: "BAxis".into() }));
}

fn settle_alignment_preview(h: &mut crate::scenario::Harness) {
    h.frames(3);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while h.app.preview.busy {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
        h.frame();
    }
}

#[test]
fn secondary_alignment_feedback_picks_the_solved_geometry() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.start("joint.create");
    let mut d = h.app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&h.app.session, face_sel(&h.app.session, p).unwrap());
    }
    h.app.dialog = Some(d);
    settle_alignment_preview(&mut h);
    assert!(h.app.preview.active);
    h.call("ui.view", json!({"view":"top"}));
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let at = proj.to_screen(Vec3::new(10.0, 10.0, 20.0)).unwrap();
    let hits = crate::viewport::pick_all(&h.app, &proj, at);
    assert!(
        hits.iter().any(|hit| matches!(hit, crate::viewport::Hit::Face {body, ..} if body == "Body2")),
        "the moved part must be pickable at its solved pose: {hits:?}"
    );
    h.app.set_origin_shown(false);
    h.app.viewport.hover = hits.into_iter().find(|hit| matches!(hit, crate::viewport::Hit::Face { body, .. } if body == "Body2"));
    let lit = crate::viewport::build_highlight(&h.app).merged();
    assert!(!lit.tris.is_empty(), "the picked face must also be highlighted");
    for vertex in lit.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>().0 {
        let x = f32::from_le_bytes(vertex[..4].try_into().unwrap());
        assert!((-0.01..=20.01).contains(&x), "feedback must follow the solved pose, not Body2's original x=30..50: {x}");
    }
    assert!(h.app.session.doc.assembly.joints.is_empty());
}

#[test]
fn secondary_alignment_feedback_invalid_first_pick_keeps_the_last_pose() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.start("joint.create");
    let mut d = h.app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&h.app.session, face_sel(&h.app.session, p).unwrap());
    }
    h.app.dialog = Some(d);
    settle_alignment_preview(&mut h);
    let key = h.app.preview.key;
    let invalid = face_sel(&h.app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap();
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, invalid);
    settle_alignment_preview(&mut h);
    assert!(h.app.preview.active, "an invalid/partial secondary pick must not return the part to its original pose");
    assert_eq!(h.app.preview.key, key);
    assert!(h.app.preview.error.is_none(), "a rejected candidate cannot create an authored error");
    assert!(h.app.dialog.as_ref().unwrap().inputs[2].items.is_empty());
    assert!(h.app.session.doc.assembly.joints.is_empty());
}

#[test]
fn secondary_alignment_preview_toggle_clears_and_restores_pickable_pose() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    app.dialog = Some(d);
    let ctx = egui::Context::default();
    settle_alignment_update(&mut app, &ctx);
    assert!(app.preview.active && app.preview.interaction.is_some());
    let doc = app.session.doc.clone();
    app.dialog.as_mut().unwrap().preview_enabled = false;
    crate::preview::update(&mut app, &ctx);
    assert!(!app.preview.active, "the upstream preview toggle must hide the retained joint pose");
    assert!(app.preview.interaction.is_none(), "disabled preview must stop picking solved geometry");
    app.dialog.as_mut().unwrap().preview_enabled = true;
    settle_alignment_update(&mut app, &ctx);
    assert!(app.preview.active && app.preview.interaction.is_some());
    assert_eq!(app.session.doc, doc);
}

fn settle_alignment_update(app: &mut SolveApp, ctx: &egui::Context) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        crate::preview::update(app, ctx);
        if !app.preview.busy {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn secondary_alignment_feedback_hover_is_temporary_and_invalid_hover_restores_authored_pose() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0), Vec3::new(40.0, 20.0, 5.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    let candidate = face_sel(&app.session, Vec3::new(20.0, 10.0, 5.0)).unwrap();
    let Sel::Face { body, index, .. } = &candidate else { panic!() };
    let hit = crate::viewport::Hit::Face { body: body.clone(), index: *index, point: Vec3::new(20.0, 10.0, 5.0) };
    app.dialog = Some(d);
    let ctx = egui::Context::default();
    settle_alignment_update(&mut app, &ctx);
    let authored = app.preview.interaction.as_ref().unwrap().to_document["Body2"];
    let doc = app.session.doc.clone();
    app.viewport.hover = Some(hit.clone());
    settle_alignment_update(&mut app, &ctx);
    assert_ne!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], authored);
    assert!(app.dialog.as_ref().unwrap().inputs[3].items.is_empty());
    assert_eq!(app.session.doc, doc);
    let Sel::Face { index: parallel, .. } = face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap() else { panic!() };
    app.viewport.hover = Some(crate::viewport::Hit::Face { body: body.clone(), index: parallel, point: Vec3::new(10.0, 10.0, 10.0) });
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], authored);
    assert!(app.preview.error.is_none(), "invalid hover cannot become an authored error");
    app.viewport.hover = Some(hit.clone());
    settle_alignment_update(&mut app, &ctx);
    crate::viewport::dialog_pick(&mut app, &hit, candidate);
    app.viewport.hover = None;
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.dialog.as_ref().unwrap().inputs[3].items.len(), 1);
    assert_ne!(app.preview.interaction.as_ref().unwrap().to_document["Body2"], authored);
    assert_eq!(app.session.doc, doc);
    app.dialog = None;
    settle_alignment_update(&mut app, &ctx);
    assert!(!app.preview.active);
    assert!(app.preview.interaction.is_none());
}

#[test]
fn secondary_alignment_feedback_primary_repick_maps_from_preview_to_author_frame() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    let old = d.inputs[1].items[0].clone();
    app.dialog = Some(d);
    let ctx = egui::Context::default();
    settle_alignment_update(&mut app, &ctx);
    let key = app.preview.key;
    app.dialog.as_mut().unwrap().active = 3;
    settle_alignment_update(&mut app, &ctx);
    assert_eq!(app.preview.key, key, "arming an alignment input does not reset the pose");
    app.dialog.as_mut().unwrap().active = 1;
    let Sel::Face { body, index, .. } = old else { panic!() };
    let point = Vec3::new(10.0, 10.0, 10.0);
    let hit = crate::viewport::Hit::Face { body: body.clone(), index, point };
    crate::viewport::dialog_pick(&mut app, &hit, Sel::Face { body, index, point });
    let Sel::Face { point, .. } = &app.dialog.as_ref().unwrap().inputs[1].items[0] else { panic!() };
    assert!(point.dist(Vec3::new(40.0, 10.0, 0.0)) < 1e-6);
    settle_alignment_update(&mut app, &ctx);
    assert!(app.preview.error.is_none());
}

#[test]
fn secondary_alignment_circle_hover_survives_preview_and_small_motion_until_real_leave() {
    let mut h = crate::scenario::Harness::new();
    h.app = round_plates();
    h.app.start("joint.create");
    let mut d = h.app.dialog.take().unwrap();
    for p in [Vec3::new(23.0, 20.0, 9.0), Vec3::new(173.0, 20.0, 9.0)] {
        d.pick(&h.app.session, face_sel(&h.app.session, p).unwrap());
    }
    let (_, moving) = rim(&h.app.session, "Body2", Vec3::new(230.0, 20.0, 10.0));
    d.pick(&h.app.session, moving);
    let (hit, selected) = rim(&h.app.session, "Body1", Vec3::new(80.0, 20.0, 10.0));
    let Sel::Edge { point, .. } = &selected else { panic!() };
    h.app.dialog = Some(d);
    h.frames(3);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let at = proj.to_screen(*point).unwrap();
    let candidate = Some((hit.clone(), Some(selected.clone())));
    assert_eq!(crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at), candidate.clone()), candidate);
    h.app.viewport.hover = Some(hit);
    settle_alignment_update(&mut h.app, &egui::Context::default());
    for _ in 0..8 {
        assert_eq!(crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at + egui::vec2(0.5, 0.5)), None), candidate);
    }
    let kept = crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at + egui::vec2(0.5, 0.5)), None).unwrap().1.unwrap();
    let mut d = h.app.dialog.take().unwrap();
    d.pick(&h.app.session, kept);
    let p = apply_commands(&h.app, &d).unwrap()[0].1.clone();
    assert_eq!(p["a_alignment"]["reference"]["type"], "circle_center");
    h.app.dialog = Some(d);
    assert!(crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at + egui::vec2(100.0, 100.0)), None).is_none());
}

#[test]
fn secondary_alignment_feedback_stationary_hover_keeps_identity_until_pointer_moves() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.start("joint.create");
    h.app.dialog.as_mut().unwrap().active = 2;
    h.frames(2);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let at = egui::pos2(700.0, 400.0);
    let first = Some((
        crate::viewport::Hit::Face { body: "Body2".into(), index: 3, point: Vec3::ZERO },
        Some(Sel::Face { body: "Body2".into(), index: 3, point: Vec3::ZERO }),
    ));
    let kept = crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at), first.clone());
    assert_eq!(kept, first);
    assert_eq!(
        crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at), None),
        first,
        "moving geometry must not erase a stationary candidate"
    );
    assert!(crate::viewport::stable_alignment_hover(&mut h.app, &proj, Some(at + egui::vec2(2.0, 0.0)), None).is_none());
    assert!(crate::viewport::stable_alignment_hover(&mut h.app, &proj, None, None).is_none());
}

#[test]
fn secondary_alignment_feedback_accepts_named_edge_and_tracks_sketch_namespace() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    let point = Vec3::new(40.0, 0.0, 0.0);
    let Sel::Edge { index, .. } = edge_sel(&app.session, point).unwrap() else { panic!() };
    let hit = crate::viewport::Hit::Edge { body: "Body2".into(), index, mid: point, point };
    let selection = d.candidate(&app.session, &hit).unwrap();
    d.pick(&app.session, selection);
    let commands = apply_commands(&app, &d).unwrap();
    assert_eq!(commands[0].1["b_alignment"]["reference"]["type"], "edge");
    app.session.scratch().execute(&commands[0].0, &commands[0].1).unwrap();
    d.active = 2;
    let hit = crate::viewport::Hit::SketchCurve { sketch: 99, id: "l1".into(), straight: true, point: Vec3::ZERO };
    assert!(d.candidate(&app.session, &hit).is_none(), "a missing sketch has no matching occurrence");
}

#[test]
fn secondary_alignment_feedback_construction_picking_uses_the_preview_placement() {
    let mut app = two_boxes();
    app.run("component.activate", json!({"component":"B"})).unwrap();
    app.run("construct.axis.two_points", json!({"a":{"point":[40,10,0]},"b":{"point":[40,20,0]},"name":"MovingAxis"})).unwrap();
    app.run("construct.plane.three_points", json!({"a":{"point":[40,10,0]},"b":{"point":[40,20,0]},"c":{"point":[40,10,5]},"name":"MovingPlane"}))
        .unwrap();
    app.run("component.activate", json!({"component":"root"})).unwrap();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    app.dialog = Some(d);
    settle_alignment_update(&mut app, &egui::Context::default());
    let (_, a, b) = crate::viewport::construction_axis_segments(&app).into_iter().find(|(name, ..)| name == "MovingAxis").unwrap();
    for point in [a, b] {
        assert!((point.x - 10.0).abs() < 1e-6 && (point.z - 10.0).abs() < 1e-6, "{point:?}");
    }
    // At this fixed orthographic view the displayed square reaches 14.4 mm from the
    // solved plane origin (10,10,10). Compare its corners with independent fixture data.
    app.cam.distance = 200.0;
    app.cam.fov = 0.0;
    let (_, _, quad) = crate::viewport::construction_quads(&app).into_iter().find(|(name, ..)| name == "MovingPlane").unwrap();
    let expected = [Vec3::new(10.0, -4.4, -4.4), Vec3::new(10.0, 24.4, -4.4), Vec3::new(10.0, 24.4, 24.4), Vec3::new(10.0, -4.4, 24.4)];
    let mut remaining = [true; 4];
    for actual in quad {
        let index = expected
            .iter()
            .enumerate()
            .find_map(|(index, expected)| {
                (remaining[index]
                    && (actual.x - expected.x).abs() < 1e-6
                    && (actual.y - expected.y).abs() < 1e-6
                    && (actual.z - expected.z).abs() < 1e-6)
                    .then_some(index)
            })
            .unwrap_or_else(|| panic!("unexpected or repeated plane corner: {actual:?}"));
        remaining[index] = false;
    }
    assert!(remaining.into_iter().all(|unmatched| !unmatched), "all independent fixture corners must be present");
}

#[test]
fn secondary_alignment_dialog_preview_edit_clear_and_cancel_keep_references() {
    let mut app = two_boxes();
    app.run("parameters.add", json!({"name": "jointTurn", "expression": "17 deg"})).unwrap();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    assert_eq!(d.inputs.len(), 4);
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0), Vec3::new(40.0, 20.0, 5.0), Vec3::new(20.0, 10.0, 5.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.angle = "jointTurn".into();
        f.reverse_alignment[0] = true;
    }
    let commands = apply_commands(&app, &d).unwrap();
    assert!(commands[0].1["a_alignment"]["face"].is_string());
    assert_eq!(commands[0].1["a_alignment"]["reverse"], true);
    let before = serde_json::to_value(&*app.session.doc).unwrap();
    app.session.scratch().execute(&commands[0].0, &commands[0].1).unwrap();
    assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before, "preview/cancel cannot change the document");
    for (id, p) in commands {
        app.run(&id, p).unwrap();
    }
    let old = app.session.doc.assembly.joints[0].clone();
    let mut edit = edit_joint(&app, old.id).unwrap();
    assert_eq!(edit.inputs.len(), 4);
    assert_eq!(stored_alignment(&edit.kind, 3).unwrap()["face"], apply_commands(&app, &d).unwrap()[0].1["a_alignment"]["face"]);
    let params = apply_commands(&app, &edit).unwrap()[0].1.clone();
    assert!(params.get("angle").is_none(), "an unchanged expression stays in the saved joint, following the current Edit Joint contract");
    assert_eq!(params["a_alignment"]["reverse"], true);
    clear_alignment(&mut edit.kind, 3);
    let params = apply_commands(&app, &edit).unwrap()[0].1.clone();
    assert_eq!(params["a_alignment"], Value::Null);
    assert!(params["b_alignment"]["face"].is_string());
    app.run("joint.edit", params).unwrap();
    assert_eq!(app.session.doc.assembly.joints[0].exprs.angle.as_deref(), Some("jointTurn"));
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.assembly.joints[0], old);
}

#[test]
fn secondary_alignment_refuses_other_occurrences_and_static_offset_origins() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    // The moving B is first; picking fixed A here is ignored.
    d.pick(&app.session, face_sel(&app.session, Vec3::new(20.0, 10.0, 5.0)).unwrap());
    let p = apply_commands(&app, &d).unwrap()[0].1.clone();
    assert!(d.inputs[2].items.is_empty());
    assert!(p.get("b_alignment").is_none());
    app.session.scratch().execute("joint.create", &p).unwrap();
    d.active = 2;
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 20.0, 5.0)).unwrap());
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.offset[0] = "2 mm".into();
    }
    assert!(apply_commands(&app, &d).unwrap_err().contains("Offset X"));
    // Upstream occurrence picking now preserves B:2 instead of aliasing it to B:1.
    // Reject a reference on fixed A, then prove an alignment and the joint keep B:2.
    app.dialog = None;
    let component = app.session.doc.find_component("B").unwrap();
    let copied = app.run("occurrence.copy", json!({"component": component, "translate": [70, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(110.0, 10.0, 0.0)).unwrap());
    d.active = 2;
    d.pick(&app.session, face_sel(&app.session, Vec3::new(20.0, 10.0, 5.0)).unwrap());
    assert!(d.inputs[3].items.is_empty(), "only the first alignment is supplied");
    assert!(d.inputs[2].items.is_empty());
    assert!(alignment_input_available(&d, 2), "the correctly resolved copied occurrence supports alignment");
    let params = apply_commands(&app, &d).unwrap()[0].1.clone();
    assert_eq!(params["b"]["occurrence"], copied, "do not silently joint the first instance");
    d.pick(&app.session, face_sel(&app.session, Vec3::new(110.0, 20.0, 5.0)).unwrap());
    let params = apply_commands(&app, &d).unwrap()[0].1.clone();
    assert!(params["b_alignment"]["face"].is_string());
    let mut scratch = app.session.scratch();
    scratch.execute("joint.create", &params).unwrap();
    assert_eq!(scratch.doc.assembly.joints.last().unwrap().b.occurrence, copied);
}

#[test]
fn secondary_alignment_single_inversion_keeps_component_flip_and_clear_saved_references() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.start("joint.create");
    let mut d = h.app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0), Vec3::new(40.0, 20.0, 5.0), Vec3::new(20.0, 10.0, 5.0)] {
        d.pick(&h.app.session, face_sel(&h.app.session, p).unwrap());
    }
    h.app.dialog = Some(d);
    h.frames(3);
    let before = h.app.session.doc.clone();
    h.step(&json!({"click":{"handle":"dialog:invert-alignment"}})).unwrap();
    h.frames(3);
    let p = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.clone();
    assert_eq!(p["b_alignment"]["reverse"], true);
    assert_eq!(p["a_alignment"]["reverse"], false);
    assert_eq!(p["flip"], false);
    assert!(h.step(&json!({"click":{"handle":"dialog:invert-alignment:2"}})).is_err());
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc, before);
    for (id, p) in commands {
        h.app.run(&id, p).unwrap();
    }
    let id = h.app.session.doc.assembly.joints[0].id;
    h.app.dialog = edit_joint(&h.app, id);
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:clear:dialog.alignment_1_optional"}})).unwrap();
    h.frames(3);
    let p = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.clone();
    assert_eq!(p["b_alignment"], Value::Null);
    assert!(p["a_alignment"]["face"].is_string());
}

fn occ(app: &SolveApp, name: &str) -> u64 {
    let c = app.session.doc.find_component(name).unwrap();
    app.session.doc.occurrence_of(c).unwrap().id
}

/// Two face picks make a joint between the components that own them; offsets across the face
/// turn component 1's origin into a frame; limits follow the type.
#[test]
fn joint_from_two_snaps() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    let (fa, fb) = (face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap(), face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    assert!(apply_commands(&app, &d).is_err(), "no snaps yet");
    d.pick(&app.session, fa);
    assert_eq!(d.active, 1, "component 2 is next");
    d.pick(&app.session, fb);
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.fit_limits(&solidcraft_engine::doc::Document::default());
        f.limits[0] = (true, "-45 deg".into(), "45 deg".into());
    }
    let c = apply_commands(&app, &d).unwrap();
    let (id, p) = &c[0];
    assert_eq!(id, "joint.create");
    assert_eq!(p["type"], "revolute");
    assert_eq!(p["a"]["occurrence"], occ(&app, "A"));
    assert_eq!(p["b"]["occurrence"], occ(&app, "B"));
    assert!(p["a"]["face"].is_array() && p["b"]["face"].is_array(), "{p}");
    assert_eq!(p["limits"], json!([["-45 deg", "45 deg"]]));
    // The bottom face of B snaps at its centre, z down.
    let sb = snap_of(&app.session, &d.inputs[1].items[0]).unwrap();
    assert!(sb.at.dist(Vec3::new(40.0, 10.0, 0.0)) < 1e-6 && sb.z.dist(-Vec3::Z) < 1e-9, "{sb:?}");
    // Offset X stays separate from the explicit attachment frame.
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.offset[0] = "5 mm".into();
    }
    let p = apply_commands(&app, &d).unwrap()[0].1.clone();
    let o = &p["a"]["frame"]["origin"];
    let at = Vec3::new(o[0].as_f64().unwrap(), o[1].as_f64().unwrap(), o[2].as_f64().unwrap());
    assert!(at.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-9, "{p}");
    assert_eq!(p["offset_x"], 5.0);
    // OK makes it.
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.len(), 1);
}

/// Reopening a joint restores all three offsets, and Cancel and Undo preserve the original joint.
#[test]
fn editing_a_joint_keeps_all_three_offsets() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut dialog = app.dialog.take().unwrap();
    for point in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        dialog.pick(&app.session, face_sel(&app.session, point).unwrap());
    }
    let Kind::Assembly(Asm::Joint(form)) = &mut dialog.kind else { panic!("joint form") };
    form.offset = ["10 mm".into(), "30 mm".into(), "30 mm".into()];
    for (id, params) in apply_commands(&app, &dialog).unwrap() {
        app.run(&id, params).unwrap();
    }
    let joint = app.session.doc.assembly.joints[0].id;
    let edited = edit_joint(&app, joint).unwrap();
    let Kind::Assembly(Asm::Joint(form)) = &edited.kind else { panic!("joint form") };
    for (text, expected) in form.offset.iter().zip([10.0, 30.0, 30.0]) {
        let actual = crate::length_input::eval(&app.session.doc, text, ValueKind::Length).unwrap();
        assert!((actual - expected).abs() < 1e-9, "reopened offset {text}, expected {expected}");
    }

    let before = app.session.doc.clone();
    let mut harness = crate::scenario::Harness::new();
    harness.app = app;
    harness.app.home.open = false;
    harness.app.dialog = Some(edited);
    harness.settle();
    harness.step(&json!({"click":{"handle":"dialog:section:Position"}})).unwrap();
    harness.frames(4);
    for name in ["Offset X", "Offset Y", "Offset Z"] {
        assert!(crate::scenario::handle_at(&format!("dialog:{name}")).is_some(), "{name} is editable");
    }
    harness.step(&json!({"click":{"handle":"dialog:Offset X"}})).unwrap();
    harness.step(&json!({"key":"A","cmd":true})).unwrap();
    harness.step(&json!({"text":"20 mm"})).unwrap();
    harness.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    harness.settle();
    assert_eq!(harness.app.session.doc, before, "Cancel preserves the joint");

    harness.app.dialog = edit_joint(&harness.app, joint);
    harness.settle();
    for (name, value) in [("Offset X", "20 mm"), ("Offset Y", "40 mm")] {
        harness.step(&json!({"click":{"handle":format!("dialog:{name}")}})).unwrap();
        harness.step(&json!({"key":"A","cmd":true})).unwrap();
        harness.step(&json!({"text":value})).unwrap();
    }
    harness.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    harness.settle();
    let changed = &harness.app.session.doc.assembly.joints[0];
    assert_eq!(changed.offset_xy, [20.0, 40.0]);
    assert_eq!(changed.offset, 30.0);
    harness.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(harness.app.session.doc, before, "Undo restores all offsets and the saved attachment");
}

/// Editing a zero offset uses the saved attachment axes of the rotated component.
#[test]
fn editing_zero_offsets_uses_the_saved_rotated_attachment_axes() {
    let mut app = two_boxes();
    let a = occ(&app, "A");
    app.run("occurrence.ground", json!({"occurrence":a, "grounded":false})).unwrap();
    app.run("occurrence.move", json!({"occurrence":a, "axis":[0,0,1], "angle":"90 deg", "origin":[0,0,0]})).unwrap();
    app.run("occurrence.ground", json!({"occurrence":a, "grounded":true})).unwrap();
    app.start("joint.create");
    let mut dialog = app.dialog.take().unwrap();
    for point in [Vec3::new(-10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        dialog.pick(&app.session, face_sel(&app.session, point).unwrap());
    }
    for (id, params) in apply_commands(&app, &dialog).unwrap() {
        app.run(&id, params).unwrap();
    }
    let saved = app.session.doc.assembly.joints[0].clone();
    let mut edited = edit_joint(&app, saved.id).unwrap();
    let Kind::Assembly(Asm::Joint(form)) = &mut edited.kind else { panic!("joint form") };
    form.offset[0] = "12 mm".into();
    let commands = apply_commands(&app, &edited).unwrap();
    assert!(commands[0].1.get("a").is_none(), "editing an offset retains the attachment");
    for (id, params) in commands {
        app.run(&id, params).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints[0].a, saved.a);
    let b = app.session.doc.occurrence_of(app.session.doc.find_component("B").unwrap()).unwrap();
    // The saved top-face frame has local X along +Y; A's 90-degree turn puts it along world -X.
    let bottom = solidcraft_engine::doc::apply_point(&b.transform, Vec3::new(40.0, 10.0, 0.0));
    assert!(bottom.dist(Vec3::new(-22.0, 10.0, 10.0)) < 1e-6, "{bottom:?}");
}

/// A joint origin can be the design origin or a sketch point, not just faces, edges and vertices
/// (#10290). The origin snaps to the world origin with z up; a sketch point snaps to its world
/// position with the sketch plane's normal as z, and both make a joint.
#[test]
fn joint_origin_can_be_the_origin_or_a_sketch_point() {
    let mut app = two_boxes();
    // A sketch point on B's top face, at world (35, 5, 10).
    app.session.execute("component.activate", &json!({"component": "B"})).unwrap();
    app.session
        .execute("sketch.create", &json!({"plane": {"origin": [30.0, 0.0, 10.0], "x_dir": [1.0, 0.0, 0.0], "y_dir": [0.0, 1.0, 0.0]}, "name": "S"}))
        .unwrap();
    app.session.execute("sketch.point", &json!({"point": [5.0, 5.0], "id": "q"})).unwrap();
    app.session.execute("sketch.finish", &json!({})).unwrap();
    let sketch = app.session.doc.find_feature("S").unwrap().id;

    // The two picks a joint origin input now accepts.
    let origin = Sel::Vertex { body: String::new(), point: Vec3::ZERO };
    let sketch_point = Sel::SketchPoint { id: format!("{sketch}:q") };
    let so = |s: &Sel| snap_of(&app.session, s).expect("a snap for the pick");
    let o = so(&origin);
    let sp = so(&sketch_point);
    assert!(o.at.dist(Vec3::ZERO) < 1e-6, "the world origin snaps to the origin: {o:?}");
    assert!(o.z.dist(Vec3::Z) < 1e-9, "the origin's z is up: {o:?}");
    assert!(sp.at.dist(Vec3::new(35.0, 5.0, 10.0)) < 1e-6, "the sketch point snaps to its world position: {sp:?}");
    assert!(sp.z.dist(Vec3::Z) < 1e-9, "the sketch point's z is the plane normal: {sp:?}");

    // And a joint can be made from them.
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, origin);
    d.pick(&app.session, sketch_point);
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.fit_limits(&app.session.doc);
    }
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.len(), 1, "a joint was made from the origin and the sketch point");
}
#[test]
fn revolute_joint_uses_the_picked_straight_edges() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    for point in [Vec3::new(20.0, 10.0, 10.0), Vec3::new(30.0, 10.0, 10.0)] {
        let edge = edge_sel(&app.session, point).unwrap();
        let snap = snap_of(&app.session, &edge).unwrap();
        assert_eq!(snap.kind, SnapKind::EdgeMid);
        assert!(snap.at.dist(point) < 1e-9, "{snap:?}");
        assert!(snap.z.cross(Vec3::Y).len() < 1e-9, "the selected edge runs along Y: {snap:?}");
        d.pick(&app.session, edge);
    }
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.fit_limits(&app.session.doc);
    }
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    let a = app.session.doc.find_component("A").unwrap();
    let b = app.session.doc.find_component("B").unwrap();
    let ground = app.session.doc.occurrence_of(a).unwrap().transform;
    let placed = |app: &SolveApp, p| solidcraft_engine::doc::apply_point(&app.session.doc.occurrence_of(b).unwrap().transform, p);
    let ends = [Vec3::new(30.0, 0.0, 10.0), Vec3::new(30.0, 20.0, 10.0)];
    let before = ends.map(|p| placed(&app, p));
    let off_axis = Vec3::new(50.0, 10.0, 10.0);
    let off_before = placed(&app, off_axis);
    let joint = app.session.doc.assembly.joints[0].id;
    app.run("joint.drive", json!({"joint": joint, "values": ["90 deg"]})).unwrap();
    for (p, at) in ends.into_iter().zip(before) {
        assert!(placed(&app, p).dist(at) < 1e-6, "both ends of the hinge edge must stay put");
    }
    assert!((placed(&app, off_axis).dist(off_before) - 20.0 * 2.0_f64.sqrt()).abs() < 1e-6, "the box makes a quarter turn");
    assert_eq!(app.session.doc.occurrence_of(a).unwrap().transform, ground);
}

/// Two arms (a 100 x 20 x `T` plate with a hole 10 mm in from each end), B jointed by its
/// bottom to A's top at a round edge, a straight edge or a corner picked in the dialog: when the
/// arm is made thicker, B sits on A's new top. (These picks were kept as points where they were
/// picked, so B stayed at the old top, sunk into A.)
#[test]
fn arms_jointed_at_edges_and_corners_follow_a_thicker_arm() {
    let picks: [(&str, Vec3, Vec3, bool); 3] = [
        ("round edge", Vec3::new(13.0, 0.0, 5.0), Vec3::new(93.0, 60.0, 0.0), false),
        ("straight edge", Vec3::new(50.0, 10.0, 5.0), Vec3::new(50.0, 70.0, 0.0), false),
        ("corner", Vec3::new(100.0, 10.0, 5.0), Vec3::new(100.0, 70.0, 0.0), true),
    ];
    for (what, pa, pb, corner) in picks {
        let mut s = Session::default();
        s.execute("parameters.add", &json!({"name": "T", "expression": "5 mm"})).unwrap();
        s.execute("component.create", &json!({"name": "Arm"})).unwrap();
        s.execute("solid.box", &json!({"length": 100, "width": 20, "height": "T", "corner": [0, -10, 0]})).unwrap();
        for x in [10, 90] {
            s.execute("solid.cylinder", &json!({"diameter": 6, "height": 100, "base": [x, 0, -50], "operation": "cut"})).unwrap();
        }
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        let a = occ(&app, "Arm");
        app.run("occurrence.ground", json!({"occurrence": a, "grounded": true})).unwrap();
        let b = app.run("occurrence.copy", json!({"occurrence": a, "translate": [0, 60, 0]})).unwrap()["occurrence"].as_u64().unwrap();
        app.start("joint.create");
        let mut d = app.dialog.clone().unwrap();
        for p in [pa, pb] {
            let edge = edge_sel(&app.session, p).unwrap();
            let sel = match (&edge, corner) {
                (Sel::Edge { body, .. }, true) => Sel::Vertex { body: body.clone(), point: p },
                _ => edge,
            };
            d.pick(&app.session, sel);
        }
        if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
            f.kind = 1;
            f.fit_limits(&app.session.doc);
        }
        for (id, p) in apply_commands(&app, &d).unwrap() {
            app.run(&id, p).unwrap();
        }
        let world = |app: &SolveApp, o: u64, p: Vec3| {
            solidcraft_engine::doc::apply_point(&solidcraft_engine::doc::joints::occurrence_world(&app.session.doc, o), p)
        };
        // The spots: a round edge's centre, else the pick; B's in the arm's own coordinates.
        let spot = |p: Vec3| if what == "round edge" { Vec3::new(p.x - 3.0, p.y, p.z) } else { p };
        let (spot_a, spot_b) = (spot(pa), spot(pb) - Vec3::new(0.0, 60.0, 0.0));
        assert!(world(&app, b, spot_b).dist(spot_a) < 1e-6, "{what}: as made, B's spot is at {:?}", world(&app, b, spot_b));
        app.run("parameters.change", json!({"name": "T", "expression": "12 mm"})).unwrap();
        let top = spot_a + Vec3::new(0.0, 0.0, 7.0);
        assert!(world(&app, b, spot_b).dist(top) < 1e-6, "{what}: B's spot is at {:?}, not on A's new top at {top:?}", world(&app, b, spot_b));
    }
}

/// The preview moves B where the joint puts it and plays the motion while animating.
#[test]
fn joint_and_drive_preview_on_the_placed_model() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let colors = crate::preview::Colors {
        body: [1, 1, 1, 255],
        added: [2, 2, 2, 255],
        new_body: [2, 2, 2, 255],
        removed: [3, 3, 3, 100],
        lost: [5, 5, 5, 100],
        selected: [7, 7, 7, 255],
        mirrored: [6, 6, 6, 100],
        edge: [0, 0, 0, 255],
        ghost: true,
        tint_new: true,
        cut_tint: true,
        glass_new: false,
    };
    let cmds = preview_commands(&app, &d).unwrap();
    let b = world_preview(&app.session, &cmds, colors).unwrap().unwrap();
    assert_eq!(b.replaced, vec!["Body2".to_string()], "B moves");
    assert!(app.session.doc.assembly.joints.is_empty(), "the preview changes nothing");
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.animate = true;
        f.phase = 0.25;
    }
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["values"], json!(["90 deg"]), "a quarter turn into the animation");
    assert!(apply_commands(&app, &d).unwrap()[0].1.get("values").is_none(), "OK makes the joint at rest");
    // Drive: the dialog's value turns the joint, the preview shows it.
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    app.start("joint.drive");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Assembly(Asm::Drive { values, .. }) = &mut d.kind {
        assert_eq!(values, &vec!["0 deg".to_string()]);
        values[0] = "30 deg".into();
    }
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].0, "joint.drive");
    let b = world_preview(&app.session, &cmds, colors).unwrap().unwrap();
    assert_eq!(b.replaced, vec!["Body2".to_string()]);
    // The moved part's old place is the ghost, unless Show ghost is off (#5905).
    assert!(!b.scene.ghost.is_empty());
    let hidden = world_preview(&app.session, &cmds, crate::preview::Colors { ghost: false, ..colors }).unwrap().unwrap();
    assert!(hidden.scene.ghost.is_empty());
    assert_eq!(hidden.replaced, b.replaced, "the moved part is still shown where it goes");
    assert_eq!(hidden.scene.tris, b.scene.tris);
    // Features preview as before while nothing is placed away from its frame, and on the
    // placed model once something is (B moved by its joint).
    assert!(world_preview(&two_boxes().session, &[("solid.extrude".into(), json!({}))], colors).is_none());
    assert!(world_preview(&app.session, &[("solid.extrude".into(), json!({}))], colors).is_some());
}

/// Edit Joint changes type, alignment and limits of the joint in place.
#[test]
fn edit_joint_runs_edit_and_limits() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run("joint.create", json!({"type": "slider", "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}))
        .unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    let mut d = edit_joint(&app, id).unwrap();
    assert_eq!(d.inputs.len(), 4, "primary sides remain first, followed by optional alignments");
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        assert_eq!(TYPES[f.kind], "slider");
        f.limits[0] = (true, "0 mm".into(), "15 mm".into());
        f.offset[2] = "2 mm".into();
    }
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["joint.edit", "joint.limits"]);
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let j = &app.session.doc.assembly.joints[0];
    assert_eq!(j.limits[0], Some((0.0, 15.0)));
    assert!((j.offset - 2.0).abs() < 1e-12);
}

/// Edit Joint sends the joint's name with every edit: a joint whose own name is over 128
/// characters (joint.create takes any) can still be edited from the dialog, its name kept.
#[test]
fn edit_joint_edits_a_joint_with_a_long_name() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    let long = "x".repeat(200);
    app.run(
        "joint.create",
        json!({"type": "slider", "name": long, "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}),
    )
    .unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    let mut d = edit_joint(&app, id).unwrap();
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.offset[2] = "2 mm".into();
    }
    let c = apply_commands(&app, &d).unwrap();
    assert!(c.iter().any(|(id, p)| id == "joint.edit" && p["name"] == json!(long)), "the dialog sends the name: {c:?}");
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let j = &app.session.doc.assembly.joints[0];
    assert!((j.offset - 2.0).abs() < 1e-12, "the edit went through");
    assert_eq!(j.name, long);
}

/// An old edge pick saved as point/z keeps its face-normal axis when reopened and edited.
#[test]
fn editing_a_saved_joint_keeps_its_explicit_axis() {
    let mut app = two_boxes();
    app.run(
        "joint.create",
        json!({"type": "revolute",
        "a": {"occurrence": occ(&app, "A"), "point": [20, 10, 10], "z": [0, 0, 1]},
        "b": {"occurrence": occ(&app, "B"), "point": [30, 10, 10], "z": [0, 0, 1]}}),
    )
    .unwrap();
    let joint = app.session.doc.assembly.joints[0].clone();
    let saved = app.session.doc.to_json();
    app = SolveApp::new(Session::new(solidcraft_engine::doc::format::read(&saved).unwrap()), Services::default());
    let mut d = edit_joint(&app, joint.id).unwrap();
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.flip = true;
    }
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    let edited = &app.session.doc.assembly.joints[0];
    assert!(edited.flip);
    assert_eq!(edited.a, joint.a);
    assert_eq!(edited.b, joint.b);
}

/// Edit Joint shows an offset typed as an expression as that expression, and OK keeps it; the
/// dialog's plain numbers keep no expression.
#[test]
fn edit_joint_keeps_an_offset_typed_as_an_expression() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run("parameters.add", json!({"name": "gap", "expression": "3 mm"})).unwrap();
    let (fa, fb) = (json!({"occurrence": a, "face": [10, 10, 10]}), json!({"occurrence": b, "face": [40, 10, 0]}));
    app.run("joint.create", json!({"type": "rigid", "a": fa, "b": fb, "offset": "gap", "angle": "0 deg"})).unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    let mut d = edit_joint(&app, id).unwrap();
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        assert_eq!(f.offset[2], "gap");
        f.flip = true;
    }
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    let j = &app.session.doc.assembly.joints[0];
    assert!(j.flip);
    assert_eq!((j.exprs.offset.as_deref(), j.exprs.angle.as_deref()), (Some("gap"), None));
}

/// Edit Joint shows both sides pre-filled, each on its own component. OK sends no side while
/// they are as they were; picking one side again sends only that side, and the joint keeps its
/// other side.
#[test]
fn edit_joint_shows_both_sides_and_a_repick_sends_only_that_side() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run("joint.create", json!({"type": "rigid", "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}))
        .unwrap();
    let before = app.session.doc.assembly.joints[0].clone();
    let mut d = edit_joint(&app, before.id).unwrap();
    assert_eq!(d.inputs.len(), 4);
    for (input, occurrence) in d.inputs.iter().zip([a, b]) {
        let Some(Sel::Face { body, .. }) = input.items.first() else { panic!("no side pick: {:?}", input.items) };
        assert_eq!(occurrence_of_body(&app.session, body), Some(occurrence), "{body}");
    }
    let c = apply_commands(&app, &d).unwrap();
    assert!(c[0].1.get("a").is_none() && c[0].1.get("b").is_none(), "{}", c[0].1);
    // B's top face, where B now sits on A: side b alone is picked again, and the angle the
    // dialog opened on is not sent (a side moved turns B as joint.create would).
    d.active = 1;
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 20.0)).unwrap());
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].0, "joint.edit");
    assert!(c[0].1.get("a").is_none() && c[0].1.get("angle").is_none(), "{}", c[0].1);
    assert_eq!(c[0].1["b"]["occurrence"], json!(b));
    // An angle the user types is sent.
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.angle = "45 deg".into();
    }
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["angle"], json!("45 deg"), "{}", c[0].1);
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    assert!((app.session.doc.assembly.joints[0].angle.to_degrees() - 45.0).abs() < 1e-9);
    let after = app.session.doc.assembly.joints[0].clone();
    assert_eq!(after.a, before.a);
    assert_eq!(after.b.occurrence, b);
    assert_ne!(after.b, before.b);
}

/// Point and frame origins have no face pick, but Edit Joint must still identify their
/// components (#7749). Showing those components must not replace either stored origin.
#[test]
fn edit_joint_shows_components_for_point_and_frame_origins() {
    for command in ["joint.create", "joint.as_built", "joint.rigid_group"] {
        let mut app = two_boxes();
        let (a, b) = (occ(&app, "A"), occ(&app, "B"));
        let params = match command {
            "joint.create" => json!({"type": "rigid", "flip": true,
                "a": {"occurrence": a, "point": [10, 10, 10], "z": [0, 0, 1]},
                "b": {"occurrence": b, "point": [40, 10, 10], "z": [0, 0, 1]}}),
            "joint.as_built" => json!({"type": "rigid", "a": a, "b": b}),
            _ => json!({"occurrences": [a, b]}),
        };
        app.run(command, params).unwrap();
        let before = app.session.doc.assembly.joints[0].clone();
        let mut d = edit_joint(&app, before.id).unwrap();
        for (input, (id, name)) in d.inputs.iter().zip([(a, "A:1"), (b, "B:1")]) {
            let item = input.items.first().expect("the stored component is shown");
            assert_eq!(item, &Sel::Occurrence { path: vec![id] }, "{command}");
            assert_eq!(crate::dialogs::selection_label(&app.session, app.preferences.origin_plane_names, &d, input, item), name);
        }
        let commands = apply_commands(&app, &d).unwrap();
        assert!(commands[0].1.get("a").is_none() && commands[0].1.get("b").is_none(), "{commands:?}");
        for (id, params) in commands {
            app.run(&id, params).unwrap();
        }
        let after = &app.session.doc.assembly.joints[0];
        assert_eq!((&after.a, &after.b), (&before.a, &before.b), "{command}");

        // Replacing just B with a face still edits only B, preserving A's point/frame.
        let body = app
            .session
            .world_state()
            .unwrap()
            .bodies
            .iter()
            .find(|body| occurrence_of_body(&app.session, &body.name) == Some(b))
            .unwrap()
            .name
            .clone();
        let picked = Sel::Face { body, index: 0, point: Vec3::new(40.0, 10.0, 10.0) };
        d.active = 1;
        d.pick(&app.session, picked);
        let commands = apply_commands(&app, &d).unwrap();
        assert!(commands[0].1.get("a").is_none(), "{commands:?}");
        assert_eq!(commands[0].1["b"]["occurrence"], b);
    }
}

#[test]
fn edit_joint_component_labels_keep_the_nested_instance_and_root() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    let parent = app.run("component.create", json!({"name": "Assembly", "activate": false})).unwrap()["occurrence"].as_u64().unwrap();
    let first =
        app.run("component.create", json!({"name": "Part", "parent": "Assembly", "activate": false})).unwrap()["occurrence"].as_u64().unwrap();
    let second = app.run("occurrence.copy", json!({"occurrence": first})).unwrap()["occurrence"].as_u64().unwrap();
    let origin = |id| json!({"occurrence": id, "frame": {"origin": [0, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}});
    app.run("joint.create", json!({"type": "rigid", "flip": true, "a": origin(0), "b": origin(second)})).unwrap();
    let before = app.session.doc.assembly.joints[0].clone();
    let mut d = edit_joint(&app, before.id).unwrap();
    for (input, (path, label)) in d.inputs.iter().zip([(vec![], app.session.doc.name.as_str()), (vec![parent, second], "Part:2")]) {
        let item = input.items.first().expect("the stored component is shown");
        assert_eq!(item, &Sel::Occurrence { path });
        assert_eq!(crate::dialogs::selection_label(&app.session, app.preferences.origin_plane_names, &d, input, item), label);
    }
    for (id, params) in apply_commands(&app, &d).unwrap() {
        app.run(&id, params).unwrap();
    }
    let after = &app.session.doc.assembly.joints[0];
    assert_eq!((&after.a, &after.b), (&before.a, &before.b));
    d.inputs[1].items.clear();
    assert!(apply_commands(&app, &d).is_err(), "clearing a side still requires a replacement origin");
}

/// A side on a hole's wall shows the wall, a cylinder, not the plate's face beside the hole: the
/// face its snap names is the one the pre-fill takes, however near the other face's plane lies.
#[test]
fn a_hole_side_shows_its_wall_not_the_plate_beside_it() {
    let mut s = Session::default();
    s.execute("component.create", &json!({"name": "Plate"})).unwrap();
    s.execute("solid.box", &json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]})).unwrap();
    s.execute("solid.cylinder", &json!({"diameter": 10, "height": 20, "base": [20, 20, -5], "operation": "cut"})).unwrap();
    s.execute("component.create", &json!({"name": "Peg"})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0]})).unwrap();
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    let (plate, peg) = (occ(&app, "Plate"), occ(&app, "Peg"));
    app.run("occurrence.ground", json!({"occurrence": plate, "grounded": true})).unwrap();
    app.run(
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": plate, "circle": [25, 20, 8]}, "b": {"occurrence": peg, "face": [65, 5, 0]}}),
    )
    .unwrap();
    let joint = app.session.doc.assembly.joints[0].clone();
    assert!(matches!(joint.a.snap, solidcraft_engine::doc::joints::Snap::CircleCenter { face: Some(_), .. }), "{:?}", joint.a.snap);
    let d = edit_joint(&app, joint.id).unwrap();
    let item = d.inputs[0].items.first().unwrap_or_else(|| panic!("no pre-fill for the hole side"));
    let snap = snap_of(&app.session, item).unwrap();
    assert_eq!(snap.kind, SnapKind::CircleCenter, "the pre-fill is the plate's face, not the hole's wall: {item:?}");
}

/// The Joint dialog's frames (glyphs, OK's check, preview, rows) reuse each pick's snap: once
/// measured, they mesh nothing until the world changes, and then the snap follows it.
#[test]
fn a_picked_snap_is_measured_once_not_every_frame() {
    let mut s = Session::default();
    s.execute("component.create", &json!({"name": "Plate"})).unwrap();
    s.execute("solid.box", &json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]})).unwrap();
    s.execute("solid.cylinder", &json!({"diameter": 10, "height": 20, "base": [20, 20, -5], "operation": "cut"})).unwrap();
    s.execute("component.create", &json!({"name": "Peg"})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0]})).unwrap();
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    let mut h = crate::scenario::Harness::new();
    h.app = SolveApp::new(s, Services::default());
    let (plate, peg) = (occ(&h.app, "Plate"), occ(&h.app, "Peg"));
    h.app.run("occurrence.ground", json!({"occurrence": plate, "grounded": true})).unwrap();
    h.app.start("joint.create");
    let mut d = h.app.dialog.clone().unwrap();
    let wall = face_sel(&h.app.session, Vec3::new(25.0, 20.0, 5.0)).unwrap();
    let bottom = face_sel(&h.app.session, Vec3::new(65.0, 5.0, 0.0)).unwrap();
    d.pick(&h.app.session, wall.clone());
    d.pick(&h.app.session, bottom.clone());
    h.app.dialog = Some(d);
    let settle = |h: &mut crate::scenario::Harness| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        h.frames(2);
        while h.app.preview.busy {
            assert!(std::time::Instant::now() < deadline, "the joint preview did not finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
            h.frame();
        }
        h.frames(2);
    };
    settle(&mut h);
    assert!(h.app.preview.active && h.app.preview.error.is_none(), "{:?}", h.app.preview.error);
    let hole = snap_of(&h.app.session, &wall).unwrap();
    assert_eq!(hole.kind, SnapKind::CircleCenter);
    assert!(Vec3::new(hole.at.x, hole.at.y, 0.0).dist(Vec3::new(20.0, 20.0, 0.0)) < 1e-6, "{hole:?}");
    let face = snap_of(&h.app.session, &bottom).unwrap();
    assert!(face.at.dist(Vec3::new(65.0, 5.0, 0.0)) < 1e-9 && face.z.dist(-Vec3::Z) < 1e-9, "{face:?}");
    let cmds = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let ((), work) = solidcraft_engine::kernel::profile::capture(|| {
        for _ in 0..3 {
            h.frame();
            apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
            preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        }
    });
    assert_eq!(work.tessellate_ms, 0.0, "the frames after the picks measured them again: {work:?}");
    assert_eq!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap(), cmds);
    for (sel, fresh) in [(&wall, &hole), (&bottom, &face)] {
        let kept = picked_snap(&h.app, sel).unwrap();
        assert!(kept.at.dist(fresh.at) < 1e-9 && kept.z.dist(fresh.z) < 1e-9 && kept.param == fresh.param, "{kept:?} {fresh:?}");
    }
    // A moved Peg is a new world: its face is measured again where it went.
    h.app.run("occurrence.move", json!({"occurrence": peg, "translate": [0, 30, 0], "capture": false})).unwrap();
    settle(&mut h);
    let moved = picked_snap(&h.app, &bottom).unwrap();
    assert!(moved.at.dist(Vec3::new(65.0, 35.0, 0.0)) < 1e-9, "{moved:?}");
    assert!(picked_snap(&h.app, &wall).unwrap().at.dist(hole.at) < 1e-9, "the grounded Plate stays");
}

#[test]
fn as_built_animation_drives_the_new_shared_id_and_handles_exhaustion() {
    let mut app = two_boxes();
    app.session.doc_mut().next_id = 500;
    app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.start("joint.as_built");
    let mut d = app.dialog.take().unwrap();
    let Kind::Assembly(Asm::Joint(f)) = &mut d.kind else { panic!("expected joint dialog") };
    f.kind = 1;
    f.animate = true;
    f.phase = 0.25;
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds[1].0, "joint.drive");
    assert_eq!(cmds[1].1["joint"], 500);
    let mut preview = app.session.scratch();
    for (command, params) in &cmds {
        preview.execute(command, params).unwrap();
    }
    let joint = &preview.doc.assembly.joints[0];
    assert_eq!(joint.id, 500);
    assert!((joint.values[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert_eq!(app.session.doc.next_id, 500, "a preview must not reserve live ids");

    app.session.doc_mut().next_id = u64::MAX;
    let cmds = preview_commands(&app, &d).unwrap();
    let result = app.session.scratch().execute(&cmds[0].0, &cmds[0].1);
    assert!(result.is_err() && !matches!(result, Err(solidcraft_engine::EngineError::Internal(..))), "{result:?}");

    // A programmatic document can also contain the largest possible joint id. The old
    // preview added one to that id without checking, before the engine's panic guard.
    let mut largest = joint.clone();
    largest.id = u64::MAX;
    app.session.doc_mut().assembly.joints.push(largest);
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds.len(), 1, "an exhausted preview must not invent a drive target");
    let result = app.session.scratch().execute(&cmds[0].0, &cmds[0].1);
    assert!(result.is_err() && !matches!(result, Err(solidcraft_engine::EngineError::Internal(..))), "{result:?}");
}

/// As-built joints and rigid groups take components from body picks; New Component makes one
/// from bodies when bodies are picked.
#[test]
fn component_picks_become_occurrences() {
    let mut app = two_boxes();
    app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.start("joint.as_built");
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["a"], occ(&app, "A"));
    assert_eq!(c[0].1["b"], occ(&app, "B"));
    app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.start("joint.rigid_group");
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["occurrences"], json!([occ(&app, "A"), occ(&app, "B")]));
    app.dialog = None;
    app.start("component.create");
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].0, "component.create");
}

/// A body of a further instance of a component belongs to that instance, not the first (#3857).
#[test]
fn body_of_a_further_instance_is_its_own_occurrence() {
    let mut app = two_boxes();
    let copy = app.session.execute("occurrence.copy", &json!({"component": "B", "translate": [0, 40, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    let first = occ(&app, "B");
    assert_ne!(copy, first);
    let frames = app.session.doc.body_frames(&app.session.model.state());
    let of = |o: u64| frames.iter().find(|f| f.2.last() == Some(&o)).map(|f| f.0.clone()).unwrap();
    assert_eq!(occurrence_of_body(&app.session, &of(first)), Some(first));
    assert_eq!(occurrence_of_body(&app.session, &of(copy)), Some(copy));
}

/// Round edges snap to their centre; straight ones are not circles.
#[test]
fn circles_from_edge_polylines() {
    let pts: Vec<Vec3> =
        (0..=24).map(|i| f64::from(i) / 24.0 * std::f64::consts::TAU).map(|t| Vec3::new(5.0 + 3.0 * t.cos(), 2.0 + 3.0 * t.sin(), 7.0)).collect();
    let (c, n) = circle_of(&pts).unwrap();
    assert!(c.dist(Vec3::new(5.0, 2.0, 7.0)) < 1e-9 && n.cross(Vec3::Z).len() < 1e-9);
    let line: Vec<Vec3> = (0..10).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect();
    assert!(circle_of(&line).is_none());
}

/// A moved component's edge axis is measured in the displayed world frame.
#[test]
fn straight_edge_snap_follows_a_rotated_occurrence() {
    let mut app = two_boxes();
    let b = app.session.doc.find_component("B").unwrap();
    app.run("occurrence.move", json!({"occurrence": occ(&app, "B"), "axis": [1, 2, 3], "angle": "37 deg", "translate": [50, 20, 10]})).unwrap();
    let transform = app.session.doc.occurrence_of(b).unwrap().transform;
    let mid = solidcraft_engine::doc::apply_point(&transform, Vec3::new(30.0, 10.0, 10.0));
    let axis = solidcraft_engine::doc::apply_vector(&transform, Vec3::Y);
    let st = crate::open_meshing::session_world(&app.session);
    let mesh = st.body("Body2").unwrap().mesh();
    let index = mesh.edges.iter().position(|e| polyline_mid(e).dist(mid) < 1e-6).unwrap();
    let snap = snap_of(&app.session, &Sel::Edge { body: "Body2".into(), index, point: mid }).unwrap();
    assert!(snap.at.dist(mid) < 1e-6);
    assert!(snap.z.cross(axis).len() < 1e-9, "{snap:?}");
    assert_eq!(snap.param["occurrence"], occ(&app, "B"));
}

/// Only finite straight polylines supply an axis; reversing their samples keeps its direction.
#[test]
fn straight_edge_axes_reject_curves_and_degenerate_samples() {
    for dir in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(-2.0, 3.0, 1.0)] {
        let mut pts = vec![Vec3::ZERO, dir * 0.5, dir];
        let axis = straight_edge_axis(&pts).unwrap();
        assert!(axis.cross(dir).len() < 1e-9);
        assert!((axis.len() - 1.0).abs() < 1e-9);
        pts.reverse();
        assert_eq!(straight_edge_axis(&pts), Some(axis));
    }
    for pts in [
        vec![],
        vec![Vec3::X],
        vec![Vec3::X, Vec3::X],
        vec![Vec3::ZERO, Vec3::Y, Vec3::X],
        vec![Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X],
        vec![Vec3::ZERO, Vec3::X * f64::INFINITY],
    ] {
        assert!(straight_edge_axis(&pts).is_none(), "{pts:?}");
    }
}

/// The centre of the first text shape reading `text` in a frame's output.
fn text_at(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.rect.center().to_vec2()),
        _ => None,
    })
}

/// OK pressed while the animated preview is computing and released once it is not (the busy
/// spinner coming and going between the two frames) still applies the joint.
#[test]
fn ok_click_survives_a_busy_preview() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.animate = true;
    }
    app.dialog = Some(d);
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp, busy: bool, events: Vec<egui::Event>| {
        app.preview.busy = busy;
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
        // Nothing is painted: drop the texture changes (an unapplied delta panics on drop).
        out.textures_delta.clear();
        out
    };
    frame(&mut app, false, vec![]);
    let out = frame(&mut app, false, vec![]);
    let ok = text_at(&out, "OK").expect("an OK button");
    let button = |pressed| egui::Event::PointerButton { pos: ok, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(&mut app, true, vec![egui::Event::PointerMoved(ok)]);
    frame(&mut app, true, vec![button(true)]);
    frame(&mut app, false, vec![button(false)]);
    assert_eq!(app.session.doc.assembly.joints.len(), 1, "the click applied the joint");
    assert!(app.dialog.is_none());
}

/// #1239: two parts as bodies of one component (a multi-body STEP import). Picks on both used to
/// make no joint with nothing said, so no option seemed to do anything. The dialog now says why
/// under the picks and offers Create Components from Bodies, after which the same picks join.
#[test]
fn picks_on_one_component_say_why_and_offer_components() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10})).unwrap();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "corner": [30, 0, 0]})).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    let bodies = vec!["Body1".to_string(), "Body2".to_string()];
    // As-built: two bodies of one component are one component.
    app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.start("joint.as_built");
    let d = app.dialog.take().unwrap();
    let Kind::Assembly(k) = &d.kind else { panic!("{:?}", d.kind) };
    assert_eq!(joint_problem(&app, k, &d.inputs).map(|p| p.1), Some(bodies.clone()));
    // Two faces of one body: pick another part (nothing to part).
    app.session.selection.clear();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 0.0)).unwrap());
    let Kind::Assembly(k) = &d.kind else { panic!("{:?}", d.kind) };
    assert_eq!(joint_problem(&app, k, &d.inputs).map(|p| p.1), Some(vec![]));
    // A face of each body.
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    assert!(apply_commands(&app, &d).is_err(), "one component: no joint");
    app.dialog = Some(d);
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp, events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
        // Nothing is painted: drop the texture changes (an unapplied delta panics on drop).
        out.textures_delta.clear();
        out
    };
    // The auto-sized dialog takes a few frames to settle its width.
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let out = frame(&mut app, vec![]);
    let why = solidcraft_ui_text::format("joint.one_component", &[("bodies", "Body1, Body2")]);
    assert!(text_at(&out, &why).is_some(), "the dialog says why");
    let at = text_at(&out, solidcraft_ui_text::tr("cmd.component_from_bodies")).expect("and offers the way out");
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(&mut app, vec![egui::Event::PointerMoved(at)]);
    frame(&mut app, vec![button(true)]);
    frame(&mut app, vec![button(false)]);
    assert!(bodies.iter().all(|b| app.session.doc.find_component(b).is_some()), "each body has a component");
    let d = app.dialog.as_ref().expect("the dialog stays open");
    let c = apply_commands(&app, d).unwrap();
    assert_eq!((c[0].1["a"]["occurrence"].clone(), c[0].1["b"]["occurrence"].clone()), (json!(occ(&app, "Body1")), json!(occ(&app, "Body2"))));
    let out = frame(&mut app, vec![]);
    assert!(text_at(&out, &why).is_none(), "nothing left to say");
}

#[test]
fn rigid_group_bodies_in_one_component_explain_how_to_separate_them() {
    for component in [None, Some("Part")] {
        let mut s = Session::default();
        if let Some(name) = component {
            s.execute("component.create", &json!({"name": name})).unwrap();
        }
        for x in [0, 30, 60, 90, 120] {
            s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        let mut app = SolveApp::new(s, Services::default());
        app.session.selection = (1..=5).map(|i| Sel::Body { name: format!("Body{i}") }).collect();
        app.start("joint.rigid_group");
        let d = app.dialog.take().unwrap();
        assert_eq!(
            apply_commands(&app, &d).unwrap_err(),
            "These 5 bodies are all in one component (or in the design itself): use Create Components from Bodies first."
        );
        app.run("component.from_bodies", json!({"bodies": ["Body1", "Body2", "Body3", "Body4", "Body5"]})).unwrap();
        assert_eq!(apply_commands(&app, &d).unwrap()[0].1["occurrences"].as_array().unwrap().len(), 5);
    }
}

#[test]
fn rigid_group_preserves_two_instances_of_one_component() {
    let mut s = Session::default();
    let first = s.execute("component.create", &json!({"name": "Part"})).unwrap()["occurrence"].as_u64().unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let second = s.execute("occurrence.copy", &json!({"occurrence": first, "translate": [30, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    s.selection = [first, second].into_iter().map(|id| Sel::Body { name: s.occurrence_bodies(&[id], false).unwrap()[0].clone() }).collect();
    let mut app = SolveApp::new(s, Services::default());
    app.start("joint.rigid_group");
    let d = app.dialog.take().unwrap();
    let commands = apply_commands(&app, &d).unwrap();
    assert_eq!(commands, vec![("joint.rigid_group".into(), json!({"occurrences": [first, second]}))]);
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    assert_eq!(app.session.doc.assembly.joints.len(), 1);
}

/// Two top faces mate face to face: B ends up upside down on A, not inside it;
/// Flip turns it back over (into A).
#[test]
fn face_snaps_mate_face_to_face() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 10.0)).unwrap());
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["flip"], false, "mating is the engine's default; no flip");
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let st = app.session.world_state().unwrap();
    let b = st.body("Body2").unwrap().mesh().bounds();
    assert!((b.min.z - 10.0).abs() < 1e-6 && (b.max.z - 20.0).abs() < 1e-6, "{b:?}");
    // Edit Joint shows the mate unflipped; Flip puts B back into A's volume.
    let id = app.session.doc.assembly.joints[0].id;
    let mut e = edit_joint(&app, id).unwrap();
    if let Kind::Assembly(Asm::Joint(f)) = &mut e.kind {
        assert!(!f.flip);
        f.flip = true;
    }
    for (id, p) in apply_commands(&app, &e).unwrap() {
        app.run(&id, p).unwrap();
    }
    let b = app.session.world_state().unwrap().body("Body2").unwrap().mesh().bounds();
    assert!((b.max.z - 10.0).abs() < 1e-6, "{b:?}");
}

/// Drive Joints takes the keyboard: typing a value and Enter drives the joint.
#[test]
fn drive_value_takes_the_keyboard() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run("joint.create", json!({"type": "revolute", "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}))
        .unwrap();
    app.start("joint.drive");
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp, events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        // Nothing renders here: drop the frame's texture changes deliberately.
        ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx())).textures_delta.clear();
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    frame(&mut app, vec![egui::Event::Text("90".into())]);
    let key = |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    frame(&mut app, vec![key(true), key(false)]);
    frame(&mut app, vec![]);
    let j = &app.session.doc.assembly.joints[0];
    assert!((j.values[0] - 90f64.to_radians()).abs() < 1e-9, "{:?}", j.values);
    assert!(app.dialog.is_none());
}

/// A real slider drag updates the requested pose while the pointer is still down.
/// Previewing and cancelling must leave the saved joint and occurrence positions alone.
#[test]
fn drive_slider_previews_intermediate_positions_before_release() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    let (a, b) = (occ(&h.app, "A"), occ(&h.app, "B"));
    h.app
        .run(
            "joint.create",
            json!({"type": "revolute", "a": {"occurrence": a, "face": [10, 10, 10]},
        "b": {"occurrence": b, "face": [40, 10, 0]}, "limits": [[-90, 90]]}),
        )
        .unwrap();
    let before = h.app.session.doc.clone();
    h.app.start("joint.drive");
    h.frames(4);
    let bounds = h
        .output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, n)| (n.role() == egui::accesskit::Role::Slider).then(|| n.bounds()).flatten())
        .expect("drive slider");
    let at = |fraction: f64| egui::pos2((bounds.x0 + fraction * (bounds.x1 - bounds.x0)) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    let start = at(0.25);
    h.app.synthetic.extend([
        egui::Event::PointerMoved(start),
        egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
    ]);
    h.frames(3);
    let first = preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    h.app.synthetic.push(egui::Event::PointerMoved(at(0.75)));
    h.frames(3);
    let second = preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_ne!(first, second, "the held slider requests a new pose before release");
    assert!(h.ctx.input(|i| i.pointer.primary_down()));
    // The preview runs on a worker thread; on a machine busy with other test binaries it can
    // take several seconds, so wait as long as the other preview tests do (#1549).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while h.app.preview.busy {
        assert!(std::time::Instant::now() < deadline, "held-slider preview did not finish");
        std::thread::sleep(std::time::Duration::from_millis(1));
        h.frame();
    }
    assert!(h.app.preview.active, "the viewport shows the pose before release");
    assert!(h.app.preview.error.is_none());
    assert_eq!(h.app.preview.replaced, ["Body2"]);
    assert_eq!(h.app.session.doc, before);
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: at(0.75),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.frames(3);
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert!(!h.app.preview.active);
    assert_eq!(h.app.session.doc, before, "cancel leaves the joint at its original angle");
}

/// Dragging the slider rewrites the value box, which keeps its focus (a drag is not a click) and
/// its selection: the rewritten text must not open the parameter suggestions over the slider.
#[test]
fn slider_drag_does_not_open_suggestions_over_the_slider() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    for name in ["depth", "drop"] {
        h.app.run("parameters.add", json!({"name": name, "expression": "5 mm"})).unwrap();
    }
    let (a, b) = (occ(&h.app, "A"), occ(&h.app, "B"));
    h.app
        .run(
            "joint.create",
            json!({"type": "revolute", "a": {"occurrence": a, "face": [10, 10, 10]},
        "b": {"occurrence": b, "face": [40, 10, 0]}, "limits": [[-90, 90]]}),
        )
        .unwrap();
    h.app.start("joint.drive");
    h.frames(4);
    let editor = egui::Id::unique("sc_drive_value");
    let bounds = h
        .output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, n)| (n.role() == egui::accesskit::Role::Slider).then(|| n.bounds()).flatten())
        .expect("drive slider");
    let at = |fraction: f64| egui::pos2((bounds.x0 + fraction * (bounds.x1 - bounds.x0)) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    h.app.synthetic.extend([
        egui::Event::PointerMoved(at(0.3)),
        egui::Event::PointerButton { pos: at(0.3), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
    ]);
    // Past 0 deg the selection left from "0 deg" ends after the "d" of "6.0 deg": a word.
    for step in 0..=10 {
        h.app.synthetic.push(egui::Event::PointerMoved(at(0.3 + 0.023 * step as f64)));
        h.frames(2);
        assert!(!crate::expression_complete::showing(&h.ctx, editor), "suggestions opened during the drag (step {step})");
    }
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: at(0.53),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.frames(3);
    assert!(h.ctx.memory(|m| m.has_focus(editor)), "the value box kept its focus through the drag");
    assert!(!crate::expression_complete::showing(&h.ctx, editor), "suggestions opened after the drag");
    let Some(Dialog { kind: Kind::Assembly(Asm::Drive { values, .. }), .. }) = h.app.dialog.as_ref() else { panic!("drive dialog") };
    assert_eq!(values[0].get(3..5), Some(" d"), "the caret ends after a \"d\": {values:?}");
}

/// Named joint origins are drawn where their component is: a drive of the occurrence, or a free
/// move not captured yet, carries them along.
#[test]
fn joint_origins_follow_their_component() {
    let mut app = two_boxes();
    app.session.execute("joint.origin", &json!({"name": "Top of B", "face": [40, 10, 10]})).unwrap();
    let at = |app: &SolveApp| {
        let v = origins_world(&app.session);
        let (_, m) = v.iter().find(|(n, _)| n == "Top of B").unwrap();
        solidcraft_engine::doc::apply_point(m, Vec3::ZERO)
    };
    assert!(at(&app).dist(Vec3::new(40.0, 10.0, 10.0)) < 1e-6, "{:?}", at(&app));
    let b = occ(&app, "B");
    app.session.execute("occurrence.move", &json!({"occurrence": b, "translate": [0, 0, 15]})).unwrap();
    assert!(at(&app).dist(Vec3::new(40.0, 10.0, 25.0)) < 1e-6, "{:?}", at(&app));
    // A drag not captured yet.
    let mut m = app.session.doc.occurrence_of(app.session.doc.find_component("B").unwrap()).unwrap().transform;
    m[3][0] += 5.0;
    app.session.pending_moves.insert(b, m);
    assert!(at(&app).dist(Vec3::new(45.0, 10.0, 25.0)) < 1e-6, "{:?}", at(&app));
}

/// A named joint origin on a hidden occurrence draws no triad; nor does one in a hidden Joints
/// folder (the design's or the placement's). Showing it again brings the triad back.
#[test]
fn joint_origin_triads_follow_occurrence_visibility() {
    let mut app = two_boxes();
    app.session.execute("joint.origin", &json!({"name": "Top of B", "face": [40, 10, 10]})).unwrap();
    assert!(origin_drawn(&app.session, "Top of B"));
    let b = occ(&app, "B");
    app.session.execute("browser.visibility", &json!({"paths": [[b]], "visible": false})).unwrap();
    assert!(!origin_drawn(&app.session, "Top of B"), "a hidden occurrence still draws its origin triad");
    app.session.execute("browser.visibility", &json!({"paths": [[b]], "visible": true})).unwrap();
    assert!(origin_drawn(&app.session, "Top of B"));
    app.session.visibility.hidden_folders.push(format!("o:{b}/joints"));
    assert!(!origin_drawn(&app.session, "Top of B"), "the placement's hidden Joints folder");
    app.session.visibility.hidden_folders.clear();
    app.session.visibility.hidden_folders.push("c0/joints".into());
    assert!(!origin_drawn(&app.session, "Top of B"), "the design's hidden Joints folder");
    app.session.visibility.hidden_folders.clear();
    // On a copy, the triad follows that copy, not the component's first placement.
    app.session.execute("occurrence.copy", &json!({"component": "B", "translate": [0, 40, 0]})).unwrap();
    app.session.execute("joint.origin", &json!({"name": "Top of copy", "face": [40, 50, 10]})).unwrap();
    let copy = app.session.doc.occurrences.iter().find(|o| o.component == app.session.doc.find_component("B").unwrap() && o.id != b).unwrap().id;
    assert_eq!(app.session.doc.assembly.origins.iter().find(|o| o.name == "Top of copy").unwrap().origin.occurrence, copy);
    app.session.execute("browser.visibility", &json!({"paths": [[b]], "visible": false})).unwrap();
    assert!(origin_drawn(&app.session, "Top of copy"), "hiding the first placement hides the copy's triad");
    app.session.execute("browser.visibility", &json!({"paths": [[b]], "visible": true})).unwrap();
    app.session.execute("browser.visibility", &json!({"paths": [[copy]], "visible": false})).unwrap();
    assert!(!origin_drawn(&app.session, "Top of copy"), "a hidden copy still draws its origin triad");
    assert!(origin_drawn(&app.session, "Top of B"));
}

/// Edit Feature on a moved component: the dialog shows the feature's picks where the
/// component is (its stored points are in the component's frame).
#[test]
fn edit_feature_on_a_moved_component_refills_in_the_world() {
    let mut app = two_boxes();
    let b = occ(&app, "B");
    app.session.execute("occurrence.move", &json!({"occurrence": b, "translate": [0, 0, 15]})).unwrap();
    let r = app.session.execute("solid.fillet", &json!({"edges": [[40, 0, 25]], "radius": 2})).unwrap();
    let id = r["feature"].as_u64().unwrap();
    let d = crate::dialogs::for_feature(&app, id, None).unwrap();
    let items: Vec<Sel> = d.inputs.iter().flat_map(|i| i.items.clone()).collect();
    let Some(Sel::Edge { body, point, .. }) = items.first() else { panic!("{items:?}") };
    assert_eq!(body, "Body2");
    assert!(point.dist(Vec3::new(40.0, 0.0, 25.0)) < 1e-6, "{point:?}");
}

/// Edit Feature of a face feature in a moved component shows its faces on the feature's own
/// body and rebuilds unchanged (#15903). Its points are stored in the component's frame: placed
/// through the frame of a root body that stands at the component's own origin, they lie on that
/// body too, and OK then sent a point on no face of the feature's body ("no face at …") or moved
/// the feature onto the root body.
#[test]
fn edit_face_features_in_a_moved_component_refill_on_their_own_body() {
    let cases = [
        ("solid.offset_face", json!({"faces": [[3, 43, 10]], "distance": 1, "body": "Inner"}), Vec3::new(3.0, 43.0, 10.0)),
        ("solid.replace_face", json!({"faces": [[3, 43, 10]], "target": "Lid", "body": "Inner"}), Vec3::new(3.0, 43.0, 10.0)),
    ];
    for (command, params, picked) in cases {
        // The same holed box in the root and in component Part, which then moves 40 mm along Y.
        let mut h = crate::scenario::Harness::new();
        for (component, body) in [("root", "Outer"), ("Part", "Inner")] {
            if component != "root" {
                h.app.run("component.create", json!({"name": component})).unwrap();
            }
            h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body})).unwrap();
            h.app.run("solid.cylinder", json!({"base": [10, 10, -1], "radius": 3, "height": 12, "body_name": "Drill"})).unwrap();
            h.app.run("solid.combine", json!({"target": body, "tools": ["Drill"], "operation": "cut"})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        let part = occ(&h.app, "Part");
        h.app.run("occurrence.ground", json!({"occurrence": part, "grounded": false})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": part, "translate": [0, 40, 0]})).unwrap();
        h.app.run("component.activate", json!({"component": "Part"})).unwrap();
        // Replace Face's target: a plane that Edit Feature shows again as it was.
        h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 12, "name": "Lid"})).unwrap();
        let id = h.app.run(command, params).unwrap()["feature"].as_u64().unwrap();
        h.app.edit_feature(id);
        h.frames(1);
        let items: Vec<Sel> = h.app.dialog.as_ref().unwrap().inputs.first().unwrap().items.clone();
        let [Sel::Face { body, point, .. }] = items.as_slice() else { panic!("{command}: {items:?}") };
        assert_eq!(body, "Inner", "{command}: {point:?}");
        assert!(point.dist(picked) < 1e-6, "{command}: {point:?}");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none(), "{command}: OK rebuilds the unchanged feature");
        let feature = h.app.session.execute("timeline.inspect", &json!({"feature": id})).unwrap();
        assert!(feature["error"].is_null(), "{command}: {feature}");
        assert_eq!(feature["definition"]["body"], "Inner", "{command}: the feature stays on its body");
    }
}

/// A crank-rocker four-bar (crank 20, coupler 50, rocker 40, ground pivots 45 apart), built as
/// it stands with the crank at 90°: joints Crank, Crank pin, Rocker pin, Rocker.
fn four_bar() -> SolveApp {
    let mut s = Session::default();
    let (p0, o4) = ([0.0, 20.0], [45.0, 0.0]);
    // The rocker pin: where the coupler (50 from the crank pin) meets the rocker (40 from O4).
    let (dx, dy) = (o4[0] - p0[0], o4[1] - p0[1]);
    let l = f64::hypot(dx, dy);
    let a = (2500.0 - 1600.0 + l * l) / (2.0 * l);
    let h = (2500.0 - a * a).sqrt();
    let q0 = [p0[0] + dx * a / l - dy * h / l, p0[1] + dy * a / l + dx * h / l];
    for (name, at) in [("Ground", [0.0, 0.0]), ("Crank", p0), ("Coupler", q0), ("Rocker", o4)] {
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        s.execute("component.create", &json!({ "name": name })).unwrap();
        s.execute("solid.box", &json!({"length": 2, "width": 2, "height": 2, "corner": [at[0], at[1], -5]})).unwrap();
    }
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    s.execute("occurrence.ground", &json!({"occurrence": "Ground:1", "grounded": true})).unwrap();
    for (name, a, b, p) in [
        ("Crank", "Ground", "Crank", [0.0, 0.0]),
        ("Crank pin", "Crank", "Coupler", p0),
        ("Rocker pin", "Coupler", "Rocker", q0),
        ("Rocker", "Ground", "Rocker", o4),
    ] {
        let at = |o: &str| json!({"occurrence": o, "point": [p[0], p[1], 0], "z": [0, 0, 1]});
        s.execute("joint.create", &json!({"type": "revolute", "a": at(a), "b": at(b), "flip": true, "name": name})).unwrap();
    }
    SolveApp::new(s, Services::default())
}

/// Drive Joints says where a drive stopped short and why: the preview names the loop that can't
/// close further, OK says it on the status line, and a drive within reach says nothing.
#[test]
fn drive_says_where_a_loop_stopped_it() {
    let mut app = four_bar();
    let colors = crate::preview::Colors {
        body: [1, 1, 1, 255],
        added: [2, 2, 2, 255],
        new_body: [2, 2, 2, 255],
        removed: [3, 3, 3, 100],
        lost: [5, 5, 5, 100],
        selected: [7, 7, 7, 255],
        mirrored: [6, 6, 6, 100],
        edge: [0, 0, 0, 255],
        ghost: true,
        tint_new: true,
        cut_tint: true,
        glass_new: false,
    };
    let rocker = app.session.doc.assembly.joints.iter().position(|j| j.name == "Rocker").unwrap();
    let mut d = drive_joint(&app, app.session.doc.assembly.joints[rocker].id).unwrap();
    let ask = |d: &mut Dialog, v: &str| {
        if let Kind::Assembly(Asm::Drive { values, .. }) = &mut d.kind {
            values[0] = v.into();
        }
    };
    // Within its swing: no stop.
    ask(&mut d, "20 deg");
    let b = world_preview(&app.session, &apply_commands(&app, &d).unwrap(), colors).unwrap().unwrap();
    assert!(b.stopped.is_none());
    // Past its toggle position (about +50.4° from where it stands).
    ask(&mut d, "100 deg");
    let b = world_preview(&app.session, &apply_commands(&app, &d).unwrap(), colors).unwrap().unwrap();
    let why = b.stopped.as_ref().and_then(drive_stop).unwrap();
    assert!(why.starts_with("Stopped at 50.") && why.contains("can't close any further"), "{why}");
    // OK drives it there and says so.
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    let (status, _, error) = app.status.clone().unwrap();
    assert!(status == why && !error, "{status}");
}

/// A contact set stops a drive too: the message names the parts that would pass through each other.
#[test]
fn drive_says_which_parts_a_contact_set_stopped() {
    let mut app = two_boxes();
    let at = |o: &str| json!({"occurrence": o, "point": [30, 10, 5], "z": [1, 0, 0]});
    app.run("joint.create", json!({"type": "slider", "a": at("A"), "b": at("B"), "flip": true, "name": "Rail"})).unwrap();
    app.run("contact.create", json!({"occurrences": ["A:1", "B:1"]})).unwrap();
    let r = app.run("joint.drive", json!({"joint": "Rail", "value": -20})).unwrap();
    let why = drive_stop(&r).unwrap();
    assert!(why.starts_with("Stopped at -1") && why.contains("A:1") && why.contains("B:1") && why.contains("would pass through"), "{why}");
}

/// A drive whose joint is joined to nothing grounded moves nothing: the preview row and the
/// status line name the part that floats.
#[test]
fn drive_says_when_its_joint_is_joined_to_nothing_grounded() {
    let mut app = two_boxes();
    for (name, x) in [("C", 100), ("D", 130)] {
        app.run("component.activate", json!({"component": "root"})).unwrap();
        app.run("component.create", json!({ "name": name })).unwrap();
        app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    let at = |o: &str, x: f64| json!({"occurrence": o, "point": [x, 10, 5], "z": [1, 0, 0]});
    app.run("joint.create", json!({"type": "slider", "a": at("A", 30.0), "b": at("B", 30.0), "flip": true, "name": "Rail"})).unwrap();
    app.run("joint.create", json!({"type": "revolute", "a": at("C", 130.0), "b": at("D", 130.0), "flip": true, "name": "Turn"})).unwrap();
    let turn = app.session.doc.assembly.joints.iter().find(|j| j.name == "Turn").unwrap().id;
    let mut d = drive_joint(&app, turn).unwrap();
    if let Kind::Assembly(Asm::Drive { values, .. }) = &mut d.kind {
        values[0] = "45 deg".into();
    }
    let colors = crate::preview::Colors {
        body: [1, 1, 1, 255],
        added: [2, 2, 2, 255],
        new_body: [2, 2, 2, 255],
        removed: [3, 3, 3, 100],
        lost: [5, 5, 5, 100],
        selected: [7, 7, 7, 255],
        mirrored: [6, 6, 6, 100],
        edge: [0, 0, 0, 255],
        ghost: true,
        tint_new: true,
        cut_tint: true,
        glass_new: false,
    };
    let b = world_preview(&app.session, &apply_commands(&app, &d).unwrap(), colors).unwrap().unwrap();
    let why = b.stopped.as_ref().and_then(drive_stop).unwrap();
    assert!(why.starts_with("Moves nothing") && why.contains("C:1"), "{why}");
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    assert_eq!(app.status.clone().map(|(status, _, error)| (status, error)), Some((why, false)));
}

/// Drive Joints opens on the joint picked in the browser; with none picked, on the first joint
/// that moves.
#[test]
fn drive_opens_on_the_picked_joint() {
    let mut app = four_bar();
    let drive_on = |app: &mut SolveApp| {
        app.dialog = None;
        app.start("joint.drive");
        match &app.dialog.as_ref().unwrap().kind {
            Kind::Assembly(Asm::Drive { joint, .. }) => app.session.doc.assembly.joints[*joint].name.clone(),
            k => panic!("{k:?}"),
        }
    };
    assert_eq!(drive_on(&mut app), "Crank");
    let rocker = app.session.doc.assembly.joints.iter().find(|j| j.name == "Rocker").unwrap().id;
    pick_joint(&mut app, rocker, false);
    assert_eq!(drive_on(&mut app), "Rocker");
    // A rigid joint picked: it opens on that one, to say why it can't drive it (#6981).
    app.run("joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Crank", "name": "Lock"})).unwrap();
    let lock = app.session.doc.assembly.joints.iter().find(|j| j.name == "Lock").unwrap().id;
    pick_joint(&mut app, lock, false);
    assert_eq!(drive_on(&mut app), "Lock");
    // Motion Trace still starts on a joint that moves.
    app.dialog = None;
    app.start("motion.trace");
    match &app.dialog.as_ref().unwrap().kind {
        Kind::Assembly(Asm::Trace(f)) => assert_eq!(app.session.doc.assembly.joints[f.joint].name, "Crank"),
        k => panic!("{k:?}"),
    }
}

/// Drive Joints in a design whose only joint is rigid (#6981): it opens on that joint and says it
/// holds its parts still and how to let them move, runs no drive (no preview error, no Create
/// Issue, OK off), and its Edit Joint button turns the dialog into Edit Joint on that joint.
#[test]
fn drive_on_a_rigid_joint_says_why_and_offers_edit_joint() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    let at = |o: &str| json!({"occurrence": o, "point": [25, 10, 5], "z": [1, 0, 0]});
    h.app.run("joint.create", json!({"type": "rigid", "a": at("A"), "b": at("B"), "name": "Glue"})).unwrap();
    let glue = h.app.session.doc.assembly.joints[0].id;
    h.app.start("joint.drive");
    h.frames(4);
    let d = h.app.dialog.clone().unwrap();
    let e = apply_commands(&h.app, &d).unwrap_err();
    assert!(e.contains("Glue") && e.contains("holds its parts still") && e.contains("Revolute") && e.contains("Edit Joint"), "{e}");
    assert!(h.app.preview.error.is_none(), "no drive is previewed");
    assert!(crate::scenario::handle_at("dialog:create_issue").is_none(), "no Create Issue for a rigid joint");
    h.step(&json!({"click": {"handle": "drive:edit_joint"}})).unwrap();
    h.frames(2);
    match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Assembly(Asm::Joint(f)) => assert_eq!(f.editing, Some(glue)),
        k => panic!("{k:?}"),
    }
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.len(), 4, "the entry point opens a complete edit form");
}

#[test]
fn secondary_alignment_review_drive_entry_restores_all_four_inputs() {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.start("joint.create");
    let mut d = h.app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0), Vec3::new(40.0, 20.0, 5.0), Vec3::new(20.0, 10.0, 5.0)] {
        d.pick(&h.app.session, face_sel(&h.app.session, p).unwrap());
    }
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    let before = h.app.session.doc.clone();
    h.app.start("joint.drive");
    h.frames(4);
    h.step(&json!({"click":{"handle":"drive:edit_joint"}})).unwrap();
    h.frames(2);
    let edit = h.app.dialog.as_ref().unwrap();
    assert_eq!(edit.inputs.len(), 4, "Drive's Edit Joint must transfer the selection inputs as well as the form kind");
    assert!(edit.inputs.iter().take(2).all(|input| !input.items.is_empty()));
    // Reopened alignment fields display their persisted identity rather than a transient Sel.
    assert!(stored_alignment(&edit.kind, 2).is_some());
    assert!(stored_alignment(&edit.kind, 3).is_some());
    assert!(crate::scenario::handle_at("dialog:clear:dialog.alignment_1_optional").is_some());
    assert!(crate::scenario::handle_at("dialog:clear:dialog.alignment_2_optional").is_some());
    let commands = apply_commands(&h.app, edit).unwrap();
    assert_eq!(commands[0].0, "joint.edit");
    let mut scratch = h.app.session.scratch();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    assert_eq!(scratch.doc.assembly.joints, before.assembly.joints);
}

/// A rigid joint has nothing to drive: Drive Joints won't start on it.
#[test]
fn drive_preset_refuses_a_rigid_joint() {
    let mut app = two_boxes();
    let at = |o: &str| json!({"occurrence": o, "point": [25, 10, 5], "z": [1, 0, 0]});
    app.run("joint.create", json!({"type": "rigid", "a": at("A"), "b": at("B"), "name": "Glue"})).unwrap();
    let mut k = Asm::Drive { joint: 0, values: Vec::new(), focus: true };
    let e = preset(&app.session, &mut k, &json!({"joint": "Glue"})).unwrap_err();
    assert!(e.contains("rigid"), "{e}");
}

/// Motion Trace on the four-bar: a pick on the coupler and the first joint (the crank) driven a
/// turn give the trace's parameters, and OK keeps the path as a 3D sketch spline. A pick on the
/// coupler where an exploded view shows it traces from where the coupler is placed.
#[test]
fn motion_trace_keeps_the_picked_point_path() {
    let mut app = four_bar();
    let coupler = occ(&app, "Coupler");
    let top = |app: &SolveApp| {
        let b = occurrence_bounds(&app.session, &[coupler]);
        Vec3::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0, b.max.z)
    };
    let placed = top(&app);
    app.start("motion.trace");
    let mut d = app.dialog.clone().unwrap();
    assert!(apply_commands(&app, &d).is_err(), "no point yet");
    d.pick(&app.session, face_sel(&app.session, placed).unwrap());
    let c = apply_commands(&app, &d).unwrap();
    let (id, p) = &c[0];
    assert_eq!(id, "motion.trace.keep");
    let crank = app.session.doc.assembly.joints.iter().find(|j| j.name == "Crank").unwrap().id;
    assert_eq!((&p["occurrence"], &p["joint"], &p["from"], &p["to"]), (&json!([coupler]), &json!(crank), &json!("0 deg"), &json!("360 deg")), "{p}");
    let at = |p: &Value| Vec3::new(p["point"][0].as_f64().unwrap(), p["point"][1].as_f64().unwrap(), p["point"][2].as_f64().unwrap());
    assert!(at(p).dist(placed) < 1e-6, "{p}");
    let mut scratch = app.session.scratch();
    scratch.param_check = solidcraft_engine::schema::ParamCheck::Strict;
    let preview = scratch.execute("motion.trace", &super::trace::preview_params(p.clone())).unwrap();
    assert!(preview["samples"] == 100 && preview["stopped_at"].is_null(), "{preview}");
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    let sketch = app.session.doc.find_feature("Motion Trace1").unwrap().id;
    let wire = app.session.doc.sketch(sketch).unwrap().wires[0].clone();
    assert_eq!(wire.fit.len(), 100);
    assert_eq!(wire.fit.first(), wire.fit.last(), "a full turn closes the spline");
    // Exploded: the coupler shows away from its placement; the pick maps back to it.
    app.run("explode.create", json!({"name": "Apart", "scale": 1})).unwrap();
    app.run("explode.show", json!({"view": "Apart"})).unwrap();
    let shown = top(&app);
    assert!(shown.dist(placed) > 1.0, "the exploded view moves the coupler");
    app.start("motion.trace");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, shown).unwrap());
    let p = apply_commands(&app, &d).unwrap()[0].1.clone();
    assert!(at(&p).dist(placed) < 1e-6, "{p}");
    // The preview is drawn moved with the coupler: it starts at the point picked where it shows.
    let start = app.session.scratch().execute("motion.trace", &p).unwrap()["points"][0].clone();
    let (_, _, seen, put) = trace::placement(&app.session, &d.inputs).unwrap();
    let drawn = {
        use solidcraft_engine::doc::{apply_point, mat_inverse};
        apply_point(&seen, apply_point(&mat_inverse(&put).unwrap(), at(&json!({"point": start}))))
    };
    assert!(drawn.dist(shown) < 1e-6, "{drawn:?} wants {shown:?}");
    // The dialog retained the picked face, rather than its old coordinates. Resizing that
    // part raises its top face and the whole saved trajectory by the same amount.
    let component = app.session.doc.find_component("Coupler").unwrap();
    let height = app
        .session
        .doc
        .features
        .iter()
        .find(|f| f.component == component && matches!(f.kind, solidcraft_engine::doc::FeatureKind::Box { .. }))
        .unwrap()
        .param_names[2]
        .clone();
    app.run("parameters.change", json!({"name":height,"expression":"4 mm"})).unwrap();
    let updated = &app.session.doc.sketch(sketch).unwrap().wires[0].fit;
    assert!(updated.len() == wire.fit.len() && updated.iter().zip(&wire.fit).all(|(a, b)| (*a - *b - Vec3::new(0.0, 0.0, 2.0)).len() < 1e-6));
}

#[test]
fn inch_joint_offsets_limits_and_drive_values_keep_their_units() {
    let mut app = two_boxes();
    app.run("document.units", json!({"units":"in"})).unwrap();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let Kind::Assembly(Asm::Joint(f)) = &mut d.kind else { panic!("no joint") };
    f.kind = TYPES.iter().position(|t| *t == "slider").unwrap();
    f.fit_limits(&app.session.doc);
    assert_eq!((f.limits[0].1.as_str(), f.limits[0].2.as_str()), ("0 in", "20 in"));
    f.offset[0] = "0.25".into();
    f.offset[2] = "2 mm".into();
    f.limits[0] = (true, "0".into(), "0.5".into());
    let (_, p) = apply_commands(&app, &d).unwrap().remove(0);
    assert_eq!(p["offset"], json!("2 mm"));
    assert_eq!(p["limits"], json!([["0 in", "0.5 in"]]));
    let origin = &p["a"]["frame"]["origin"];
    let at = Vec3::new(origin[0].as_f64().unwrap(), origin[1].as_f64().unwrap(), origin[2].as_f64().unwrap());
    assert!(at.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-9);
    assert_eq!(p["offset_x"], json!(6.35));
    app.run("joint.create", p).unwrap();
    let joint = app.session.doc.assembly.joints[0].id;
    app.start("joint.drive");
    let mut d = app.dialog.clone().unwrap();
    let Kind::Assembly(Asm::Drive { values, .. }) = &mut d.kind else { panic!("no drive") };
    values[0] = "0.25".into();
    let (_, p) = apply_commands(&app, &d).unwrap().remove(0);
    assert_eq!(p["values"], json!(["0.25 in"]));
    app.run("joint.drive", p).unwrap();
    let actual = app.session.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap().values[0];
    assert!((actual - 6.35).abs() < 1e-9, "{actual}");
    let edit = edit_joint(&app, joint).unwrap();
    let Kind::Assembly(Asm::Joint(f)) = edit.kind else { panic!("no edit") };
    assert_eq!(f.offset[0], "0.25 in");
    assert_eq!((f.limits[0].1.as_str(), f.limits[0].2.as_str()), ("0 in", "0.5 in"));
}

/// The Assemble ribbon must open a body picker, without executing the JSON command yet.
#[test]
fn components_from_bodies_dialog_select_all_cancel_and_ok() {
    let mut h = crate::scenario::Harness::new();
    for x in [0, 30] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0]})).unwrap();
    }
    let before = h.app.session.doc.clone();
    let names: Vec<_> = h.app.session.model.state().bodies.iter().map(|body| body.name.to_string()).collect();
    h.step(&json!({"start":"component.from_bodies"})).unwrap();
    let d = h.app.dialog.as_ref().expect("From Bodies opens a dialog");
    assert!(matches!(d.kind, Kind::Assembly(Asm::FromBodies)));
    assert!(apply_commands(&h.app, d).is_err(), "OK requires at least one body");
    assert_eq!(h.app.session.doc, before);
    h.step(&json!({"widget":solidcraft_ui_text::tr("ui.select_all")})).unwrap();
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands, vec![("component.from_bodies".into(), json!({"bodies":names}))]);
    if let Ok(path) = std::env::var("SOLIDCRAFT_COMPONENTS_DIALOG_EVIDENCE") {
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let mut textures = Default::default();
        for frame in 0..6 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                h.app.logic(ui.ctx());
                h.app.ui(ui);
            });
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            if frame == 5 {
                crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures).save(&path).unwrap();
            }
        }
    }
    h.step(&json!({"widget":"Cancel"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc, before, "Cancel makes no components");
    // Existing body picks seed the same standard multi-selection input.
    h.app.session.selection = vec![Sel::Body { name: names[0].clone() }];
    h.step(&json!({"start":"component.from_bodies"})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items.body_names(), vec![names[0].clone()]);
    h.step(&json!({"widget":solidcraft_ui_text::tr("ui.select_all")})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none());
    for name in &names {
        let component = h.app.session.doc.find_component(name).expect("one component per body");
        assert_eq!(h.app.session.doc.model_body_component(h.app.session.model.state().body(name).unwrap()), component);
    }
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc, before, "the dialog applies one undoable command");
}

/// Edit Joint's Find Limits on a door hinged at the origin with a block in its way: clicked in
/// the Limits section, it fills both limits from where the door meets the block each way round
/// (rounded toward the door's value, so they stay clear), says what stopped each side under the
/// row, and OK keeps them. The design is not changed until then.
#[test]
fn find_limits_fills_the_limits_from_where_the_door_meets_the_block() {
    let mut s = Session::default();
    for (name, corner, size) in [("Door", [0, 0, 0], [100, 4, 50]), ("Block", [60, 30, 0], [20, 20, 50])] {
        for (id, p) in [
            ("component.activate", json!({"component": "root"})),
            ("component.create", json!({"name": name, "grounded": false})),
            ("solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner})),
        ] {
            s.execute(id, &p).unwrap();
        }
    }
    let hinge = json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]}, "flip": true, "name": "Hinge"});
    s.execute("joint.create", &hinge).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    let id = app.session.doc.assembly.joints[0].id;
    app.dialog = edit_joint(&app, id);
    let before = app.session.doc.clone();
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp, events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
        // Nothing is painted: drop the texture changes (an unapplied delta panics on drop).
        out.textures_delta.clear();
        out
    };
    let click = |app: &mut SolveApp, text: &str| {
        // The auto-sized dialog takes a few frames to settle.
        for _ in 0..3 {
            frame(app, vec![]);
        }
        let out = frame(app, vec![]);
        let at = text_at(&out, text).unwrap_or_else(|| panic!("no `{text}` to click"));
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(app, vec![egui::Event::PointerMoved(at)]);
        frame(app, vec![button(true)]);
        frame(app, vec![button(false)]);
    };
    click(&mut app, &solidcraft_ui_text::tr_label("Limits").to_uppercase());
    click(&mut app, solidcraft_ui_text::tr("joint.find_limits"));
    assert_eq!(*app.session.doc, *before, "nothing changes until OK");
    let Some(Kind::Assembly(Asm::Joint(f))) = app.dialog.as_ref().map(|d| &d.kind) else { panic!("the dialog stays open") };
    // The door's face 4 mm off its hinge line meets the block's corner (80, 30) opening, and the
    // face through its hinge line the corner (60, 50) a turn back the other way.
    let max = (30f64.atan2(80.0) - (4.0 / 80f64.hypot(30.0)).asin()).to_degrees();
    let min = 50f64.atan2(60.0).to_degrees() - 360.0;
    let (lo, hi) = (((min * 10.0) - 1e-6).ceil() / 10.0, ((max * 10.0) + 1e-6).floor() / 10.0);
    assert_eq!(f.limits[0], (true, num(lo, "deg"), num(hi, "deg")), "{:?}", f.found);
    assert!(f.found[0].contains("Door:1") && f.found[0].contains("Block:1") && f.found[0].lines().count() == 2, "{:?}", f.found);
    assert!(f.found_note.is_none(), "{:?}", f.found_note);
    let d = app.dialog.as_ref().unwrap();
    for (id, p) in apply_commands(&app, d).unwrap() {
        app.run(&id, p).unwrap();
    }
    let (a, b) = app.session.doc.assembly.joints[0].limits[0].unwrap();
    assert!((a.to_degrees() - lo).abs() < 1e-9 && (b.to_degrees() - hi).abs() < 1e-9, "{a} {b}");
}

/// The search runs on the joint as the dialog has it, not as saved: a door edited to swing the
/// other way (flipped) before Find Limits gets that door's limits, which OK then applies with it.
/// A slider whose way out is free leaves that side's field as it was and the row unticked.
#[test]
fn find_limits_searches_the_joint_as_edited_and_leaves_a_free_side() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run("joint.create", json!({"type": "slider", "a": {"occurrence": a, "face": [20, 10, 5]}, "b": {"occurrence": b, "face": [30, 10, 5]}}))
        .unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    let mut d = edit_joint(&app, id).unwrap();
    let Kind::Assembly(Asm::Joint(f)) = &mut d.kind else { panic!("no edit") };
    let untouched = f.limits[0].clone();
    // With the sides as the dialog opened on them (Edit Joint's side picks).
    assert_eq!(d.inputs.len(), 4);
    find_limits(&app, f, id, &d.inputs);
    // B slides into A one way (they touch face to face already: it can't go further) and away
    // from it the other, where nothing is in its way.
    assert_eq!(f.limits[0].0, untouched.0, "one side found: the row stays as it was: {:?}", f.found);
    assert!(f.limits[0].1 == untouched.1 || f.limits[0].2 == untouched.2, "{:?} {:?}", f.limits, f.found);
    assert!(
        f.found[0].contains(solidcraft_ui_text::tr_label("Minimum")) && f.found[0].contains(solidcraft_ui_text::tr_label("Maximum")),
        "{:?}",
        f.found
    );
    assert!(f.found_note.is_none(), "{:?}", f.found_note);
    // Edited after the search, the joint is no longer the one searched (its notes then go).
    let searched = f.searched.clone().expect("what was searched");
    assert!(searched.current(&app, f, &d.inputs));
    f.flip = !f.flip;
    assert!(!searched.current(&app, f, &d.inputs), "a flipped joint is not the one searched");
    f.flip = !f.flip;
    // A form that can't be applied says why instead.
    f.offset[2] = "nonsense".into();
    find_limits(&app, f, id, &d.inputs);
    assert!(f.found_note.is_some() && f.found.is_empty(), "{:?}", f.found_note);
}

/// Compute in Interference intersects and meshes each overlap on the kernel's workers. On the
/// window thread (a 1 MiB stack on Windows) the kernel ran out of stack and the app closed
/// (#11483); here the dialog's own thread has 64 KiB.
#[test]
fn interference_meshes_each_overlap_on_the_kernels_stack() {
    let mut s = Session::default();
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 10)], |id, p| s.execute(id, &p).unwrap());
    let r = s.execute("inspect.clearance", &json!({"clearance": 0, "nearest": 1})).unwrap();
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(1), "{r}");
    let st = s.world_state().unwrap();
    let meshes = std::thread::Builder::new().stack_size(64 << 10).spawn(move || overlap_meshes(&st, &r)).unwrap().join().unwrap();
    // A's 0..20 and B's 10..30 along X share 10..20.
    let b = meshes.first().map(Mesh::bounds).expect("the shared volume");
    assert_eq!(meshes.len(), 1);
    let near = |p: Vec3, q: [f64; 3]| (p.x - q[0]).abs() < 1e-6 && (p.y - q[1]).abs() < 1e-6 && (p.z - q[2]).abs() < 1e-6;
    assert!(near(b.min, [10.0, 0.0, 0.0]) && near(b.max, [20.0, 20.0, 10.0]), "{b:?}");
}

/// With the app running commands off the window thread, Compute in Interference checks on a
/// thread of its own (a slow pair froze the window until the app was closed, #11483): the
/// overlap and its shared volume arrive in a later frame, and a check of a design changed
/// meanwhile is dropped.
#[test]
fn interference_checks_off_the_window_thread_and_drops_a_check_of_a_changed_design() {
    let mut s = Session::default();
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 10)], |id, p| s.execute(id, &p).unwrap());
    let mut app = SolveApp::new(s, Services::default());
    app.apply_off_thread = Some(std::time::Duration::ZERO);
    app.start("inspect.interference");
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let state = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Assembly(Asm::Interference { result, overlaps, check, .. })) => (result.clone(), overlaps.len(), check.is_some()),
        _ => panic!("the dialog stays open"),
    };
    // Compute as its button starts it, then frames until its reply is read.
    let compute = |app: &mut SolveApp, change: bool| {
        let c = interference(app, &[], "0");
        if let Some(Kind::Assembly(Asm::Interference { result, overlaps, check, .. })) = app.dialog.as_mut().map(|d| &mut d.kind) {
            (*result, *overlaps, *check) = (None, Vec::new(), Some(c));
        }
        if change {
            app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0]})).unwrap();
        }
        for _ in 0..3000 {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| crate::dialogs::show(app, ui.ctx()));
            out.textures_delta.clear();
            if !state(app).2 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the check never came back");
    };
    compute(&mut app, false);
    let (result, overlaps, _) = state(&app);
    let result = result.expect("the check's reply");
    assert_eq!(result["overlaps"].as_array().map(Vec::len), Some(1), "{result}");
    assert_eq!(overlaps, 1, "the shared volume, drawn in red");
    compute(&mut app, true);
    assert_eq!(state(&app), (None, 0, false), "a check of the design before the change is dropped");
}

/// The Joint dialog over two boxes, Component 1 taking a pick, in the harness (a real viewport).
fn joint_picking_component_1() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app = two_boxes();
    h.app.cam.distance = 160.0;
    h.settle();
    h.app.start("joint.create");
    h
}

/// Hovering an edge in a joint input wakes the points of the face beside it (#11227, #11476): the
/// edge's middle, clicked from past the edge's own reach, lies on that face, with the face's
/// normal as z rather than the edge's direction, kept by the edge's and the face's names.
#[test]
fn a_woken_middle_lies_on_the_face_beside_its_edge() {
    let mut h = joint_picking_component_1();
    h.step(&json!({ "move": { "world": [15, 0, 10] } })).unwrap();
    let middle = Vec3::new(10.0, 0.0, 10.0);
    let woken: Vec<Vec3> = h.app.viewport.woken.points.iter().map(|p| p.at).collect();
    for p in [middle, Vec3::new(0.0, 0.0, 10.0), Vec3::new(20.0, 0.0, 10.0)] {
        assert!(woken.iter().any(|w| w.dist(p) < 1e-6), "{p:?} in {woken:?}");
    }
    assert!(matches!(h.app.viewport.woken.item, Some(Sel::Face { .. })), "the face beside the edge: {:?}", h.app.viewport.woken.item);
    let at = h.call("ui.at", json!({ "world": [10, 0, 10] }))["result"].clone();
    let below = json!([at[0].as_f64().unwrap(), at[1].as_f64().unwrap() + 12.0]);
    h.step(&json!({ "click": below })).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let picked = d.inputs[0].items.clone();
    assert!(matches!(picked.as_slice(), [Sel::Vertex { body, point }] if body == "Body1" && point.dist(middle) < 1e-6), "{picked:?}");
    let got = snap_in(&h.app, &d.inputs[0], &picked[0]).unwrap();
    assert_eq!(got.kind, SnapKind::EdgeMid);
    assert!(got.z.dist(-Vec3::Y) < 1e-9 || got.z.dist(Vec3::Z) < 1e-9, "the front or the top face's normal: {got:?}");
    assert!(got.param["edge"].is_array() && got.param["on"].is_array() && got.param.get("z").is_none(), "{}", got.param);
}

/// With only Faces in the selection filter, a joint origin clicked right on an edge is the face
/// there, not the edge or a woken point on it, and no points wake (#18255). Unfiltered, the same
/// click takes the edge's middle.
#[test]
fn a_faces_only_filter_keeps_joint_origins_on_faces() {
    let mut h = joint_picking_component_1();
    let middle = Vec3::new(10.0, 0.0, 10.0);
    let at = h.call("ui.at", json!({ "world": [10, 0, 10] }))["result"].clone();
    h.step(&json!({ "move": { "world": [10, 0, 10] } })).unwrap();
    h.step(&json!({ "click": at })).unwrap();
    let picked = h.app.dialog.as_ref().unwrap().inputs[0].items.clone();
    assert!(matches!(picked.as_slice(), [Sel::Edge { .. } | Sel::Vertex { .. }]), "unfiltered, the edge: {picked:?}");

    let mut h = joint_picking_component_1();
    h.app.ui.pick_off = vec!["edges".into(), "vertices".into()];
    h.step(&json!({ "move": { "world": [10, 0, 10] } })).unwrap();
    assert!(h.app.viewport.woken.points.is_empty(), "no points wake: {:?}", h.app.viewport.woken.points);
    h.step(&json!({ "click": at })).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let picked = d.inputs[0].items.clone();
    assert!(matches!(picked.as_slice(), [Sel::Face { body, .. }] if body == "Body1"), "the face at the edge: {picked:?}");
    let got = snap_in(&h.app, &d.inputs[0], &picked[0]).unwrap();
    assert_eq!(got.kind, SnapKind::FaceCenter);
    assert_ne!(got.at, middle, "the face's centre, not the edge's middle: {got:?}");
}

/// A woken point that is not its item's own snap (a straight edge's end) stays a point, and a
/// face's centre is the face; with no woken item a vertex is a vertex, as before.
#[test]
fn woken_points_stand_for_their_item_only_where_its_snap_is() {
    let mut app = two_boxes();
    app.start("joint.create");
    let edge = edge_sel(&app.session, Vec3::new(10.0, 0.0, 10.0)).unwrap();
    let corner = Sel::Vertex { body: "Body1".into(), point: Vec3::new(0.0, 0.0, 10.0) };
    assert_eq!(woken_pick(&app, true, corner.clone()), corner, "nothing woken");
    app.viewport.woken = crate::snap_wake::Woken {
        item: Some(edge.clone()),
        points: solidcraft_engine::snap::snap_points(&app.session, &edge),
        revision: app.session.revision,
        ..Default::default()
    };
    assert_eq!(woken_pick(&app, true, corner.clone()), corner, "an end stays a point");
    assert_eq!(woken_pick(&app, true, Sel::Vertex { body: "Body1".into(), point: Vec3::new(10.0, 0.0, 10.0) }), edge);
    // Another body's point at the same place stays that body's.
    let other = Sel::Vertex { body: "Body2".into(), point: Vec3::new(10.0, 0.0, 10.0) };
    assert_eq!(woken_pick(&app, true, other.clone()), other, "a point of another body");
    // The dialog the caller has decides, not the app's: one whose input doesn't take snaps keeps
    // the point.
    let mut moved = app.dialog.clone().unwrap();
    assert!(takes_snaps(&moved));
    moved.inputs[0].accept = FACES;
    assert!(!takes_snaps(&moved));
    let mid = Sel::Vertex { body: "Body1".into(), point: Vec3::new(10.0, 0.0, 10.0) };
    assert_eq!(woken_pick(&app, takes_snaps(&moved), mid.clone()), mid);
    let face = face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    app.viewport.woken = crate::snap_wake::Woken {
        item: Some(face.clone()),
        points: solidcraft_engine::snap::snap_points(&app.session, &face),
        revision: app.session.revision,
        ..Default::default()
    };
    assert_eq!(woken_pick(&app, true, Sel::Vertex { body: "Body1".into(), point: Vec3::new(10.0, 10.0, 10.0) }), face);
}

/// A face of the rim's own body crossed on the way to its centre adds its centre to the rim's
/// points; that centre, picked, is the face, and the rim's middle is still the rim.
#[test]
fn a_crossed_faces_centre_is_the_face() {
    let mut app = two_boxes();
    app.start("joint.create");
    let edge = edge_sel(&app.session, Vec3::new(10.0, 0.0, 10.0)).unwrap();
    let face = face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let mut points = solidcraft_engine::snap::snap_points(&app.session, &edge);
    points.extend(solidcraft_engine::snap::snap_points(&app.session, &face));
    // `..Default::default()` keeps this compiling once #11810 adds `kept_over`.
    #[allow(clippy::needless_update)]
    let woken =
        crate::snap_wake::Woken { item: Some(edge.clone()), face: Some(face.clone()), points, revision: app.session.revision, ..Default::default() };
    app.viewport.woken = woken;
    assert_eq!(woken_pick(&app, true, Sel::Vertex { body: "Body1".into(), point: Vec3::new(10.0, 10.0, 10.0) }), face);
    assert_eq!(woken_pick(&app, true, Sel::Vertex { body: "Body1".into(), point: Vec3::new(10.0, 0.0, 10.0) }), edge);
}

#[test]
fn joint_dialog_applies_custom_name_or_falls_back_to_type_default() {
    // 1. Empty name field: commands() generates payload without "name", backend creates "Rigid Joint1"
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].0, "joint.create");
    assert!(cmds[0].1.get("name").is_none());
    for (id, p) in cmds {
        let r = app.run(&id, p).unwrap();
        assert_eq!(r["name"], "Rigid Joint1");
    }

    // 2. Custom name in f.name: reset to fresh boxes so B is at its initial coordinates
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.name = "MyCustomJoint".into();
    }
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["name"], "MyCustomJoint");
    for (id, p) in cmds {
        let r = app.run(&id, p).unwrap();
        assert_eq!(r["name"], "MyCustomJoint");
    }
}

/// An imported assembly makes a joint per placed part: with "Group joints in the History" on, from
/// the second joint the timeline shows them as one grouped button with their count, and clicking
/// it shows (and hides) each one.
/// Two parts, A and B, held by one rigid joint, "Held", with "Group joints in the History" on.
fn one_held_joint(h: &mut crate::scenario::Harness) {
    assert_eq!(h.call("ui.prefs", json!({"history_group_joints": true}))["ok"], true);
    for (name, x) in [("A", 0), ("B", 30)] {
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("component.create", json!({ "name": name })).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let at = |o: &str| json!({"occurrence": o, "point": [30, 10, 5], "z": [1, 0, 0]});
    h.app.run("joint.create", json!({"type": "rigid", "a": at("A"), "b": at("B"), "name": "Held"})).unwrap();
    h.frames(2);
}

/// Copies of the first joint, enough for the History to group them.
fn joints_enough_to_group(h: &mut crate::scenario::Harness) {
    let first = h.app.session.doc.assembly.joints[0].clone();
    for i in 1..=JOINT_GROUP_MIN as u64 {
        let mut copy = first.clone();
        copy.id = first.id + i;
        copy.name = format!("Held {i}");
        h.app.session.doc_mut().assembly.joints.push(copy);
    }
    h.frames(2);
}

#[test]
fn many_joints_start_grouped_in_the_timeline() {
    let mut h = crate::scenario::Harness::new();
    one_held_joint(&mut h);
    assert!(crate::scenario::handle_at("timeline:joints").is_none(), "one joint shows on its own");
    joints_enough_to_group(&mut h);
    assert_eq!(h.app.session.doc.assembly.joints.len(), 2);
    assert!(!joints_open(&h.app, JOINT_GROUP_MIN + 1, JointRun::Loose), "a second joint starts them grouped");
    let group = crate::scenario::handle_at("timeline:joints").expect("the grouped joints button");
    assert_eq!(h.call("ui.click", json!({"x": group.x, "y": group.y}))["ok"], true);
    assert!(joints_open(&h.app, JOINT_GROUP_MIN + 1, JointRun::Loose), "clicking the group shows each joint");
    let group = crate::scenario::handle_at("timeline:joints").expect("the grouped joints button");
    assert_eq!(h.call("ui.click", json!({"x": group.x, "y": group.y}))["ok"], true);
    assert!(!joints_open(&h.app, JOINT_GROUP_MIN + 1, JointRun::Loose), "clicking it again groups them");
}

/// A Browser search filters the Browser's rows, not the History's: grouped joints stay grouped
/// while it runs, and a click on the group opens them in the History's own fold, which is still
/// open once the search ends.
#[test]
fn grouped_joints_keep_their_fold_while_the_browser_searches() {
    let mut h = crate::scenario::Harness::new();
    one_held_joint(&mut h);
    joints_enough_to_group(&mut h);
    h.app.tree.search.show();
    h.app.tree.search.text = "Held".to_string();
    h.frames(2);
    assert!(h.app.tree.search.on());
    assert!(!joints_open(&h.app, JOINT_GROUP_MIN + 1, JointRun::Loose), "a search leaves the group as it was");
    let group = crate::scenario::handle_at("timeline:joints").expect("the grouped joints button");
    assert_eq!(h.call("ui.click", json!({"x": group.x, "y": group.y}))["ok"], true);
    assert!(joints_open(&h.app, JOINT_GROUP_MIN + 1, JointRun::Loose), "the click opens the History's own fold");
    h.app.tree.search.clear();
    h.frames(2);
    assert!(joints_open(&h.app, JOINT_GROUP_MIN + 1, JointRun::Loose), "and it is still open after the search");
}

/// With "Group joints in the History" off, as it is by default, the History shows each joint as it
/// always has: no grouped button, a tile for every joint, and the left History's JOINTS header
/// without a count. Turned on, the joints group; turned off again, they show one by one.
#[test]
fn joints_show_one_by_one_when_their_grouping_is_off() {
    let mut h = crate::scenario::Harness::new();
    for (name, x) in [("A", 0), ("B", 30)] {
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("component.create", json!({ "name": name })).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let at = |o: &str| json!({"occurrence": o, "point": [30, 10, 5], "z": [1, 0, 0]});
    for name in ["Held", "Held again"] {
        h.app.run("joint.create", json!({"type": "rigid", "a": at("A"), "b": at("B"), "name": name})).unwrap();
    }
    h.frames(2);
    assert!(!h.app.preferences.history_group_joints, "grouping is off by default");
    assert!(joints_open(&h.app, 2, JointRun::Loose), "each joint shows");
    assert!(crate::scenario::handle_at("timeline:joints").is_none(), "no grouped button");
    for name in ["Held", "Held again"] {
        assert!(crate::scenario::handle_at(&format!("timeline-joint:{name}")).is_some(), "{name} has its own tile");
    }
    assert_eq!(h.call("ui.prefs", json!({"history_group_joints": true}))["ok"], true);
    h.frames(2);
    assert!(!joints_open(&h.app, 2, JointRun::Loose), "turned on, the two joints group");
    assert!(crate::scenario::handle_at("timeline:joints").is_some(), "the grouped button appears");
    assert!(crate::scenario::handle_at("timeline-joint:Held").is_none());
    assert_eq!(h.call("ui.prefs", json!({"history_group_joints": false}))["ok"], true);
    h.frames(2);
    assert!(joints_open(&h.app, 2, JointRun::Loose), "turned off again, each joint shows");
    assert!(crate::scenario::handle_at("timeline:joints").is_none(), "and the grouped button goes");
    assert!(crate::scenario::handle_at("timeline-joint:Held").is_some());
}

/// The joints after the History bar's features belong to the whole design. Without the bar's
/// groups, as by default, they stay while the bar shows only the edited component, as they always
/// have; with the groups, a bar that shows one component leaves them out.
#[test]
fn joints_stay_in_the_history_bar_showing_one_component_without_its_groups() {
    let mut h = crate::scenario::Harness::new();
    for (name, x) in [("A", 0), ("B", 30)] {
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("component.create", json!({ "name": name })).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let at = |o: &str| json!({"occurrence": o, "point": [30, 10, 5], "z": [1, 0, 0]});
    h.app.run("joint.create", json!({"type": "rigid", "a": at("A"), "b": at("B"), "name": "Held"})).unwrap();
    h.app.run("component.activate", json!({"component": "A"})).unwrap();
    h.frames(2);
    assert!(!h.app.preferences.history_bar_groups, "the bar's groups are off by default");
    assert!(crate::timeline_scope::shown(&h.app).is_some(), "the bar shows the edited component");
    assert!(crate::scenario::handle_at("timeline-joint:Held").is_some(), "the design's joint still shows");
    assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
    h.frames(2);
    assert!(crate::scenario::handle_at("timeline-joint:Held").is_none(), "with the groups, one component's bar leaves it out");
}

/// The grouped joints' arrow has a well of its own at the button's end: it lights up only under
/// the pointer, and clicking it shows and hides the joints like the rest of the button.
#[test]
fn the_grouped_joints_arrow_lights_up_under_the_pointer() {
    let mut h = crate::scenario::Harness::new();
    assert_eq!(h.call("ui.prefs", json!({"history_group_joints": true}))["ok"], true);
    for (name, x) in [("A", 0), ("B", 30)] {
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("component.create", json!({ "name": name })).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let at = |o: &str| json!({"occurrence": o, "point": [30, 10, 5], "z": [1, 0, 0]});
    h.app.run("joint.create", json!({"type": "rigid", "a": at("A"), "b": at("B"), "name": "Held"})).unwrap();
    let first = h.app.session.doc.assembly.joints[0].clone();
    // Thousands of joints, as an imported assembly makes: the count is four digits wide.
    for i in 1..=1200u64 {
        let mut copy = first.clone();
        copy.id = first.id + i;
        copy.name = format!("Held {i}");
        h.app.session.doc_mut().assembly.joints.push(copy);
    }
    h.frames(2);
    let arrow = crate::scenario::rect_of("timeline:joints_arrow").expect("the arrow's well");
    let group = crate::scenario::handle_at("timeline:joints").expect("the grouped joints button");
    assert!(arrow.left() > group.x, "the arrow sits at the button's right end");
    let hot = |h: &mut crate::scenario::Harness, x: f32, y: f32| {
        h.call("ui.move", json!({"x": x, "y": y}));
        h.frames(1);
        crate::scenario::count_of("timeline:joints_arrow_hot")
    };
    assert_eq!(hot(&mut h, arrow.center().x, arrow.center().y), Some(1.0), "the well lights up under the pointer");
    let icon_x = arrow.left() - 2.0 * (arrow.left() - group.x) + 6.0;
    assert_eq!(hot(&mut h, icon_x, arrow.center().y), Some(0.0), "not over the icon or the count");
    assert_eq!(h.call("ui.click", json!({"x": arrow.center().x, "y": arrow.center().y}))["ok"], true);
    assert!(joints_open(&h.app, 1201, JointRun::Loose), "clicking the arrow shows each joint");
    let arrow = crate::scenario::rect_of("timeline:joints_arrow").expect("the arrow's well");
    assert_eq!(h.call("ui.click", json!({"x": arrow.center().x, "y": arrow.center().y}))["ok"], true);
    assert!(!joints_open(&h.app, 1201, JointRun::Loose), "and groups them again");
}

/// Box A with a Ø4 hole through it along y at x = 6, z = 5 (off the front face's centre), the Joint dialog picking Component 1,
/// and A's front face (y = 0, its normal -y) hovered, so its points are woken (#11476).
fn joint_over_a_front_face() -> (SolveApp, String) {
    let mut app = two_boxes();
    app.run("component.activate", json!({"component": "A"})).unwrap();
    app.run("solid.cylinder", json!({"radius": 2, "height": 30, "base": [6, -5, 5], "axis": [0, 1, 0], "operation": "cut"})).unwrap();
    app.run("component.activate", json!({"component": "root"})).unwrap();
    app.start("joint.create");
    let front = face_sel(&app.session, Vec3::new(3.0, 0.0, 8.0)).unwrap();
    let Sel::Face { body, .. } = &front else { panic!("the front is a face pick") };
    let body = body.clone();
    let points = solidcraft_engine::snap::snap_points(&app.session, &front);
    app.viewport.woken = crate::snap_wake::Woken { item: Some(front), points, revision: app.session.revision, ..Default::default() };
    (app, body)
}

fn v3(x: &serde_json::Value) -> Vec3 {
    Vec3::new(x[0].as_f64().unwrap(), x[1].as_f64().unwrap(), x[2].as_f64().unwrap())
}

/// A corner of the hovered face, picked into a joint input, lies on the face (#11476): its z is
/// the face's outward normal, not the part's +Z, and the face, edge and corner are kept by name.
#[test]
fn a_faces_corner_picked_from_the_face_lies_on_it() {
    let (mut app, body) = joint_over_a_front_face();
    let at = Vec3::new(20.0, 0.0, 0.0);
    let corner = Sel::Vertex { body: body.clone(), point: at };
    crate::viewport::dialog_pick(&mut app, &crate::viewport::Hit::Vertex { body, point: at }, corner.clone());
    let mut d = app.dialog.clone().unwrap();
    assert_eq!(d.inputs[0].items, std::slice::from_ref(&corner));
    let snap = snap_in(&app, &d.inputs[0], &corner).unwrap();
    assert!(snap.z.dist(-Vec3::Y) < 1e-9, "{snap:?}");
    assert!(snap.param.get("z").is_none() && snap.param["on"].is_array() && v3(&snap.param["corner"]).dist(at) < 1e-9, "{}", snap.param);
    // B's bottom face as Component 2; OK makes a joint whose A side sits on the corner, z out of the front face.
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    let j = app.session.execute("joint.inspect", &json!({})).unwrap();
    let a = &j["joints"][0]["a"];
    assert_eq!(a["resolves"], true, "{j}");
    assert!(v3(&a["frame"]["origin"]).dist(at) < 1e-6 && v3(&a["frame"]["z"]).dist(-Vec3::Y) < 1e-9, "{j}");
}

/// A hole's centre offered by the face it opens onto is a `circle` origin with the face's
/// normal, also for the hover glyph, which is drawn while the overlay holds the dialog apart from
/// the app (#11476); a point the face doesn't offer, or a face not woken, stays as before.
#[test]
fn a_holes_centre_from_its_face_keeps_the_faces_normal() {
    let (mut app, body) = joint_over_a_front_face();
    let centre = Sel::Vertex { body: body.clone(), point: Vec3::new(6.0, 0.0, 5.0) };
    let d = app.dialog.take();
    let hovered = snap_hovered(&app, &centre).unwrap();
    app.dialog = d;
    assert_eq!(hovered.kind, SnapKind::CircleCenter);
    assert!(hovered.z.dist(-Vec3::Y) < 1e-9 && v3(&hovered.param["circle"]).dist(Vec3::new(6.0, 0.0, 5.0)) < 1e-9, "{hovered:?}");

    let elsewhere = Sel::Vertex { body, point: Vec3::new(14.0, 0.0, 5.0) };
    assert!(face_point_source(&app, true, &elsewhere).is_none(), "not one of the face's points");
    assert!(face_point_source(&app, false, &centre).is_none(), "an input that takes no joint origins");
    app.viewport.woken = crate::snap_wake::Woken::default();
    assert!(face_point_source(&app, true, &centre).is_none(), "no face woken");
}

/// An edge of the woken face, hovered or picked in a joint input, is the face's point on it: the
/// edge's middle, with the face's normal as z rather than the edge's direction (#11476). With no
/// face woken an edge keeps its own direction, so a revolute joint on it turns about it (#5457).
#[test]
fn a_woken_faces_edge_takes_the_faces_normal() {
    let (mut app, body) = joint_over_a_front_face();
    let edge = edge_sel(&app.session, Vec3::new(20.0, 0.0, 7.0)).unwrap();
    let picked = woken_pick(&app, true, edge.clone());
    assert_eq!(picked, Sel::Vertex { body, point: Vec3::new(20.0, 0.0, 5.0) });
    let d = app.dialog.take();
    let hovered = snap_hovered(&app, &picked).unwrap();
    app.dialog = d;
    assert_eq!(hovered.kind, SnapKind::EdgeMid);
    assert!(hovered.z.dist(-Vec3::Y) < 1e-9, "{hovered:?}");
    assert_eq!(woken_pick(&app, false, edge.clone()), edge, "an input that takes no joint origins");

    app.viewport.woken = crate::snap_wake::Woken::default();
    assert_eq!(woken_pick(&app, true, edge.clone()), edge);
    let own = snap_of(&app.session, &edge).unwrap();
    assert!(own.z.cross(Vec3::Z).len() < 1e-9, "along the edge: {own:?}");
}

/// In a joint origin input an edge under the pointer wakes the points of the face beside it that
/// the pointer is over, and that face is lit while the pointer is on the edge or one of its points
/// (#11476): the face a pick there lies on.
#[test]
fn an_edge_in_a_joint_input_wakes_and_lights_the_face_beside_it() {
    let mut app = two_boxes();
    app.start("joint.create");
    let Some(Sel::Face { body, index, point }) = face_sel(&app.session, Vec3::new(10.0, 0.0, 5.0)) else { panic!("A's front face") };
    let Some(Sel::Edge { index: edge, point: on_edge, .. }) = edge_sel(&app.session, Vec3::new(20.0, 0.0, 7.0)) else { panic!("its right edge") };
    let edge_hit = crate::viewport::Hit::Edge { body: body.clone(), index: edge, mid: Vec3::new(20.0, 0.0, 5.0), point: on_edge };
    let face_hit = crate::viewport::Hit::Face { body: body.clone(), index, point };
    app.viewport.hover = Some(edge_hit.clone());
    crate::snap_wake::update(&mut app, &[edge_hit, face_hit], false, None);
    assert!(matches!(&app.viewport.woken.item, Some(Sel::Face { index: i, .. }) if *i == index), "{:?}", app.viewport.woken.item);
    let corner = Vec3::new(20.0, 0.0, 0.0);
    assert!(app.viewport.woken.points.iter().any(|p| p.at.dist(corner) < 1e-9), "the face's corners: {:?}", app.viewport.woken.points);
    assert!(matches!(crate::snap_wake::lit_face(&app), Some(crate::viewport::Hit::Face { index: i, .. }) if i == index));
    app.viewport.hover = Some(crate::viewport::Hit::Vertex { body, point: corner });
    assert!(crate::snap_wake::lit_face(&app).is_some(), "lit while on its corner");
}

/// A pick on a part in a sub-assembly placed twice sends the placement it was made on: on the
/// second copy the origin is on that copy, inside the part; on the first it is stored as before
/// (#11426).
#[test]
fn joint_origin_picked_on_a_repeated_sub_assembly_is_on_that_copy() {
    let mut s = Session::default();
    for (id, p) in [
        ("component.create", json!({"name": "Board", "parent": "root"})),
        ("component.create", json!({"name": "Header", "parent": "Board"})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Pin"})),
        ("component.activate", json!({"component": "root"})),
        // Header turned and moved in Board: its top's centre at (-5, 25, 10).
        ("occurrence.move", json!({"occurrence": "Header:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]})),
        ("occurrence.move", json!({"occurrence": "Header:1", "translate": [0, 20, 0]})),
        ("occurrence.copy", json!({"component": "Board", "translate": [100, 0, 0]})),
    ] {
        s.execute(id, &p).unwrap();
    }
    let mut app = SolveApp::new(s, Services::default());
    let board = app.session.doc.find_component("Board").unwrap();
    let boards: Vec<u64> = app.session.doc.occurrences.iter().filter(|o| o.component == board).map(|o| o.id).collect();
    let header = occ(&app, "Header");
    for (x, board, body, stored) in
        [(95.0, boards[1], "Pin (Board:2/Header:1)", (boards[1], vec![header])), (-5.0, boards[0], "Pin", (header, vec![]))]
    {
        app.start("joint.origin");
        let mut d = app.dialog.clone().unwrap();
        let top = face_sel(&app.session, Vec3::new(x, 25.0, 10.0)).unwrap();
        assert!(matches!(&top, Sel::Face { body: b, .. } if b == body), "{top:?}");
        d.pick(&app.session, top);
        let commands = apply_commands(&app, &d).unwrap();
        assert_eq!(commands[0].1["path"], json!([board, header]), "{commands:?}");
        for (id, p) in commands {
            app.run(&id, p).unwrap();
        }
        let o = &app.session.doc.assembly.origins.last().unwrap().origin;
        assert_eq!((o.occurrence, o.inside.clone()), stored, "{body}: {o:?}");
    }
    // The same part in two copies is two sides of a joint, not "one component".
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for x in [-5.0, 95.0] {
        d.pick(&app.session, face_sel(&app.session, Vec3::new(x, 25.0, 10.0)).unwrap());
    }
    let Kind::Assembly(k) = &d.kind else { panic!("{:?}", d.kind) };
    assert_eq!(joint_problem(&app, k, &d.inputs), None);
    // Edit Joint shows a side on the second copy on that copy's part.
    app.run("component.create", json!({"name": "Probe", "parent": "root", "grounded": false})).unwrap();
    app.run("solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [50, -30, 0]})).unwrap();
    app.run("component.activate", json!({"component": "root"})).unwrap();
    let on_copy = app.session.doc.assembly.origins[0].name.clone();
    app.run("joint.create", json!({"type": "rigid", "a": on_copy, "b": {"occurrence": "Probe", "face": [51, -29, 0]}})).unwrap();
    let d = edit_joint(&app, app.session.doc.assembly.joints[0].id).unwrap();
    let Some(Sel::Face { body, .. }) = d.inputs[0].items.first() else { panic!("no side pick: {:?}", d.inputs[0].items) };
    assert_eq!(body, "Pin (Board:2/Header:1)");
}

/// The Joint and Edit Joint dialogs take origin planes and axes as joint sides, keep them on
/// edit, refuse sketch curves as sides, and leave Measure's sketch-curve picks alone.
#[test]
fn joint_dialog_picks_origin_plane_and_axis() {
    let mut app = two_boxes();

    // 1. Pick origin plane XY in the Joint dialog.
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    let plane_sel = Sel::Plane { name: "XY".into() };
    let snap_plane = snap_of(&app.session, &plane_sel).expect("snap for XY plane");
    assert_eq!(snap_plane.kind, SnapKind::FaceCenter);
    assert_eq!(snap_plane.z, Vec3::Z);
    d.pick(&app.session, plane_sel);
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let cmds = apply_commands(&app, &d).expect("joint between plane and component B");
    assert_eq!(cmds[0].0, "joint.create");
    assert_eq!(cmds[0].1["a"]["plane"], "XY");
    assert_eq!(cmds[0].1["b"]["occurrence"], occ(&app, "B"));
    for (cmd, params) in cmds {
        app.run(&cmd, params).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.len(), 1);

    // 2. Pick origin axis Z in the Joint dialog, on a fresh scene: the first joint carried B
    //    onto the XY plane (a solve captures the pose into B's occurrence), so B no longer
    //    sits where it was placed and the old world face pick would find nothing.
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    let axis_sel = Sel::Axis { name: "Z".into() };
    let snap_axis = snap_of(&app.session, &axis_sel).expect("snap for Z axis");
    assert_eq!(snap_axis.kind, SnapKind::EdgeMid);
    assert_eq!(snap_axis.z, Vec3::Z);
    d.pick(&app.session, axis_sel);
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let cmds = apply_commands(&app, &d).expect("joint between axis and component B");
    assert_eq!(cmds[0].0, "joint.create");
    assert_eq!(cmds[0].1["a"]["axis"], "Z");
    assert_eq!(cmds[0].1["b"]["occurrence"], occ(&app, "B"));
    for (cmd, params) in cmds {
        app.run(&cmd, params).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.len(), 1);
}

#[test]
fn edit_joint_preserves_plane_and_axis_sides() {
    let mut app = two_boxes();

    // 1. Create a joint using a construction plane and verify Edit Joint pre-fills Sel::Plane.
    let wp = app.run("construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "WorkPlane"})).unwrap();
    let feat_id = wp["feature"].as_u64().unwrap();
    let j_plane = app
        .run(
            "joint.create",
            json!({
                "type": "planar",
                "a": {"plane": {"type": "construction", "feature": feat_id}},
                "b": {"occurrence": occ(&app, "B"), "face": [40, 10, 0]}
            }),
        )
        .unwrap();
    let plane_joint_id = j_plane["joint"].as_u64().unwrap();
    let edit_plane = edit_joint(&app, plane_joint_id).unwrap();
    assert_eq!(edit_plane.inputs[0].items, vec![Sel::Plane { name: "WorkPlane".into() }]);
    let cmds = apply_commands(&app, &edit_plane).unwrap();
    assert!(cmds[0].1.get("a").is_none() && cmds[0].1.get("b").is_none(), "unchanged sides are not re-sent");
    app.run("joint.delete", json!({"joint": plane_joint_id})).unwrap();

    // 2. Create a joint using a construction axis and verify Edit Joint pre-fills Sel::Axis.
    app.run(
        "construct.axis.two_points",
        json!({
            "a": {"point": [0, 0, 0]},
            "b": {"point": [0, 0, 50]},
            "name": "WorkAxis"
        }),
    )
    .unwrap();
    let j_axis = app
        .run(
            "joint.create",
            json!({
                "type": "revolute",
                "a": {"axis": "WorkAxis"},
                "b": {"occurrence": occ(&app, "B"), "face": [40, 10, 0]}
            }),
        )
        .unwrap();
    let axis_joint_id = j_axis["joint"].as_u64().unwrap();
    let edit_axis = edit_joint(&app, axis_joint_id).unwrap();
    assert_eq!(edit_axis.inputs[0].items, vec![Sel::Axis { name: "WorkAxis".into() }]);
    let cmds = apply_commands(&app, &edit_axis).unwrap();
    assert!(cmds[0].1.get("a").is_none() && cmds[0].1.get("b").is_none(), "unchanged sides are not re-sent");
}

#[test]
fn joint_sides_refuse_sketch_curves() {
    let mut app = two_boxes();
    app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();

    app.start("joint.create");
    let mut d = app.dialog.clone().unwrap();
    assert_eq!(d.inputs[0].accept, crate::dialogs_assembly::SNAPS);

    let sketch_sel = Sel::SketchCurve { id: "l1".into(), sketch: None };
    d.pick(&app.session, sketch_sel);
    assert!(d.inputs[0].items.is_empty(), "straight sketch curve must be rejected by SNAPS input");
}

/// Measure's items take more than the joint-origin kinds (sketch curves among them), so the
/// joint-origin rejection must leave them alone: a sketch curve stays pickable in Measure.
#[test]
fn measure_still_takes_sketch_curve_picks() {
    let mut app = two_boxes();
    app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();

    app.start("inspect.measure");
    let mut d = app.dialog.clone().unwrap();
    assert_ne!(d.inputs[0].accept, crate::dialogs_assembly::SNAPS);
    assert!(!d.inputs[0].rejects(&Sel::SketchCurve { id: "l1".into(), sketch: None }));

    let sketch_sel = Sel::SketchCurve { id: "l1".into(), sketch: None };
    d.pick(&app.session, sketch_sel.clone());
    assert_eq!(d.inputs[0].items, vec![sketch_sel], "Measure takes a sketch curve");
}

/// Constraint rules keep their existing controls until explicitly converted to a plain joint.
#[test]
fn lateral_offset_controls_do_not_change_assembly_constraint_rules() {
    let mut harness = crate::scenario::Harness::new();
    harness.app = two_boxes();
    harness.app.home.open = false;
    harness
        .app
        .run(
            "assembly.constraint.create",
            json!({"type":"concentric",
        "a":{"occurrence":"A","axis":"Z"},"b":{"occurrence":"B","axis":"Z"}}),
        )
        .unwrap();
    let joint = harness.app.session.doc.assembly.joints[0].id;
    harness.app.dialog = edit_joint(&harness.app, joint);
    harness.settle();
    harness.step(&json!({"click":{"handle":"dialog:section:Position"}})).unwrap();
    harness.frames(4);
    assert!(crate::scenario::handle_at("dialog:Offset Z").is_some());
    for name in ["Offset X", "Offset Y"] {
        assert!(crate::scenario::handle_at(&format!("dialog:{name}")).is_none());
    }
    let Kind::Assembly(Asm::Joint(form)) = &mut harness.app.dialog.as_mut().unwrap().kind else { panic!("joint form") };
    let original_kind = form.kind;
    form.kind = 0;
    form.offset[0] = "missing_parameter".into();
    harness.settle();
    for name in ["Offset X", "Offset Y"] {
        assert!(crate::scenario::handle_at(&format!("dialog:{name}")).is_some());
    }
    let Kind::Assembly(Asm::Joint(form)) = &mut harness.app.dialog.as_mut().unwrap().kind else { panic!("joint form") };
    form.kind = original_kind;
    harness.settle();
    let commands = apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap();
    assert!(commands[0].1.get("offset_x").is_none() && commands[0].1.get("offset_y").is_none());
    harness.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    harness.settle();
    assert!(harness.app.dialog.is_none(), "hidden lateral text must not block OK");
    assert!(harness.app.session.doc.assembly.joints[0].constraint.is_some());
    harness.app.dialog = edit_joint(&harness.app, joint);
    let Kind::Assembly(Asm::Joint(form)) = &mut harness.app.dialog.as_mut().unwrap().kind else { panic!("joint form") };
    form.kind = 0;
    form.offset[0] = "12 mm".into();
    harness.settle();
    harness.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    harness.settle();
    assert_eq!(harness.app.session.doc.assembly.joints[0].constraint, None);
    assert_eq!(harness.app.session.doc.assembly.joints[0].offset_xy, [12.0, 0.0]);
}

/// A joint origin clicked inside a face, a few points from its edge, is the face: an edge is taken
/// only within `JOINT_EDGE_REACH` of the pointer, not the 6 points other picks use (#18255).
#[test]
fn a_joint_origin_a_few_points_inside_a_face_is_the_face() {
    let pick_at = |dy: f64| {
        let mut h = joint_picking_component_1();
        let at = h.call("ui.at", json!({ "world": [15, 0, 10] }))["result"].clone();
        let p = json!([at[0].as_f64().unwrap(), at[1].as_f64().unwrap() + dy]);
        h.step(&json!({ "move": p })).unwrap();
        h.step(&json!({ "click": p })).unwrap();
        h.app.dialog.as_ref().unwrap().inputs[0].items.clone()
    };
    let near = pick_at(1.5);
    assert!(matches!(near.as_slice(), [Sel::Edge { .. } | Sel::Vertex { .. }]), "right on the edge: {near:?}");
    let inside = pick_at(4.5);
    assert!(matches!(inside.as_slice(), [Sel::Face { body, .. }] if body == "Body1"), "inside the face: {inside:?}");
}

#[test]
fn secondary_alignment_review_primary_hover_uses_the_displayed_pose_once() {
    let mut app = round_plates();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(50.0, 40.0, 10.0), Vec3::new(200.0, 40.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.angle = "27 deg".into();
        f.flip = true;
    }
    app.dialog = Some(d);
    settle_alignment_update(&mut app, &egui::Context::default());
    let shown = solidcraft_engine::doc::mat_inverse(&app.preview.interaction.as_ref().unwrap().to_document["Body2"]).unwrap();
    let point = solidcraft_engine::doc::apply_point(&shown, Vec3::new(150.0, 0.0, 0.0));
    let vertex = Sel::Vertex { body: "Body2".into(), point };
    let snap = snap_hovered(&app, &vertex).unwrap();
    assert!(snap.at.dist(point) < 1e-6, "a raw hovered vertex is already in the shown pose: {:?} != {:?}", snap.at, point);
    let expected_z = solidcraft_engine::doc::apply_vector(&shown, Vec3::Z);
    assert!(snap.z.dist(expected_z) < 1e-6);
    // Both curved and flat faces must resolve in the same pose as the pointer.
    for original in [Vec3::new(173.0, 20.0, 5.0), Vec3::new(200.0, 40.0, 10.0)] {
        let authored = face_sel(&app.session, original).unwrap();
        let expected = snap_of(&app.session, &authored).unwrap();
        let Sel::Face { body, index, point } = authored else { panic!() };
        let hovered = Sel::Face { body, index, point: solidcraft_engine::doc::apply_point(&shown, point) };
        let actual = snap_hovered(&app, &hovered).expect("the hovered face retains its marker");
        assert!(actual.at.dist(solidcraft_engine::doc::apply_point(&shown, expected.at)) < 1e-6);
        assert!(actual.z.dist(solidcraft_engine::doc::apply_vector(&shown, expected.z)) < 1e-6);
    }
}

fn review_alignment_marker(app: &mut SolveApp, hit: crate::viewport::Hit) -> Option<SnapPt> {
    app.viewport.hover = Some(hit);
    let mut d = app.dialog.take().unwrap();
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(Default::default(), |ui| {
        let painter = ui.ctx().layer_painter(egui::LayerId::background());
        let proj = crate::viewport::Proj {
            cam: app.cam,
            vp: app.cam.view_proj(1.0, 100.0),
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 720.0)),
            bounds: None,
        };
        overlay(app, &painter, &proj, &mut d);
    });
    let result = match &d.kind {
        Kind::Assembly(Asm::Joint(f)) => f.hover.as_ref().and_then(|(_, s)| s.clone()),
        _ => None,
    };
    app.dialog = Some(d);
    result
}

#[test]
fn secondary_alignment_review_helper_markers_follow_their_occurrence_without_a_body_map() {
    let mut app = two_boxes();
    app.run("component.activate", json!({"component":"B"})).unwrap();
    app.run("construct.axis.two_points", json!({"a":{"point":[40,10,0]},"b":{"point":[40,20,0]},"name":"MovingAxis"})).unwrap();
    app.run("construct.plane.three_points", json!({"a":{"point":[40,10,0]},"b":{"point":[40,20,0]},"c":{"point":[40,10,5]},"name":"MovingPlane"}))
        .unwrap();
    app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = app.session.active_sketch.unwrap();
    app.run("sketch.line", json!({"points":[[40,10],[40,20]]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.run("component.activate", json!({"component":"root"})).unwrap();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    for p in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(40.0, 10.0, 0.0)] {
        d.pick(&app.session, face_sel(&app.session, p).unwrap());
    }
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.angle = "27 deg".into();
        f.flip = true;
    }
    d.active = 2;
    app.dialog = Some(d);
    settle_alignment_update(&mut app, &egui::Context::default());
    let shown = solidcraft_engine::doc::mat_inverse(&app.preview.interaction.as_ref().unwrap().to_document["Body2"]).unwrap();
    app.preview.interaction.as_mut().unwrap().to_document.clear();
    let before = app.session.doc.clone();
    for (hit, at, z) in [
        (crate::viewport::Hit::Axis { name: "MovingAxis".into() }, Vec3::new(40.0, 10.0, 0.0), Vec3::Y),
        (crate::viewport::Hit::Plane { name: "MovingPlane".into(), point: Vec3::ZERO }, Vec3::new(40.0, 10.0, 0.0), Vec3::X),
        (crate::viewport::Hit::SketchCurve { sketch, id: "l1".into(), straight: true, point: Vec3::ZERO }, Vec3::new(40.0, 15.0, 0.0), Vec3::Y),
    ] {
        let marker = review_alignment_marker(&mut app, hit).expect("a valid helper has an alignment marker");
        let expected = solidcraft_engine::doc::apply_point(&shown, at);
        assert!(marker.at.dist(expected) < 1e-6, "helper feedback must follow its occurrence: {:?} != {:?}", marker.at, expected);
        assert!(marker.z.dist(solidcraft_engine::doc::apply_vector(&shown, z)) < 1e-6);
    }
    assert_eq!(app.session.doc, before);
}
