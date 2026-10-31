//! The Edit Canvas / Calibrate card the Browser opens for a reference image: position, size,
//! angle, opacity and flip (live, one undo step per opening), or two points on the image and
//! their true distance. Calibrate accepts clicks in the viewport on the image (#9108).

use egui::{Pos2, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::doc::canvas::Canvas;
use solidcraft_engine::geom::{Plane, Vec2};

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
                ui_kit::text(
                    ui,
                    crate::i18n::tr(if m.next == 0 { "browser.calibrate_click_a" } else { "browser.calibrate_click_b" }),
                    ui_kit::Text::Note("calibrate_hint"),
                );
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
        if app.run("canvas.calibrate", json!({ "canvas": c.id, "a": m.a, "b": m.b, "distance": m.distance })).is_ok() {
            app.tree.canvas_panel = None;
        }
        return;
    }
    app.tree.canvas_panel = if closed || action == ui_kit::Action::Cancel { None } else { Some(m) };
}

/// Screen positions of the direct Edit Canvas grips: centre/move, four corner/scale grips,
/// rotation above the top edge, opacity along the bottom edge, and both flip buttons.
fn edit_handles(c: &Canvas, plane: &Plane, proj: &Proj) -> Vec<(u8, Pos2)> {
    let [bl, br, tr, tl] = c.corners();
    let top = (tl + tr) * 0.5;
    let outward = top - c.center;
    let rotation = top + outward * 0.25;
    let along = (tr - tl) * 0.12;
    let flip_h = rotation - along;
    let flip_v = rotation + along;
    let opacity = bl + (br - bl) * c.opacity.clamp(0.0, 1.0);
    [
        (0, c.center), (1, bl), (1, br), (1, tr), (1, tl),
        (2, rotation), (3, opacity), (4, flip_h), (5, flip_v),
    ]
    .into_iter()
    .filter_map(|(kind, point)| proj.to_screen(plane.to_world(point)).map(|screen| (kind, screen)))
    .collect()
}

/// Whether a calibration marker or Edit Canvas grip owns the pointer.
pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, screen: Pos2) -> bool {
    let Some(panel) = app.tree.canvas_panel.as_ref() else { return false };
    if panel.dragging.is_some() || panel.edit_drag.is_some() { return true; }
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas) else { return false };
    let (vals, _) = doc.param_values();
    let Ok(plane) = doc.canvas_plane(&vals, c) else { return false };
    if panel.calibrate {
        [panel.a, panel.b].into_iter().any(|point| {
            proj.to_screen(plane.to_world(Vec2::new(point[0], point[1])))
                .is_some_and(|p| p.distance(screen) <= 12.0)
        })
    } else {
        edit_handles(c, &plane, proj).into_iter().any(|(_, p)| p.distance(screen) <= 12.0)
    }
}

/// Start or continue dragging a calibration marker or direct-edit grip.
pub(crate) fn drag(app: &mut SolveApp, proj: &Proj, screen: Option<Pos2>, pressed: bool, down: bool) -> bool {
    let Some(mut panel) = app.tree.canvas_panel.clone() else { return false };
    if !down {
        let owned = panel.dragging.take().is_some() || panel.edit_drag.take().is_some();
        app.tree.canvas_panel = Some(panel);
        return owned;
    }
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas).cloned() else { return false };
    let (vals, _) = doc.param_values();
    let Ok(plane) = doc.canvas_plane(&vals, &c) else { return false };
    let Some(screen) = screen else { return panel.dragging.is_some() || panel.edit_drag.is_some() };

    if panel.calibrate {
        if pressed && panel.dragging.is_none() {
            panel.dragging = [panel.a, panel.b].into_iter().enumerate().filter_map(|(i, point)| {
                let marker = proj.to_screen(plane.to_world(Vec2::new(point[0], point[1])))?;
                Some((marker.distance(screen), i as u8))
            }).filter(|(distance, _)| *distance <= 12.0)
              .min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, i)| i);
        }
        let Some(index) = panel.dragging else { return false };
        let (origin, direction) = proj.ray(screen);
        if let Some(world) = plane.intersect_ray(origin, direction) {
            let local = plane.to_local(world);
            if on_image(&c, local) {
                let point = [local.x, local.y];
                if index == 0 { panel.a = point; } else { panel.b = point; }
                panel.next = 1 - index;
            }
        }
        app.tree.canvas_panel = Some(panel);
        return true;
    }

    let (origin, direction) = proj.ray(screen);
    let Some(world) = plane.intersect_ray(origin, direction) else { return panel.edit_drag.is_some() };
    let local = plane.to_local(world);
    if pressed && panel.edit_drag.is_none() {
        let kind = edit_handles(&c, &plane, proj).into_iter()
            .filter_map(|(kind, marker)| (marker.distance(screen) <= 12.0).then_some((marker.distance(screen), kind)))
            .min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, kind)| kind);
        if let Some(kind) = kind {
            if kind == 4 || kind == 5 {
                let mut params = json!({"canvas": c.id, "flip": !c.flip});
                if kind == 5 { params["angle"] = json!(c.angle.to_degrees() + 180.0); }
                app.tree.canvas_panel = Some(panel);
                let _ = app.run("canvas.edit", params);
                return true;
            }
            panel.edit_drag = Some(crate::browser::CanvasEditDrag {
                kind, pointer: [local.x, local.y], center: [c.center.x, c.center.y],
                width: c.width, angle: c.angle,
            });
        }
    }
    let Some(drag) = panel.edit_drag.as_ref() else { return false };
    let start = Vec2::new(drag.pointer[0], drag.pointer[1]);
    let centre = Vec2::new(drag.center[0], drag.center[1]);
    let mut params = json!({"canvas": c.id});
    match drag.kind {
        0 => { let next = centre + (local - start); params["center"] = json!([next.x, next.y]); }
        1 => {
            let before = start.dist(centre); let after = local.dist(centre);
            if before > 1e-9 && after.is_finite() { params["width"] = json!((drag.width * after / before).clamp(1e-6, 1e7)); }
        }
        2 => {
            let before = (start.y - centre.y).atan2(start.x - centre.x);
            let after = (local.y - centre.y).atan2(local.x - centre.x);
            params["angle"] = json!((drag.angle + after - before).to_degrees());
        }
        3 => {
            let (sin, cos) = drag.angle.sin_cos();
            let along = (local.x - centre.x) * cos + (local.y - centre.y) * sin;
            params["opacity"] = json!((along / drag.width + 0.5).clamp(0.0, 1.0));
        }
        _ => return false,
    }
    app.tree.canvas_panel = Some(panel);
    let _ = app.run("canvas.edit", params);
    true
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
    if panel.next == 0 {
        panel.a = point;
        panel.next = 1;
        app.set_status(crate::i18n::tr("browser.calibrate_click_b"), false);
    } else {
        panel.b = point;
        panel.next = 0;
        app.set_status(crate::i18n::tr("browser.calibrate_click_a"), false);
    }
    app.tree.canvas_panel = Some(panel);
    true
}

/// Markers for the two calibration points on the image.
pub(crate) fn paint_marks(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(panel) = app.tree.canvas_panel.as_ref() else { return };
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas) else { return };
    let (vals, _) = doc.param_values();
    let Ok(plane) = doc.canvas_plane(&vals, c) else { return };
    let t = Tokens::get();
    if !panel.calibrate {
        let screens = c.corners().map(|point| proj.to_screen(plane.to_world(point)));
        if let [Some(bl), Some(br), Some(tr), Some(tl)] = screens {
            let outline = Stroke::new(1.5, t.sel_edge);
            painter.line_segment([bl, br], outline); painter.line_segment([br, tr], outline);
            painter.line_segment([tr, tl], outline); painter.line_segment([tl, bl], outline);
            for point in [bl, br, tr, tl] {
                painter.rect_filled(egui::Rect::from_center_size(point, vec2(9.0, 9.0)), 1.0, t.manipulator);
            }
        }
        for (kind, point) in edit_handles(c, &plane, proj) {
            match kind {
                0 => {
                    painter.circle_stroke(point, 8.0, Stroke::new(2.0, t.manipulator));
                    painter.line_segment([point - vec2(6.0, 0.0), point + vec2(6.0, 0.0)], Stroke::new(1.5, t.manipulator));
                    painter.line_segment([point - vec2(0.0, 6.0), point + vec2(0.0, 6.0)], Stroke::new(1.5, t.manipulator));
                }
                2 => {
                    painter.circle_stroke(point, 7.0, Stroke::new(2.0, t.accent));
                    painter.text(point + vec2(10.0, -8.0), egui::Align2::LEFT_BOTTOM, "↻", egui::FontId::proportional(t.font_body), t.accent);
                }
                3 => {
                    painter.circle_filled(point, 6.0, t.accent);
                    painter.text(point + vec2(9.0, 10.0), egui::Align2::LEFT_TOP, format!("{:.0}%", c.opacity * 100.0), egui::FontId::proportional(t.font_body), t.text);
                }
                4 | 5 => {
                    painter.circle_filled(point, 9.0, t.manipulator);
                    painter.circle_stroke(point, 9.0, Stroke::new(1.5, t.text));
                    painter.text(point, egui::Align2::CENTER_CENTER, if kind == 4 { "↔" } else { "↕" }, egui::FontId::proportional(t.font_body), t.text);
                }
                _ => {}
            }
        }
        return;
    }
    let screens = [panel.a, panel.b].map(|point| proj.to_screen(plane.to_world(Vec2::new(point[0], point[1]))));
    if let [Some(a), Some(b)] = screens {
        painter.line_segment([a, b], Stroke::new(1.5, t.sel_edge));
        let measured = Vec2::new(panel.a[0], panel.a[1]).dist(Vec2::new(panel.b[0], panel.b[1]));
        if measured.is_finite() && measured > 1e-9 {
            painter.text(a + (b - a) * 0.5 + vec2(0.0, -10.0), egui::Align2::CENTER_BOTTOM,
                crate::canvas::format_len(doc, measured), egui::FontId::proportional(t.font_body), t.text);
        }
    }
    for (i, screen) in screens.into_iter().enumerate() {
        let Some(screen) = screen else { continue };
        let active = panel.next as usize == i;
        painter.circle_stroke(screen, 7.0, Stroke::new(if active { 2.0 } else { 1.5 }, if active { t.accent } else { t.sel_edge }));
        painter.circle_filled(screen, 2.5, if active { t.accent } else { t.sel_vertex });
        painter.text(screen + vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, if i == 0 { "A" } else { "B" }, egui::FontId::proportional(t.font_body), if active { t.accent } else { t.text });
    }
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
        assert_eq!(panel.next, 0);
        assert!((panel.b[0] - bx).abs() < 0.5, "{:?}", panel.b);
        let before = panel.a;
        let miss = plane.to_world(solidcraft_engine::geom::Vec2::new(c.center.x + 500.0, c.center.y));
        if let Some(sm) = proj.to_screen(miss) {
            assert!(!super::click(&mut h.app, &proj, sm));
            assert_eq!(h.app.tree.canvas_panel.as_ref().unwrap().a, before);
        }
    }
}
