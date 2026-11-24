//! New Part and New Assembly inside a design (#6892, experimental). Under Parts and assemblies,
//! with a saved design shown, New makes a component in that design instead of a file of its
//! own: it is in the Browser at once and active, so what is modelled next goes into it. With a
//! sub-assembly edited in place, the design shown is that sub-assembly, so the new part goes
//! into it.
//!
//! Once it has a body and is no longer the active component (the design, or another component,
//! was activated), it is stored in the design as an internal component (`component.make_internal`,
//! which moves its history to a part of its own). Until then it is a component of the design
//! like any other, saved with it.

use serde_json::json;

use crate::SolveApp;

/// A component made by New, waiting to be stored as an internal component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    /// The design it was made in ([`solidcraft_engine::Session::uid`]).
    pub session: u64,
    pub component: u64,
}

fn design_name(app: &SolveApp) -> String {
    crate::documents::tab_name(app, &app.session)
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
    // Named after its kind, with the first number no component here has: the name is the stored
    // file's name too (`part1.scprt`).
    let stem = if kind == "assembly" { "assembly" } else { "part" };
    let taken = |name: &str| app.session.doc.components.iter().any(|c| c.name.eq_ignore_ascii_case(name));
    let name = (1..10_000).map(|n| format!("{stem}{n}")).find(|name| !taken(name)).unwrap_or_else(|| stem.to_string());
    // Always in the design itself, whichever component is active.
    match app.run("component.create", json!({"parent": 0, "name": name})) {
        Ok(reply) => {
            if let Some(component) = reply["component"].as_u64() {
                app.new_internal.push(Pending { session: app.session.uid, component });
                let name = reply["name"].as_str().unwrap_or_default().to_string();
                app.set_status(solidcraft_ui_text::format("new_internal.made", &[("name", &name), ("design", &design_name(app))]), false);
            }
        }
        Err(e) => app.set_status(e, true),
    }
    true
}

/// Store the new components of the design shown that are ready: one with a body that is no
/// longer the active component. One that is gone (undone, deleted) is forgotten.
pub(crate) fn settle(app: &mut SolveApp) {
    if app.new_internal.is_empty() {
        return;
    }
    let session = app.session.uid;
    let here: Vec<u64> = app.new_internal.iter().filter(|pending| pending.session == session).map(|pending| pending.component).collect();
    for component in here {
        let exists = app.session.doc.components.iter().any(|c| c.id == component);
        let linked = app.session.doc.links.contains_key(&component);
        if !exists || linked {
            app.new_internal.retain(|pending| !(pending.session == session && pending.component == component));
            continue;
        }
        let active = app.session.active_component == component || app.session.doc.component_within(app.session.active_component, component);
        if active || app.session.path.is_none() || crate::browser::bodies_of_component(app, component).is_empty() {
            continue;
        }
        app.new_internal.retain(|pending| !(pending.session == session && pending.component == component));
        let _language = solidcraft_ui_text::scope(&app.preferences.language);
        let name = app.session.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default();
        match app.run("component.make_internal", json!({"component": component})) {
            Ok(_) => app.set_status(solidcraft_ui_text::format("new_internal.stored", &[("name", &name), ("design", &design_name(app))]), false),
            Err(e) => app.set_status(e, true),
        }
    }
}
