//! Temporary visual review: rasterise the app's actual egui meshes without WindowServer.
use egui::{ColorImage, TextureId, pos2, vec2};
use serde_json::json;
use solidcraft_ui_egui::{
    dialogs::Kind,
    scenario::{self, Harness},
};
use std::collections::HashMap;

fn paint(h: &mut Harness, textures: &mut HashMap<TextureId, ColorImage>, events: Vec<egui::Event>) -> egui::FullOutput {
    let time = h.ctx.cumulative_frame_nr() as f64 / 15.0;
    let mut out = h.ctx.run_ui(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1100.0, 1500.0))),
            focused: true,
            events,
            ..Default::default()
        },
        |ui| {
            h.app.logic(ui.ctx());
            h.app.ui(ui);
        },
    );
    for (id, deltas) in &out.textures_delta.set {
        for delta in deltas {
            let egui::ImageData::Color(image) = &delta.image;
            if let Some([x, y]) = delta.pos {
                let dest = textures.get_mut(id).unwrap();
                let dest_width = dest.width();
                for row in 0..image.height() {
                    for col in 0..image.width() {
                        dest.pixels[(y + row) * dest_width + x + col] = image.pixels[row * image.width() + col];
                    }
                }
            } else {
                textures.insert(*id, (**image).clone());
            }
        }
    }
    out.textures_delta.clear();
    out
}

fn save(h: &mut Harness, textures: &HashMap<TextureId, ColorImage>, out: egui::FullOutput, path: &str) {
    let (width, height) = (1100u32, 1500u32);
    let mut pixels = vec![255u8; (width * height * 4) as usize];
    let primitives = h.ctx.tessellate(out.shapes, out.pixels_per_point);
    for clipped in primitives {
        let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive else { continue };
        let Some(texture) = textures.get(&mesh.texture_id) else { continue };
        for indices in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [mesh.vertices[indices[0] as usize], mesh.vertices[indices[1] as usize], mesh.vertices[indices[2] as usize]];
            let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
            let area = cross(b.pos - a.pos, c.pos - a.pos);
            if area.abs() < 1e-6 {
                continue;
            }
            let left = a.pos.x.min(b.pos.x).min(c.pos.x).max(clipped.clip_rect.left()).max(0.0).floor() as u32;
            let right = a.pos.x.max(b.pos.x).max(c.pos.x).min(clipped.clip_rect.right()).min(width as f32).ceil() as u32;
            let top = a.pos.y.min(b.pos.y).min(c.pos.y).max(clipped.clip_rect.top()).max(0.0).floor() as u32;
            let bottom = a.pos.y.max(b.pos.y).max(c.pos.y).min(clipped.clip_rect.bottom()).min(height as f32).ceil() as u32;
            for y in top..bottom {
                for x in left..right {
                    let p = pos2(x as f32 + 0.5, y as f32 + 0.5);
                    let w = [cross(b.pos - p, c.pos - p) / area, cross(c.pos - p, a.pos - p) / area, cross(a.pos - p, b.pos - p) / area];
                    if w.iter().any(|v| *v < -1e-5) {
                        continue;
                    }
                    let uv = a.uv.to_vec2() * w[0] + b.uv.to_vec2() * w[1] + c.uv.to_vec2() * w[2];
                    let tx = (uv.x * texture.width() as f32 - 0.5).clamp(0.0, texture.width() as f32 - 1.0);
                    let ty = (uv.y * texture.height() as f32 - 0.5).clamp(0.0, texture.height() as f32 - 1.0);
                    let (ix, iy) = (tx.floor() as usize, ty.floor() as usize);
                    let get = |x: usize, y: usize| texture.pixels[y * texture.width() + x].to_array().map(|v| v as f32 / 255.0);
                    let samples = [
                        get(ix, iy),
                        get((ix + 1).min(texture.width() - 1), iy),
                        get(ix, (iy + 1).min(texture.height() - 1)),
                        get((ix + 1).min(texture.width() - 1), (iy + 1).min(texture.height() - 1)),
                    ];
                    let (fx, fy) = (tx.fract(), ty.fract());
                    let col = [a.color.to_array(), b.color.to_array(), c.color.to_array()];
                    let mut src = [0.0; 4];
                    for i in 0..4 {
                        let tex = samples[0][i] * (1.0 - fx) * (1.0 - fy)
                            + samples[1][i] * fx * (1.0 - fy)
                            + samples[2][i] * (1.0 - fx) * fy
                            + samples[3][i] * fx * fy;
                        src[i] = tex * (col[0][i] as f32 * w[0] + col[1][i] as f32 * w[1] + col[2][i] as f32 * w[2]) / 255.0;
                    }
                    let base = ((y * width + x) * 4) as usize;
                    for i in 0..3 {
                        pixels[base + i] = ((src[i] + pixels[base + i] as f32 / 255.0 * (1.0 - src[3])) * 255.0).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
    image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8).unwrap();
}

#[test]
fn capture() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/popup-review");
    let root = directory.to_str().unwrap();
    std::fs::create_dir_all(root).unwrap();
    let crashes = format!("{root}/crashes");
    let id = "20261018-210412-bb60791ed6692f1d";
    std::fs::create_dir_all(format!("{crashes}/{id}")).unwrap();
    std::fs::write(format!("{crashes}/{id}/report.json"),json!({"format":"solidcraft-crash/1","id":id,"kind":"command_panic","count":3,"first_seen":"2026-10-18T21:04:12Z","message":"The radius produced a degenerate face while rebuilding Fillet2.","command":{"command":"solid.fillet","params":{"radius":2}},"backtrace":[],"app":{"version":"0.16.1"},"system":{}}).to_string()).unwrap();
    std::fs::write(format!("{crashes}/{id}/replay.json"), "{}").unwrap();
    for dark in [false, true] {
        for mode in ["crash", "appearance", "params", "config", "prefs"] {
            let mut h = Harness::new();
            h.call("ui.resize", json!({"width":1100,"height":1500}));
            h.call("ui.set", json!({"dark":dark}));
            h.app.home.open = false;
            if mode == "crash" || mode == "empty" {
                h.app.crashes.folder = Some(std::path::PathBuf::from(if mode == "empty" { format!("{root}/empty") } else { crashes.clone() }));
                h.call("ui.help", json!({"item":"crashes"}));
            } else if mode == "params" || mode == "config" {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
                h.app.run("parameters.add", json!({"name":"width","expression":"10 mm"})).unwrap();
                if mode == "config" {
                    h.app.run("config.configure", json!({"params":["width"]})).unwrap();
                    h.app.start("config.table");
                } else {
                    h.app.start("parameters.change");
                }
            } else if mode == "prefs" {
                h.call("ui.preferences", json!({}));
            } else {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
                h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":"#ccd0d6","appearance":"Aluminum - Satin","opacity":1.0})).unwrap();
                h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Body1".into() }];
                let path = format!("{root}/review-library.json");
                h.app.run("library.export", json!({"path":path,"overwrite":true})).unwrap();
                h.app.start("appearance.assign");
                let Kind::Appearance(k) = &mut h.app.dialog.as_mut().unwrap().kind else { panic!() };
                k.local.path = path.clone();
                k.local.catalog = Some(solidcraft_engine::io::catalog::Catalog::read(&path).unwrap());
                k.local.selected = k.local.catalog.as_ref().and_then(|c| c.entries.first()).map(|e| e.id.clone());
                k.note = Some("Appearance applied".into());
                if mode == "pbr" {
                    k.pbr = Some(Default::default());
                    k.component = Some("Plate".into());
                }
            }
            h.ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
            let mut textures = HashMap::new();
            for _ in 0..6 {
                paint(&mut h, &mut textures, vec![]);
            }
            let theme = if dark { "dark" } else { "light" };
            let out = paint(&mut h, &mut textures, vec![]);
            save(&mut h, &textures, out, &format!("{root}/{mode}-{theme}.png"));
            if mode == "appearance" {
                let at = scenario::handle_at("dialog:color:Colour").unwrap();
                for pressed in [true, false] {
                    paint(
                        &mut h,
                        &mut textures,
                        vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
                        ],
                    );
                }
                for _ in 0..3 {
                    paint(&mut h, &mut textures, vec![]);
                }
                let out = paint(&mut h, &mut textures, vec![]);
                save(&mut h, &textures, out, &format!("{root}/picker-{theme}.png"));
            }
        }
    }
}
