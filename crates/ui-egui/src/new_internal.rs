//! New Part and New Assembly inside a design (#6892, experimental). Under Parts and assemblies,
//! with a saved design shown, New makes a component in that design instead of a file of its
//! own: it is in the Browser at once and active, so what is modelled next goes into it. With a
//! sub-assembly edited in place, the design shown is that sub-assembly, so the new part goes
//! into it. A new part made while a new assembly is active goes into that assembly.
//!
//! When the design is saved, or the new component is finished (Finish Edit in the Browser's
//! band, or another component is activated), a window asks for its name and it is stored in the
//! design as an internal component (`component.make_internal`, which moves its history to a
//! part of its own). One with no body yet has nothing to store and is asked about later.

use serde_json::json;

use crate::SolveApp;
use crate::ui_kit;

/// A component made by New, waiting to be stored as an internal component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    /// The design it was made in ([`solidcraft_engine::Session::uid`]).
    pub session: u64,
    pub component: u64,
    /// Made by New Assembly: a new part made while it is active goes into it.
    pub assembly: bool,
}

/// The name being asked for, for one new component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub session: u64,
    pub component: u64,
    pub name: String,
    /// Asked by Save: the design is saved once this is answered.
    pub then_save: bool,
    /// The name field takes the keyboard when the window first shows, so a name can be typed at once.
    pub focus: bool,
}

/// The new components waiting, and the name being asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub pending: Vec<Pending>,
    pub prompt: Option<Prompt>,
}

fn design_name(app: &SolveApp) -> String {
    crate::documents::tab_name(app, &app.session)
}

fn forget(app: &mut SolveApp, session: u64, component: u64) {
    app.new_internal.pending.retain(|pending| !(pending.session == session && pending.component == component));
}

/// The new component being modelled in the design shown: the active component, or the one the
/// active component is inside.
pub(crate) fn active(app: &SolveApp) -> Option<Pending> {
    let now = app.session.active_component;
    app.new_internal
        .pending
        .iter()
        .find(|pending| pending.session == app.session.uid && (pending.component == now || app.session.doc.component_within(now, pending.component)))
        .copied()
}

/// Make the new part or assembly in the design shown, when there is one to make it in. False
/// when New makes a design of its own as before: on Home, in another environment, or with a
/// design that has no file yet.
pub(crate) fn make(app: &mut SolveApp, kind: &str) -> bool {
    if app.home.open || !crate::prefs::parts_and_assemblies(&app.preferences) || app.session.path.is_none() || app.session.read_only {
        return false;
    }
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    // A part holds no parts: its assembly is where a new one goes.
    if app.session.design_kind == solidcraft_engine::io::DesignKind::Part {
        app.set_status(solidcraft_ui_text::tr("new_internal.in_part").to_string(), true);
        return true;
    }
    // Named after its kind, with the first number no component here has: the name offered when
    // it is stored, which is the stored file's name too (`part1.scprt`).
    let assembly = kind == "assembly";
    let stem = if assembly { "assembly" } else { "part" };
    let taken = |name: &str| app.session.doc.components.iter().any(|c| c.name.eq_ignore_ascii_case(name));
    let name = (1..10_000).map(|n| format!("{stem}{n}")).find(|name| !taken(name)).unwrap_or_else(|| stem.to_string());
    // In the design itself, or in the new assembly being built.
    let parent = active(app).filter(|pending| pending.assembly).map_or(0, |pending| pending.component);
    match app.run("component.create", json!({"parent": parent, "name": name})) {
        Ok(reply) => {
            if let Some(component) = reply["component"].as_u64() {
                app.new_internal.pending.push(Pending { session: app.session.uid, component, assembly });
                let name = reply["name"].as_str().unwrap_or_default().to_string();
                let made = if assembly { "new_internal.made_assembly" } else { "new_internal.made" };
                app.set_status(solidcraft_ui_text::format(made, &[("name", &name), ("design", &design_name(app))]), false);
            }
        }
        Err(e) => app.set_status(e, true),
    }
    true
}

/// The new components of the design shown that can be stored now: those with a body that are
/// not inside another new component (that one brings them with it). Ones that are gone (undone,
/// deleted) or stored already are forgotten.
fn ready(app: &mut SolveApp) -> Vec<Pending> {
    let session = app.session.uid;
    let here: Vec<Pending> = app.new_internal.pending.iter().filter(|pending| pending.session == session).copied().collect();
    let mut found = Vec::new();
    for pending in &here {
        let exists = app.session.doc.components.iter().any(|c| c.id == pending.component);
        if !exists || app.session.doc.links.contains_key(&pending.component) {
            forget(app, session, pending.component);
            continue;
        }
        let inside_another =
            here.iter().any(|outer| outer.component != pending.component && app.session.doc.component_within(pending.component, outer.component));
        if !inside_another && !crate::browser::bodies_of_component(app, pending.component).is_empty() {
            found.push(*pending);
        }
    }
    found
}

fn ask(app: &mut SolveApp, pending: Pending, then_save: bool) {
    let name = app.session.doc.components.iter().find(|c| c.id == pending.component).map(|c| c.name.clone()).unwrap_or_default();
    app.new_internal.prompt = Some(Prompt { session: pending.session, component: pending.component, name, then_save, focus: true });
}

/// Ask for the name of a new component that was finished: one with a body that is no longer
/// being modelled.
pub(crate) fn settle(app: &mut SolveApp) {
    if app.new_internal.pending.is_empty() || app.new_internal.prompt.is_some() || app.session.path.is_none() {
        return;
    }
    let being_modelled = active(app).map(|pending| pending.component);
    if let Some(pending) = ready(app).into_iter().find(|pending| Some(pending.component) != being_modelled) {
        ask(app, pending, false);
    }
}

/// Save was asked for: a new component with a body is named and stored first. True when the
/// save waits for that answer.
pub(crate) fn before_save(app: &mut SolveApp) -> bool {
    if app.new_internal.prompt.is_some() {
        return true;
    }
    if app.new_internal.pending.is_empty() || app.session.path.is_none() {
        return false;
    }
    match ready(app).into_iter().next() {
        Some(pending) => {
            ask(app, pending, true);
            true
        }
        None => false,
    }
}

/// Finish the new component being modelled, as Finish Edit in the Browser's band does: the
/// design is activated again, and the name is asked for.
pub(crate) fn finish(app: &mut SolveApp) {
    if let Err(e) = app.run("component.activate", json!({"component": 0})) {
        app.set_status(e, true);
    }
}

/// Store the component under the name given, and save when Save asked.
fn store(app: &mut SolveApp, prompt: &Prompt) {
    forget(app, prompt.session, prompt.component);
    if prompt.session != app.session.uid {
        return;
    }
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let name = prompt.name.trim().to_string();
    let current = app.session.doc.components.iter().find(|c| c.id == prompt.component).map(|c| c.name.clone()).unwrap_or_default();
    let mut done = Ok(());
    if !name.is_empty() && name != current {
        done = app.run("component.rename", json!({"component": prompt.component, "name": name})).map(|_| ());
    }
    // Stored, it is a linked component: the design is what is modelled in from here.
    let now = app.session.active_component;
    if done.is_ok() && (now == prompt.component || app.session.doc.component_within(now, prompt.component)) {
        done = app.run("component.activate", json!({"component": 0})).map(|_| ());
    }
    match done.and_then(|()| app.run("component.make_internal", json!({"component": prompt.component}))) {
        Ok(_) => {
            let name = if name.is_empty() { current } else { name };
            app.set_status(solidcraft_ui_text::format("new_internal.stored", &[("name", &name), ("design", &design_name(app))]), false);
        }
        Err(e) => app.set_status(e, true),
    }
}

/// The window that asks for the new component's name.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut prompt) = app.new_internal.prompt.clone() else { return };
    if prompt.session != app.session.uid {
        // Asked in another design's tab: it is asked again when that design is shown.
        app.new_internal.prompt = None;
        return;
    }
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let assembly =
        app.new_internal.pending.iter().any(|pending| pending.session == prompt.session && pending.component == prompt.component && pending.assembly);
    let title = solidcraft_ui_text::tr(if assembly { "new_internal.name_title_assembly" } else { "new_internal.name_title" });
    let detail = solidcraft_ui_text::format("new_internal.name_detail", &[("design", &design_name(app))]);
    let valid = !prompt.name.trim().is_empty();
    let mut entered = false;
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_new_internal_name"), "component", title)
        .actions(solidcraft_ui_text::tr("new_internal.store"), solidcraft_ui_text::tr("ui.cancel"), None)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Report("new_internal_name", false));
            let (enter, field) = ui_kit::text_field_reserving(ui, &mut prompt.name, solidcraft_ui_text::tr("new_internal.name"), 0.0);
            crate::scenario::publish_target("new_internal:name", field.rect);
            if prompt.focus {
                field.request_focus();
                prompt.focus = false;
            }
            entered = enter;
        });
    let escaped = ctx.input(|x| x.key_pressed(egui::Key::Escape));
    if matches!(action, ui_kit::Action::Ok) || (entered && valid) {
        app.new_internal.prompt = None;
        store(app, &prompt);
        if prompt.then_save {
            crate::toolbar::save(app);
        }
    } else if matches!(action, ui_kit::Action::Cancel) || closed || escaped {
        // Left as a component of the design, as New Component makes one: Make Internal stores
        // it later. A save that asked goes on without it.
        app.new_internal.prompt = None;
        forget(app, prompt.session, prompt.component);
        if escaped {
            app.esc_handled = true;
        }
        if prompt.then_save {
            crate::toolbar::save(app);
        }
    } else {
        app.new_internal.prompt = Some(prompt);
    }
}
