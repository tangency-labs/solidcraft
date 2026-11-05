//! The View angles readout under the orientation gizmo (#6032): the view's pitch, yaw and roll in
//! degrees and its projection, shown while View ▸ View angles is on (`view.angle_readout`). A
//! click on it opens a small card to type exact angles; its Apply runs `view.apply`.
//!
//! Each axis in the card also has a zero button that turns that angle to 0° at once, and a lock
//! (#4581): a locked angle stays put while orbiting, turning with the arrow keys or using a 3D
//! mouse, so a symmetrical view can hold its yaw and roll at 0° while only the pitch is adjusted.
//!
//! The angles are the camera's own (`Camera::view_angles`), about the up axis: yaw is the heading
//! of the line of sight (Front 0°, Left 90°, Right −90°, Back 180°), pitch the eye's elevation
//! (Top 90°, Bottom −90°), and a positive roll turns the model clockwise on screen.

use crate::ui_kit::BackingTooltipExt as _;
use egui::{Align2, FontId, Rect, Sense, pos2, vec2};
use serde_json::json;
use solidcraft_engine::render::{Camera, ViewAngles};

use crate::SolveApp;
use crate::fonts::RichTextExt as _;
use crate::theme::Tokens;
use crate::ui_kit::{self, Container, Surface};

/// The angles typed in the card while it is open; no entry while it is closed.
pub(crate) fn draft_id() -> egui::Id {
    egui::Id::new("sc_view_angles_draft")
}

/// Where the readout was drawn last frame over the viewport `viewport`, so a click on it never
/// reaches the model below.
pub(crate) fn readout_rect(ctx: &egui::Context, viewport: Rect) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<(Rect, Rect)>(egui::Id::new("sc_view_angles_readout"))).filter(|(v, _)| *v == viewport).map(|(_, r)| r)
}

fn set_readout_rect(ui: &egui::Ui, viewport: Rect, rect: Option<Rect>) {
    let id = egui::Id::new("sc_view_angles_readout");
    ui.data_mut(|d| match rect {
        Some(r) => {
            d.insert_temp(id, (viewport, r));
        }
        None => {
            d.remove::<(Rect, Rect)>(id);
        }
    });
}

/// An angle as the readout prints it: Preferences' angle decimals, and never "-0".
pub(crate) fn degrees(value: f64, decimals: u8) -> String {
    let decimals = usize::from(decimals.min(6));
    let k = 10f64.powi(i32::try_from(decimals).unwrap_or(6));
    let rounded = if value.is_finite() { (value * k).round() / k + 0.0 } else { 0.0 };
    format!("{rounded:.decimals$}°")
}

/// The readout's two lines: the angles, then the projection the view is drawn with.
pub(crate) fn lines(app: &SolveApp) -> (String, String) {
    let a = app.cam.view_angles();
    let d = app.preferences.angle_decimals;
    let angles =
        crate::i18n::format("ui.view_angles_readout", &[("pitch", &degrees(a.pitch, d)), ("yaw", &degrees(a.yaw, d)), ("roll", &degrees(a.roll, d))]);
    let projection = if app.perspective_view() { "ui.view_angles_persp" } else { "ui.view_angles_ortho" };
    (angles, crate::i18n::tr(projection).to_string())
}

/// Whether the angle of `axis` (`"yaw"`, `"pitch"` or `"roll"`) is locked in the card (#4581).
pub(crate) fn locked_angle(app: &SolveApp, axis: &str) -> bool {
    match axis {
        "yaw" => app.ui.view_angles_lock_yaw,
        "pitch" => app.ui.view_angles_lock_pitch,
        _ => app.ui.view_angles_lock_roll,
    }
}

fn set_lock(app: &mut SolveApp, axis: &str, locked: bool) {
    match axis {
        "yaw" => app.ui.view_angles_lock_yaw = locked,
        "pitch" => app.ui.view_angles_lock_pitch = locked,
        _ => app.ui.view_angles_lock_roll = locked,
    }
}

/// A camera `to` that a mouse or key motion just produced, with every locked angle kept at its
/// value on `from`: orbiting, arrow turns and 3D-mouse motion never move a locked axis (#4581).
pub(crate) fn keep_locked(app: &SolveApp, from: Camera, to: Camera) -> Camera {
    if !(locked_angle(app, "yaw") || locked_angle(app, "pitch") || locked_angle(app, "roll")) {
        return to;
    }
    let mut angles = to.view_angles();
    let held = from.view_angles();
    if locked_angle(app, "yaw") {
        angles.yaw = held.yaw;
    }
    if locked_angle(app, "pitch") {
        angles.pitch = held.pitch;
    }
    if locked_angle(app, "roll") {
        angles.roll = held.roll;
    }
    to.with_view_angles(angles).unwrap_or(to)
}

/// Turn the view to typed angles through `view.apply`, keeping its target, distance and projection.
/// A locked axis keeps the angle the view has now, whatever the card holds for it.
pub(crate) fn apply(app: &mut SolveApp, mut angles: ViewAngles) {
    let held = app.cam.view_angles();
    if locked_angle(app, "yaw") {
        angles.yaw = held.yaw;
    }
    if locked_angle(app, "pitch") {
        angles.pitch = held.pitch;
    }
    if locked_angle(app, "roll") {
        angles.roll = held.roll;
    }
    let Some(cam) = app.cam.with_view_angles(angles) else { return };
    let camera = json!({"target": cam.target, "yaw": cam.yaw, "pitch": cam.pitch, "roll": cam.roll, "distance": cam.distance, "fov": cam.fov});
    if app.run("view.apply", json!({ "camera": camera })).is_ok() {
        app.follow_view_request(true);
    }
}

/// Paint the readout under the gizmo of the viewport `rect`, and the card when it is open.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let axis = crate::viewport::axis_rect(app, rect);
    if !app.preferences.show_view_angles || !axis.is_positive() || !axis.is_finite() {
        ui.data_mut(|d| d.remove::<ViewAngles>(draft_id()));
        set_readout_rect(ui, rect, None);
        return;
    }
    let t = Tokens::get();
    let (angles, projection) = lines(app);
    let painter = ui.painter_at(rect);
    let font = FontId::proportional(t.font_caption);
    let first = painter.layout_no_wrap(angles.clone(), font.clone(), t.text);
    let second = painter.layout_no_wrap(projection.clone(), font, t.text_dim);
    let gap = 2.0;
    let size = vec2(first.size().x.max(second.size().x), first.size().y + gap + second.size().y) + vec2(12.0, 6.0);
    let chip = Rect::from_min_size(pos2(axis.right() - 4.0 - size.x, axis.bottom() + 2.0), size);
    if !rect.contains_rect(chip) {
        ui.data_mut(|d| d.remove::<ViewAngles>(draft_id()));
        set_readout_rect(ui, rect, None);
        return;
    }
    set_readout_rect(ui, rect, Some(chip));
    let mut draft = ui.data(|d| d.get_temp::<ViewAngles>(draft_id()));
    painter.rect(chip, t.radius_sm, t.overlay, t.control_border_stroke(if draft.is_some() { t.accent } else { t.border }), egui::StrokeKind::Inside);
    let right = chip.right() - 6.0;
    painter.galley(pos2(right - first.size().x, chip.top() + 3.0), first, t.text);
    painter.galley(pos2(right - second.size().x, chip.bottom() - 3.0 - second.size().y), second, t.text_dim);
    crate::scenario::publish_target("view_angles:readout", chip);
    let label = format!("{angles} · {projection}");
    let response = ui.interact(chip, ui.id().with("view_angles_readout"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response = response.backing_on_hover_text(crate::i18n::tr("ui.view_angles_help"));
    if response.clicked() {
        draft = if draft.is_some() { None } else { Some(app.cam.view_angles()) };
    }
    let Some(mut typed) = draft else {
        ui.data_mut(|d| d.remove::<ViewAngles>(draft_id()));
        return;
    };
    let area = Container::new(ui.ctx(), Surface::Popup)
        .area(egui::Id::new("sc_view_angles_card"))
        .order(egui::Order::Foreground)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(chip.right_bottom() + vec2(0.0, 4.0));
    let mut applied = false;
    let card = area.show(ui.ctx(), |ui| {
        let frame = Container::popup_frame(ui.style());
        Container::show_backing(ui, frame, |ui| {
            ui.set_width(260.0);
            ui.label(egui::RichText::new(crate::i18n::tr("ui.view_angles")).semibold().color(t.text).size(t.font_caption));
            for (id, axis) in [("ui.view_angles_pitch", "pitch"), ("ui.view_angles_yaw", "yaw"), ("ui.view_angles_roll", "roll")] {
                let locked = locked_angle(app, axis);
                let axis_name = crate::i18n::tr(id);
                let mut value = match axis {
                    "pitch" => typed.pitch,
                    "yaw" => typed.yaw,
                    _ => typed.roll,
                };
                let zeroed = ui_kit::row(ui, axis_name, |ui| {
                    let mut zeroed = false;
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(!locked, |ui| ui_kit::angle_field(ui, &mut value));
                        let zero = ui_kit::button(ui, "0°", ui_kit::Button::Enabled(!locked))
                            .backing_on_hover_text(crate::i18n::format("ui.view_angles_zero", &[("axis", axis_name)]));
                        zeroed = zero.clicked();
                        crate::scenario::publish_target(&format!("view_angles:zero:{axis}"), zero.rect);
                        let mut lock = locked;
                        let help = if locked { "ui.view_angles_unlock" } else { "ui.view_angles_lock" };
                        let checkbox = ui_kit::checkbox(ui, "", &mut lock).backing_on_hover_text(crate::i18n::format(help, &[("axis", axis_name)]));
                        crate::scenario::publish_target(&format!("view_angles:lock:{axis}"), checkbox.rect);
                        if lock != locked {
                            set_lock(app, axis, lock);
                        }
                    });
                    zeroed
                });
                if zeroed {
                    value = 0.0;
                }
                match axis {
                    "pitch" => typed.pitch = value,
                    "yaw" => typed.yaw = value,
                    _ => typed.roll = value,
                }
                if zeroed {
                    apply(app, typed);
                }
            }
            ui_kit::text(ui, crate::i18n::tr("ui.view_angles_convention"), ui_kit::Text::Hint("view_angles"));
            applied = ui_kit::primary_button(ui, "Apply").clicked();
        });
    });
    crate::scenario::publish_rect("view_angles:card", card.response.rect);
    if applied {
        apply(app, typed);
    }
    let clicked_out = ui.input(|i| i.pointer.any_pressed())
        && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !card.response.rect.contains(p) && !chip.contains(p));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    ui.data_mut(|d| {
        if clicked_out || escape {
            d.remove::<ViewAngles>(draft_id());
        } else {
            d.insert_temp(draft_id(), typed);
        }
    });
}
