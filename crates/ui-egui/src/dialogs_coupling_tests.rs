use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::Vec3;

use super::*;
use crate::Services;
use crate::dialogs::{Kind, apply_commands, preview_commands};

#[test]
fn missing_joint_messages_are_drawn_in_full() {
    for language in ["en", "de", "vi"] {
        let _language = crate::i18n::scope(language);
        for (command, message) in [
            ("joint.screw", "needs a joint that turns and one that slides"),
            ("joint.gear", "needs two joints that turn"),
            ("joint.rack_pinion", "needs a joint that turns and one that slides"),
        ] {
            let mut app = SolveApp::new(Session::default(), Services::default());
            app.preferences.language = language.to_owned();
            app.start(command);
            let ctx = crate::fonts::test_context();
            let mut textures = std::collections::HashMap::new();
            let mut draw = |app: &mut SolveApp| {
                let input =
                    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
                let mut output = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
                crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                output
            };
            for _ in 0..3 {
                draw(&mut app);
            }
            let output = draw(&mut app);
            if let Ok(dir) = std::env::var("SOLIDCRAFT_DIALOG_STATUS_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                crate::ui_kit::test_raster(
                    &ctx,
                    output.shapes.clone(),
                    output.pixels_per_point,
                    app.viewport.dialog_rect.expect("dialog bounds").expand(4.0),
                    &textures,
                )
                .save(std::path::Path::new(&dir).join(format!("{command}-{language}.png")))
                .unwrap();
            }
            let message = crate::i18n::tr_label(message);
            let text = crate::test_support::text_shapes(&output).find(|text| text.galley.text() == message).expect("missing-joint message");
            assert!(!text.galley.elided, "{command} ({language}) cuts off: {message}");
            let bounds = text.galley.rect.translate(text.pos.to_vec2());
            let clip =
                output.shapes.iter().find(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text() == message)).unwrap().clip_rect;
            assert!(clip.contains_rect(bounds), "{command} ({language}) clips the message: {bounds:?} outside {clip:?}");
        }
    }
}

/// A grounded base with discs G1 (radius 10 at x = 0) and G2 (radius 20 at x = 30) turning on
/// revolute joints about +Z, and a nut on a cylindrical joint.
fn discs() -> SolveApp {
    let mut s = Session::default();
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    for (name, base, r) in [("Base", [0, -60, -20], 2), ("G1", [0, 0, 0], 10), ("G2", [30, 0, 0], 20), ("Nut", [0, 60, 0], 3)] {
        run("component.activate", json!({"component": "root"}));
        run("component.create", json!({ "name": name }));
        run("solid.cylinder", json!({"base": base, "axis": [0, 0, 1], "radius": r, "height": 5}));
    }
    run("component.activate", json!({"component": "root"}));
    run("occurrence.ground", json!({"occurrence": "Base:1", "grounded": true}));
    for (kind, name, x, y) in [("revolute", "G1", 0, 0), ("revolute", "G2", 30, 0), ("cylindrical", "Nut", 0, 60)] {
        let at = |o: &str| json!({"occurrence": o, "point": [x, y, 0], "z": [0, 0, 1]});
        run("joint.create", json!({"type": kind, "a": at("Base"), "b": at(name), "flip": true, "name": name}));
    }
    SolveApp::new(s, Services::default())
}

/// A pick of the round wall a point of the exact geometry lies on (the facets of the wall's mesh
/// cut inside it, so the nearest face of any body).
fn wall(app: &SolveApp, p: Vec3) -> Sel {
    let st = app.session.world_state();
    let mut best: Option<(f64, Sel)> = None;
    for b in &st.bodies {
        let m = b.mesh();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let d = crate::preview::point_triangle_dist(p, a, bb, c);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, Sel::Face { body: b.name.clone(), index: *f as usize, point: p }));
            }
        }
    }
    best.map(|x| x.1).unwrap()
}

fn joint(app: &SolveApp, name: &str) -> u64 {
    app.session.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.id).unwrap_or(0)
}

/// Picking a gear's round face chooses the joint it turns on and takes its radius; the ratio
/// row says what the pair does, and OK makes the coupling.
#[test]
fn picked_gear_faces_choose_their_joints_and_radii() {
    let mut app = discs();
    app.start("joint.gear");
    let mut d = app.dialog.take().unwrap();
    // G2 first, G1 second: the joints follow the picks, whatever the dialog started on.
    for (i, p) in [Vec3::new(50.0, 0.0, 2.5), Vec3::new(10.0, 0.0, 2.5)].into_iter().enumerate() {
        d.active = i;
        let pick = wall(&app, p);
        d.pick(&app.session, pick);
    }
    app.dialog = Some(d);
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp| {
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        crate::test_support::frame(&ctx, input, |ui| crate::dialogs::show(app, ui.ctx()))
    };
    frame(&mut app);
    let out = frame(&mut app);
    let shown: Vec<_> = crate::test_support::text_shapes(&out).map(|t| t.galley.text().to_string()).collect();
    assert!(shown.iter().any(|t| t == "-2 deg / deg"), "{shown:?}");
    assert!(shown.iter().any(|t| t.starts_with("20 mm")), "{shown:?}");
    let d = app.dialog.as_ref().expect("the dialog stays open");
    let c = apply_commands(&app, d).unwrap();
    let (id, p) = &c[0];
    assert_eq!((id.as_str(), p["a"].clone(), p["b"].clone()), ("joint.gear", json!(joint(&app, "G2")), json!(joint(&app, "G1"))), "{p}");
    assert!(p["ra"]["circle"].is_array() && p["rb"]["occurrence"].is_u64(), "{p}");
    // Play turns the driving gear in the preview.
    let mut d = d.clone();
    if let Kind::Assembly(Asm::Coupling(f)) = &mut d.kind {
        f.animate = true;
        f.phase = 0.25;
    }
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds.get(1).map(|c| c.1["values"].clone()), Some(json!(["90 deg"])), "{cmds:?}");
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let l = app.session.doc.assembly.links.first().cloned().unwrap();
    assert!((l.ratio + 2.0).abs() < 1e-9, "{l:?}");
}

/// Two instances of one gear (a copied component) are two occurrences, each on its own joint:
/// picks on each choose each one's joint, and the engine finds each pick on its own instance.
#[test]
fn instances_of_one_gear_are_coupled_as_two() {
    let mut s = Session::default();
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    for (name, r) in [("Base", 2), ("Twin", 15)] {
        run("component.activate", json!({"component": "root"}));
        run("component.create", json!({ "name": name }));
        run("solid.cylinder", json!({"base": [0, 0, if name == "Base" { -20 } else { 0 }], "axis": [0, 0, 1], "radius": r, "height": 5}));
    }
    run("component.activate", json!({"component": "root"}));
    run("occurrence.copy", json!({"component": "Twin", "translate": [30, 0, 0]}));
    run("occurrence.ground", json!({"occurrence": "Base:1", "grounded": true}));
    for (name, x) in [("Twin:1", 0), ("Twin:2", 30)] {
        let at = |o: &str| json!({"occurrence": o, "point": [x, 0, 0], "z": [0, 0, 1]});
        run("joint.create", json!({"type": "revolute", "a": at("Base:1"), "b": at(name), "flip": true, "name": name}));
    }
    let mut app = SolveApp::new(s, Services::default());
    let occ = |app: &SolveApp, name: &str| app.session.doc.occurrences.iter().find(|o| o.name == name).map(|o| o.id).unwrap_or(0);
    app.start("joint.gear");
    let mut d = app.dialog.take().unwrap();
    for (i, p) in [Vec3::new(-15.0, 0.0, 2.5), Vec3::new(45.0, 0.0, 2.5)].into_iter().enumerate() {
        d.active = i;
        let pick = wall(&app, p);
        d.pick(&app.session, pick);
    }
    app.dialog = Some(d);
    let ctx = crate::fonts::test_context();
    for _ in 0..2 {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(&mut app, ui.ctx()));
        out.textures_delta.clear();
    }
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    let p = &c[0].1;
    assert_eq!((p["a"].clone(), p["b"].clone()), (json!(joint(&app, "Twin:1")), json!(joint(&app, "Twin:2"))), "{p}");
    assert_eq!((p["ra"]["occurrence"].clone(), p["rb"]["occurrence"].clone()), (json!(occ(&app, "Twin:1")), json!(occ(&app, "Twin:2"))), "{p}");
    app.run(&c[0].0, p.clone()).unwrap();
    assert!((app.session.doc.assembly.links[0].ratio + 1.0).abs() < 1e-9);
}

/// A cylindrical joint is a screw on its own: the dialog sends it as `joint` with the pitch.
#[test]
fn a_cylindrical_joint_is_a_screw_on_its_own() {
    let mut app = discs();
    app.start("joint.screw");
    let d = app.dialog.take().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0], ("joint.screw".to_string(), json!({"joint": joint(&app, "Nut"), "pitch": "1.5 mm", "reverse": false})));
    app.run(&c[0].0, c[0].1.clone()).unwrap();
    app.run("joint.drive", json!({"joint": "Nut", "values": [360]})).unwrap();
    let nut = solidcraft_engine::doc::joints::joint_values(&app.session.doc.assembly).get(&joint(&app, "Nut")).cloned().unwrap_or_default();
    assert!((nut.get(1).copied().unwrap_or(f64::NAN) - 1.5).abs() < 1e-9, "{nut:?}");
}

/// Play turns the pinion's angle, wherever it is among its joint's values: a planar joint's is
/// the third, after its x and y, which keep theirs.
#[test]
fn play_turns_the_angle_of_a_planar_pinion() {
    let mut app = discs();
    let at = |o: &str| json!({"occurrence": o, "point": [0, 0, 0], "z": [0, 0, 1]});
    app.run("joint.edit", json!({"joint": "G1", "type": "planar"})).unwrap();
    app.run("joint.drive", json!({"joint": "G1", "values": [2, 0, 0]})).unwrap();
    app.run("joint.create", json!({"type": "slider", "a": at("Base"), "b": at("G2"), "flip": true, "name": "Rail"})).unwrap();
    app.start("joint.rack_pinion");
    let mut d = app.dialog.take().unwrap();
    if let Kind::Assembly(Asm::Coupling(f)) = &mut d.kind {
        (f.a, f.animate, f.phase) = (joint(&app, "G1"), true, 0.5);
    }
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds.last().map(|c| c.1["values"].clone()), Some(json!(["2 mm", "0 mm", "180 deg"])), "{cmds:?}");
}

/// The dialog's rows pass their English text, which the catalogs translate by its key.
#[test]
fn the_dialog_labels_translate() {
    let de = crate::i18n::Locale::for_language("de");
    for (english, german) in [
        ("Gear 1", "Zahnrad 1"),
        ("Radius 2", "Radius 2"),
        ("Internal gear", "Innenzahnrad"),
        ("Pitch", "Steigung"),
        ("Rack and Pinion", "Zahnstange und Ritzel"),
    ] {
        assert_eq!(de.label(english), german);
    }
    assert_eq!(crate::i18n::Locale::for_language("ja").label("not a round face"), "円筒面ではありません");
}
