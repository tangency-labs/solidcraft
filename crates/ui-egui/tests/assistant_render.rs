//! Render the actual Assistant panel with a public-safe transcript, without an agent or GPU.
#[path = "support/raster.rs"]
mod raster;

use std::collections::HashMap;

use egui::{ColorImage, TextureId};
use serde_json::json;
use solidcraft_ui_egui::{
    agent_chat::{
        Item,
        backend::{PermissionOption, ToolStatus},
    },
    scenario::{self, Harness},
    theme,
};

#[test]
fn assistant_transcript_tools_and_approvals_fit_in_both_themes() {
    let previous = theme::is_dark();
    for dark in [false, true] {
        for width in [280.0, 440.0, 760.0] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1100,"height":1500}));
            h.call("ui.set", json!({"dark":dark}));
            h.call("ui.agent_chat", json!({"features":true,"open":true,"picker":false,"width":width}));
            h.app.agent_chat.items = vec![
                Item::User {
                    text: "Make the plate 80 mm wide, soften its edges, and export a STEP file.".into(),
                    image: false,
                    pictures: vec![],
                    files: vec!["plate-notes.txt".into()],
                },
                Item::Agent { thought: false, text: "I’ll update the width and check the selected edges before exporting.".into() },
                Item::Tool {
                    id: "width".into(),
                    title: "parameters.set".into(),
                    kind: "other".into(),
                    status: Some(ToolStatus::Completed),
                    output: Some("plate_width = 80 mm".into()),
                },
                Item::Tool {
                    id: "fillet".into(),
                    title: "solid.fillet".into(),
                    kind: "other".into(),
                    status: Some(ToolStatus::Failed),
                    output: Some("The requested radius is too large. Try a smaller radius.".into()),
                },
                Item::Agent {
                    thought: false,
                    text: "The width is updated. The fillet needs a smaller radius. May I export the current plate for review?".into(),
                },
                Item::Permission {
                    key: 1,
                    title: "Export plate.step".into(),
                    answer: None,
                    options: vec![
                        PermissionOption { id: "deny".into(), name: "Deny".into(), kind: "reject_once".into() },
                        PermissionOption { id: "always".into(), name: "Always allow".into(), kind: "allow_always".into() },
                        PermissionOption { id: "once".into(), name: "Allow once".into(), kind: "allow_once".into() },
                    ],
                },
            ];
            h.ctx = egui::Context::default();
            h.ctx.enable_accesskit();
            solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
            theme::apply(&h.ctx);
            let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
            for _ in 0..8 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            // Expanding the failed call must preserve every approval and composer target.
            let at = scenario::handle_at("agent_chat:tool:3").expect("failed tool target");
            for pressed in [true, false] {
                raster::paint(
                    &mut h,
                    &mut textures,
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                    ],
                );
            }
            for _ in 0..8 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            h.output = output.platform_output.clone();
            let screenshot = raster::raster(&mut h, &textures, output);
            let panel = scenario::rect_of("agent_chat:panel").expect("Assistant panel");
            for name in ["permission:once", "permission:always", "permission:deny", "permissions", "send"] {
                let at = scenario::handle_at(&format!("agent_chat:{name}")).expect("Assistant action retained");
                assert!(panel.contains(at), "{name} outside {width} pt panel: {at:?} {panel:?}");
            }
            assert!(h.widget("The requested radius is too large. Try a smaller radius.").is_some(), "failed tool output expands");
            assert_eq!(h.app.agent_chat.waiting(), 1, "rendering and expanding tools never answer an approval");
            let approval = scenario::rect_of("assistant:approval").expect("approval card");
            assert!(approval.height() < 160.0, "approval stays compact at {width}: {approval:?}");
            if let Ok(dir) = std::env::var("SOLIDCRAFT_ASSISTANT_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                image::imageops::crop_imm(&screenshot, panel.left() as u32, panel.top() as u32, panel.width() as u32, panel.height() as u32)
                    .to_image()
                    .save(std::path::Path::new(&dir).join(format!("assistant-{width:.0}-{}.png", if dark { "dark" } else { "light" })))
                    .unwrap();
            }
        }
    }
    theme::set_dark(previous);
}
