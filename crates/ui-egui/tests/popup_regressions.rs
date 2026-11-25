//! Raster checks for the real Parameters and Crash Reports cards at device scales.
#[path = "support/raster.rs"]
mod raster;

use egui::{ColorImage, Rect, TextureId, pos2};
use serde_json::json;
use solidcraft_theme::Tokens;
use solidcraft_ui_egui::scenario::{self, Harness};
use std::collections::HashMap;

fn setup(dark: bool, ppp: f32) -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1100,"height":1500}));
    h.call("ui.set", json!({"dark":dark}));
    h.app.home.open = false;
    h.app.preferences.ui_scale = ppp;
    h
}

fn capture(h: &mut Harness, name: &str) -> (image::RgbaImage, egui::FullOutput) {
    // Control calls run layout frames. A fresh context ensures this renderer receives
    // the initial font atlas and every icon texture, including the white fill texel.
    h.ctx = egui::Context::default();
    h.ctx.set_pixels_per_point(h.app.preferences.ui_scale);
    solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
    solidcraft_theme::apply(&h.ctx);
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    for _ in 0..8 {
        raster::paint(h, &mut textures, vec![]);
    }
    if name.starts_with("crash-") {
        // ScrollAreaOutput reports the viewport before auto-shrink; use the painted list bounds.
        let viewport =
            scenario::rect_of("dialog:list:reports:viewport").unwrap().intersect(scenario::rect_of("dialog:list:reports").unwrap().shrink(1.0));
        let at = if name.ends_with("true") { viewport.right_top() + egui::vec2(-1.0, 4.0) } else { viewport.right_bottom() - egui::vec2(4.0, 1.0) };
        for _ in 0..8 {
            raster::paint(h, &mut textures, vec![egui::Event::PointerMoved(at)]);
        }
    }
    let out = raster::paint(h, &mut textures, vec![]);
    let image = raster::raster(h, &textures, out.clone());
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/popup-regressions");
    std::fs::create_dir_all(&dir).unwrap();
    let card = scenario::rect_of("dialog:window")
        .unwrap_or_else(|| scenario::rect_of("dialog:header").unwrap().union(scenario::rect_of("dialog:footer").unwrap()));
    let ppp = h.ctx.pixels_per_point();
    crop(&image, card, ppp).save(dir.join(format!("{name}.png"))).unwrap();
    (image, out)
}

fn crop(image: &image::RgbaImage, rect: Rect, ppp: f32) -> image::RgbaImage {
    image::imageops::crop_imm(
        image,
        (rect.left() * ppp).max(0.0) as u32,
        (rect.top() * ppp).max(0.0) as u32,
        (rect.width() * ppp) as u32,
        (rect.height() * ppp) as u32,
    )
    .to_image()
}

#[test]
fn parameter_transfer_buttons_use_existing_distinct_tray_arrows_at_1x_and_2x() {
    for dark in [false, true] {
        for ppp in [1.0, 2.0] {
            let mut h = setup(dark, ppp);
            h.app.start("parameters.change");
            let (image, out) = capture(&mut h, &format!("parameters-{dark}-{ppp}x"));
            let meshes = h.ctx.tessellate(out.shapes, ppp);
            let t = Tokens::get();
            for (label, id) in [("Import…", "import_tray"), ("Export CSV…", "export_tray"), ("Export JSON…", "export_tray")] {
                let button = scenario::rect_of(&format!("dialog:button::{label}")).unwrap();
                let expected = format!("icon:{id}:{}", (t.space_14 * ppp) as u32);
                assert_ne!(solidcraft_ui_egui::icon_svg::resolve(id).master, "placeholder");
                assert!(
                    meshes.iter().any(|clipped| match &clipped.primitive {
                        egui::epaint::Primitive::Mesh(mesh) =>
                            h.ctx.tex_manager().read().meta(mesh.texture_id).is_some_and(|meta| meta.name == expected)
                                && mesh.vertices.iter().any(|v| button.contains(v.pos)),
                        _ => false,
                    }),
                    "{label} must render {expected} inside its own button"
                );
                let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/popup-regressions");
                crop(&image, button, ppp).save(dir.join(format!("{id}-{label}-{dark}-{ppp}x.png"))).unwrap();
            }
            assert_ne!(solidcraft_ui_egui::icon_svg::resolve("import_tray").master, solidcraft_ui_egui::icon_svg::resolve("export_tray").master);
        }
    }
}

const REPORT: &str = "20261018-210412-bb60791ed6692f1d";

#[test]
fn crash_card_has_no_horizontal_bar_but_long_reports_still_scroll_vertically() {
    for dark in [false, true] {
        for ppp in [1.0, 2.0] {
            for long in [false, true] {
                let mut h = setup(dark, ppp);
                let folder = h.dir.join("crashes");
                for (id, message) in [
                    (REPORT, "The radius produced a degenerate face while rebuilding Fillet2.".to_owned()),
                    ("20261017-210412-bb60791ed6692f1d", "Long report line.\n".repeat(800)),
                ]
                .into_iter()
                .take(if long { 2 } else { 1 })
                {
                    std::fs::create_dir_all(folder.join(id)).unwrap();
                    std::fs::write(folder.join(id).join("report.json"), json!({"format":"solidcraft-crash/1", "id":id, "kind":"command_panic", "count":3,
                        "first_seen":"2026-10-18T21:04:12Z", "message":message,"command":{"command":"solid.fillet","params":{"radius":2}}, "backtrace":[], "app":{"version":"0.16.1"}, "system":{}}).to_string()).unwrap();
                }
                h.app.crashes.folder = Some(folder);
                h.call("ui.help", json!({"item":"crashes"}));
                let (image, _) = capture(&mut h, &format!("crash-{dark}-{ppp}x-long-{long}"));
                let viewport = scenario::rect_of("dialog:list:reports:viewport")
                    .unwrap()
                    .intersect(scenario::rect_of("dialog:list:reports").unwrap().shrink(1.0));
                let message = scenario::rect_of(&format!("dialog:report:message:{REPORT}")).unwrap();
                let below = Rect::from_min_max(
                    pos2(viewport.left() + 2.0, message.bottom() + 1.0),
                    pos2(viewport.right() - 12.0, (message.bottom() + 20.0).min(viewport.bottom())),
                );
                let divider = scenario::rect_of(&format!("dialog:divider:end:{REPORT}"));
                if long {
                    // A divider separates reports only; it is a full-width hairline.
                    let divider = divider.unwrap();
                    let frame = scenario::rect_of("dialog:list:reports").unwrap();
                    assert!((divider.left() - frame.left() - 1.0).abs() <= 1.0 / ppp);
                    assert!((frame.right() - divider.right() - 1.0).abs() <= 1.0 / ppp);
                    assert_eq!(divider.height(), 1.0);
                } else {
                    assert!(divider.is_none(), "no divider after the last report");
                }
                let bar = Tokens::get().form_ink.to_array();
                assert_eq!(
                    crop(&image, below, ppp).pixels().filter(|p| p.0 == bar).count(),
                    0,
                    "no scrollbar pixels below the message: dark={dark}, ppp={ppp}, long={long}"
                );
                let right = Rect::from_min_max(pos2(viewport.right() - 4.0, viewport.top() + 2.0), pos2(viewport.right(), viewport.bottom() - 2.0));
                let bar_pixels = crop(&image, right, ppp).pixels().filter(|p| p.0 == bar).count();
                assert_eq!(bar_pixels > 0, long, "vertical scrollbar pixels: {bar_pixels}, long={long}, dark={dark}, ppp={ppp}");
            }
        }
    }
}
