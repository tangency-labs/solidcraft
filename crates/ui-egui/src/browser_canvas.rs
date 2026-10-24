//! The Edit Canvas / Calibrate card the Browser opens for a reference image: position, size,
//! angle, opacity and flip (live, one undo step per opening), or two points on the image and
//! their true distance.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::browser::panel_pos;
use crate::ui_kit;

pub(crate) fn panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.tree.canvas_panel.clone() else { return };
    let Some(c) = app.session.doc.canvases.iter().find(|c| c.id == m.canvas).cloned() else {
        app.tree.canvas_panel = None;
        return;
    };
    let mut edit: Option<Value> = None;
    let (units, decimals) = (app.session.doc.units.clone(), app.preferences.length_decimals);
    let title = crate::i18n::format(if m.calibrate { "browser.calibrate_title" } else { "browser.edit_canvas_title" }, &[("name", &c.name)]);
    let at = panel_pos(app);
    let (calibrate, name) = (m.calibrate, c.id);
    let (ok, cancel) = if calibrate { (crate::i18n::tr_label("Calibrate"), crate::i18n::tr("ui.cancel")) } else { ("", crate::i18n::tr("ui.close")) };
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_canvas_panel"), "canvas", &title)
        .placed(egui::Align2::LEFT_TOP, at)
        .fields()
        .actions(ok, cancel, None)
        .show(ctx, true, |ui| {
            if calibrate {
                let picking = m.picking;
                let mut pick = None;
                for (i, (label, p)) in [("Point A", &mut m.a), ("Point B", &mut m.b)].into_iter().enumerate() {
                    ui_kit::row(ui, crate::i18n::tr_label(label), |ui| {
                        let [x, y] = p;
                        ui.horizontal(|ui| {
                            ui_kit::length_field(ui, x, &units, decimals);
                            ui_kit::length_field(ui, y, &units, decimals);
                            // Or click it on the image (#4940).
                            let style = if picking == Some(i) { ui_kit::Button::Primary } else { ui_kit::Button::Normal };
                            let r = ui_kit::button(ui, crate::i18n::tr_label("Pick"), style)
                                .on_hover_text(crate::i18n::tr_label("Click the point on the image"));
                            crate::scenario::publish_handle(&format!("canvas:pick:{i}"), r.rect.center());
                            if r.clicked() {
                                pick = Some(if picking == Some(i) { None } else { Some(i) });
                            }
                        });
                    });
                }
                if let Some(next) = pick {
                    m.picking = next;
                }
                if m.picking.is_some() {
                    ui.label(crate::i18n::tr_label("Click the point on the image"));
                }
                ui_kit::row(ui, crate::i18n::tr_label("Distance"), |ui| ui_kit::length_field(ui, &mut m.distance, &units, decimals));
                m.distance = m.distance.clamp(0.001, 1e6);
            } else {
                let (mut x, mut y, mut w, mut ang, mut op, mut flip) = (c.center.x, c.center.y, c.width, c.angle.to_degrees(), c.opacity, c.flip);
                let mut changed = false;
                changed |= ui_kit::row(ui, crate::i18n::tr_label("Center X"), |ui| ui_kit::length_field(ui, &mut x, &units, decimals)).changed();
                changed |= ui_kit::row(ui, crate::i18n::tr_label("Center Y"), |ui| ui_kit::length_field(ui, &mut y, &units, decimals)).changed();
                changed |= ui_kit::row(ui, crate::i18n::tr_label("Width"), |ui| ui_kit::length_field(ui, &mut w, &units, decimals)).changed();
                changed |= ui_kit::row(ui, crate::i18n::tr_label("Angle"), |ui| ui_kit::angle_field(ui, &mut ang)).changed();
                changed |=
                    ui_kit::row(ui, crate::i18n::tr_label("Opacity"), |ui| ui_kit::slider(ui, &mut op, 0.0..=1.0, Default::default())).changed();
                if ui_kit::checkbox_row(ui, "flip", crate::i18n::tr_label("Flip"), flip) {
                    flip = !flip;
                    changed = true;
                }
                if changed {
                    edit = Some(json!({ "canvas": name, "center": [x, y], "width": w.clamp(0.01, 1e6), "angle": ang, "opacity": op, "flip": flip }));
                }
            }
        });
    if let Some(p) = edit
        && app.run("canvas.edit", p).is_ok()
    {
        // One undo step for the whole edit.
        if app.session.undo.len() > m.depth + 1 {
            let keep = app.session.undo.get(m.depth).cloned();
            app.session.undo.truncate(m.depth);
            app.session.undo.extend(keep);
        }
    }
    if action == ui_kit::Action::Ok {
        if app.run("canvas.calibrate", json!({ "canvas": c.id, "a": m.a, "b": m.b, "distance": m.distance })).is_ok() {
            app.tree.canvas_panel = None;
        }
        return;
    }
    app.tree.canvas_panel = if closed || action == ui_kit::Action::Cancel { None } else { Some(m) };
}

/// The canvas being calibrated and its plane in the world.
fn calibrating(app: &SolveApp) -> Option<(&crate::browser::CanvasPanel, solidcraft_engine::geom::Plane)> {
    let m = app.tree.canvas_panel.as_ref().filter(|m| m.calibrate)?;
    let c = app.session.doc.canvases.iter().find(|c| c.id == m.canvas)?;
    let (vals, _) = app.session.doc.param_values();
    Some((m, app.session.doc.canvas_plane(&vals, c).ok()?))
}

/// A click in the view while Calibrate waits for Point A or B: the point where it meets the
/// canvas's plane (#4940). True when the click was taken.
pub(crate) fn pick_click(app: &mut SolveApp, proj: &crate::viewport::Proj, at: egui::Pos2) -> bool {
    let Some((m, plane)) = calibrating(app) else { return false };
    let Some(i) = m.picking else { return false };
    let (origin, dir) = proj.ray(at);
    let Some(hit) = plane.intersect_ray(origin, dir) else { return true };
    let p = plane.to_local(hit);
    if let Some(m) = app.tree.canvas_panel.as_mut() {
        if i == 0 {
            m.a = [p.x, p.y]
        } else {
            m.b = [p.x, p.y]
        }
        m.picking = None;
    }
    true
}

/// Point A and B (and the line between them) on the canvas while it is calibrated, so the
/// typed or picked points show where they are on the image.
pub(crate) fn show_points(app: &SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let Some((m, plane)) = calibrating(app) else { return };
    let t = crate::theme::Tokens::get();
    let screen = |p: [f64; 2]| proj.to_screen(plane.to_world(solidcraft_engine::geom::Vec2::new(p[0], p[1])));
    let (Some(a), Some(b)) = (screen(m.a), screen(m.b)) else { return };
    painter.line_segment([a, b], egui::Stroke::new(1.5, t.accent));
    for (p, name) in [(a, "A"), (b, "B")] {
        painter.circle(p, 4.0, t.accent, egui::Stroke::new(1.5, t.on_accent));
        painter.text(p + egui::vec2(7.0, -7.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(t.font_label), t.accent);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn canvas(h: &mut crate::scenario::Harness) -> u64 {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(400u32.to_be_bytes());
        png.extend(200u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let data = solidcraft_engine::doc::canvas::base64_encode(&png);
        h.app.run("canvas.insert", json!({"data": data, "plane": "XY", "width": 80})).unwrap()["canvas"].as_u64().unwrap()
    }

    #[test]
    fn edit_canvas_is_a_card_whose_close_keeps_the_live_edits() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, false));
        h.frames(4);
        assert!(crate::scenario::handle_at("dialog:close").is_some() && h.widget("Close").is_some());
        assert!(h.widget("Calibrate").is_none(), "editing has no Calibrate action");
        for row in ["dialog:length:Center X", "dialog:angle:Angle", "dialog:slider:Opacity"] {
            assert!(crate::scenario::handle_at(row).is_some(), "{row}");
        }
        h.step(&json!({"widget": "Close"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none());
        assert_eq!(h.app.session.doc.canvases.len(), 1);
    }

    #[test]
    fn calibrate_runs_the_command_and_cancel_leaves_the_canvas_alone() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let id = canvas(&mut h);
        let width = |h: &crate::scenario::Harness| h.app.session.doc.canvases[0].width;
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, true));
        h.frames(4);
        h.step(&json!({"widget": "Cancel"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none());
        assert!((width(&h) - 80.0).abs() < 1e-9);
        let mut panel = crate::browser::CanvasPanel::new(&h.app, id, true);
        panel.distance = 40.0;
        h.app.tree.canvas_panel = Some(panel);
        h.frames(4);
        h.step(&json!({"widget": "Calibrate"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none());
        assert!((width(&h) - 40.0).abs() < 1e-6, "calibrating to half the distance halves the image: {}", width(&h));
    }

    /// #4940: Pick, then a click on the image, sets Point A (or B) where the click meets the
    /// canvas's plane; Pick again cancels.
    #[test]
    fn calibrate_points_are_picked_on_the_image() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, true));
        h.frames(4);
        let panel = |h: &crate::scenario::Harness| h.app.tree.canvas_panel.clone().unwrap();
        h.step(&json!({"click": {"handle": "canvas:pick:0"}})).unwrap();
        assert_eq!(panel(&h).picking, Some(0));
        h.step(&json!({"click": {"world": [12, 5, 0]}})).unwrap();
        let m = panel(&h);
        assert!((m.a[0] - 12.0).abs() < 0.5 && (m.a[1] - 5.0).abs() < 0.5, "{:?}", m.a);
        assert_eq!(m.picking, None, "one click per Pick");
        assert!(h.app.session.selection.is_empty(), "the click selects nothing");
        h.step(&json!({"click": {"handle": "canvas:pick:1"}})).unwrap();
        h.step(&json!({"click": {"handle": "canvas:pick:1"}})).unwrap();
        assert_eq!(panel(&h).picking, None, "Pick again cancels");
    }
}
