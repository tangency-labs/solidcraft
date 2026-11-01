use crate::test_support::run_all;
use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{Dialog, apply_commands, for_feature as edit_dialog};

fn app_with(cmds: &[(&str, Value)]) -> SolveApp {
    let mut s = Session::default();
    for (id, p) in cmds {
        s.execute(id, p).unwrap();
    }
    SolveApp::new(s, Services::default())
}

/// A 600 × 400 rectangle on XY as the frame sketch `Frame`, and, drawn after it on YZ, a ring of
/// two circles as the section sketch `Ring` (its circles' ids are not the frame's line ids).
fn frame_app() -> SolveApp {
    app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "YZ", "name": "Ring"})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 20})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 17})),
        ("sketch.finish", json!({})),
    ])
}

fn sketch_id(app: &SolveApp, name: &str) -> u64 {
    app.session.doc.find_feature(name).unwrap().id
}

fn frame_lines(app: &SolveApp) -> Vec<String> {
    app.session.doc.sketch(sketch_id(app, "Frame")).unwrap().curves.iter().map(|c| c.id.clone()).collect()
}

fn body_names(app: &SolveApp) -> Vec<String> {
    app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect()
}

/// The dialog started with the given lines picked.
fn dialog_on(app: &mut SolveApp, lines: &[String]) -> Dialog {
    app.session.selection = lines.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect();
    app.start("frame.member");
    app.dialog.clone().unwrap()
}

fn member(d: &mut Dialog) -> &mut Member {
    match &mut d.kind {
        Kind::Frame(m) => m,
        k => panic!("{k:?}"),
    }
}

/// Picked lines and the default built-in profile make one member a line; Edit Feature refills
/// the dialog and runs `timeline.redefine`, and the members keep their names.
#[test]
fn members_from_picked_lines_and_a_built_in_profile() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines[..2]);
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].0, "frame.member");
    let p = &c[0].1;
    assert_eq!(p["sketch"], json!(sketch_id(&app, "Frame")), "the frame sketch, not the newer section sketch: {p}");
    assert_eq!(p["lines"], json!(lines[..2]));
    assert_eq!(p["profile"], json!({"kind": "rect_tube", "width": "40 mm", "height": "40 mm", "wall": "3 mm"}));
    assert!(p.get("anchor").is_none() && p.get("angle").is_none() && p.get("mirror").is_none() && p.get("body_name").is_none(), "{p}");
    // Another built-in profile and its dimensions; an optional one left empty takes its default.
    let m = member(&mut d);
    m.kind = frame::profile_kinds().iter().position(|k| k.kind == "angle").unwrap();
    m.dims[m.kind][2] = "4 mm".into();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["profile"], json!({"kind": "angle", "width": "50 mm", "height": "50 mm", "thickness": "4 mm"}));
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Angle 50x50x4", "Angle 50x50x4 (2)"]);
    app.run("body.rename", json!({"body": "Angle 50x50x4 (2)", "name": "Rail"})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert_eq!(e.inputs[0].items.len(), 2);
    let m = member(&mut e);
    assert_eq!((frame::profile_kinds()[m.kind].kind, m.dims[m.kind][2].as_str()), ("angle", "4 mm"));
    m.dims[m.kind][2] = "5 mm".into();
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Angle 50x50x4", "Rail"], "the members keep their names through the edit");
}

/// A section picked in another sketch is the profile, the picks naming its regions; All Lines,
/// anchor, angle, mirror and name reach the command and come back when editing.
#[test]
fn a_picked_section_and_the_placement() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let ring = sketch_id(&app, "Ring");
    let mut d = dialog_on(&mut app, &lines[..1]);
    // The ring between the circles: the region whose interior point lies between radii 17 and 20.
    let ss = app.session.world_state().sketch(ring).unwrap().clone();
    let index = ss.profiles.iter().position(|p| (p.region.interior_point().len() - 18.5).abs() < 1.5).unwrap();
    d.inputs[1].items = vec![Sel::Profile { sketch: ring, index }];
    let m = member(&mut d);
    m.all = true;
    m.anchor = ANCHORS.iter().position(|a| a.0 == "bottom").unwrap();
    m.angle = "15 deg".into();
    m.mirror = true;
    m.name = " Rail ".into();
    let c = apply_commands(&app, &d).unwrap();
    let p = &c[0].1;
    assert_eq!(p["sketch"], json!(sketch_id(&app, "Frame")));
    assert!(p.get("lines").is_none() && p.get("profile").is_none(), "{p}");
    assert_eq!((&p["profile_sketch"], &p["profiles"]), (&json!(ring), &json!([index])));
    assert_eq!((&p["anchor"], &p["angle"], &p["mirror"], &p["body_name"]), (&json!("bottom"), &json!("15 deg"), &json!(true), &json!("Rail")));
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Rail", "Rail (2)", "Rail (3)", "Rail (4)"]);
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert_eq!(e.inputs[0].items.len(), 4, "every line of the sketch shows as picked");
    assert_eq!(e.inputs[1].items, [Sel::Profile { sketch: ring, index }]);
    let m = member(&mut e);
    assert!(m.all && m.mirror && m.name == "Rail" && m.angle == "15 deg" && ANCHORS[m.anchor].0 == "bottom", "{m:?}");
}

/// A member whose section is a whole profile sketch keeps it when edited: the dialog shows the
/// sketch rather than picking every region of it, which would fill the tube's hole.
#[test]
fn a_whole_sketch_section_stays_whole_through_an_edit() {
    let mut app = frame_app();
    app.run("frame.member", json!({"sketch": "Frame", "profile_sketch": "Ring"})).unwrap();
    let volume = |app: &mut SolveApp| app.session.execute("inspect.measure", &json!({})).unwrap()["bodies"][0]["volume_mm3"].as_f64().unwrap();
    let before = volume(&mut app);
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert!(e.inputs[1].items.is_empty());
    assert_eq!(member(&mut e).whole, Some(sketch_id(&app, "Ring")));
    member(&mut e).angle = "30 deg".into();
    let c = apply_commands(&app, &e).unwrap();
    assert!(c[0].1["params"].get("profiles").is_none(), "{}", c[0].1);
    run_all(&mut app, c);
    assert!((volume(&mut app) - before).abs() < 1e-6 * before, "still the ring, not the filled circle");
    // Unticked, the section is a built-in profile again.
    let mut e = edit_dialog(&app, f, None).unwrap();
    member(&mut e).whole = None;
    assert_eq!(apply_commands(&app, &e).unwrap()[0].1["params"]["profile"]["kind"], "rect_tube");
}

/// Without lines there is nothing to place; lines and the section in one sketch are refused.
#[test]
fn missing_or_mixed_picks_are_refused() {
    let mut app = frame_app();
    let d = dialog_on(&mut app, &[]);
    assert!(apply_commands(&app, &d).unwrap_err().contains("lines"));
    let ring = sketch_id(&app, "Ring");
    let circles: Vec<String> = app.session.doc.sketch(ring).unwrap().curves.iter().map(|c| c.id.clone()).collect();
    let mut d = dialog_on(&mut app, &circles[..1]);
    d.inputs[1].items = vec![Sel::Profile { sketch: ring, index: 0 }];
    assert!(apply_commands(&app, &d).is_err());
}

/// Lines picked in the view keep their sketch: two 3D sketches both number their lines `w1`…, and
/// the members go on the lines that were clicked, not on the newer sketch's lines of those ids.
#[test]
fn picked_lines_stay_in_the_sketch_they_were_picked_in() {
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Uprights"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 500]]})),
        ("sketch.line3d", json!({"points": [[300, 0, 0], [300, 0, 500]]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "XY", "name": "Rails"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 250], [300, 0, 250]]})),
        ("sketch.line3d", json!({"points": [[0, 0, 480], [300, 0, 480]]})),
        ("sketch.finish", json!({})),
    ]);
    let (uprights, rails) = (sketch_id(&app, "Uprights"), sketch_id(&app, "Rails"));
    let wires = |id: u64| -> Vec<String> { app.session.doc.sketch(id).unwrap().wires.iter().map(|w| w.id.clone()).collect() };
    let ids = wires(uprights);
    assert_eq!(ids, wires(rails), "the two sketches share their line ids");
    app.start("frame.member");
    let mut d = app.dialog.clone().unwrap();
    d.inputs[0].items = ids.iter().map(|id| Sel::ExternalSketchCurve { sketch: uprights, id: id.clone() }).collect();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["sketch"], json!(uprights));
    run_all(&mut app, c.clone());
    // Two posts the height of the uprights, not two rails.
    let m = app.session.execute("inspect.measure", &json!({})).unwrap();
    for b in m["bodies"].as_array().unwrap() {
        let (lo, hi) = (b["bbox"]["min"][2].as_f64().unwrap(), b["bbox"]["max"][2].as_f64().unwrap());
        assert!(lo.abs() < 1e-6 && (hi - 500.0).abs() < 1e-6, "{b}");
    }
    // Lines picked in two sketches are refused.
    d.inputs[0].items.push(Sel::ExternalSketchCurve { sketch: rails, id: ids[0].clone() });
    assert!(apply_commands(&app, &d).unwrap_err().contains("one sketch"));
}

fn gusset_with_picked_supports() -> SolveApp {
    let mut app = app_with(&[
        ("solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":300,"body_name":"Post"})),
        ("solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"})),
    ]);
    for (name, point, normal) in [("Post", [20., 0., 100.], [1., 0., 0.]), ("Rail", [100., 0., 40.], [0., 0., 1.])] {
        let st = app.session.model.state();
        let body = st.body(name).unwrap();
        let point = solidcraft_engine::geom::Vec3::new(point[0], point[1], point[2]);
        let normal = solidcraft_engine::geom::Vec3::new(normal[0], normal[1], normal[2]);
        let index = (0..body.body.face_count())
            .find(|i| body.body.planar_face(*i).is_some_and(|p| p.normal().dot(normal) > 0.999 && (p.origin - point).dot(normal).abs() < 1e-6))
            .unwrap();
        app.session.selection.push(Sel::Face { body: name.into(), index, point });
    }
    app
}

#[test]
fn gusset_preselection_preview_and_edit_keep_exact_supports() {
    let mut app = gusset_with_picked_supports();
    app.start("frame.gusset");
    let mut d = app.dialog.clone().unwrap();
    assert!(d.previews());
    assert_eq!(d.inputs.iter().map(|i| i.items.len()).collect::<Vec<_>>(), [1, 1]);
    let Kind::Gusset(g) = &mut d.kind else { panic!("wrong dialog") };
    g.leg1 = "80 mm".into();
    g.leg2 = "100 mm".into();
    let preview = crate::dialogs::preview_commands(&app, &d).unwrap();
    assert_eq!(preview[0].0, "frame.gusset");
    assert!(preview[0].1["supports"][0]["index"].is_number());
    let c = apply_commands(&app, &d).unwrap();
    run_all(&mut app, c);
    let fid = app.session.doc.features.last().unwrap().id;
    let mut edit = edit_dialog(&app, fid, None).unwrap();
    assert!(edit.inputs.iter().all(|i| i.items.len() == 1));
    let Kind::Gusset(g) = &mut edit.kind else { panic!("wrong edit dialog") };
    assert_eq!(g.leg2, "100 mm");
    g.thickness = "8 mm".into();
    let c = apply_commands(&app, &edit).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    run_all(&mut app, c);
    let state = app.session.model.state();
    let body = state.bodies.iter().find(|b| b.feature == fid).unwrap();
    assert!((body.mesh().measure().volume - 32000.).abs() < 1e-5);
}

#[test]
fn gusset_placement_icons_select_both_sides_and_center_with_exact_bounds() {
    let mut h = crate::scenario::Harness::new();
    h.app = gusset_with_picked_supports();
    h.app.home.open = false;
    h.app.start("frame.gusset");
    h.frames(3);
    assert!(crate::scenario::rect_of("dialog:segments:Thickness Placement").is_some(), "placement is a visible icon strip");
    let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
    for label in ["Negative Side", "Centered", "Positive Side"] {
        assert!(nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(label)), "accessible icon: {label}");
    }
    for (label, placement, min, max) in
        [("Negative Side", "negative", 0., 6.), ("Centered", "center", -3., 3.), ("Positive Side", "positive", -6., 0.)]
    {
        crate::test_support::click(&mut h, &format!("dialog:Thickness Placement:{label}"));
        let d = h.app.dialog.as_ref().unwrap();
        let commands = crate::dialogs::preview_commands(&h.app, d).unwrap();
        assert_eq!(commands[0].1["placement"], placement);
        let mut preview = Session::new(solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap());
        let result = preview.execute(&commands[0].0, &commands[0].1).unwrap();
        let state = preview.model.state();
        let body = state.bodies.iter().find(|b| b.feature == result["feature"].as_u64().unwrap()).unwrap();
        let bounds = body.mesh().bounds();
        assert!((bounds.min.y - min).abs() < 1e-6 && (bounds.max.y - max).abs() < 1e-6, "{label}: {bounds:?}");
    }
    // Icon-only controls still have translated names for keyboard/screen-reader users.
    h.app.preferences.language = "de".into();
    h.frames(3);
    let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
    for id in ["ui.gusset.negative", "ui.gusset.center", "ui.gusset.positive"] {
        assert!(nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(h.app.tr(id))), "{id}");
    }
}
