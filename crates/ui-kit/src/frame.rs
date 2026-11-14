//! Dock dragging and bounded anchors. Outer hosts and surfaces live in `crate::Container`.

use egui::{Pos2, Ui};

/// Width limits of a docked command dialog.
pub const DIALOG_MIN_W: f32 = 260.0;
pub const DIALOG_MAX_W: f32 = 380.0;
/// Which edge of the viewport a docked panel sits against. The Sketch Palette holds the right
/// edge while a sketch is open, so a dialog started there takes the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
}

impl Side {
    /// Command dialogs share a position on each side of the viewport.
    pub fn dialog_offset_key(self) -> &'static str {
        match self {
            Side::Right => "sc_command_dialog_right",
            Side::Left => "sc_command_dialog_left",
        }
    }

    /// The x of the viewport edge this side docks against.
    pub fn anchor_x(self, viewport: egui::Rect) -> f32 {
        match self {
            Side::Right => viewport.right(),
            Side::Left => viewport.left(),
        }
    }
}

/// How far a docked panel with this key was dragged.
pub fn dock_offset(ctx: &egui::Context, key: &str) -> egui::Vec2 {
    ctx.data(|d| d.get_temp::<egui::Vec2>(egui::Id::unique(("sc_dock_offset", key)))).unwrap_or(egui::Vec2::ZERO)
}

/// A title drag owns the pointer even when it outruns the dialog's previous position.
pub fn dock_dragging(ctx: &egui::Context) -> bool {
    let frame = ctx.cumulative_frame_nr();
    // The viewport can draw before the dialog, so retain the preceding frame's marker.
    ctx.data(|d| d.get_temp::<u64>(egui::Id::unique("sc_dock_drag_frame"))).is_some_and(|at| at >= frame.saturating_sub(1))
        && ctx.input(|i| i.pointer.any_down())
}

/// A docked dialog's anchor moved by `offset`, kept so its docked corner stays in `vp` with
/// room for its title band.
pub fn offset_anchor(docked: Pos2, offset: egui::Vec2, vp: egui::Rect, side: Side) -> Pos2 {
    let p = docked + offset;
    let (lo, hi) = match side {
        Side::Right => (vp.left() + DIALOG_MIN_W, vp.right()),
        Side::Left => (vp.left(), vp.right() - DIALOG_MIN_W),
    };
    let x = if lo <= hi { p.x.clamp(lo, hi) } else { docked.x };
    let y = if vp.top() <= vp.bottom() - 40.0 { p.y.clamp(vp.top(), vp.bottom() - 40.0) } else { docked.y };
    egui::pos2(x, y)
}

/// Let `band` (a docked dialog's title band) drag the dialog: the drag adds to its offset
/// `moved` (as last frame left it; `applied` of it is where the dialog is now, after keeping
/// it on screen). Called before the band's buttons are added, so they keep their clicks.
pub fn drag_dock(ui: &Ui, band: egui::Rect, key: &str, moved: egui::Vec2, applied: egui::Vec2) {
    drag_dock_named(ui, "dialog:title", band, key, moved, applied);
}

/// [`drag_dock`] for a panel that stays on screen beside a command dialog (the Sketch palette),
/// so its band is published under its own scenario `handle` instead of the dialog's.
pub fn drag_dock_named(ui: &Ui, handle: &str, band: egui::Rect, key: &str, moved: egui::Vec2, applied: egui::Vec2) {
    solidcraft_ui_registry::publish_handle(handle, band.center());
    let response = ui.interact(band, ui.scope_id().with("sc_dock_drag"), egui::Sense::drag());
    if response.dragged() {
        let frame = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sc_dock_drag_frame"), frame));
    }
    // The drag runs from where the press started, so no movement is lost while egui decides
    // it is a drag, nor on the frame it ends.
    let base = egui::Id::unique("sc_dock_drag_base");
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        ui.ctx().data_mut(|d| d.insert_temp(base, (applied, origin)));
    }
    if response.dragged() || response.drag_stopped() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let start = ui.ctx().data(|d| d.get_temp::<(egui::Vec2, Pos2)>(base));
        let to = match (start, ui.input(|i| i.pointer.latest_pos())) {
            (Some((start, from)), Some(now)) => start + (now - from),
            _ => applied + response.drag_delta(),
        };
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("sc_dock_offset", key)), to));
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    } else if moved != applied {
        // Kept on screen: remember where it is, not where it was dragged off to.
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("sc_dock_offset", key)), applied));
    }
}
