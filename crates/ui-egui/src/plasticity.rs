//! Plasticity controls: a keyboard, mouse and selection scheme for people coming from
//! Plasticity, so they can model without learning SolidCraft's keys first.
//!
//! Built from Plasticity's public command and shortcut list (2026.1) and its default
//! selection modes; no Plasticity code or assets. The scheme is a layer over the normal
//! keymap: its keys replace the commands' default keys while it is on (the user's own
//! rebindings still win), and a handful of Plasticity habits that SolidCraft has no command
//! for (selection modes 1–5, hide/isolate keys, view keys) run as UI actions here.
//!
//! Inside a sketch the scheme steps aside: SolidCraft's sketch keys (L, R, C, D, T, O, X…)
//! work as usual, since Plasticity draws curves without a sketch mode.
//!
//! egui reports the numeric keypad's digits as the top-row digits, so Plasticity's numpad
//! view keys can't be told from its selection-mode keys 1–5: views are on Alt+digit here.

use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;
use crate::prefs::OrbitAround;

pub const SCHEME_PLASTICITY: &str = "plasticity";
pub const SCHEME_SOLIDCRAFT: &str = "solidcraft";
/// The mouse preset the scheme picks (`prefs::NAV_PRESETS`).
pub const NAV: &str = "plasticity";

/// Command keys of the scheme: command id → keys (the first is the one shown).
pub const COMMAND_KEYS: &[(&str, &[&str])] = &[
    ("solid.extrude", &["E"]),
    // Plasticity's O is Offset Curve, which it runs on loose curves in the scene. SolidCraft's
    // curves live inside a sketch, where O is already its own offset, so outside one the key goes
    // to the nearest thing there is: offsetting a face.
    ("solid.offset_face", &["O"]),
    ("solid.fillet", &["B"]),
    // Plasticity switches to chamfer inside Fillet (C); SolidCraft has a separate command.
    ("solid.chamfer", &["Alt+B"]),
    ("solid.combine", &["Q"]),
    ("solid.cut_body", &["C"]),
    ("solid.loft", &["L"]),
    ("solid.sweep", &["Shift+P"]),
    // Plasticity gives its own primitives no key at all (Center Box, Corner Box, Three Point Box
    // and Cylinder all show an empty shortcut), so these are SolidCraft's own. Shift+B and
    // Shift+C, the obvious pair, are Bridge Curve and Center Circle there; Shift+X and Shift+Y
    // Plasticity leaves free.
    ("solid.box", &["Shift+X"]),
    ("solid.cylinder", &["Shift+Y"]),
    // Move covers Plasticity's Move (G) and Rotate (R): the triad does both.
    ("solid.move", &["G", "R"]),
    ("solid.scale", &["S"]),
    ("solid.mirror", &["Alt+X"]),
    ("appearance.assign", &["M"]),
    ("inspect.measure", &["W", "Ctrl+Equals"]),
    ("selection.delete", &["X"]),
    ("file.save_as", &["Ctrl+Shift+S"]),
    ("file.export", &["Ctrl+Shift+E"]),
    ("file.insert_mesh", &["Ctrl+Shift+O"]),
];

/// Plasticity's own curve keys, added beside SolidCraft's sketch keys while a sketch is open.
///
/// Plasticity draws curves with no sketch mode, so these are the keys the hand already knows:
/// `Shift+A` for a line and `Shift+C` for a circle from its centre. SolidCraft's own L and C stay
/// first, so they are what the shortcut list shows; these answer as well. Offset (`O`) and Trim
/// (`T`) already mean the same thing in both, and the rest of Plasticity's curve tools -- the
/// rectangles, the other circles, the outline -- have no key there to borrow.
pub const SKETCH_KEYS: &[(&str, &[&str])] = &[("sketch.line", &["Shift+A"]), ("sketch.circle.center", &["Shift+C"])];

/// UI actions of the scheme: key, action id, what it does (for the shortcut list).
pub const UI_KEYS: &[(&str, &str, &str)] = &[
    ("1", "select.vertex", "Selection mode: control points (vertices)"),
    ("2", "select.edge", "Selection mode: edges"),
    ("3", "select.face", "Selection mode: faces"),
    ("4", "select.solid", "Selection mode: solids"),
    ("5", "select.all_modes", "Selection mode: everything"),
    ("Shift+1", "toggle.vertex", "Selection: toggle control points"),
    ("Shift+2", "toggle.edge", "Selection: toggle edges"),
    ("Shift+3", "toggle.face", "Selection: toggle faces"),
    ("Shift+4", "toggle.solid", "Selection: toggle solids"),
    ("A", "select.everything", "Select all"),
    ("Alt+A", "select.invert", "Invert selection"),
    ("H", "hide", "Hide selected"),
    ("Shift+H", "hide.unselected", "Hide unselected"),
    ("Alt+H", "unhide.all", "Unhide all"),
    ("Ctrl+H", "hide.invert", "Invert hidden"),
    ("Period", "isolate", "Isolate the selection (again: back as it was)"),
    ("Slash", "focus", "Focus: frame the selection, or everything"),
    ("Space", "look", "Navigate to selection (with something selected)"),
    ("Shift+Space", "sketch", "Construction plane from selection (starts a sketch)"),
    ("Alt+1", "view.front", "Front view (numpad 1 in Plasticity)"),
    ("Alt+3", "view.right", "Right view (numpad 3)"),
    ("Alt+7", "view.top", "Top view (numpad 7)"),
    ("Ctrl+Alt+1", "view.back", "Back view (Ctrl+numpad 1)"),
    ("Ctrl+Alt+3", "view.left", "Left view (Ctrl+numpad 3)"),
    ("Ctrl+Alt+7", "view.bottom", "Bottom view (Ctrl+numpad 7)"),
    ("Alt+0", "view.iso", "Isometric view (numpad 0)"),
    ("Alt+5", "view.ortho", "Toggle orthographic / perspective (numpad 5)"),
    ("Alt+Z", "xray", "Toggle x-ray"),
    ("Shift+Z", "wire", "Toggle wireframe"),
    ("Shift+D", "duplicate", "Duplicate, then move the copy"),
    ("Shift+R", "repeat", "Repeat the last command"),
    ("Ctrl+Shift+Z", "redo", "Redo"),
    ("F", "search", "Search commands"),
];

/// Actions that still work inside a sketch (views and search).
fn works_in_sketch(action: &str) -> bool {
    action.starts_with("view.") || matches!(action, "focus" | "search" | "redo")
}

/// The Plasticity scheme is on.
pub fn active(app: &SolveApp) -> bool {
    app.preferences.control_scheme == SCHEME_PLASTICITY
}

/// The scheme's keys apply now (it is on and no sketch is open).
pub fn keys_apply(app: &SolveApp) -> bool {
    active(app) && app.session.active_sketch.is_none()
}

/// A command's keys under the scheme (none: the scheme leaves it without a key).
pub fn command_keys(id: &str) -> &'static [&'static str] {
    COMMAND_KEYS.iter().find(|(c, _)| *c == id).map(|(_, k)| *k).unwrap_or(&[])
}

/// Plasticity's own keys for a sketch command, beside SolidCraft's ([`SKETCH_KEYS`]).
pub fn sketch_keys(id: &str) -> &'static [&'static str] {
    SKETCH_KEYS.iter().find(|(c, _)| *c == id).map(|(_, k)| *k).unwrap_or(&[])
}

/// A key name as people read it (`Period` → `.`).
pub fn readable(key: &str) -> String {
    key.replace("Period", ".").replace("Slash", "/").replace("Equals", "=")
}

/// Keys the scheme's UI actions take (not bindable to commands while it is on).
pub fn ui_key(key: &str) -> bool {
    UI_KEYS.iter().any(|(k, _, _)| *k == key)
}

/// Switch the control scheme: Plasticity also picks its mouse preset and orbits about the
/// point under the cursor; SolidCraft goes back to the default mouse preset.
pub fn apply(app: &mut SolveApp, scheme: &str) {
    if scheme == SCHEME_PLASTICITY {
        app.preferences.control_scheme = SCHEME_PLASTICITY.into();
        app.preferences.nav = NAV.into();
        app.preferences.zoom_reverse = false;
        app.preferences.zoom_to_cursor = true;
        app.preferences.orbit_around = OrbitAround::Cursor;
    } else {
        let d = crate::prefs::Prefs::default();
        app.preferences.control_scheme = SCHEME_SOLIDCRAFT.into();
        app.preferences.nav = d.nav;
        app.preferences.zoom_reverse = d.zoom_reverse;
        app.preferences.orbit_around = d.orbit_around;
    }
}

/// Run a key press as a Plasticity action. True when the scheme took the key (the caller
/// stops there); false leaves it to the normal shortcut handling.
pub fn handle_key(app: &mut SolveApp, ctx: &egui::Context, name: &str) -> bool {
    if !active(app) {
        return false;
    }
    let in_sketch = app.session.active_sketch.is_some();
    // S runs Scale here (SolidCraft opens its command box on S; F does that instead).
    if name == "S" && !in_sketch {
        crate::keymap::press(app, "S", false);
        return true;
    }
    let Some(&(_, action, _)) = UI_KEYS.iter().find(|(k, _, _)| *k == name) else { return false };
    if in_sketch && !works_in_sketch(action) {
        return false;
    }
    // Space with nothing selected is the hold-to-pan key.
    if action == "look" && app.session.selection.is_empty() {
        return false;
    }
    run(app, ctx, action);
    true
}

/// Selection modes: which picks are switched off (`app.ui.pick_off`) and whether a face
/// picks its body (`app.ui.pick_bodies`).
const KINDS: [&str; 3] = ["vertices", "edges", "faces"];

fn set_mode(app: &mut SolveApp, keep: Option<&str>, bodies: bool) {
    app.ui.pick_off.retain(|k| !KINDS.contains(&k.as_str()));
    if let Some(keep) = keep {
        for k in KINDS {
            if k != keep {
                app.ui.pick_off.push(k.to_string());
            }
        }
    }
    app.ui.pick_bodies = bodies;
}

fn toggle_kind(app: &mut SolveApp, kind: &str) {
    if app.ui.pick_off.iter().any(|k| k == kind) {
        app.ui.pick_off.retain(|k| k != kind);
    } else {
        app.ui.pick_off.push(kind.to_string());
        // Leave at least one kind on, as Plasticity does.
        if KINDS.iter().all(|k| app.ui.pick_off.iter().any(|o| o == k)) && !app.ui.pick_bodies {
            app.ui.pick_off.retain(|k| k != kind);
        }
    }
}

fn mode_name(app: &SolveApp) -> String {
    if app.ui.pick_bodies {
        return "solids".into();
    }
    let on: Vec<&str> = [("vertices", "control points"), ("edges", "edges"), ("faces", "faces")]
        .iter()
        .filter(|(k, _)| !app.ui.pick_off.iter().any(|o| o == k))
        .map(|(_, n)| *n)
        .collect();
    if on.len() == 3 { "everything".into() } else { on.join(" + ") }
}

fn all_bodies(app: &SolveApp) -> Vec<String> {
    app.session.world_state().map(|st| st.bodies.iter().map(|b| b.name.clone()).collect()).unwrap_or_default()
}

/// Bodies the selection touches (a body, or a face or edge of one).
fn selected_bodies(app: &SolveApp) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in &app.session.selection {
        let b = match s {
            Sel::Body { name } => name,
            Sel::Face { body, .. } | Sel::Edge { body, .. } => body,
            _ => continue,
        };
        if !out.contains(b) {
            out.push(b.clone());
        }
    }
    out
}

fn select_bodies(app: &mut SolveApp, names: &[String]) {
    let items: Vec<Value> = names.iter().map(|b| json!({"type": "body", "name": b})).collect();
    if items.is_empty() {
        let _ = app.run("select.clear", json!({}));
    } else {
        let _ = app.run("select.set", json!({ "items": items }));
    }
}

fn show_all(app: &mut SolveApp) {
    app.hidden_before_isolate = None;
    let _ = app.run("browser.visibility", json!({"folder": "bodies", "visible": true}));
    app.session.visibility.hidden_sketches.clear();
    app.session.visibility.hidden_folders.clear();
}

thread_local! {
    /// The visual style to go back to when x-ray or wireframe is toggled off.
    static STYLE_BEFORE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn toggle_style(app: &mut SolveApp, style: u8) {
    if app.ui.visual_style == style {
        app.ui.visual_style = STYLE_BEFORE.with(std::cell::Cell::get);
    } else {
        STYLE_BEFORE.with(|s| s.set(app.ui.visual_style));
        app.ui.visual_style = style;
    }
}

/// Run a Plasticity UI action by id (see [`UI_KEYS`]).
pub fn run(app: &mut SolveApp, ctx: &egui::Context, action: &str) {
    match action {
        "select.vertex" => set_mode(app, Some("vertices"), false),
        "select.edge" => set_mode(app, Some("edges"), false),
        "select.face" => set_mode(app, Some("faces"), false),
        "select.solid" => set_mode(app, None, true),
        "select.all_modes" => set_mode(app, None, false),
        "toggle.vertex" => toggle_kind(app, "vertices"),
        "toggle.edge" => toggle_kind(app, "edges"),
        "toggle.face" => toggle_kind(app, "faces"),
        "toggle.solid" => app.ui.pick_bodies = !app.ui.pick_bodies,
        "select.everything" => {
            let all = all_bodies(app);
            select_bodies(app, &all);
        }
        "select.invert" => {
            let picked = selected_bodies(app);
            let rest: Vec<String> = all_bodies(app).into_iter().filter(|b| !picked.contains(b)).collect();
            select_bodies(app, &rest);
        }
        // Hiding or unhiding by hand makes the remembered state stale, so isolate starts over.
        "hide" => {
            app.hidden_before_isolate = None;
            crate::context_menu::toggle_visibility(app);
        }
        "hide.unselected" => {
            app.hidden_before_isolate = None;
            let keep = selected_bodies(app);
            if !keep.is_empty() {
                app.session.visibility.hidden_bodies = all_bodies(app).into_iter().filter(|b| !keep.contains(b)).collect();
            }
        }
        "unhide.all" => show_all(app),
        "hide.invert" => {
            app.hidden_before_isolate = None;
            let hidden = app.session.visibility.hidden_bodies.clone();
            app.session.visibility.hidden_bodies = all_bodies(app).into_iter().filter(|b| !hidden.contains(b)).collect();
        }
        // Isolate is a toggle, and what pressing it again comes back to is what was hidden
        // before, not everything: isolating one body of a design that already had others put
        // away and then coming back out should not drag those others into view.
        "isolate" => {
            if let Some(before) = app.hidden_before_isolate.take() {
                app.session.visibility.hidden_bodies = before;
            } else {
                let keep = selected_bodies(app);
                if keep.is_empty() {
                    show_all(app);
                } else {
                    app.hidden_before_isolate = Some(app.session.visibility.hidden_bodies.clone());
                    app.session.visibility.hidden_bodies = all_bodies(app).into_iter().filter(|b| !keep.contains(b)).collect();
                    // The camera frames what is left; coming back out leaves it where it is, so
                    // the thing that was being looked at stays in view.
                    app.animate_view("fit");
                }
            }
        }
        "focus" => app.animate_view("fit"),
        "look" => crate::viewport::look_at_selection(app),
        "sketch" => app.start("sketch.create"),
        "view.ortho" => app.ui.perspective = !app.ui.perspective,
        v if v.starts_with("view.") => app.animate_view(&v["view.".len()..]),
        // SolidCraft's styles: 0 shaded with edges, 1 shaded, 2 wireframe, 3 hidden edges shown.
        "xray" => toggle_style(app, 3),
        "wire" => toggle_style(app, 2),
        "duplicate" => {
            if crate::edit_clipboard::copy(app) && crate::edit_clipboard::paste(app, None) {
                app.start("solid.move");
            }
        }
        "repeat" => {
            if let Some((id, _)) = app.last_command.clone() {
                app.start(&id);
            }
        }
        "redo" => drop(app.run("edit.redo", json!({}))),
        "search" => {
            let at = ctx.input(|i| i.pointer.latest_pos());
            crate::shortcut_box::open(app, at);
        }
        _ => {}
    }
    if action.starts_with("select.") && action != "select.everything" && action != "select.invert" || action.starts_with("toggle.") {
        let m = mode_name(app);
        app.set_status(format!("Select: {m}"), false);
    }
}

/// The scheme's own keys, listed in Preferences > Keyboard Shortcuts above the command keys.
///
/// They are not command shortcuts, so the list below cannot show them: a selection mode or a
/// view key runs here rather than through the registry. Without this, the only place to read
/// them is the handbook.
pub fn keys_section(app: &SolveApp, ui: &mut egui::Ui) {
    if !active(app) {
        return;
    }
    let title = crate::i18n::tr("keymap.plasticity_keys").to_owned();
    let note = crate::i18n::tr("keymap.plasticity_keys_note").to_owned();
    crate::ui_kit::settings_section(ui, "plasticity_keys", &title, |ui| {
        crate::ui_kit::text(ui, &note, crate::ui_kit::Text::Note("plasticity_keys_note"));
        egui::Grid::new("plasticity_keys_grid").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
            for (key, _, what) in UI_KEYS {
                ui.label(egui::RichText::new(crate::keymap::display(&readable(key))).strong());
                ui.label(crate::i18n::tr_label(what));
                ui.end_row();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{commands_for, effective, reserved};
    use crate::prefs::{NavButton, nav_preset};
    use crate::viewport::NavMode;

    fn app() -> SolveApp {
        SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default())
    }

    #[test]
    fn scheme_replaces_the_default_keys_and_custom_keys_still_win() {
        let mut a = app();
        assert_eq!(effective(&a, "solid.fillet").as_deref(), Some("F"));
        apply(&mut a, SCHEME_PLASTICITY);
        assert_eq!(effective(&a, "solid.fillet").as_deref(), Some("B"));
        assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("E"));
        // Move answers to both G and R; H (Hole in SolidCraft) is a UI key now, not a command key.
        assert!(commands_for(&a, "G").contains(&"solid.move") && commands_for(&a, "R").contains(&"solid.move"));
        assert!(commands_for(&a, "H").is_empty());
        a.keymap.custom.insert("solid.fillet".into(), vec!["Shift+K".to_string()]);
        assert_eq!(effective(&a, "solid.fillet").as_deref(), Some("Shift+K"));
    }

    /// The scheme must not answer a key with a different command from the one Plasticity runs.
    /// Shift+B and Shift+C are Bridge Curve and Center Circle there, so the primitives, which
    /// Plasticity gives no key at all, keep off them.
    #[test]
    fn the_primitives_keep_off_the_keys_plasticity_already_uses() {
        let mut a = app();
        apply(&mut a, SCHEME_PLASTICITY);
        assert_eq!(effective(&a, "solid.box").as_deref(), Some("Shift+X"));
        assert_eq!(effective(&a, "solid.cylinder").as_deref(), Some("Shift+Y"));
        for taken in ["Shift+B", "Shift+C"] {
            assert!(commands_for(&a, taken).is_empty(), "{taken} is Plasticity's, not ours: {:?}", commands_for(&a, taken));
        }
        // And nothing else in the scheme has quietly taken them either.
        for (_, keys) in COMMAND_KEYS {
            assert!(!keys.contains(&"Shift+B") && !keys.contains(&"Shift+C"), "{keys:?}");
        }
        assert!(!ui_key("Shift+B") && !ui_key("Shift+C"));
    }

    /// Inside a sketch SolidCraft's own keys stay, and Plasticity's curve keys answer as well:
    /// it draws curves with no sketch mode, so `Shift+A` and `Shift+C` are what the hand knows.
    #[test]
    fn a_sketch_takes_plasticitys_curve_keys_beside_solidcrafts() {
        let mut a = app();
        apply(&mut a, SCHEME_PLASTICITY);
        a.run("sketch.create", serde_json::json!({"plane": "XY"})).unwrap();
        assert!(a.session.active_sketch.is_some());
        // The one shown is still SolidCraft's.
        assert_eq!(effective(&a, "sketch.line").as_deref(), Some("L"));
        assert_eq!(effective(&a, "sketch.circle.center").as_deref(), Some("C"));
        assert!(commands_for(&a, "Shift+A").contains(&"sketch.line"), "{:?}", commands_for(&a, "Shift+A"));
        assert!(commands_for(&a, "Shift+C").contains(&"sketch.circle.center"), "{:?}", commands_for(&a, "Shift+C"));
        assert!(commands_for(&a, "L").contains(&"sketch.line") && commands_for(&a, "C").contains(&"sketch.circle.center"));
        // Outside one they mean nothing of the sort: Shift+C is free and L is Loft again.
        a.run("sketch.finish", serde_json::json!({})).unwrap();
        assert!(commands_for(&a, "Shift+A").is_empty(), "{:?}", commands_for(&a, "Shift+A"));
        assert!(!commands_for(&a, "Shift+C").contains(&"sketch.circle.center"));
        assert!(commands_for(&a, "L").contains(&"solid.loft"));
    }

    #[test]
    fn inside_a_sketch_the_sketch_keys_come_back() {
        let mut a = app();
        apply(&mut a, SCHEME_PLASTICITY);
        assert!(commands_for(&a, "L").contains(&"solid.loft"));
        a.session.active_sketch = Some(1);
        assert!(!keys_apply(&a));
        assert_eq!(effective(&a, "sketch.line").as_deref(), Some("L"));
        assert!(!commands_for(&a, "L").contains(&"solid.loft"));
    }

    #[test]
    fn keys_do_not_collide() {
        let mut seen: Vec<&str> = Vec::new();
        for (id, keys) in COMMAND_KEYS {
            assert!(solidcraft_engine::find_command(id).is_some(), "no command {id}");
            for k in *keys {
                assert!(!seen.contains(k), "{k} is bound twice");
                assert!(!ui_key(k), "{k} is both a command key and a UI key");
                seen.push(k);
            }
        }
        let mut ui: Vec<&str> = UI_KEYS.iter().map(|(k, _, _)| *k).collect();
        ui.sort_unstable();
        let n = ui.len();
        ui.dedup();
        assert_eq!(ui.len(), n, "a UI key is listed twice");
    }

    /// Isolate is a toggle, and what it comes back to is what was hidden before it, not
    /// everything: a body put away by hand before isolating stays away afterwards.
    #[test]
    fn isolating_again_comes_back_to_what_was_hidden_before() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let ctx = egui::Context::default();
        for at in [0.0, 50.0, 100.0] {
            h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [at, 0, 0]})).unwrap();
        }
        h.settle();
        assert_eq!(all_bodies(&h.app).len(), 3, "three to hide between");
        // One put away by hand, before anything is isolated.
        h.app.session.visibility.hidden_bodies = vec!["Body3".to_string()];
        select_bodies(&mut h.app, &["Body1".to_string()]);
        run(&mut h.app, &ctx, "isolate");
        let mut hidden = h.app.session.visibility.hidden_bodies.clone();
        hidden.sort();
        assert_eq!(hidden, ["Body2", "Body3"], "everything but the selection is away");
        // Again: back as it was, which is not the same as everything shown.
        run(&mut h.app, &ctx, "isolate");
        assert_eq!(h.app.session.visibility.hidden_bodies, ["Body3"], "the one put away by hand stays away");
        // And again isolates afresh, so the key keeps toggling.
        run(&mut h.app, &ctx, "isolate");
        let mut hidden = h.app.session.visibility.hidden_bodies.clone();
        hidden.sort();
        assert_eq!(hidden, ["Body2", "Body3"]);
    }

    /// Showing everything by hand clears what isolate would come back to, so the next isolate
    /// remembers the state it actually found rather than a stale one.
    #[test]
    fn showing_everything_forgets_what_isolate_would_return_to() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let ctx = egui::Context::default();
        for at in [0.0, 50.0] {
            h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [at, 0, 0]})).unwrap();
        }
        h.settle();
        h.app.session.visibility.hidden_bodies = vec!["Body2".to_string()];
        select_bodies(&mut h.app, &["Body1".to_string()]);
        run(&mut h.app, &ctx, "isolate");
        run(&mut h.app, &ctx, "unhide.all");
        assert!(h.app.session.visibility.hidden_bodies.is_empty(), "everything is shown");
        assert!(h.app.hidden_before_isolate.is_none(), "and there is nothing stale to come back to");
        select_bodies(&mut h.app, &["Body1".to_string()]);
        run(&mut h.app, &ctx, "isolate");
        run(&mut h.app, &ctx, "isolate");
        assert!(h.app.session.visibility.hidden_bodies.is_empty(), "it comes back to everything shown, as it found it");
    }

    /// Shift+D is Plasticity's Duplicate: a copy of the selected solid stands where the original
    /// is and Move opens on it, so dragging or typing a distance carries the copy away. This base
    /// copies and pastes bodies itself (through `component.copy_bodies`, which places them in the
    /// component they are pasted into); what the scheme adds is the Move that follows.
    #[test]
    fn duplicate_leaves_a_copy_with_move_open_on_it() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let ctx = egui::Context::default();
        apply(&mut h.app, SCHEME_PLASTICITY);
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
        h.settle();
        let before = all_bodies(&h.app);
        assert_eq!(before.len(), 1);
        select_bodies(&mut h.app, &before);
        run(&mut h.app, &ctx, "duplicate");
        h.settle();
        assert_eq!(all_bodies(&h.app).len(), 2, "the copy stands beside the original");
        assert!(
            matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Move { .. })),
            "Move is open on the copy: {:?}",
            h.app.dialog.as_ref().map(|d| &d.kind)
        );
    }

    #[test]
    fn reserved_keys_follow_the_scheme() {
        let mut a = app();
        assert!(reserved(&a, "S") && !reserved(&a, "1"));
        apply(&mut a, SCHEME_PLASTICITY);
        assert!(!reserved(&a, "S") && reserved(&a, "1") && reserved(&a, "Escape"));
    }

    #[test]
    fn mouse_preset_and_switching_back() {
        let mut a = app();
        apply(&mut a, SCHEME_PLASTICITY);
        let p = nav_preset(&a.preferences.nav);
        assert_eq!(p.id, NAV);
        assert_eq!(p.mode(NavButton::Middle, egui::Modifiers::NONE), Some(NavMode::Orbit));
        assert_eq!(p.mode(NavButton::Middle, egui::Modifiers::SHIFT), Some(NavMode::Pan));
        assert_eq!(p.mode(NavButton::Middle, egui::Modifiers::CTRL), Some(NavMode::Zoom));
        assert_eq!(p.mode(NavButton::Right, egui::Modifiers::NONE), Some(NavMode::Orbit));
        assert_eq!(a.preferences.orbit_around, OrbitAround::Cursor);
        apply(&mut a, SCHEME_SOLIDCRAFT);
        assert_eq!(a.preferences.nav, crate::prefs::Prefs::default().nav);
        assert_eq!(effective(&a, "solid.fillet").as_deref(), Some("F"));
    }

    #[test]
    fn selection_modes() {
        let mut a = app();
        let ctx = egui::Context::default();
        apply(&mut a, SCHEME_PLASTICITY);
        run(&mut a, &ctx, "select.edge");
        assert!(a.ui.pick_off.iter().any(|k| k == "faces") && a.ui.pick_off.iter().any(|k| k == "vertices"));
        assert!(!a.ui.pick_off.iter().any(|k| k == "edges") && !a.ui.pick_bodies);
        run(&mut a, &ctx, "toggle.face");
        assert!(!a.ui.pick_off.iter().any(|k| k == "faces"));
        run(&mut a, &ctx, "select.solid");
        assert!(a.ui.pick_bodies);
        run(&mut a, &ctx, "select.all_modes");
        assert!(!a.ui.pick_bodies && KINDS.iter().all(|k| !a.ui.pick_off.iter().any(|o| o == k)));
        // The last kind can't be switched off.
        run(&mut a, &ctx, "select.vertex");
        run(&mut a, &ctx, "toggle.vertex");
        assert!(!a.ui.pick_off.iter().any(|k| k == "vertices"));
    }
}
