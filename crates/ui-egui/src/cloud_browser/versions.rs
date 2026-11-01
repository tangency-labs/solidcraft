//! Explicit inspection of competing offline tips. Opening keeps every capture queued.
use super::*;

pub(super) struct Choice {
    destination: Destination,
    revisions: Vec<solidcraft_cloud::LocalRevision>,
    selected: Option<String>,
}
impl Choice {
    pub(super) fn new(destination: Destination, revisions: Vec<solidcraft_cloud::LocalRevision>) -> Self {
        Self { destination, revisions, selected: None }
    }
}

pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut choice) = app.cloud.browser.versions.take() else { return };
    if app.session.cloud_settings().connection(&choice.destination.connection).is_none() {
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_local_versions"), "open", tr("cloud.local_versions"))
        .fields()
        .cards(2.0)
        .actions(tr("ui.open"), "Cancel", None)
        .show(ctx, choice.selected.is_some() && !app.session.cloud_configuring(), |ui| {
            ui.label(format!("{} / {}", choice.destination.project, choice.destination.file));
            ui_kit::text(ui, tr("cloud.local_versions_hint"), ui_kit::Text::Hint("cloud_local_versions_hint"));
            egui::ScrollArea::vertical().max_height(240.0).show_rows(ui, ui.spacing().interact_size.y, choice.revisions.len(), |ui, rows| {
                for index in rows {
                    let Some(revision) = choice.revisions.get(index) else { continue };
                    let title = if revision.title.is_empty() { tr("cloud.local_version") } else { &revision.title };
                    let label = format!("{} · {}", title, revision.revision.get(..12).unwrap_or(&revision.revision));
                    let response = ui_kit::list_picker(ui, &revision.revision, &label, choice.selected.as_deref() == Some(&revision.revision));
                    crate::scenario::publish_target(&format!("cloud:version:{index}"), response.rect);
                    if response.clicked() {
                        choice.selected = Some(revision.revision.clone());
                    }
                }
            });
        });
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    if action == ui_kit::Action::Ok {
        if let Some(revision) = choice.selected.as_deref() {
            super::open_revision(app, &choice.destination, Some(revision));
        }
    } else {
        app.cloud.browser.versions = Some(choice);
    }
}
