//! The Edit Canvas / Calibrate card the Browser opens for a reference image: position, size,
//! angle, opacity and flip (live, one undo step per opening), or two points on the image and
//! their true distance. Calibrate accepts clicks in the viewport on the image (#9108).

use egui::{Pos2, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::doc::canvas::Canvas;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::browser::panel_pos;
use crate::theme::Tokens;
use crate::ui_kit;
use crate::viewport::Proj;

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
                ui_kit::text(ui, crate::i18n::tr(calibrate_hint(m.next)), ui_kit::Text::Note("calibrate_hint"));
                for (label, p) in [("Point A", &mut m.a), ("Point B", &mut m.b)] {
                    ui_kit::row(ui, crate::i18n::tr_label(label), |ui| {
                        let [x, y] = p;
                        ui.horizontal(|ui| {
                            ui_kit::length_field(ui, x, &units, decimals);
                            ui_kit::length_field(ui, y, &units, decimals);
                        });
                    });
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
        apply(app, &m);
        return;
    }
    app.tree.canvas_panel = if closed || action == ui_kit::Action::Cancel { None } else { Some(m) };
}

/// Calibrate with the card's points and distance; the card closes when it took.
fn apply(app: &mut SolveApp, m: &crate::browser::CanvasPanel) {
    if app.run("canvas.calibrate", json!({ "canvas": m.canvas, "a": m.a, "b": m.b, "distance": m.distance })).is_ok() {
        app.tree.canvas_panel = None;
    }
}

/// A viewport click while Calibrate is open: set Point A or B on the image (#9108).
pub(crate) fn click(app: &mut SolveApp, proj: &Proj, screen: Pos2) -> bool {
    let Some(mut panel) = app.tree.canvas_panel.clone() else {
        return false;
    };
    if !panel.calibrate {
        return false;
    }
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas).cloned() else {
        return false;
    };
    let (vals, _) = doc.param_values();
    let Ok(plane) = doc.canvas_plane(&vals, &c) else {
        return false;
    };
    let (origin, direction) = proj.ray(screen);
    let Some(world) = plane.intersect_ray(origin, direction) else {
        return false;
    };
    let local = plane.to_local(world);
    if !on_image(&c, local) {
        return false;
    }
    let point = [local.x, local.y];
    if panel.next == 1 {
        panel.b = point;
        panel.next = 2;
        // The distance starts as the one measured on the image, for the real one to replace.
        let measured = Vec2::new(panel.a[0], panel.a[1]).dist(local);
        if measured > 1e-9 {
            panel.distance = measured;
        }
        panel.distance_text = None;
    } else {
        panel.a = point;
        panel.next = 1;
    }
    app.set_status(crate::i18n::tr(calibrate_hint(panel.next)), false);
    app.tree.canvas_panel = Some(panel);
    true
}

/// What Calibrate asks for next, in the card and the status line: Point A, Point B, then the
/// real distance between them.
fn calibrate_hint(next: u8) -> &'static str {
    match next {
        0 => "browser.calibrate_click_a",
        1 => "browser.calibrate_click_b",
        _ => "browser.calibrate_type_distance",
    }
}

/// Markers for the two calibration points on the image. Once both are clicked, a line joins
/// them with a Distance box at its middle, which edits the card's Distance.
pub(crate) fn paint_marks(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some(panel) = app.tree.canvas_panel.clone().filter(|p| p.calibrate) else {
        return;
    };
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas) else {
        return;
    };
    let (vals, _) = doc.param_values();
    let Ok(plane) = doc.canvas_plane(&vals, c) else {
        return;
    };
    let t = Tokens::get();
    let screens = [panel.a, panel.b].map(|p| proj.to_screen(plane.to_world(Vec2::new(p[0], p[1]))));
    let line = if panel.next == 2 { screens[0].zip(screens[1]) } else { None };
    if let Some((a, b)) = line {
        painter.line_segment([a, b], Stroke::new(2.0, t.accent));
    }
    for (i, screen) in screens.into_iter().enumerate() {
        let Some(screen) = screen else {
            continue;
        };
        let active = panel.next as usize == i;
        let stroke = Stroke::new(if active { 2.0 } else { 1.5 }, if active { t.accent } else { t.sel_edge });
        painter.circle_stroke(screen, 7.0, stroke);
        painter.circle_filled(screen, 2.5, if active { t.accent } else { t.sel_vertex });
        let label = if i == 0 { "A" } else { "B" };
        painter.text(
            screen + vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(t.font_body),
            if active { t.accent } else { t.text },
        );
    }
    if let Some((a, b)) = line {
        distance_box(app, ui, proj, a.lerp(b, 0.5));
    }
}

/// The Distance box over the line A–B. It shows the card's Distance and takes the keyboard
/// (all of it selected) unless another field has it; what is typed there updates the card as
/// it goes, and Enter calibrates, as the card's Calibrate does.
fn distance_box(app: &mut SolveApp, ui: &mut egui::Ui, proj: &Proj, mid: Pos2) {
    let Some(mut panel) = app.tree.canvas_panel.clone() else { return };
    let id = egui::Id::new("sc_calibrate_value");
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    // Taken here, so the card's Enter-for-Calibrate doesn't run it a second time.
    let enter = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let take = !focused && ui.ctx().memory(|m| m.focused().is_none());
    let shown = crate::prefs::show_mm(app, panel.distance, 1);
    let mut text = if focused { panel.distance_text.clone().unwrap_or_else(|| shown.clone()) } else { shown.clone() };
    let ids = (egui::Id::new("sc_calibrate_area"), id);
    let at = (mid + vec2(8.0, 8.0), egui::Align2::LEFT_TOP);
    crate::canvas::value_box_in(ui, ids, "Distance", &mut text, at, take, false, crate::canvas::box_place(app, proj));
    let typed = app.session.doc.eval(&text, ValueKind::Length).ok().filter(|d| d.is_finite() && *d > 0.0);
    if focused || take {
        // Only a change: the shown value is rounded, and must not round the card's Distance.
        if let Some(d) = typed.filter(|_| text != shown) {
            panel.distance = d.clamp(0.001, 1e6);
        }
        panel.distance_text = Some(text.clone());
    } else {
        panel.distance_text = None;
    }
    if enter {
        if typed.is_none() {
            app.tree.canvas_panel = Some(panel);
            return app.set_status(format!("{}: {text}", crate::i18n::tr_label("Distance")), true);
        }
        return apply(app, &panel);
    }
    app.tree.canvas_panel = Some(panel);
}

fn on_image(c: &Canvas, p: Vec2) -> bool {
    let [bl, br, _, tl] = c.corners();
    let (u, v) = (br - bl, tl - bl);
    let (uu, vv) = (u.dot(u), v.dot(v));
    if !(uu > 1e-18 && vv > 1e-18) {
        return true;
    }
    let w = p - bl;
    let (s, t) = (w.dot(u) / uu, w.dot(v) / vv);
    (-0.05..=1.05).contains(&s) && (-0.05..=1.05).contains(&t)
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

    /// Viewport clicks on the image fill Point A then Point B (#9108).
    #[test]
    fn calibrate_picks_two_points_from_viewport_clicks() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, true));
        h.frames(6);
        let rect = h.app.viewport.rect.expect("viewport");
        let proj = crate::viewport::projection(&h.app, rect);
        let c = h.app.session.doc.canvases[0].clone();
        let (vals, _) = h.app.session.doc.param_values();
        let plane = h.app.session.doc.canvas_plane(&vals, &c).unwrap();
        let ax = c.center.x - 20.0;
        let bx = c.center.x + 20.0;
        let sa = proj.to_screen(plane.to_world(solidcraft_engine::geom::Vec2::new(ax, c.center.y))).expect("point A on screen");
        let sb = proj.to_screen(plane.to_world(solidcraft_engine::geom::Vec2::new(bx, c.center.y))).expect("point B on screen");
        assert!(super::click(&mut h.app, &proj, sa), "first click sets A");
        let panel = h.app.tree.canvas_panel.as_ref().unwrap();
        assert_eq!(panel.next, 1);
        assert!((panel.a[0] - ax).abs() < 0.5, "{:?}", panel.a);
        assert!(super::click(&mut h.app, &proj, sb), "second click sets B");
        let panel = h.app.tree.canvas_panel.as_ref().unwrap();
        assert_eq!(panel.next, 2, "both points picked: the distance is next");
        assert!((panel.b[0] - bx).abs() < 0.5, "{:?}", panel.b);
        let status = h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert_eq!(status, crate::i18n::tr("browser.calibrate_type_distance"), "the status line asks for the distance");
        let before = panel.a;
        let miss = plane.to_world(solidcraft_engine::geom::Vec2::new(c.center.x + 500.0, c.center.y));
        if let Some(sm) = proj.to_screen(miss) {
            assert!(!super::click(&mut h.app, &proj, sm));
            assert_eq!(h.app.tree.canvas_panel.as_ref().unwrap().a, before);
        }
    }

    /// Once both points are clicked, a line joins them with a Distance box in the view: it holds
    /// the measured distance, as the card does, and the real one typed there with Enter
    /// calibrates, as one undo step.
    #[test]
    fn calibrate_takes_the_distance_typed_in_the_view() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.call("ui.view", json!({"view": "top"}));
        h.frames(3);
        let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Canvas { id });
        let item = items.iter().find(|i| i.id == "ui.calibrateCanvas").unwrap().clone();
        crate::context_menu::run_item(&mut h.app, &item, egui::pos2(0.0, 0.0));
        h.frames(3);
        let c = h.app.session.doc.canvases[0].clone();
        let plane = h.app.session.doc.canvas_plane(&h.app.session.doc.param_values().0, &c).unwrap();
        for x in [-20.0, 20.0] {
            let p = plane.to_world(solidcraft_engine::geom::Vec2::new(c.center.x + x, c.center.y));
            let at = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap()).to_screen(p).unwrap();
            h.call("ui.click", json!({"x": at.x, "y": at.y}));
            h.frames(2);
        }
        let panel = h.app.tree.canvas_panel.clone().expect("still calibrating");
        assert_eq!(panel.next, 2);
        assert!((panel.distance - 40.0).abs() < 1e-3, "the card holds the measured distance: {}", panel.distance);
        h.frames(2);
        let shown = h.app.tree.canvas_panel.as_ref().unwrap().distance_text.clone();
        assert!(shown.as_deref().is_some_and(|t| t.starts_with("40")), "the box in the view has the keyboard and the distance: {shown:?}");
        let undo = h.app.session.undo.len();
        h.step(&json!({"text": "80"})).unwrap();
        h.frames(1);
        assert!((h.app.tree.canvas_panel.as_ref().unwrap().distance - 80.0).abs() < 1e-9, "typing in the view updates the card");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none(), "Enter calibrates and closes the card: {:?}", h.app.status);
        let width = h.app.session.doc.canvases[0].width;
        assert!((width - 160.0).abs() < 1e-6, "40 mm on the image are really 80: {width}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step, not two");
    }
}
