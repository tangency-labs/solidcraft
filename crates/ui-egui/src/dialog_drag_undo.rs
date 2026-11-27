//! Undo and Redo of a dialog's handle drags (#22838). A drag on a canvas handle (Extrude's
//! distance arrow or taper handle, a hole's depth, a fillet's radius, …) changes only the open
//! dialog's values, not the design. So Ctrl+Z takes back the last drag first, the way it takes
//! back a spline's last point; with no drag left to take back it undoes the design as before.
//!
//! Only drags made one after another go back: typing, or a click that drags no handle, forgets
//! them, so Undo never throws away a value typed or chosen after a drag.

use crate::SolveApp;
use crate::dialogs::Kind;

/// At most this many drags of one dialog are kept.
const KEPT: usize = 64;

/// A dialog's values that a handle drag can change: its kind's fields, and the parameters it
/// keeps beside them (a spline fillet's radii and knots).
type Values = (Kind, serde_json::Map<String, serde_json::Value>);

/// A dialog's handle drags: its values on the far side of each. UI state only, never saved or
/// sent with a command.
#[derive(Clone, Debug, Default)]
pub(crate) struct DragUndo {
    /// The values when the primary button went down, while that press lasts.
    pressed: Option<Values>,
    /// A handle was dragged since that press.
    dragged: bool,
    undo: Vec<Values>,
    redo: Vec<Values>,
}

impl DragUndo {
    /// Forget every drag, the one under a press that still lasts too.
    fn forget(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pressed = None;
        self.dragged = false;
    }
}

/// Once a frame, before the canvas handles draw: `pressed` is a primary press this frame, `down`
/// whether the primary button is held, and `dragging` whether a handle was dragged in the
/// previous frame. A press that ends after a drag keeps the values from before it for Undo.
pub(crate) fn track(app: &mut SolveApp, ctx: &egui::Context, pressed: bool, down: bool, dragging: bool) {
    let Some(d) = app.dialog.as_mut() else { return };
    let track = &mut d.drag_undo;
    let typed = ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Text(_) | egui::Event::Paste(_))
                || matches!(e, egui::Event::Key { key: egui::Key::Backspace | egui::Key::Delete, pressed: true, .. })
        })
    });
    if typed {
        track.forget();
    }
    if pressed {
        track.pressed = Some((d.kind.clone(), d.extra.clone()));
        track.dragged = false;
        return;
    }
    track.dragged |= dragging;
    if down || dragging {
        return;
    }
    let Some(before) = track.pressed.take() else { return };
    if !std::mem::take(&mut track.dragged) {
        track.forget();
        return;
    }
    if track.undo.len() == KEPT {
        track.undo.remove(0);
    }
    track.undo.push(before);
    track.redo.clear();
}

/// Undo's or Redo's key, pressed while a dialog is open and the keyboard is free or in that
/// dialog's value box: the last handle drag goes back (or again), and the key is used up, so
/// the design's history and the value box's own text undo are left alone. False when no drag
/// went back or again: the key then works as before.
pub(crate) fn keys(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    let Some(d) = app.dialog.as_ref() else { return false };
    let focus = ctx.memory(|memory| memory.focused());
    if focus.is_some() && focus != d.value_focus() {
        return false;
    }
    // Any key: Undo and Redo may be bound to other keys than Z and Y.
    let presses: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    for (key, mods) in presses {
        let commands = crate::keymap::commands_for(app, &crate::keymap::key_name(key, mods));
        let done = (commands.contains(&"edit.undo") && step(app, true)) || (commands.contains(&"edit.redo") && step(app, false));
        if done {
            ctx.input_mut(|i| i.consume_key(mods, key));
            return true;
        }
    }
    false
}

/// Put the open dialog's values back to before its last handle drag (`back`), or to after the
/// last one taken back. False when there is none, or while the primary button is held.
fn step(app: &mut SolveApp, back: bool) -> bool {
    let Some(d) = app.dialog.as_mut() else { return false };
    let track = &mut d.drag_undo;
    if track.pressed.is_some() {
        return false;
    }
    let (from, to) = if back { (&mut track.undo, &mut track.redo) } else { (&mut track.redo, &mut track.undo) };
    let Some((kind, extra)) = from.pop() else { return false };
    to.push((std::mem::replace(&mut d.kind, kind), std::mem::replace(&mut d.extra, extra)));
    true
}
