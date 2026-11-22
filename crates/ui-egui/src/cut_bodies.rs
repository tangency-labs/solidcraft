//! "Bodies to Cut": the list a Cut shows when the design has bodies in more than one component
//! (#20647).
//!
//! A Revolve, Loft, Sweep or primitive window set to Cut lists every shown solid body with a
//! checkbox. The ticks start as the bodies the Cut takes without the list (the shown scope,
//! `solidcraft_engine::shown_scope`, else every listed body), and OK with the list untouched sends
//! nothing, so the Cut does what it did before. Once the user changes a tick, OK sends the ticked
//! bodies as the feature's `targets`, whichever component each is in. With every body in one
//! component there is no list. Extrude has its own Target Bodies pick and is left alone. A feature
//! being edited keeps the targets it was saved with.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, OPS};
use crate::ui_kit;

/// Where the window keeps the bodies the user ticked, once they change one (removed before the
/// commands are built). Absent: the list is untouched.
pub(crate) const SKIPPED: &str = "cut_bodies_chosen";

/// The window is a Cut that takes its bodies from this list.
fn cutting(kind: &Kind) -> bool {
    match kind {
        Kind::Revolve { operation, .. } | Kind::Loft { operation, .. } | Kind::Sweep { operation, .. } | Kind::Primitive { operation, .. } => {
            OPS.get(*operation) == Some(&"cut")
        }
        _ => false,
    }
}

/// Every shown solid body with its component's id and name.
fn shown_solids(app: &SolveApp) -> Vec<(String, u64, String)> {
    let session = &app.session;
    let state = session.model.state();
    let owners = session.doc.model_body_components(&state);
    let mut out = Vec::new();
    for (body, owner) in state.bodies.iter().zip(owners) {
        if body.envelope || body.is_surface() || !session.body_visible(&body.name) {
            continue;
        }
        let component = session.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.clone()).unwrap_or_default();
        out.push((body.name.clone(), owner, component));
    }
    out
}

/// The list to show: every shown solid body with the name of its component, when the bodies
/// lie in two or more components; empty otherwise.
fn candidates(app: &SolveApp) -> Vec<(String, String)> {
    let shown = shown_solids(app);
    let components: std::collections::BTreeSet<u64> = shown.iter().map(|(_, owner, _)| *owner).collect();
    if components.len() < 2 {
        return Vec::new();
    }
    shown.into_iter().map(|(body, _, component)| (body, component)).collect()
}

/// The bodies the user ticked, if they changed the list.
fn chosen(extra: &serde_json::Map<String, Value>) -> Option<Vec<String>> {
    extra.get(SKIPPED).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

/// The ticks of an untouched list: what the Cut takes without it.
fn untouched(app: &SolveApp, list: &[(String, String)]) -> Vec<String> {
    match solidcraft_engine::shown_scope(&app.session) {
        Some(shown) => list.iter().map(|(body, _)| body.clone()).filter(|body| shown.contains(body)).collect(),
        None => list.iter().map(|(body, _)| body.clone()).collect(),
    }
}

/// The "Bodies to Cut" section, below the Operation row.
pub(crate) fn rows(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    if d.editing.is_some() || !cutting(&d.kind) {
        return;
    }
    let list = candidates(app);
    if list.is_empty() {
        return;
    }
    let mut ticked = chosen(&d.extra).unwrap_or_else(|| untouched(app, &list));
    let mut changed = false;
    ui_kit::section(ui, "cut_bodies", "ui.cut_bodies", true, |ui| {
        for (body, component) in &list {
            let mut on = ticked.contains(body);
            let label = if component.is_empty() { body.clone() } else { format!("{body} ({component})") };
            if ui_kit::checkbox(ui, &label, &mut on).changed() {
                ticked.retain(|n| n != body);
                if on {
                    ticked.push(body.clone());
                }
                changed = true;
            }
        }
    });
    if changed {
        d.extra.insert(SKIPPED.into(), json!(ticked));
    }
}

/// The ticked bodies as the `targets` of a new Cut that names none, once the user has changed the
/// list. Nothing changes when the window shows no list or the list is untouched.
pub(crate) fn scope(app: &SolveApp, d: &Dialog, cmds: &mut [(String, Value)]) -> Result<(), String> {
    if d.editing.is_some() || !cutting(&d.kind) {
        return Ok(());
    }
    // Untouched, the Cut takes its bodies as it did before the list.
    let Some(picked) = chosen(&d.extra) else { return Ok(()) };
    // A changed list holds even when its bodies are no longer listed (one was hidden while the
    // window was open): only ticked bodies still shown are cut, and none left is refused, never
    // a fall back to bodies the user did not tick.
    // Every shown solid, not only the list on show: hiding a body can leave one component
    // shown, and the ticked bodies still shown are still cut.
    let ticked: Vec<String> = shown_solids(app).into_iter().map(|(body, _, _)| body).filter(|body| picked.contains(body)).collect();
    for (_, p) in cmds {
        let cut = p.get("operation").and_then(Value::as_str) == Some("cut");
        let named = ["targets", "participants"].iter().any(|k| p.get(*k).and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
        if !cut || named {
            continue;
        }
        if ticked.is_empty() {
            return Err(solidcraft_ui_text::tr("dialog.no_cut_body").to_string());
        }
        if let Some(map) = p.as_object_mut() {
            map.insert("targets".into(), Value::from(ticked.clone()));
        }
    }
    Ok(())
}
