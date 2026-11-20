//! The optional Mecha navigator: the original hand mesh and compact camera controls.
//! Camera motion stays in the existing navigation handler and view commands.
use super::*;
use crate::ui_kit::{self, Button};
use solidcraft_viewport::ViewAngles;

/// A fixed reservation keeps controls and the model steady as hover expands the navigator.
pub(super) const SIZE: f32 = 240.0;

fn bounds(outer: Rect, centre: [f32; 2], size: [f32; 2]) -> Rect {
    let scale = outer.width().min(outer.height()) / SIZE;
    Rect::from_center_size(outer.min + vec2(centre[0], centre[1]) * scale, vec2(size[0], size[1]) * scale)
}

/// Only the hand owns orbit and zoom; view buttons cannot begin a camera drag.
pub(super) fn hand_rect(outer: Rect) -> Rect {
    bounds(outer, [120.0, 90.0], [126.0, 126.0])
}

fn button(ui: &mut egui::Ui, outer: Rect, id: &str, centre: [f32; 2], label: &str, help: &str, selected: bool) -> egui::Response {
    let t = Tokens::get();
    let width = if matches!(id, "mecha:view:top" | "mecha:view:bottom") { 72.0 } else { 52.0 };
    let rect = bounds(outer, centre, [width, 24.0]);
    let scale = (outer.width() / SIZE).min(1.0);
    let font = FontId::proportional(t.font_caption * scale);
    let label = ui_kit::ellipsize_middle(ui, label, font.clone(), (rect.width() - 12.0).max(0.0));
    let response =
        ui_kit::button_at(ui, id, rect, egui::RichText::new(label).font(font), if selected { Button::SmallSelected } else { Button::Small })
            .backing_on_hover_text(help);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, help));
    crate::scenario::publish_target(id, response.rect);
    response
}

fn view(app: &mut SolveApp, command: &str) {
    if command == "view.fit" {
        let _ = app.run(command, json!({}));
    } else {
        let camera = app.cam_anim.map_or(app.cam, |animation| animation.to);
        if app.run(command, json!({ "camera": camera })).is_ok()
            && app.preferences.navigator_fit
            && let Some(to) = app.session.camera.take()
        {
            app.animate_turn(to);
        }
    }
}

fn angles(app: &SolveApp, key: &str) -> String {
    let values = app.cam.view_angles();
    let decimals = app.preferences.angle_decimals;
    solidcraft_ui_text::format(
        key,
        &[
            ("azimuth", &crate::view_angles::degrees(values.yaw, decimals)),
            ("elevation", &crate::view_angles::degrees(values.pitch, decimals)),
            ("roll", &crate::view_angles::degrees(values.roll, decimals)),
        ],
    )
}

/// The cue uses the existing monochrome navigation artwork.
fn orbit_cue(painter: &egui::Painter, outer: Rect, active: bool) {
    let scale = outer.width().min(outer.height()) / SIZE;
    let t = Tokens::get().navigator(active);
    let centre = outer.min + vec2(120.0, 146.0) * scale;
    let points: Vec<Pos2> = (0..=32)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 32.0;
            centre + vec2(angle.cos() * 44.0, angle.sin() * 8.0) * scale
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.0, t.navigator_border)));
    crate::icons::navigation::arrow(painter, centre + vec2(38.0, -4.0) * scale, vec2(0.7, 0.4) * scale, false, t);
    crate::icons::navigation::arrow(painter, centre + vec2(-38.0, 4.0) * scale, vec2(-0.7, -0.4) * scale, false, t);
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, viewport: Rect, shape: &crate::view_gizmo::Shape) {
    let outer = axis_rect(app, viewport);
    if !outer.is_positive() || !outer.is_finite() {
        app.viewport.axis_drag = None;
        app.viewport.mecha_expanded = false;
        crate::view_angles::hide(ui, viewport);
        return;
    }
    crate::scenario::publish_rect("axis_view:outer", outer);
    let hand = hand_rect(outer);
    crate::scenario::publish_target("mecha:hand", hand);
    let response = ui.interact(hand, ui.scope_id().with("mecha_hand"), Sense::click()).backing_on_hover_text(app.tr("mecha.orbit_help"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, app.tr("mecha.orbit_help")));
    let orbiting = axis_view_orbit(app, ui, viewport);
    if response.clicked() && !orbiting {
        app.viewport.mecha_pinned = !app.viewport.mecha_pinned;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(if orbiting { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    let hovered =
        ui.rect_contains_pointer(outer) && ui.input(|input| input.pointer.hover_pos()).is_some_and(|point| !crate::shell::covered(app, point));
    let editing = ui.data(|data| data.get_temp::<ViewAngles>(crate::view_angles::draft_id()).is_some());
    let previous = app.viewport.mecha_expanded;
    let mut focused = response.has_focus() || ui.memory(|memory| memory.has_focus(crate::view_angles::readout_id()));
    let expanded = app.viewport.mecha_pinned || hovered || orbiting || editing || focused || app.viewport.mecha_focused;
    app.viewport.mecha_expanded = expanded;
    if expanded != previous {
        ui.ctx().request_repaint();
    }
    crate::scenario::publish_count("mecha:expanded", f64::from(u8::from(expanded)));
    crate::scenario::publish_count("navigator:active", f64::from(u8::from(expanded || orbiting)));
    crate::scenario::publish_count("axis_view_highlights", 0.0);
    let painter = ui.painter_at(outer);
    paint_navigator_preview(&painter, hand, shape, &app.cam);
    orbit_cue(&painter, outer, expanded);
    if response.has_focus() {
        painter.add(ui_kit::native_button_focus(ui, &response, hand));
    }

    let expand_help = app.tr(if app.viewport.mecha_pinned { "mecha.collapse_help" } else { "mecha.expand_help" }).to_owned();
    let expand = button(ui, outer, "mecha:expand", [210.0, 14.0], "⋯", &expand_help, app.viewport.mecha_pinned);
    focused |= expand.has_focus();
    if expand.clicked() {
        app.viewport.mecha_pinned = !app.viewport.mecha_pinned;
        ui.ctx().request_repaint();
    }

    for (id, position, label, help, command) in [
        ("mecha:view:iso", [88.0, 195.0], "mecha.iso", "mecha.iso_help", "view.isometric"),
        ("mecha:fit", [150.0, 195.0], "mecha.fit", "mecha.fit_help", "view.fit"),
    ] {
        let response = button(ui, outer, id, position, app.tr(label), app.tr(help), false);
        focused |= response.has_focus();
        if response.clicked() {
            view(app, command);
        }
    }

    if expanded {
        for (id, position, label, help, command) in [
            ("top", [120.0, 14.0], "mecha.top", "cmd.view_top", "view.top"),
            ("bottom", [120.0, 168.0], "mecha.bottom", "cmd.view_bottom", "view.bottom"),
            ("left", [29.0, 98.0], "mecha.left", "cmd.view_left", "view.left"),
            ("right", [211.0, 98.0], "mecha.right", "cmd.view_right", "view.right"),
            ("front", [29.0, 145.0], "mecha.front", "cmd.view_front", "view.front"),
            ("back", [211.0, 145.0], "mecha.back", "cmd.view_back", "view.back"),
        ] {
            let response = button(ui, outer, &format!("mecha:view:{id}"), position, app.tr(label), app.tr(help), false);
            focused |= response.has_focus();
            if response.clicked() {
                view(app, command);
            }
        }
        for (id, position, degrees, help, action) in [
            ("negative", [29.0, 45.0], -90, "mecha.roll_negative", "counterclockwise"),
            ("positive", [211.0, 45.0], 90, "mecha.roll_positive", "clockwise"),
        ] {
            let label = format!("{degrees:+}°");
            let response = button(ui, outer, &format!("mecha:roll:{id}"), position, &label, app.tr(help), false);
            focused |= response.has_focus();
            if response.clicked() {
                // The signed label is literal; preferences that invert drawn arrows do not change it.
                navigator_controls::navigate(app, Some(action), None);
            }
        }
        let chip = bounds(outer, [120.0, 224.0], [232.0, 24.0]);
        crate::scenario::publish_target("mecha:angles", chip);
        let label = angles(app, "mecha.angles_short");
        crate::view_angles::show_at(app, ui, viewport, chip, &label);
        focused |= ui.memory(|memory| memory.has_focus(crate::view_angles::readout_id()));
    } else {
        crate::view_angles::hide(ui, viewport);
    }
    if expanded && !editing && (hovered || focused) && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.viewport.mecha_pinned = false;
        app.viewport.mecha_focused = false;
        ui.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    } else {
        app.viewport.mecha_focused = focused;
    }
}
