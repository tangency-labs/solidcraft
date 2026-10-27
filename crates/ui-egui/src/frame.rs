//! Dock dragging and bounded anchors. Outer hosts and surfaces live in `ui_kit::Container`.

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
    ctx.data(|d| d.get_temp::<egui::Vec2>(egui::Id::new(("sc_dock_offset", key)))).unwrap_or(egui::Vec2::ZERO)
}

/// A title drag owns the pointer even when it outruns the dialog's previous position.
pub(crate) fn dock_dragging(ctx: &egui::Context) -> bool {
    if crate::ui_kit::floating::handoff_dragging(ctx) {
        return true;
    }
    let frame = ctx.cumulative_frame_nr();
    // The viewport can draw before the dialog, so retain the preceding frame's marker.
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new("sc_dock_drag_frame"))).is_some_and(|at| at >= frame.saturating_sub(1))
        && ctx.input(|i| i.pointer.any_down())
}

/// Only the header drag, excluding resize handles, for cards that may leave the app.
pub(crate) fn title_dragging(ctx: &egui::Context, key: &str) -> bool {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(("sc_title_drag_frame", key)))).is_some_and(|at| at >= frame.saturating_sub(1))
        && ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released())
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
    crate::scenario::publish_handle("dialog:title", band.center());
    let response = ui.interact(band, ui.id().with("sc_dock_drag"), egui::Sense::drag());
    if response.dragged() || response.drag_stopped() {
        let frame = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sc_dock_drag_frame"), frame));
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("sc_title_drag_frame", key)), frame));
    }
    // The drag runs from where the press started, so no movement is lost while egui decides
    // it is a drag, nor on the frame it ends.
    let base = egui::Id::new("sc_dock_drag_base");
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
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("sc_dock_offset", key)), to));
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    } else if moved != applied {
        // Kept on screen: remember where it is, not where it was dragged off to.
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("sc_dock_offset", key)), applied));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn feature_forms_use_available_height_and_keep_actions_visible() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            use crate::scenario::Harness;
            use serde_json::json;
            for (width, height, scale) in [(1400.0, 900.0, 1.0), (1400.0, 950.0, 2.0)] {
                let mut h = Harness::new();
                h.call("ui.set", json!({"layout":mode}));
                h.app.preferences.ui_scale = scale;
                h.call("ui.resize", json!({"width":width / scale,"height":height / scale}));
                h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
                h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
                h.step(&json!({"start":"solid.extrude"})).unwrap();
                h.frames(10);
                let dialog = h.app.viewport.dialog_rect.unwrap();
                let dialog_content = crate::scenario::count_of("dialog:body:content_height").unwrap();
                let viewport = h.app.viewport.rect.unwrap();
                assert!(dialog.height() > 230.0, "scale={scale}: {dialog:?}");
                assert!(viewport.expand(1.0).contains_rect(dialog), "scale={scale}: {dialog:?} outside {viewport:?}");
                assert!(h.widget("Distance").is_some(), "distance must be visible without scrolling");
                if scale == 1.0 {
                    assert!(h.widget("Operation").is_some(), "operation must be visible without scrolling");
                }
                assert!(dialog.contains(crate::scenario::handle_at("dialog:OK").unwrap()));
                assert!(h.widget("Cancel").is_some(), "cancel must be visible without scrolling");
                h.step(&json!({"widget":"Cancel"})).unwrap();
                assert!(h.app.dialog.is_none());
                h.step(&json!({"start":"solid.shell"})).unwrap();
                h.frames(8);
                let short = h.app.viewport.dialog_rect.unwrap();
                let short_content = crate::scenario::count_of("dialog:body:content_height").unwrap();
                assert!(
                    short_content < dialog_content,
                    "short forms have less content even when both need scrolling: {short_content}, {dialog_content}"
                );
                assert!(short.height() <= dialog.height(), "short forms cannot grow taller: {short:?}, {dialog:?}");
                let t = crate::theme::Tokens::get();
                let chrome = f64::from(t.form_header_height + t.form_footer_height + t.form_gap_y * 2.0 + t.space_2);
                if short_content + chrome < f64::from(dialog.height()) {
                    assert!(short.height() < dialog.height(), "unconstrained short forms must shrink: {short:?}, {dialog:?}");
                }
                assert!(viewport.expand(1.0).contains_rect(short));
                assert!(h.widget("Cancel").is_some());
                h.step(&json!({"widget":"Cancel"})).unwrap();
                h.step(&json!({"start":"solid.hole"})).unwrap();
                if let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Hole { kind, opts, .. }, .. }) = h.app.dialog.as_mut() {
                    *kind = 1;
                    opts.tapped = true;
                }
                h.frames(8);
                let long = h.app.viewport.dialog_rect.unwrap();
                assert!(viewport.expand(1.0).contains_rect(long), "long form clipped: {long:?}");
                assert!(h.widget("Cancel").is_some(), "long-form actions must stay visible");
                assert!(long.contains(crate::scenario::handle_at("dialog:OK").unwrap()));
                h.step(&json!({"widget":"Cancel"})).unwrap();
                assert!(h.app.dialog.is_none());
            }
        }
    }

    /// #5086: while a docked dialog's title is dragged, the pointer can outrun the dialog; the
    /// model under it must not highlight until the drag ends.
    #[test]
    fn dragging_a_docked_dialog_blocks_viewport_hover() {
        use crate::scenario::Harness;
        use serde_json::json;

        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout": mode}));
            h.call("ui.resize", json!({"width": 2400, "height": 900}));
            h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
            h.call("ui.view", json!({"view": "fit"}));
            h.app.ui.pick_bodies = true;
            let p = h.app.viewport.rect.unwrap().center();
            h.call("ui.move", json!({"x": p.x, "y": p.y}));
            assert!(h.app.viewport.hover.is_some(), "the uncovered model must highlight");
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.frames(6);
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            assert!(!h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_params_dialog"))).unwrap().contains(p));
            h.call("ui.move", json!({"x": title.x, "y": title.y}));
            h.app.synthetic.push(egui::Event::PointerButton {
                pos: title,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            });
            h.frame();
            h.app.synthetic.push(egui::Event::PointerMoved(p));
            h.frame();
            assert!(h.ctx.dragged_id().is_some(), "the title must own the drag");
            assert!(!h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_params_dialog"))).unwrap().contains(p), "the pointer must outrun the dialog");
            assert!(h.app.viewport.hover.is_none(), "dragging a dialog must block model highlights");
            assert!(h.app.viewport.mouse.is_none(), "the viewport must not receive the drag pointer");
            h.frames(2);
            assert!(h.app.viewport.hover.is_none(), "a stationary drag must still block highlights");
            h.app.synthetic.push(egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            });
            h.frame();
            h.app.params = None;
            h.frames(2);
            assert!(h.app.viewport.hover.is_some(), "hover must return after the dialog closes");
        }
    }

    /// #1335: a dialog keeps its size while its Select chip is hovered, and its title band
    /// drags it; reopening keeps its position.
    #[test]
    fn a_docked_dialog_keeps_its_size_under_the_pointer_and_moves_by_its_title() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            use crate::scenario::Harness;
            use serde_json::json;
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.resize", json!({"width":1400.0,"height":900.0}));
            h.step(&json!({"start":"sketch.create"})).unwrap();
            h.frames(6);
            let rest = h.app.viewport.dialog_rect.unwrap();
            let chip = crate::scenario::handle_at("dialog:Plane").unwrap();
            h.call("ui.move", json!({"x": chip.x, "y": chip.y}));
            h.frames(6);
            let hovered = h.app.viewport.dialog_rect.unwrap();
            assert!((hovered.size() - rest.size()).length() < 0.01, "hovering resized the dialog: {rest:?} -> {hovered:?}");
            // Drag the published title band, independent of similarly labelled toolbar controls.
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.call("ui.drag", json!({"x0": title.x, "y0": title.y, "x1": title.x - 300.0, "y1": title.y + 120.0, "steps": 8}));
            h.frames(4);
            let moved = h.app.viewport.dialog_rect.unwrap();
            assert!((moved.min - rest.min - egui::vec2(-300.0, 120.0)).length() < 2.0, "{rest:?} -> {moved:?}");
            assert!(h.app.dialog.is_some(), "dragging the title keeps the dialog open");
            // Deliberate pulls tear off on desktop; browser cards remain bounded.
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.call("ui.drag", json!({"x0": title.x, "y0": title.y, "x1": title.x + 3000.0, "y1": title.y + 3000.0, "steps": 8}));
            h.frames(4);
            #[cfg(not(target_arch = "wasm32"))]
            assert!(crate::ui_kit::floating::detached(&h.ctx, egui::Id::new(("sc_dialog", "CREATE SKETCH"))));
            #[cfg(target_arch = "wasm32")]
            assert!(h.app.viewport.rect.unwrap().contains(h.app.viewport.dialog_rect.unwrap().min));
            let last = h.app.viewport.dialog_rect.unwrap();
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.dialog.is_none());
            h.step(&json!({"start":"sketch.create"})).unwrap();
            h.frames(6);
            assert!((h.app.viewport.dialog_rect.unwrap().min - last.min).length() < 0.01, "reopening keeps the dragged position");
        }
    }
    #[test]
    fn command_dialogs_keep_the_dragged_position_after_closing() {
        use crate::scenario::Harness;
        use serde_json::json;
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.resize", json!({"width":1400.0,"height":900.0}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            h.app.run("solid.extrude", json!({"distance":10})).unwrap();
            h.step(&json!({"start":"solid.extrude"})).unwrap();
            h.frames(6);
            let rest = h.app.viewport.dialog_rect.unwrap();
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x-300.0,"y1":title.y+60.0,"steps":8}));
            h.frames(4);
            let moved = h.app.viewport.dialog_rect.unwrap();
            assert!((moved.min - rest.min - egui::vec2(-300.0, 60.0)).length() < 2.0);
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(4);
            assert!(h.app.dialog.is_none());
            h.step(&json!({"start":"solid.fillet"})).unwrap();
            h.frames(6);
            let fillet = h.app.viewport.dialog_rect.unwrap();
            assert!((fillet.right_top() - moved.right_top()).length() < 2.0, "Extrude: {moved:?}, Fillet: {fillet:?}");
            // A smaller window keeps the kept place on screen.
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(4);
            h.call("ui.resize", json!({"width":700.0,"height":500.0}));
            h.step(&json!({"start":"solid.fillet"})).unwrap();
            h.frames(6);
            let viewport = h.app.viewport.rect.unwrap();
            assert!(viewport.contains(h.app.viewport.dialog_rect.unwrap().min), "{viewport:?}");
        }
    }
}
