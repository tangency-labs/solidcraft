//! The Edit Canvas / Calibrate card the Browser opens for a reference image: position, size,
//! angle, opacity and flip (live, one undo step per opening), or two points on the image and
//! their true distance. Calibrate accepts clicks in the viewport on the image (#9108). Straighten
//! takes two points along an edge of the image the same way and turns the image so that edge is
//! horizontal, vertical or at a typed angle (#11989).

use egui::{CursorIcon, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::browser::CanvasPointDrag;
use crate::ui_kit;
use crate::viewport::Proj;
use solidcraft_theme::Tokens;

/// How near the pointer must be to Point A or Point B to hover or grab it (screen points, #23210).
const GRAB_PX: f32 = 10.0;

pub(crate) fn panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.tree.canvas_panel.clone() else { return };
    let Some(c) = app.session.doc.canvases.iter().find(|c| c.id == m.canvas).cloned() else {
        app.tree.canvas_panel = None;
        return;
    };
    let mut edit: Option<Value> = None;
    // "Follow a frame in the sketch" clicked: Some(true) ties the image to a construction
    // rectangle in a sketch (Frame Sketch), Some(false) sets it free again (#18405).
    let mut follow: Option<bool> = None;
    let (units, decimals) = (app.session.doc.units.clone(), app.preferences.length_decimals);
    let title_id = if m.trace {
        "browser.trace_title"
    } else if m.straighten {
        "browser.straighten_title"
    } else if m.calibrate {
        "browser.calibrate_title"
    } else {
        "browser.edit_canvas_title"
    };
    let title = solidcraft_ui_text::format(title_id, &[("name", &c.name)]);
    let (pivot, at) = crate::shell::floating_editor_anchor(app, ctx);
    let (picks, straighten, name) = (m.picks_points(), m.straighten, c.id);
    let (ok, cancel) = if m.trace {
        (solidcraft_ui_text::tr("ui.trace"), solidcraft_ui_text::tr("ui.cancel"))
    } else if straighten {
        (solidcraft_ui_text::tr("ui.straighten"), solidcraft_ui_text::tr("ui.cancel"))
    } else if picks {
        (solidcraft_ui_text::tr_label("Calibrate"), solidcraft_ui_text::tr("ui.cancel"))
    } else {
        ("", solidcraft_ui_text::tr("ui.close"))
    };
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_canvas_panel"), "canvas", &title)
        .placed(pivot, at)
        .fields()
        .actions(ok, cancel, None)
        .show(ctx, true, |ui| {
            if m.trace {
                ui_kit::text(ui, &hint_text(&m), ui_kit::Text::Note("trace_hint"));
                ui_kit::row(ui, solidcraft_ui_text::tr("browser.trace_tolerance"), |ui| {
                    ui_kit::slider(ui, &mut m.tolerance, 0.0..=100.0, Default::default())
                });
                if !m.tolerance.is_finite() {
                    m.tolerance = 20.0;
                }
                m.tolerance = m.tolerance.clamp(0.0, 100.0);
            } else if picks {
                ui_kit::text(ui, solidcraft_ui_text::tr(hint(&m)), ui_kit::Text::Note("calibrate_hint"));
                for (label, p) in [("Point A", &mut m.a), ("Point B", &mut m.b)] {
                    ui_kit::row(ui, solidcraft_ui_text::tr_label(label), |ui| {
                        let [x, y] = p;
                        ui.horizontal(|ui| {
                            ui_kit::length_field(ui, x, &units, decimals);
                            ui_kit::length_field(ui, y, &units, decimals);
                        });
                    });
                }
                if straighten {
                    let options = [
                        solidcraft_ui_text::tr("browser.straighten_horizontal"),
                        solidcraft_ui_text::tr("browser.straighten_vertical"),
                        solidcraft_ui_text::tr("ui.angle"),
                    ];
                    let targets = ["straighten:horizontal", "straighten:vertical", "straighten:angle"];
                    ui_kit::row(ui, solidcraft_ui_text::tr("browser.straighten_angle"), |ui| {
                        ui_kit::segmented(ui, &mut m.direction, &options, ui_kit::Segments { targets: &targets, ..Default::default() })
                    });
                    if m.direction == 2 {
                        ui_kit::row(ui, solidcraft_ui_text::tr("ui.angle"), |ui| ui_kit::angle_field(ui, &mut m.angle));
                        if !m.angle.is_finite() {
                            m.angle = 0.0;
                        }
                        m.angle = m.angle.clamp(-360.0, 360.0);
                    }
                } else {
                    ui_kit::row(ui, solidcraft_ui_text::tr_label("Distance"), |ui| ui_kit::length_field(ui, &mut m.distance, &units, decimals));
                    m.distance = m.distance.clamp(0.001, 1e6);
                }
            } else if c.anchor.is_some() {
                if ui_kit::checkbox_row(ui, "canvas_follow_frame", solidcraft_ui_text::tr("browser.canvas_follow_frame"), true) {
                    follow = Some(false);
                }
                // Its sketch's points place, turn and size it: only its look is edited here.
                ui_kit::text(ui, solidcraft_ui_text::tr("browser.canvas_anchored"), ui_kit::Text::Note("canvas_anchored"));
                let (mut op, mut flip) = (c.opacity, c.flip);
                let mut changed =
                    ui_kit::row(ui, solidcraft_ui_text::tr_label("Opacity"), |ui| ui_kit::slider(ui, &mut op, 0.0..=1.0, Default::default()))
                        .changed();
                if ui_kit::checkbox_row(ui, "flip", solidcraft_ui_text::tr_label("Flip"), flip) {
                    flip = !flip;
                    changed = true;
                }
                if changed {
                    edit = Some(json!({ "canvas": name, "opacity": op, "flip": flip }));
                }
            } else {
                if ui_kit::checkbox_row(ui, "canvas_follow_frame", solidcraft_ui_text::tr("browser.canvas_follow_frame"), false) {
                    follow = Some(true);
                }
                let (mut x, mut y, mut w, mut ang, mut op, mut flip) = (c.center.x, c.center.y, c.width, c.angle.to_degrees(), c.opacity, c.flip);
                let mut changed = false;
                changed |=
                    ui_kit::row(ui, solidcraft_ui_text::tr_label("Center X"), |ui| ui_kit::length_field(ui, &mut x, &units, decimals)).changed();
                changed |=
                    ui_kit::row(ui, solidcraft_ui_text::tr_label("Center Y"), |ui| ui_kit::length_field(ui, &mut y, &units, decimals)).changed();
                changed |= ui_kit::row(ui, solidcraft_ui_text::tr_label("Width"), |ui| ui_kit::length_field(ui, &mut w, &units, decimals)).changed();
                changed |= ui_kit::row(ui, solidcraft_ui_text::tr_label("Angle"), |ui| ui_kit::angle_field(ui, &mut ang)).changed();
                changed |= ui_kit::row(ui, solidcraft_ui_text::tr_label("Opacity"), |ui| ui_kit::slider(ui, &mut op, 0.0..=1.0, Default::default()))
                    .changed();
                if ui_kit::checkbox_row(ui, "flip", solidcraft_ui_text::tr_label("Flip"), flip) {
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
    match follow {
        Some(true) => {
            if let Err(e) = app.run("canvas.frame_sketch", json!({ "canvas": c.id })) {
                app.set_status(e, true);
            }
        }
        Some(false) => {
            if let Err(e) = app.run("canvas.edit", json!({ "canvas": c.id, "anchor": null })) {
                app.set_status(e, true);
            }
        }
        None => {}
    }
    if action == ui_kit::Action::Ok && m.trace {
        if !m.picked_both {
            app.set_status(solidcraft_ui_text::tr("browser.trace_click"), false);
            app.tree.canvas_panel = Some(m);
            return;
        }
        match app.run("canvas.to_sketch", json!({ "canvas": c.id, "at": m.points, "tolerance": m.tolerance })) {
            Ok(_) => app.tree.canvas_panel = None,
            Err(e) => {
                app.set_status(e, true);
                app.tree.canvas_panel = Some(m);
            }
        }
        return;
    }
    if action == ui_kit::Action::Ok {
        let (id, p) = if m.straighten {
            ("canvas.straighten", json!({ "canvas": c.id, "a": m.a, "b": m.b, "angle": m.target_angle() }))
        } else {
            ("canvas.calibrate", json!({ "canvas": c.id, "a": m.a, "b": m.b, "distance": m.distance }))
        };
        if app.run(id, p).is_ok() {
            app.tree.canvas_panel = None;
        }
        return;
    }
    app.tree.canvas_panel = if closed || action == ui_kit::Action::Cancel { None } else { Some(m) };
}

/// The most regions one trace takes: the most points `canvas.to_sketch` accepts in `at`.
const MAX_TRACE_POINTS: usize = 64;

/// The card's hint as shown: the message of `hint`, with the number of shapes picked in Trace Region.
pub(crate) fn hint_text(panel: &crate::browser::CanvasPanel) -> String {
    if panel.trace && panel.picked_both {
        return solidcraft_ui_text::format(hint(panel), &[("count", &panel.points.len().to_string())]);
    }
    solidcraft_ui_text::tr(hint(panel)).to_string()
}

/// The message id of the card's hint (and the status line) for what the user does next.
pub(crate) fn hint(panel: &crate::browser::CanvasPanel) -> &'static str {
    if panel.trace {
        return if panel.picked_both { "browser.trace_picked" } else { "browser.trace_click" };
    }
    match (panel.straighten, panel.next, panel.picked_both) {
        (true, 0, true) => "browser.straighten_choose",
        (true, 0, false) => "browser.straighten_click_a",
        (true, _, _) => "browser.straighten_click_b",
        (false, 0, _) => "browser.calibrate_click_a",
        (false, _, _) => "browser.calibrate_click_b",
    }
}

/// A viewport click while Calibrate is open: set Point A or B on the image (#9108).
pub(crate) fn click(app: &mut SolveApp, proj: &Proj, screen: Pos2, add: bool) -> bool {
    let Some(mut panel) = app.tree.canvas_panel.clone() else {
        return false;
    };
    if !panel.picks_points() && !panel.trace {
        return false;
    }
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas).cloned() else {
        return false;
    };
    let (vals, _) = doc.param_values();
    let state = app.session.model.state();
    // An anchored canvas shows, and is traced, in its anchor sketch's plane through the anchor's
    // frame: a trace click is read there, as `canvas.to_sketch` reads its point.
    let anchored = c.anchor.as_ref().filter(|_| panel.trace).and_then(|a| {
        let frame = doc.canvas_anchor_frame(&c, a, &state).ok()?;
        Some((state.sketch(a.sketch)?.plane, frame))
    });
    let (plane, corners) = match anchored {
        Some(found) => found,
        None => {
            let Ok(plane) = doc.canvas_plane(&vals, &c, &state) else {
                return false;
            };
            (plane, c.corners())
        }
    };
    let (origin, direction) = proj.ray(screen);
    let Some(world) = plane.intersect_ray(origin, direction) else {
        return false;
    };
    let local = plane.to_local(world);
    if !on_image(corners, local) {
        return false;
    }
    let point = [local.x, local.y];
    if panel.trace {
        // Ctrl+click adds one more region (up to what `canvas.to_sketch` takes); a plain click picks
        // that one alone.
        if add && panel.picked_both && panel.points.len() >= MAX_TRACE_POINTS {
            app.set_status(solidcraft_ui_text::format("browser.trace_limit", &[("max", &MAX_TRACE_POINTS.to_string())]), true);
            return true;
        }
        if !add || !panel.picked_both {
            panel.points.clear();
        }
        panel.points.push(point);
        panel.a = point;
        panel.picked_both = true;
    } else if panel.next == 0 {
        panel.a = point;
        panel.next = 1;
        panel.picked_both = false;
    } else {
        panel.b = point;
        panel.next = 0;
        panel.picked_both = true;
    }
    app.set_status(hint_text(&panel), false);
    app.tree.canvas_panel = Some(panel);
    true
}

fn canvas_plane(app: &SolveApp, panel: &crate::browser::CanvasPanel) -> Option<Plane> {
    let doc = &app.session.doc;
    let c = doc.canvases.iter().find(|c| c.id == panel.canvas)?;
    let (vals, _) = doc.param_values();
    doc.canvas_plane(&vals, c, &app.session.model.state()).ok()
}

fn on_plane(plane: &Plane, proj: &Proj, screen: Pos2) -> Option<Vec2> {
    let (origin, direction) = proj.ray(screen);
    let world = plane.intersect_ray(origin, direction)?;
    let local = plane.to_local(world);
    local.is_finite().then_some(local)
}

fn hit_point_in(panel: &crate::browser::CanvasPanel, plane: &Plane, proj: &Proj, screen: Pos2) -> Option<u8> {
    let mut best: Option<(u8, f32)> = None;
    for (i, point) in [panel.a, panel.b].into_iter().enumerate() {
        let Some(at) = proj.to_screen(plane.to_world(Vec2::new(point[0], point[1]))) else {
            continue;
        };
        let d = at.distance(screen);
        if d > GRAB_PX {
            continue;
        }
        let idx = i as u8;
        let better = match best {
            None => true,
            Some((prev, prev_d)) => d + 1e-3 < prev_d || ((d - prev_d).abs() <= 1e-3 && idx == panel.next && prev != panel.next),
        };
        if better {
            best = Some((idx, d));
        }
    }
    best.map(|(idx, _)| idx)
}

/// Which calibration/straightening point (`0` = A, `1` = B) is under `screen`, if any (#23210).
pub(crate) fn hit_point(app: &SolveApp, proj: &Proj, screen: Pos2) -> Option<u8> {
    let panel = app.tree.canvas_panel.as_ref().filter(|p| p.picks_points())?;
    let plane = canvas_plane(app, panel)?;
    hit_point_in(panel, &plane, proj, screen)
}

/// Whether Point A or Point B is currently being dragged in the viewport (#23210).
pub(crate) fn dragging(app: &SolveApp) -> bool {
    app.tree.canvas_panel.as_ref().is_some_and(|p| p.picks_points() && p.drag.is_some())
}

/// Whether the pointer is over a draggable calibration/straightening point or actively dragging one.
pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, screen: Pos2) -> bool {
    dragging(app) || hit_point(app, proj, screen).is_some()
}

/// Begin dragging Point A or Point B when a primary drag starts on its marker (#23210).
pub(crate) fn drag_start(app: &mut SolveApp, proj: &Proj, press: Pos2) -> bool {
    let Some(mut panel) = app.tree.canvas_panel.clone() else {
        return false;
    };
    if !panel.picks_points() {
        return false;
    }
    let Some(plane) = canvas_plane(app, &panel) else {
        return false;
    };
    let Some(point) = hit_point_in(&panel, &plane, proj, press) else {
        return false;
    };
    let Some(at) = on_plane(&plane, proj, press) else {
        return false;
    };
    let origin = if point == 0 { panel.a } else { panel.b };
    panel.drag = Some(CanvasPointDrag { point, offset: [at.x - origin[0], at.y - origin[1]] });
    app.tree.canvas_panel = Some(panel);
    true
}

/// Move the actively dragged Point A or Point B smoothly on the canvas plane (#23210).
pub(crate) fn drag_to(app: &mut SolveApp, proj: &Proj, screen: Pos2) -> bool {
    let Some(mut panel) = app.tree.canvas_panel.clone() else {
        return false;
    };
    if !panel.picks_points() {
        return false;
    }
    let Some(drag) = panel.drag else {
        return false;
    };
    let Some(plane) = canvas_plane(app, &panel) else {
        return false;
    };
    let Some(at) = on_plane(&plane, proj, screen) else {
        return false;
    };
    let next = [at.x - drag.offset[0], at.y - drag.offset[1]];
    if !next[0].is_finite() || !next[1].is_finite() {
        return false;
    }
    if drag.point == 0 {
        panel.a = next;
    } else {
        panel.b = next;
    }
    app.tree.canvas_panel = Some(panel);
    true
}

/// Finish dragging Point A or Point B, committing its current coordinates (#23210).
pub(crate) fn drag_end(app: &mut SolveApp) -> bool {
    let Some(panel) = app.tree.canvas_panel.as_mut() else {
        return false;
    };
    panel.drag.take().is_some()
}

/// Formatted live distance between Point A and Point B in document units.
pub(crate) fn distance_preview_label(app: &SolveApp, panel: &crate::browser::CanvasPanel) -> String {
    solidcraft_engine::doc::display_units::format_length(panel.measured_distance(), &app.session.doc.units, app.preferences.length_decimals)
}

/// Markers for the two calibration or straightening points on the image. Calibrate shows the
/// A–B line and live measured distance preview; Straighten draws the line A–B (following the
/// pointer while waiting for Point B, #11989). Hovering or dragging A or B shows a grab cue (#23210).
pub(crate) fn paint_marks(app: &SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj, hover: Option<Pos2>) {
    let Some(panel) = app.tree.canvas_panel.as_ref().filter(|p| p.picks_points() || p.trace) else {
        return;
    };
    let Some(plane) = canvas_plane(app, panel) else {
        return;
    };
    let t = Tokens::get();
    if panel.trace {
        // A ring on every region picked, the last one the accent.
        for (i, point) in panel.points.iter().enumerate() {
            let Some(screen) = proj.to_screen(plane.to_world(Vec2::new(point[0], point[1]))) else {
                continue;
            };
            let last = i + 1 == panel.points.len();
            let stroke = Stroke::new(if last { 2.0 } else { 1.5 }, if last { t.accent } else { t.sel_edge });
            painter.circle_stroke(screen, 7.0, stroke);
            painter.circle_filled(screen, 2.5, if last { t.accent } else { t.sel_vertex });
        }
        return;
    }
    let dragged = panel.drag.map(|d| d.point);
    let hovered = if dragged.is_none() { hover.and_then(|p| hit_point_in(panel, &plane, proj, p)) } else { None };
    if dragged.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if hovered.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    let sa = proj.to_screen(plane.to_world(Vec2::new(panel.a[0], panel.a[1])));
    let sb = proj.to_screen(plane.to_world(Vec2::new(panel.b[0], panel.b[1])));
    if panel.straighten {
        let b = if panel.next == 1 && dragged.is_none() {
            // The pointer, when it is over the view.
            painter.ctx().pointer_hover_pos().filter(|p| painter.clip_rect().contains(*p))
        } else {
            sb
        };
        if let (Some(a), Some(b)) = (sa, b) {
            painter.line_segment([a, b], Stroke::new(1.5, t.accent));
        }
    } else if panel.calibrate
        && let (Some(a), Some(b)) = (sa, sb)
    {
        painter.line_segment([a, b], Stroke::new(1.5, t.accent));
        let label = distance_preview_label(app, panel);
        let mid = pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let dir = b - a;
        let perp = if dir.length() > 1e-3 {
            let n = vec2(-dir.y, dir.x).normalized();
            if n.y > 0.0 { -n } else { n }
        } else {
            vec2(0.0, -1.0)
        };
        let pill_center = mid + perp * 16.0;
        let text_color = if dragged.is_some() { t.accent } else { t.text };
        let galley = painter.layout_no_wrap(label, egui::FontId::proportional(t.font_body), text_color);
        let pill = Rect::from_center_size(pill_center, galley.size() + vec2(10.0, 4.0));
        painter.rect_filled(pill, 3.0, t.panel.gamma_multiply(0.92));
        painter.rect_stroke(pill, 3.0, Stroke::new(1.0, if dragged.is_some() { t.accent } else { t.sel_edge }), StrokeKind::Middle);
        painter.galley(pill.center() - galley.size() * 0.5, galley, text_color);
    }
    for (i, screen) in [sa, sb].into_iter().enumerate() {
        let Some(screen) = screen else {
            continue;
        };
        let idx = i as u8;
        let is_dragged = dragged == Some(idx);
        let is_hovered = hovered == Some(idx);
        let interactive = is_dragged || is_hovered;
        let active = panel.next == idx;
        if interactive {
            let r = if is_dragged { 9.5 } else { 9.0 };
            painter.circle_filled(screen, r, t.accent.gamma_multiply(if is_dragged { 0.28 } else { 0.18 }));
            painter.circle_stroke(screen, r, Stroke::new(2.5, t.accent));
            painter.circle_filled(screen, 3.5, t.accent);
        } else {
            let stroke = Stroke::new(if active { 2.0 } else { 1.5 }, if active { t.accent } else { t.sel_edge });
            painter.circle_stroke(screen, 7.0, stroke);
            painter.circle_filled(screen, 2.5, if active { t.accent } else { t.sel_vertex });
        }
        let label = if i == 0 { "A" } else { "B" };
        painter.text(
            screen + vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(t.font_body),
            if interactive || active { t.accent } else { t.text },
        );
        crate::scenario::publish_handle(if i == 0 { "canvas_point_a" } else { "canvas_point_b" }, screen);
    }
}

fn on_image([bl, br, _, tl]: [Vec2; 4], p: Vec2) -> bool {
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
    use solidcraft_engine::geom::prelude::*;

    fn canvas(h: &mut crate::scenario::Harness) -> u64 {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(400u32.to_be_bytes());
        png.extend(200u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let data = crate::scenario::image_file(&png);
        h.app.run("canvas.insert", json!({"path": data, "plane": "XY", "width": 80})).unwrap()["canvas"].as_u64().unwrap()
    }

    #[test]
    fn trace_region_asks_for_a_click_then_the_tolerance() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        let id = canvas(&mut h);
        let mut panel = crate::browser::CanvasPanel::trace(&h.app, id);
        assert!(panel.trace && !panel.picks_points() && panel.tolerance == 20.0, "{panel:?}");
        assert_eq!(super::hint(&panel), "browser.trace_click");
        panel.picked_both = true;
        assert_eq!(super::hint(&panel), "browser.trace_picked");
        // Several regions picked (Ctrl+click): the hint counts them, Trace sends them all.
        panel.points = vec![[1.0, 2.0], [3.0, 4.0]];
        assert!(super::hint_text(&panel).contains("2"), "{}", super::hint_text(&panel));
    }

    /// Trace Region: a click picks one region, Ctrl+click adds another, a plain click starts over (#20798).
    #[test]
    fn trace_region_ctrl_click_adds_a_region() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::trace(&h.app, id));
        h.frames(6);
        let rect = h.app.viewport.rect.expect("viewport");
        let proj = crate::viewport::projection(&h.app, rect);
        let c = h.app.session.doc.canvases[0].clone();
        let (vals, _) = h.app.session.doc.param_values();
        let plane = h.app.session.doc.canvas_plane(&vals, &c, &h.app.session.model.state()).unwrap();
        let screen = |x: f64, y: f64| proj.to_screen(plane.to_world(solidcore_record::Vec2::new(x, y))).expect("on screen");
        let points = |h: &crate::scenario::Harness| h.app.tree.canvas_panel.as_ref().unwrap().points.clone();
        let (a, b, d) = ((c.center.x - 20.0, c.center.y), (c.center.x + 20.0, c.center.y), (c.center.x, c.center.y + 10.0));
        assert!(super::click(&mut h.app, &proj, screen(a.0, a.1), false));
        assert_eq!(points(&h).len(), 1);
        // Ctrl+click adds, in the order picked; the card's hint counts them.
        assert!(super::click(&mut h.app, &proj, screen(b.0, b.1), true));
        assert!(super::click(&mut h.app, &proj, screen(d.0, d.1), true));
        let picked = points(&h);
        assert_eq!(picked.len(), 3, "{picked:?}");
        assert!((picked[0][0] - a.0).abs() < 1e-3 && (picked[1][0] - b.0).abs() < 1e-3 && (picked[2][1] - d.1).abs() < 1e-3, "{picked:?}");
        assert!(super::hint_text(h.app.tree.canvas_panel.as_ref().unwrap()).contains('3'));
        // A plain click replaces them all.
        assert!(super::click(&mut h.app, &proj, screen(b.0, b.1), false));
        let picked = points(&h);
        assert!(picked.len() == 1 && (picked[0][0] - b.0).abs() < 1e-3, "{picked:?}");
        // At the limit, Ctrl+click is refused with the selection kept; a plain click still replaces it.
        h.app.tree.canvas_panel.as_mut().unwrap().points = vec![[0.0, 0.0]; super::MAX_TRACE_POINTS];
        assert!(super::click(&mut h.app, &proj, screen(a.0, a.1), true));
        assert_eq!(points(&h).len(), super::MAX_TRACE_POINTS);
        assert!(super::click(&mut h.app, &proj, screen(a.0, a.1), false));
        assert_eq!(points(&h).len(), 1);
        // A click off the image adds nothing, even with Ctrl.
        assert!(!super::click(&mut h.app, &proj, screen(c.center.x + 500.0, c.center.y), true));
        assert_eq!(points(&h).len(), 1);
    }

    #[test]
    fn edit_canvas_is_a_card_whose_close_keeps_the_live_edits() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
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

    /// Straighten takes two clicks along an edge and turns the image so the edge is
    /// horizontal, vertical or at the typed angle (#11989).
    #[test]
    fn straighten_picks_two_points_and_turns_the_canvas() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::straighten(&h.app, id));
        h.frames(6);
        assert!(h.widget("Straighten").is_some() && h.widget("Calibrate").is_none());
        for target in ["straighten:horizontal", "straighten:vertical", "straighten:angle"] {
            assert!(crate::scenario::handle_at(target).is_some(), "{target}");
        }
        // Two clicks along a line that rises 10 units over 30.
        let rect = h.app.viewport.rect.expect("viewport");
        let proj = crate::viewport::projection(&h.app, rect);
        let c = h.app.session.doc.canvases[0].clone();
        let (vals, _) = h.app.session.doc.param_values();
        let plane = h.app.session.doc.canvas_plane(&vals, &c, &h.app.session.model.state()).unwrap();
        let screen = |x: f64, y: f64| proj.to_screen(plane.to_world(solidcore_record::Vec2::new(x, y))).expect("on screen");
        let status = |h: &crate::scenario::Harness| h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(super::click(&mut h.app, &proj, screen(c.center.x - 15.0, c.center.y - 5.0), false));
        assert_eq!(super::hint(h.app.tree.canvas_panel.as_ref().unwrap()), "browser.straighten_click_b");
        assert_eq!(status(&h), crate::i18n::tr_diagnostic(solidcraft_ui_text::tr("browser.straighten_click_b")));
        assert!(super::click(&mut h.app, &proj, screen(c.center.x + 15.0, c.center.y + 5.0), false));
        // With both points picked, the card and the status line ask for the direction, not Point A.
        assert_eq!(super::hint(h.app.tree.canvas_panel.as_ref().unwrap()), "browser.straighten_choose");
        assert_eq!(status(&h), crate::i18n::tr_diagnostic(solidcraft_ui_text::tr("browser.straighten_choose")));
        let picked = h.app.tree.canvas_panel.clone().unwrap();
        let rise = (picked.b[1] - picked.a[1]).atan2(picked.b[0] - picked.a[0]).to_degrees();
        assert!((rise - 18.43).abs() < 1.0, "{picked:?}");
        // Vertical: the image turns by 90 - rise about Point A, in one undo step.
        let undo = h.app.session.undo.len();
        h.step(&json!({"click": {"handle": "straighten:vertical"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.tree.canvas_panel.as_ref().map(|p| p.direction), Some(1));
        h.step(&json!({"widget": "Straighten"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none());
        let first = h.app.session.doc.canvases[0].angle.to_degrees();
        assert!((first - (90.0 - rise)).abs() < 1e-6, "{first}");
        assert_eq!(h.app.session.undo.len(), undo + 1);
        // The typed angle is what the command gets.
        let mut panel = crate::browser::CanvasPanel::straighten(&h.app, id);
        (panel.a, panel.b, panel.direction, panel.angle) = ([0.0, 0.0], [10.0, 0.0], 2, 30.0);
        assert_eq!(panel.target_angle(), 30.0);
        h.app.tree.canvas_panel = Some(panel);
        h.frames(4);
        assert!(crate::scenario::handle_at("dialog:angle:Angle").is_some());
        h.step(&json!({"widget": "Straighten"})).unwrap();
        h.frames(2);
        let turned = h.app.session.doc.canvases[0].angle.to_degrees() - first;
        assert!((turned - 30.0).abs() < 1e-6, "{turned}");
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
        let plane = h.app.session.doc.canvas_plane(&vals, &c, &h.app.session.model.state()).unwrap();
        let ax = c.center.x - 20.0;
        let bx = c.center.x + 20.0;
        let sa = proj.to_screen(plane.to_world(solidcore_record::Vec2::new(ax, c.center.y))).expect("point A on screen");
        let sb = proj.to_screen(plane.to_world(solidcore_record::Vec2::new(bx, c.center.y))).expect("point B on screen");
        assert!(super::click(&mut h.app, &proj, sa, false), "first click sets A");
        let panel = h.app.tree.canvas_panel.as_ref().unwrap();
        assert_eq!(panel.next, 1);
        assert!((panel.a[0] - ax).abs() < 0.5, "{:?}", panel.a);
        assert!(super::click(&mut h.app, &proj, sb, false), "second click sets B");
        let panel = h.app.tree.canvas_panel.as_ref().unwrap();
        assert_eq!(panel.next, 0);
        assert!((panel.b[0] - bx).abs() < 0.5, "{:?}", panel.b);
        let before = panel.a;
        let miss = plane.to_world(solidcore_record::Vec2::new(c.center.x + 500.0, c.center.y));
        if let Some(sm) = proj.to_screen(miss) {
            assert!(!super::click(&mut h.app, &proj, sm, false));
            assert_eq!(h.app.tree.canvas_panel.as_ref().unwrap().a, before);
        }
    }

    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    /// Hovering and dragging Point A updates its coordinates and live calibration distance preview
    /// without disturbing Point B, orbiting/panning the camera, or starting a box selection (#23210).
    #[test]
    fn calibrate_drag_point_a_updates_coordinates_and_live_distance_without_moving_b() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, true));
        h.frames(6);
        let rect = h.app.viewport.rect.expect("viewport");
        let proj = crate::viewport::projection(&h.app, rect);
        let c = h.app.session.doc.canvases[0].clone();
        let (vals, _) = h.app.session.doc.param_values();
        let plane = h.app.session.doc.canvas_plane(&vals, &c, &h.app.session.model.state()).unwrap();
        let screen = |x: f64, y: f64| proj.to_screen(plane.to_world(solidcore_record::Vec2::new(x, y))).expect("on screen");

        // Place initial A and B cleanly inside the canvas.
        let panel = h.app.tree.canvas_panel.as_mut().unwrap();
        panel.a = [c.center.x - 20.0, c.center.y];
        panel.b = [c.center.x + 20.0, c.center.y];
        panel.distance = 40.0;
        let b_before = panel.b;
        let dist_before = panel.measured_distance();
        assert!((dist_before - 40.0).abs() < 1e-6);
        h.frames(2);

        let start_a = screen(c.center.x - 20.0, c.center.y);
        let mid_a = screen(c.center.x - 10.0, c.center.y + 5.0);
        let end_a = screen(c.center.x, c.center.y);
        let cam_before = h.app.cam;

        // Hover over Point A shows the Grab cue and owns the pointer.
        h.frame_with(vec![egui::Event::PointerMoved(start_a)]);
        assert_eq!(h.output.cursor_icon, egui::CursorIcon::Grab);
        assert_eq!(super::hit_point(&h.app, &proj, start_a), Some(0));
        assert!(crate::canvas::owns_pointer(&h.app, &proj, start_a));

        // Press and drag Point A to mid_a: live update while still holding.
        h.frame_with(vec![pointer_button(start_a, true)]);
        h.frame_with(vec![egui::Event::PointerMoved(mid_a)]);
        assert!(super::dragging(&h.app), "Point A drag must be active while holding primary");
        assert_eq!(h.output.cursor_icon, egui::CursorIcon::Grabbing);
        assert!(h.app.viewport.boxsel.is_none(), "dragging Point A must not start box selection");
        {
            let p = h.app.tree.canvas_panel.as_ref().unwrap();
            assert!((p.a[0] - (c.center.x - 10.0)).abs() < 0.75, "mid-drag A.x: {:?}", p.a);
            assert!((p.a[1] - (c.center.y + 5.0)).abs() < 0.75, "mid-drag A.y: {:?}", p.a);
            assert_eq!(p.b, b_before, "Point B must remain undisturbed while dragging A");
            assert!((p.measured_distance() - dist_before).abs() > 5.0, "live measured distance must update in real time: {}", p.measured_distance());
            assert_eq!(
                super::distance_preview_label(&h.app, p),
                solidcraft_engine::doc::display_units::format_length(
                    p.measured_distance(),
                    &h.app.session.doc.units,
                    h.app.preferences.length_decimals,
                )
            );
        }

        // Continue dragging to end_a and release.
        h.frame_with(vec![egui::Event::PointerMoved(end_a)]);
        h.frame_with(vec![pointer_button(end_a, false)]);
        h.frames(2);
        assert!(!super::dragging(&h.app), "release must end Point A drag");
        assert!(h.app.viewport.boxsel.is_none());
        let p = h.app.tree.canvas_panel.as_ref().unwrap();
        assert!((p.a[0] - c.center.x).abs() < 0.75, "committed A.x: {:?}", p.a);
        assert!((p.a[1] - c.center.y).abs() < 0.75, "committed A.y: {:?}", p.a);
        assert_eq!(p.b, b_before, "Point B must remain undisturbed after releasing A");
        assert!((p.measured_distance() - 20.0).abs() < 0.75, "new A-B distance: {}", p.measured_distance());
        assert_eq!(h.app.cam, cam_before, "camera must not orbit or pan");

        // Calibrating now uses the dragged Point A (20 mm -> 40 mm doubles canvas width from 80 to 160).
        h.step(&json!({"widget": "Calibrate"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none());
        let width = h.app.session.doc.canvases[0].width;
        assert!((width - 160.0).abs() < 4.0, "expected ~160 after calibrating dragged A: {width}");
    }

    /// Dragging Point B works smoothly (including same-frame move+release and OpenSCAD left-drag orbit preset)
    /// without disturbing Point A or orbiting the camera (#23210).
    #[test]
    fn calibrate_drag_point_b_preserves_point_a_and_blocks_orbit() {
        for one_frame_release in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.language = "en".into();
            h.app.preferences.nav = crate::prefs::OPENSCAD_NAV.into();
            h.app.home.open = false;
            let id = canvas(&mut h);
            h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, true));
            h.frames(6);
            let rect = h.app.viewport.rect.expect("viewport");
            let proj = crate::viewport::projection(&h.app, rect);
            let c = h.app.session.doc.canvases[0].clone();
            let (vals, _) = h.app.session.doc.param_values();
            let plane = h.app.session.doc.canvas_plane(&vals, &c, &h.app.session.model.state()).unwrap();
            let screen = |x: f64, y: f64| proj.to_screen(plane.to_world(solidcore_record::Vec2::new(x, y))).expect("on screen");

            let panel = h.app.tree.canvas_panel.as_mut().unwrap();
            panel.a = [c.center.x - 15.0, c.center.y];
            panel.b = [c.center.x + 15.0, c.center.y];
            let a_before = panel.a;
            h.frames(2);

            let start_b = screen(c.center.x + 15.0, c.center.y);
            let mid_b = screen(c.center.x + 5.0, c.center.y - 6.0);
            let end_b = screen(c.center.x + 25.0, c.center.y - 8.0);
            let cam_before = h.app.cam;

            h.frame_with(vec![egui::Event::PointerMoved(start_b)]);
            assert_eq!(h.output.cursor_icon, egui::CursorIcon::Grab);
            assert_eq!(super::hit_point(&h.app, &proj, start_b), Some(1));

            h.frame_with(vec![pointer_button(start_b, true)]);
            h.frame_with(vec![egui::Event::PointerMoved(mid_b)]);
            assert!(super::dragging(&h.app));
            if one_frame_release {
                h.frame_with(vec![egui::Event::PointerMoved(end_b), pointer_button(end_b, false)]);
            } else {
                h.frame_with(vec![egui::Event::PointerMoved(end_b)]);
                h.frame_with(vec![pointer_button(end_b, false)]);
            }
            h.frames(2);

            assert!(!super::dragging(&h.app));
            let p = h.app.tree.canvas_panel.as_ref().unwrap();
            assert_eq!(p.a, a_before, "Point A must stay undisturbed (one_frame_release={one_frame_release})");
            assert!((p.b[0] - (c.center.x + 25.0)).abs() < 0.75, "Point B.x: {:?}", p.b);
            assert!((p.b[1] - (c.center.y - 8.0)).abs() < 0.75, "Point B.y: {:?}", p.b);
            assert_eq!(h.app.cam, cam_before, "OpenSCAD left-drag orbit must not trigger when dragging Point B");
        }
    }

    /// Clicking away from A/B or clicking without dragging keeps existing click-to-place intact,
    /// and Escape / Cancel leaves the canvas untouched with no dangling drag state (#23210).
    #[test]
    fn calibrate_click_without_drag_preserves_click_to_place_and_escape_cancel() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        let id = canvas(&mut h);
        h.app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(&h.app, id, true));
        h.frames(6);
        let rect = h.app.viewport.rect.expect("viewport");
        let proj = crate::viewport::projection(&h.app, rect);
        let c = h.app.session.doc.canvases[0].clone();
        let (vals, _) = h.app.session.doc.param_values();
        let plane = h.app.session.doc.canvas_plane(&vals, &c, &h.app.session.model.state()).unwrap();
        let screen = |x: f64, y: f64| proj.to_screen(plane.to_world(solidcore_record::Vec2::new(x, y))).expect("on screen");

        // 1. Plain click away from A and B places Point A (next: 0 -> 1).
        let click_a = screen(c.center.x - 12.0, c.center.y + 6.0);
        h.frame_with(vec![egui::Event::PointerMoved(click_a)]);
        h.frame_with(vec![pointer_button(click_a, true)]);
        h.frame_with(vec![pointer_button(click_a, false)]);
        h.frames(2);
        {
            let p = h.app.tree.canvas_panel.as_ref().unwrap();
            assert_eq!(p.next, 1, "first click places Point A and advances next to B");
            assert!((p.a[0] - (c.center.x - 12.0)).abs() < 0.75, "{:?}", p.a);
            assert!((p.a[1] - (c.center.y + 6.0)).abs() < 0.75, "{:?}", p.a);
        }

        // 2. Plain click away from A and B places Point B (next: 1 -> 0).
        let click_b = screen(c.center.x + 18.0, c.center.y - 4.0);
        h.frame_with(vec![egui::Event::PointerMoved(click_b)]);
        h.frame_with(vec![pointer_button(click_b, true)]);
        h.frame_with(vec![pointer_button(click_b, false)]);
        h.frames(2);
        {
            let p = h.app.tree.canvas_panel.as_ref().unwrap();
            assert_eq!(p.next, 0, "second click places Point B and cycles next back to A");
            assert!((p.b[0] - (c.center.x + 18.0)).abs() < 0.75, "{:?}", p.b);
            assert!((p.b[1] - (c.center.y - 4.0)).abs() < 0.75, "{:?}", p.b);
        }

        // 3. Clicking directly on existing Point B WITHOUT dragging still triggers click-to-place (sets A to B's spot).
        let on_b = screen(c.center.x + 18.0, c.center.y - 4.0);
        assert_eq!(super::hit_point(&h.app, &proj, on_b), Some(1));
        h.frame_with(vec![egui::Event::PointerMoved(on_b)]);
        h.frame_with(vec![pointer_button(on_b, true)]);
        h.frame_with(vec![pointer_button(on_b, false)]);
        h.frames(2);
        {
            let p = h.app.tree.canvas_panel.as_ref().unwrap();
            assert_eq!(p.next, 1, "click without drag on existing marker must keep click-to-place");
            assert!((p.a[0] - (c.center.x + 18.0)).abs() < 0.75, "{:?}", p.a);
            assert!((p.a[1] - (c.center.y - 4.0)).abs() < 0.75, "{:?}", p.a);
        }

        // 4. Escape closes Calibrate cleanly without modifying the canvas.
        let width_before = h.app.session.doc.canvases[0].width;
        h.frame_with(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        h.frames(2);
        assert!(h.app.tree.canvas_panel.is_none(), "Escape must close Calibrate panel");
        assert!(!super::dragging(&h.app));
        assert!((h.app.session.doc.canvases[0].width - width_before).abs() < 1e-9);
    }
}
