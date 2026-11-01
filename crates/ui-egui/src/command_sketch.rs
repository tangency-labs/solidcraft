//! A new sketch drawn from inside a feature's dialog (#6383). New Sketch (or a click on an
//! origin or construction plane while the profiles are still empty) puts the dialog aside and
//! opens a sketch; finishing or cancelling the sketch brings the dialog back, with every profile
//! of the new sketch picked. OK makes the sketch and the feature one undo step; Cancel leaves
//! the design and its undo history as they were before New Sketch.
//!
//! The sketch stays a timeline entry of its own and the feature's command runs as from any other
//! sketch: only the dialog and the undo history know the two were made together. A dialog kind
//! offers it through [`profile_input`]; only Extrude does so far.

use std::sync::Arc;

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Document, FeatureKind};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};

/// A feature dialog that drew (or is drawing) its own sketch.
pub struct InlineSketch {
    /// The waiting dialog, while its sketch (or the pick of the sketch's plane) is open.
    parked: Option<Dialog>,
    /// The command whose dialog it is: its label names the merged undo step.
    command: &'static str,
    /// The input of the dialog that takes the new sketch's profiles.
    input: usize,
    /// The design and undo history before the first New Sketch: Cancel goes back to it.
    checkpoint: u64,
    start_doc: Arc<Document>,
    start_undo: usize,
    /// The design when the dialog was put aside: a sketch not in it is the new one.
    parked_doc: Arc<Document>,
    /// The status line has said how to go back to the dialog.
    told: bool,
}

/// The command of a dialog that offers New Sketch and the input its profiles go to. Editing a
/// feature keeps its own sketch.
pub fn profile_input(d: &Dialog) -> Option<(&'static str, usize)> {
    if d.editing.is_some() {
        return None;
    }
    match d.kind {
        Kind::Extrude { .. } => Some(("solid.extrude", 0)),
        _ => None,
    }
}

/// A plane clicked now starts a sketch on it: the profiles input is active and still empty
/// (a plane is not a profile, so the click means nothing else there).
pub fn plane_starts_sketch(d: &Dialog) -> bool {
    profile_input(d).is_some_and(|(_, input)| input == d.active && d.inputs.get(input).is_some_and(|i| i.items.is_empty()))
}

/// The New Sketch button, under the dialog's selection inputs.
pub fn offer_row(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    if profile_input(d).is_none() || app.applying.is_some() {
        return;
    }
    let label = crate::i18n::tr_label("New Sketch");
    let button = crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal).on_hover_text(crate::i18n::tr("dialog.new_sketch_hint"));
    crate::scenario::publish_handle("dialog:new_sketch", button.rect.center());
    if button.clicked() {
        d.new_sketch = true;
    }
}

/// Put `d` aside and open a sketch: on `plane` when one was clicked, otherwise through Create
/// Sketch's own plane pick. A dialog that offers no New Sketch stays as it is.
pub fn begin(app: &mut SolveApp, d: Dialog, plane: Option<Sel>) {
    let Some((command, input)) = profile_input(&d).filter(|_| app.applying.is_none() && app.session.active_sketch.is_none()) else {
        app.dialog = Some(d);
        return;
    };
    let parked_doc = app.session.doc.clone();
    let inline = match app.inline_sketch.take() {
        // Another sketch for the same dialog: Cancel still goes back to before the first.
        Some(inline) => InlineSketch { parked: Some(d), parked_doc, told: false, ..inline },
        None => InlineSketch {
            parked: Some(d),
            command,
            input,
            start_doc: parked_doc.clone(),
            start_undo: app.session.undo.len(),
            checkpoint: app.session.checkpoint(),
            parked_doc,
            told: false,
        },
    };
    app.inline_sketch = Some(inline);
    let _ = app.session.execute("select.clear", &json!({}));
    match plane {
        Some(sel) => {
            if let Err(error) = crate::dialogs::start_sketch(app, &sel) {
                resume(app);
                app.set_status(error, true);
            }
        }
        None => app.dialog = Dialog::for_command(app, "sketch.create"),
    }
}

/// Once a frame: bring the waiting dialog back when its sketch is done, and let go of a flow
/// another command took over (what it made stays, as from any sketch).
pub fn tick(app: &mut SolveApp) {
    let Some(inline) = &mut app.inline_sketch else { return };
    if inline.parked.is_none() {
        // Back in the dialog: it ends through `closed`. A dialog replaced by another one ends it too.
        if !app.dialog.as_ref().is_some_and(|d| profile_input(d).is_some()) {
            forget(app);
        }
        return;
    }
    if app.session.active_sketch.is_some() {
        if !std::mem::replace(&mut inline.told, true) {
            let label = solidcraft_engine::find_command(inline.command).map_or("", |c| c.label);
            let label = crate::i18n::tr_label(label).to_string();
            app.set_status(crate::i18n::format("dialog.new_sketch_return", &[("command", &label)]), false);
        }
        return;
    }
    match &app.dialog {
        // Still picking the sketch's plane.
        Some(d) if matches!(d.kind, Kind::Sketch) => {}
        Some(_) => forget(app),
        None => resume(app),
    }
}

/// A Start of the waiting dialog's own command (its ribbon button, shortcut or gesture) while its
/// sketch is open finishes the sketch and goes back to the dialog. True when it did.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    let waiting = app.inline_sketch.as_ref().is_some_and(|i| i.parked.is_some() && i.command == id);
    if waiting && app.session.active_sketch.is_some() {
        app.finish_sketch();
        return true;
    }
    false
}

/// The dialog closed: applied, the steps since New Sketch become the feature's one undo step;
/// otherwise the design and undo history go back to before it. The plane pick closing while the
/// feature's dialog waits is not the end.
pub fn closed(app: &mut SolveApp, applied: bool) {
    if app.inline_sketch.as_ref().is_none_or(|i| i.parked.is_some()) {
        return;
    }
    let Some(inline) = app.inline_sketch.take() else { return };
    if applied {
        let label = solidcraft_engine::find_command(inline.command).map_or(inline.command, |c| c.label);
        app.session.merge_undo_since(&inline.start_doc, inline.start_undo, label);
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": inline.checkpoint, "forget": true}));
    } else {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": inline.checkpoint}));
    }
}

/// The waiting dialog again, with the profiles of the sketch just made picked. With no new
/// sketch at all (it was cancelled), the design and history go back to before New Sketch, so
/// nothing of it is left to undo.
fn resume(app: &mut SolveApp) {
    let Some(mut inline) = app.inline_sketch.take() else { return };
    let Some(mut d) = inline.parked.take() else { return };
    let is_new = |doc: &Document, id: u64| doc.feature(id).is_none();
    let sketches = |doc: &Arc<Document>| {
        app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }) && is_new(doc, f.id)).map(|f| f.id).next_back()
    };
    let made = sketches(&inline.parked_doc);
    if sketches(&inline.start_doc).is_none() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": inline.checkpoint}));
        inline.checkpoint = app.session.checkpoint();
    }
    if let Some(sketch) = made {
        let picked = drawn_profiles(app, sketch);
        if let Some(input) = d.inputs.get_mut(inline.input) {
            input.items = picked.into_iter().map(|index| Sel::Profile { sketch, index }).collect();
        }
        d.profile_selection_changed = true;
        d.error = None;
        d.refused = None;
    }
    d.active = inline.input;
    d.focus = true;
    app.dialog = Some(d);
    app.inline_sketch = Some(inline);
}

/// The profiles of `sketch` to pick: those whose outer boundary has a curve drawn in it, so the
/// face a sketch was started on (its outline projected there) is not extruded with them, also
/// not as the ring around a shape drawn inside it; every profile when none has one.
fn drawn_profiles(app: &SolveApp, sketch: u64) -> Vec<usize> {
    let state = app.session.world_state();
    let Some(ss) = state.sketch(sketch) else { return Vec::new() };
    let drawn = |id: &String| ss.sketch.curve_index(id).is_some_and(|ci| !ss.sketch.is_linked_curve(ci) || ss.sketch.is_text_curve(ci));
    let own: Vec<usize> = ss.profiles.iter().enumerate().filter(|(_, p)| p.outer_curves.iter().any(drawn)).map(|(i, _)| i).collect();
    if own.is_empty() { (0..ss.profiles.len()).collect() } else { own }
}

/// Let go of the flow, keeping the design as it is.
fn forget(app: &mut SolveApp) {
    if let Some(inline) = app.inline_sketch.take() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": inline.checkpoint, "forget": true}));
    }
}
