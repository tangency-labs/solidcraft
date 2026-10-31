//! Review the real Move overlay and its readouts over the app's headless model render.
#[path = "support/raster.rs"]
mod raster;
use egui::{ColorImage, Event, PointerButton, Pos2, Rect, TextureId, vec2};
use serde_json::json;
use solidcraft_engine::{Sel, geom::Vec3};
use solidcraft_ui_egui::{
    scenario::{self, Harness},
    theme, viewport,
};
use std::collections::HashMap;

fn bearing(kind: &str) -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("component.create", json!({"name":"Housing","grounded":true})).unwrap();
    h.app.run("solid.box", json!({"length":80,"width":20,"height":80,"corner":[-40,0,0]})).unwrap();
    h.app.run("solid.cylinder", json!({"base":[0,0,40],"axis":[0,1,0],"radius":24,"height":20,"operation":"cut"})).unwrap();
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Bearing","grounded":false})).unwrap();
    h.app.run("solid.cylinder", json!({"base":[0,-20,40],"axis":[0,1,0],"radius":23,"height":28})).unwrap();
    h.app.run("solid.cylinder", json!({"base":[0,-20,40],"axis":[0,1,0],"radius":10,"height":28,"operation":"cut"})).unwrap();
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    let at = |name: &str| json!({"occurrence":name,"point":[0,0,40],"z":[0,1,0]});
    let joint =
        h.app.run("joint.create", json!({"type":kind,"a":at("Housing"),"b":at("Bearing"),"flip":true,"angle":0,"name":"Guide"})).unwrap()["joint"]
            .as_u64()
            .unwrap();
    let st = h.app.session.world_state();
    let body = st.bodies.iter().find(|b| h.app.session.doc.model_body_component(b) == part["component"].as_u64().unwrap()).unwrap();
    let mesh = body.mesh();
    let face = mesh
        .triangles
        .iter()
        .zip(&mesh.tri_face)
        .find(|(t, _)| t.iter().all(|i| (mesh.positions[*i as usize].y + 20.).abs() < 1e-5))
        .map(|(_, f)| *f as usize)
        .unwrap();
    h.app.session.selection = vec![Sel::Face { body: body.name.clone(), index: face, point: Vec3::new(15., -20., 40.) }];
    h.call("ui.view", json!({"view":"iso"}));
    h.app.cam.target = Vec3::new(0., 0., 40.);
    h.app.cam.distance = 240.;
    h.app.preferences.language = "ja".into();
    (h, joint)
}

fn frame(h: &mut Harness, textures: &mut HashMap<TextureId, ColorImage>, events: Vec<Event>) -> egui::FullOutput {
    raster::paint_at(h, textures, events, vec2(1460., 1000.))
}

fn save_frame(h: &mut Harness, textures: &mut HashMap<TextureId, ColorImage>, mut out: egui::FullOutput, path: &std::path::Path) {
    let rect = h.app.viewport.rect.unwrap();
    // This is the app's actual themed model render; handles and panels are actual egui output.
    let png = solidcraft_ui_egui::control::view_png(&h.app, rect.width().round() as u32, rect.height().round() as u32).unwrap();
    let model = image::load_from_memory(&png).unwrap().to_rgba8();
    let id = TextureId::User(77);
    textures.insert(id, ColorImage::from_rgba_unmultiplied([model.width() as usize, model.height() as usize], model.as_raw()));
    let mut background = out
        .shapes
        .iter()
        .position(|s| match &s.shape {
            egui::Shape::Path(path) => path.stroke.width >= 13.5,
            _ => false,
        })
        .expect("Move handle underlay");
    // The dashed axis is painted just before the first arrow's underlay.
    if background > 0 && matches!(&out.shapes[background - 1].shape, egui::Shape::Vec(_)) {
        background -= 1;
    }
    out.shapes.insert(
        background,
        egui::epaint::ClippedShape::new(rect, egui::Shape::image(id, rect, Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)), egui::Color32::WHITE)),
    );
    raster::raster(h, textures, out).save(path).unwrap();
}

#[test]
fn joint_move_has_readable_rotation_and_translation_in_both_themes() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/joint-move-review");
    std::fs::create_dir_all(&dir).unwrap();
    solidcraft_ui_egui::gpu::install_cpu_picture_renderer();
    for dark in [false, true] {
        for (kind, name) in [("revolute", "rotation"), ("slider", "translation"), ("cylindrical", "combined")] {
            let (mut h, joint) = bearing(kind);
            h.app.ui.dark = dark;
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
            theme::set_dark(dark);
            theme::apply(&h.ctx);
            let mut textures = HashMap::new();
            raster::paint(&mut h, &mut textures, vec![]);
            h.app.start("occurrence.move");
            for _ in 0..8 {
                frame(&mut h, &mut textures, vec![]);
            }
            let idle = frame(&mut h, &mut textures, vec![]);
            let heads: Vec<_> = idle
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Path(path) if path.points.len() == 3 && path.fill == theme::Tokens::get().joint_manipulator => Some(&path.points),
                    _ => None,
                })
                .collect();
            assert_eq!(heads.len(), if kind == "cylindrical" { 4 } else { 2 }, "two large heads per movable axis");
            for p in heads {
                let u = p[1] - p[0];
                let v = p[2] - p[0];
                assert!((u.x * v.y - u.y * v.x).abs() * 0.5 >= 400.0, "legible arrowhead, independent of arc tessellation");
            }
            save_frame(&mut h, &mut textures, idle, &dir.join(format!("{}-{name}-idle.png", if dark { "dark" } else { "light" })));
            let from = scenario::handle_at(&format!("joint-move:{joint}:0")).expect("Move handle");
            let rect = h.app.viewport.rect.unwrap();
            let proj = viewport::projection(&h.app, rect);
            let to = if kind != "slider" {
                let origin = Vec3::new(0., -20., 40.);
                let axis = Vec3::Y;
                let (o, d) = proj.ray(from);
                let q = o + d * ((origin - o).dot(axis) / d.dot(axis)) - origin;
                let angle = 27.0_f64.to_radians();
                proj.to_screen(origin + q * angle.cos() + axis.cross(q) * angle.sin()).unwrap()
            } else {
                from + (proj.to_screen(Vec3::Y * 18.75).unwrap() - proj.to_screen(Vec3::ZERO).unwrap())
            };
            frame(
                &mut h,
                &mut textures,
                vec![
                    Event::PointerMoved(from),
                    Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Default::default() },
                ],
            );
            for k in 1..=12 {
                frame(&mut h, &mut textures, vec![Event::PointerMoved(from + (to - from) * (k as f32 / 12.))]);
            }
            let out = frame(&mut h, &mut textures, vec![]);
            let readout = if kind != "slider" { "開始位置からの回転角" } else { "開始位置からの移動距離" };
            assert!(
                out.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains(readout))),
                "visible relative readout"
            );
            save_frame(&mut h, &mut textures, out, &dir.join(format!("{}-{name}.png", if dark { "dark" } else { "light" })));
        }
    }
}

#[test]
fn joint_move_zero_degree_warning_is_readable_in_both_themes() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/joint-move-review");
    std::fs::create_dir_all(&dir).unwrap();
    for dark in [false, true] {
        let (mut h, _) = bearing("rigid");
        let before = h.app.session.doc.to_json();
        h.app.ui.dark = dark;
        h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
        h.ctx = egui::Context::default();
        solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
        theme::set_dark(dark);
        theme::apply(&h.ctx);
        let mut textures = HashMap::new();
        for _ in 0..4 {
            frame(&mut h, &mut textures, vec![]);
        }
        h.app.start("occurrence.move");
        for _ in 0..6 {
            frame(&mut h, &mut textures, vec![]);
        }
        let out = frame(&mut h, &mut textures, vec![]);
        assert!(out.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("この部品は移動できません"))));
        assert!(out.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("Rigid合致"))));
        assert_eq!(h.app.session.doc.to_json(), before);
        let rect = scenario::rect_of("dialog:window").unwrap();
        let image = raster::raster(&mut h, &textures, out);
        let x = (rect.min.x.max(0.) as u32).saturating_sub(8);
        let y = (rect.min.y.max(0.) as u32).saturating_sub(8);
        let width = (rect.width().ceil() as u32 + 16).min(image.width() - x);
        let height = (rect.height().ceil() as u32 + 16).min(image.height() - y);
        image::imageops::crop_imm(&image, x, y, width, height)
            .to_image()
            .save(dir.join(format!("{}-warning.png", if dark { "dark" } else { "light" })))
            .unwrap();
    }
}
