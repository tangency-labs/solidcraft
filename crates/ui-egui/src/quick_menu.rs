//! The quick menu (#5501): a list at the cursor of the commands that take the selection, or,
//! with nothing they take selected, the tools of the part of the application in use (a sketch's,
//! a Solid tab's…: the mouse-gesture ring's tools for that studio).
//!
//! [`KEY`] opens it at the cursor. With the preference `quick_menu_after_selection` (off by
//! default) it also opens by itself shortly after a click in the viewport selects something a
//! command takes, and only then. It is a context menu ([`Target::Quick`]): Esc, a click elsewhere
//! or a pick closes it, and so does the pointer moving [`LEAVE`] points away from it or the
//! selection changing.
//!
//! What fits comes from the commands themselves (`CommandSpec::for_selection`): a constraint
//! runs with the parameters the selection gives; any other command starts as a toolbar click
//! would, and its dialog takes the selection.

use egui::{Pos2, Rect};
use solidcraft_engine::Sel;

use crate::SolveApp;
use crate::context_menu::Target;
use crate::ui_kit::Item;

/// Opens the quick menu at the cursor, while no command is bound to it (a binding the person made
/// wins).
pub const KEY: &str = "Shift+S";
/// How far the pointer may move away from the open menu before it closes, in points.
pub(crate) const LEAVE: f32 = 60.0;
/// How long after a selecting click the menu waits before it opens by itself, in seconds.
const DELAY: f64 = 0.3;
/// How far a click may move and still count as a click (not a box selection), in points.
const CLICK: f32 = 6.0;

/// What the quick menu keeps between frames.
#[derive(Default)]
pub struct Watch {
    /// The selection last seen: a different one is a change.
    seen: Vec<Sel>,
    /// Where the primary button went down in the viewport, while it is down.
    press: Option<Pos2>,
    /// The last viewport click that did not move (where and when it was let go).
    click: Option<(Pos2, f64)>,
    /// A selection made by a click: the menu opens at this point at this time.
    due: Option<(Pos2, f64)>,
}

/// The commands that take the session's selection, in toolbar order: constraints with the
/// parameters it gives them, other commands to start as from the toolbar.
pub(crate) fn fitting(app: &SolveApp) -> Vec<Item> {
    if app.session.selection.is_empty() {
        return Vec::new();
    }
    solidcraft_engine::command_specs()
        .into_iter()
        .filter(|spec| !spec.tab.is_empty() && spec.planned.is_none())
        .filter_map(|spec| {
            let params = spec.for_selection(&app.session)?;
            let item = crate::context_menu::cmd(app, spec.id, spec.label);
            Some(if spec.panel == "CONSTRAINTS" { item.with(params) } else { item })
        })
        .collect()
}

/// The menu's rows: the commands that take the selection, else the studio's tools.
pub(crate) fn items(app: &SolveApp) -> Vec<Item> {
    let fit = fitting(app);
    if !fit.is_empty() {
        return fit;
    }
    crate::mouse_gesture::items(app, crate::mouse_gesture::Studio::of(app))
}

/// Opens the quick menu at `at`.
pub fn open(app: &mut SolveApp, at: Pos2) {
    crate::context_menu::open_for(app, at, Target::Quick);
}

/// Whether the pointer has left the open quick menu `rect` far enough to close it.
pub(crate) fn left(rect: Rect, pointer: Option<Pos2>) -> bool {
    pointer.is_some_and(|p| !rect.expand(LEAVE).contains(p))
}

/// One frame: follows clicks in the viewport and the selection, opens the menu after a
/// selecting click (with the preference on) and closes it when the selection changes.
pub(crate) fn watch(app: &mut SolveApp, ctx: &egui::Context) {
    let (pressed, released, pos, now) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.latest_pos(), i.time));
    if pressed {
        app.menu.quick.press = pos.filter(|_| app.viewport.mouse.is_some());
    }
    if released
        && let (Some(p0), Some(p)) = (app.menu.quick.press.take(), pos)
        && p0.distance(p) <= CLICK
    {
        app.menu.quick.click = Some((p, now));
    }
    if app.session.selection != app.menu.quick.seen {
        app.menu.quick.seen = app.session.selection.clone();
        app.menu.quick.due = None;
        if app.viewport.context_menu.is_some() && app.menu.target == Target::Quick {
            crate::context_menu::close(app);
        }
        // Only a click's own selection: not a box selection, a browser pick or a script's.
        // The click is used up here, so a later change of the selection can't take it again.
        if let Some((p, t)) = app.menu.quick.click.take()
            && now - t < 0.5
            && after_selection_ready(app)
        {
            app.menu.quick.due = Some((p, now + DELAY));
        }
    }
    let Some((p, t)) = app.menu.quick.due else { return };
    if !after_selection_ready(app) || pos.is_some_and(|q| q.distance(p) > CLICK) {
        app.menu.quick.due = None;
    } else if now >= t {
        app.menu.quick.due = None;
        if !fitting(app).is_empty() {
            open(app, p);
        }
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((t - now).max(0.01)));
    }
}

/// The preference is on and nothing else has the pointer: no menu, tool or dialog.
fn after_selection_ready(app: &SolveApp) -> bool {
    app.preferences.quick_menu_after_selection && app.viewport.context_menu.is_none() && app.tool.is_none() && app.dialog.is_none()
}

#[cfg(test)]
#[path = "quick_menu_tests.rs"]
mod tests;
