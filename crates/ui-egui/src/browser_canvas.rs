//! The Edit Canvas / Calibrate card the Browser opens for a reference image: position, size,
//! angle, opacity and flip (live, one undo step per opening), or two points on the image and
//! their true distance. Calibrate accepts clicks in the viewport on the image (#9108). Straighten
//! takes two points along an edge of the image the same way and turns the image so that edge is
//! horizontal, vertical or at a typed angle (#11989).

use egui::{Pos2, Stroke, vec2};
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::ui_kit;
use crate::viewport::Proj;
use solidcraft_theme::Tokens;

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
        // Ctrl+click adds one more region; a plain click picks that one alone.
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

/// Markers for the two calibration or straightening points on the image. Straighten also draws
/// the line A–B; while it waits for Point B, the line follows the pointer from Point A (#11989).
pub(crate) fn paint_marks(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(panel) = app.tree.canvas_panel.as_ref().filter(|p| p.picks_points() || p.trace) else {
        return;
    };
    let doc = &app.session.doc;
    let Some(c) = doc.canvases.iter().find(|c| c.id == panel.canvas) else {
        return;
    };
    let (vals, _) = doc.param_values();
    let Ok(plane) = doc.canvas_plane(&vals, c, &app.session.model.state()) else {
        return;
    };
    let t = Tokens::get();
    if panel.straighten {
        let a = proj.to_screen(plane.to_world(Vec2::new(panel.a[0], panel.a[1])));
        let b = if panel.next == 1 {
            // The pointer, when it is over the view.
            painter.ctx().pointer_hover_pos().filter(|p| painter.clip_rect().contains(*p))
        } else {
            proj.to_screen(plane.to_world(Vec2::new(panel.b[0], panel.b[1])))
        };
        if let (Some(a), Some(b)) = (a, b) {
            painter.line_segment([a, b], Stroke::new(1.5, t.accent));
        }
    }
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
    for (i, point) in [panel.a, panel.b].into_iter().enumerate() {
        let Some(screen) = proj.to_screen(plane.to_world(Vec2::new(point[0], point[1]))) else {
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
        assert!(super::click(&mut h.app, &proj, screen(c.center.x - 15.0, c.center.y - 5.0)));
        assert_eq!(super::hint(h.app.tree.canvas_panel.as_ref().unwrap()), "browser.straighten_click_b");
        assert_eq!(status(&h), crate::i18n::tr_diagnostic(solidcraft_ui_text::tr("browser.straighten_click_b")));
        assert!(super::click(&mut h.app, &proj, screen(c.center.x + 15.0, c.center.y + 5.0)));
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
        assert!(super::click(&mut h.app, &proj, sa), "first click sets A");
        let panel = h.app.tree.canvas_panel.as_ref().unwrap();
        assert_eq!(panel.next, 1);
        assert!((panel.a[0] - ax).abs() < 0.5, "{:?}", panel.a);
        assert!(super::click(&mut h.app, &proj, sb), "second click sets B");
        let panel = h.app.tree.canvas_panel.as_ref().unwrap();
        assert_eq!(panel.next, 0);
        assert!((panel.b[0] - bx).abs() < 0.5, "{:?}", panel.b);
        let before = panel.a;
        let miss = plane.to_world(solidcore_record::Vec2::new(c.center.x + 500.0, c.center.y));
        if let Some(sm) = proj.to_screen(miss) {
            assert!(!super::click(&mut h.app, &proj, sm));
            assert_eq!(h.app.tree.canvas_panel.as_ref().unwrap().a, before);
        }
    }
}
