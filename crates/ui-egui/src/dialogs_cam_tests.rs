use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::Vec3;

use super::*;
use crate::Services;
use crate::dialogs::{Kind, apply_commands, preview_commands};

/// A grounded base, a cam (a disc of radius 30 about (10, 0)) turning on a revolute joint about +Z
/// through the origin, a tappet on a slider along +X, its end on the cam, and a spare part on a
/// revolute joint of its own.
fn valve() -> SolveApp {
    let mut s = Session::default();
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run("component.create", json!({"name": "Base"}));
    run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [-80, -80, -20]}));
    run("component.activate", json!({"component": "root"}));
    run("component.create", json!({"name": "Cam"}));
    run("solid.cylinder", json!({"base": [10, 0, 0], "axis": [0, 0, 1], "radius": 30, "height": 5}));
    run("component.activate", json!({"component": "root"}));
    run("component.create", json!({"name": "Tappet"}));
    run("solid.box", json!({"length": 20, "width": 4, "height": 5, "corner": [40, -2, 0]}));
    run("component.activate", json!({"component": "root"}));
    run("component.create", json!({"name": "Spare"}));
    run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [-80, 60, 0]}));
    run("component.activate", json!({"component": "root"}));
    run("occurrence.ground", json!({"occurrence": "Base:1", "grounded": true}));
    for (kind, name, z) in [("revolute", "Cam", [0, 0, 1]), ("slider", "Tappet", [1, 0, 0]), ("revolute", "Spare", [0, 0, 1])] {
        let at = |o: &str| json!({"occurrence": o, "point": [0, 0, 2.5], "z": z});
        run("joint.create", json!({"type": kind, "a": at("Base"), "b": at(name), "flip": true, "name": name}));
    }
    SolveApp::new(s, Services::default())
}

/// A pick of the mesh edge nearest a point.
fn edge(app: &SolveApp, p: Vec3) -> Sel {
    let st = app.session.world_state().unwrap();
    let mut best: Option<(f64, Sel)> = None;
    for b in &st.bodies {
        for (index, e) in b.mesh().edges.iter().enumerate() {
            let d = e.windows(2).filter_map(|w| Some(p.dist_to_segment(*w.first()?, *w.get(1)?))).fold(f64::MAX, f64::min);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, Sel::Edge { body: b.name.clone(), index, point: p }));
            }
        }
    }
    best.map(|x| x.1).unwrap()
}

fn joint(app: &SolveApp, name: &str) -> u64 {
    app.session.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.id).unwrap_or(0)
}

/// Picking the cam's edge chooses the joint it turns on; OK makes the cam, and Play turns the cam
/// in the preview.
#[test]
fn a_picked_cam_edge_makes_the_cam() {
    let mut app = valve();
    app.start("joint.cam");
    let mut d = app.dialog.take().unwrap();
    // A second revolute joint for the cam to start on: the pick puts the cam right.
    if let Kind::Assembly(Asm::Cam(f)) = &mut d.kind {
        (f.a, f.b) = (joint(&app, "Spare"), joint(&app, "Tappet"));
    }
    d.active = 0;
    // The far side of the cam's top edge (its near side touches the tappet).
    let pick = edge(&app, Vec3::new(-20.0, 0.0, 5.0));
    d.pick(&app.session, pick);
    app.dialog = Some(d);
    let ctx = crate::fonts::test_context();
    for _ in 0..3 {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(&mut app, ui.ctx()));
        out.textures_delta.clear();
    }
    let mut d = app.dialog.clone().expect("the dialog stays open");
    if let Kind::Assembly(Asm::Cam(f)) = &mut d.kind {
        // The joint the picked part turns on, and a follower that is another joint.
        assert_eq!((f.a, f.b), (joint(&app, "Cam"), joint(&app, "Tappet")));
        (f.follower, f.radius) = (1, "4 mm".into());
    }
    let c = apply_commands(&app, &d).unwrap();
    let (id, p) = &c[0];
    assert_eq!((id.as_str(), p["follower"].clone(), p["radius"].clone()), ("joint.cam", json!("roller"), json!("4 mm")), "{p}");
    assert!(p["profile"]["edges"][0].is_array() && p["profile"]["occurrence"].is_u64(), "{p}");
    if let Kind::Assembly(Asm::Cam(f)) = &mut d.kind {
        (f.follower, f.animate, f.phase) = (0, true, 0.25);
    }
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds.get(1).map(|c| (c.0.as_str(), c.1["values"].clone())), Some(("joint.drive", json!(["90 deg"]))), "{cmds:?}");
    let c = apply_commands(&app, &d).unwrap();
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let l = app.session.doc.assembly.links.first().cloned().unwrap();
    assert_eq!(l.lift.len(), 360);
    // A quarter turn lowers the knife tappet from 40 to √(30² − 10²).
    app.run("joint.drive", json!({"joint": "Cam", "value": 90})).unwrap();
    let v = solidcraft_engine::doc::joints::joint_values(&app.session.doc.assembly).get(&joint(&app, "Tappet")).cloned().unwrap_or_default();
    assert!((v.first().copied().unwrap_or(f64::NAN) - (800f64.sqrt() - 40.0)).abs() < 1e-3, "{v:?}");
}

/// The dialog's rows pass their English text, which the catalogs translate by its key.
#[test]
fn the_cam_dialog_labels_translate() {
    let de = crate::i18n::Locale::for_language("de");
    for (english, german) in [("Cam", "Nocken"), ("Follower", "Stößel"), ("Roller", "Rolle"), ("Tip (optional)", "Spitze (optional)")] {
        assert_eq!(de.label(english), german);
    }
    assert_eq!(crate::i18n::Locale::for_language("zh").label("Knife edge"), "尖顶");
}
