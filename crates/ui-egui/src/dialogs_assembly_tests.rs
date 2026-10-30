use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, edge_sel, face_sel, preview_commands};

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
    let st = s.world_state();
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
    let before = h.app.session.doc.to_json();
    h.call("ui.prefs", json!({"open":"Design"}));
    let checkbox = crate::scenario::rect_of("prefs:joint_auto_alignment").unwrap().center();
    h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_auto_alignment"], false);
    let mut restored = SolveApp::new(Session::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(serde_json::to_value(&restored.preferences).unwrap()["joint_auto_alignment"], false);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_auto_alignment"], true);
    h.call("ui.prefs", json!({"open":"Design","joint_auto_alignment":false}));
    h.call("ui.prefs", json!({"button":"apply"}));
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
    let (hit, fixed) = rim(&app.session, "Body1", Vec3::new(20.0, 60.0, 10.0));
    let ctx = egui::Context::default();
    app.dialog = Some(d.clone());
    settle_alignment_update(&mut app, &ctx);
    let confirmed_pose = app.preview.interaction.as_ref().unwrap().to_document["Body2"];
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
    h.call("ui.prefs", json!({"open":"Design"}));
    let checkbox = crate::scenario::rect_of("prefs:joint_moving_first").unwrap().center();
    h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_moving_first"], true);
    let mut restored = SolveApp::new(Session::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(serde_json::to_value(&restored.preferences).unwrap()["joint_moving_first"], true);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap()["joint_moving_first"], false);
    h.call("ui.prefs", json!({"open":"Design","joint_moving_first":true}));
    h.call("ui.prefs", json!({"button":"apply"}));
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
        assert!(commands[0].1[if reverse { "a" } else { "b" }]["circle"].is_string());
    }
}

#[test]
fn secondary_alignment_nonround_primary_edge_disables_alignment_before_secondary_picking() {
    let mut app = two_boxes();
    app.start("joint.create");
    let mut d = app.dialog.take().unwrap();
    let st = app.session.world_state();
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
        assert_eq!(alignment_side(&edit.kind, 0), Some(expected));
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
                .and_then(|c| world_preview(&app.session, &c, crate::preview::Colors::from_tokens(&crate::theme::Tokens::get())))
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
    assert_eq!(alignment_side(&edit.kind, 0), Some(1));
    assert_eq!(stored_alignment(&edit.kind, 0).unwrap()["reference"]["type"], "circle_center");
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
    assert_eq!(stored_alignment(&edit.kind, 1).unwrap()["reference"]["type"], "cylinder");
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
    let state = app.session.model.state();
    let body = state.body("Body2").unwrap();
    let mesh = body.mesh();
    let index = mesh.edges.iter().position(|e| e.len() >= 2 && (e.last().unwrap().x - e[0].x).abs() > 19.0).unwrap();
    let point = (mesh.edges[index][0] + *mesh.edges[index].last().unwrap()) / 2.0;
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
    let (_, _, quad) = crate::viewport::construction_quads(&app).into_iter().find(|(name, ..)| name == "MovingPlane").unwrap();
    let center = quad.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) / 4.0;
    assert!(center.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-6, "{center:?}");
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
    assert_eq!(edit.inputs.len(), 2);
    assert_eq!(stored_alignment(&edit.kind, 1).unwrap()["face"], apply_commands(&app, &d).unwrap()[0].1["a_alignment"]["face"]);
    let params = apply_commands(&app, &edit).unwrap()[0].1.clone();
    assert_eq!(params["angle"], "jointTurn");
    assert_eq!(params["a_alignment"]["reverse"], true);
    clear_alignment(&mut edit.kind, 1);
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
    h.step(&json!({"click":{"handle":"dialog:clear:Alignment 1 (optional)"}})).unwrap();
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
    // Offset X: an explicit frame 5 mm along the face.
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.offset[0] = "5 mm".into();
    }
    let p = apply_commands(&app, &d).unwrap()[0].1.clone();
    let o = &p["a"]["frame"]["origin"];
    let at = Vec3::new(o[0].as_f64().unwrap(), o[1].as_f64().unwrap(), o[2].as_f64().unwrap());
    assert!((at.dist(Vec3::new(10.0, 10.0, 10.0)) - 5.0).abs() < 1e-9, "{p}");
    // OK makes it.
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.len(), 1);
}

/// A straight edge is the revolute axis, even when both adjacent faces are perpendicular to it.
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
        edge: [0, 0, 0, 255],
        ghost: true,
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
    assert_eq!(d.inputs.iter().map(|i| i.label).collect::<Vec<_>>(), ["Alignment 1 (optional)", "Alignment 2 (optional)"]);
    assert!(d.inputs.iter().all(|i| i.accept == ALIGNMENTS), "editing never asks to replace the primary origins");
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
    let st = app.session.world_state();
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
    let why = crate::i18n::format("joint.one_component", &[("bodies", "Body1, Body2")]);
    assert!(text_at(&out, &why).is_some(), "the dialog says why");
    let at = text_at(&out, crate::i18n::tr("cmd.component_from_bodies")).expect("and offers the way out");
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
    let st = app.session.world_state();
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
    let b = app.session.world_state().body("Body2").unwrap().mesh().bounds();
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
    let editor = egui::Id::new("sc_drive_value");
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
        edge: [0, 0, 0, 255],
        ghost: true,
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
        edge: [0, 0, 0, 255],
        ghost: true,
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
    let preview = app.session.scratch().execute("motion.trace", p).unwrap();
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
    assert!((at.dist(Vec3::new(10.0, 10.0, 10.0)) - 6.35).abs() < 1e-9);
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
    assert_eq!((f.limits[0].1.as_str(), f.limits[0].2.as_str()), ("0 in", "0.5 in"));
}
