//! Recovery components on open (#6892): an internal component (a part stored inside the design's
//! file) whose unpacked file beside the design differs from what the design's file holds, as a
//! crash leaves it. The design shows and saves what its file holds until each is settled; one
//! window lists them, each with Restore (the unpacked work comes into the design) and Discard (the
//! design's copy is unpacked again, the dropped work kept beside it).
//! The engine only reports (`doc.open`'s `recovery_components`) and settles
//! (`component.restore_recovery`, `component.discard_recovery`); this window is the app's layer.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;

/// The window's state: the recovery components not settled yet.
#[derive(Debug, Default, PartialEq)]
pub struct RecoveryComponents {
    pub rows: Vec<Row>,
}

/// One recovery component.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub component: u64,
    /// The component's name in the design.
    pub name: String,
    /// Its name inside the design's file.
    pub member: String,
}

/// After an open: the window, when the reply names recovery components.
pub(crate) fn offer(app: &mut SolveApp, reply: &Value) {
    let rows: Vec<Row> = reply["recovery_components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|member| {
            let path = solidcraft_engine::doc::internal::link(member);
            let component = app.session.doc.links.iter().find(|(_, link)| link.path == path).map(|(component, _)| *component)?;
            let name = app.session.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone())?;
            Some(Row { component, name, member: member.to_string() })
        })
        .collect();
    app.recovery_components = (!rows.is_empty()).then_some(RecoveryComponents { rows });
}

/// Keep the unpacked work of the recovery component `component` and bring it into the design, as
/// Update Links brings in any changed part.
pub(crate) fn restore(app: &mut SolveApp, component: u64) {
    if let Err(e) = app.run("component.restore_recovery", json!({"component": component})) {
        app.set_status(e, true);
        return;
    }
    if let Err(e) = app.run("component.update_links", json!({"component": component})) {
        app.set_status(e, true);
    }
    settled(app, component);
}

/// Drop the unpacked work of the recovery component `component`: the design's copy is unpacked
/// again, and the dropped work stays beside it as a backup.
pub(crate) fn discard(app: &mut SolveApp, component: u64) {
    if let Err(e) = app.run("component.discard_recovery", json!({"component": component})) {
        app.set_status(e, true);
        return;
    }
    settled(app, component);
}

/// The recovery component `component` is settled: its row goes, and the window with the last one.
fn settled(app: &mut SolveApp, component: u64) {
    let Some(recovery) = app.recovery_components.as_mut() else { return };
    recovery.rows.retain(|r| r.component != component);
    if recovery.rows.is_empty() {
        app.recovery_components = None;
    }
}

/// The window, while recovery components wait.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(recovery) = app.recovery_components.as_ref() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let rows = recovery.rows.clone();
    // The row asked about, and whether to restore it (else discard).
    let mut asked: Option<(u64, bool)> = None;
    let (closed, action) =
        ui_kit::Confirm::new(egui::Id::unique("sc_recovery_components"), "status_warning", solidcraft_ui_text::tr("recovery_components.title"))
            .tone(ui_kit::Tone::Warning)
            .actions(solidcraft_ui_text::tr("recovery_components.restore_all"), solidcraft_ui_text::tr("recovery_components.later"), None)
            .show(ctx, true, |ui| {
                ui_kit::text(ui, solidcraft_ui_text::tr("recovery_components.detail"), ui_kit::Text::Report("recovery_components", false));
                for row in &rows {
                    ui.horizontal(|ui| {
                        for (label, tip, restore) in [
                            ("recovery_components.restore", "recovery_components.restore_tip", true),
                            ("recovery_components.discard", "recovery_components.discard_tip", false),
                        ] {
                            let button = ui_kit::text_button(ui, solidcraft_ui_text::tr(label), true);
                            if button.backing_on_hover_text(solidcraft_ui_text::tr(tip)).clicked() {
                                asked = Some((row.component, restore));
                            }
                        }
                        ui_kit::text(ui, &row.name, ui_kit::Text::Note("recovery_components_row"));
                    });
                }
            });
    match asked {
        Some((component, true)) => restore(app, component),
        Some((component, false)) => discard(app, component),
        None => {}
    }
    match action {
        ui_kit::Action::Ok => {
            for row in rows {
                restore(app, row.component);
            }
        }
        // Closed without an answer: they stay recovery components, held as the file has them.
        ui_kit::Action::Cancel => app.recovery_components = None,
        _ if closed => app.recovery_components = None,
        _ => {}
    }
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        app.recovery_components = None;
        app.esc_handled = true;
    }
}
