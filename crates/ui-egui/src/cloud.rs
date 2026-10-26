//! Platform workers own storage; sessions own captured saves. The UI only polls and commands.
use std::collections::BTreeMap;

use solidcraft_cloud::jobs::{JobStatus, Phase};

use crate::SolveApp;
pub(crate) mod configuration;
pub(crate) mod operations;
mod startup;

#[derive(Default)]
pub(crate) struct State {
    pub setup: Option<crate::cloud_setup::Wizard>,
    pub browser: crate::cloud_browser::Browser,
    pub editor: Option<crate::cloud_settings::Editor>,
    #[cfg(not(target_arch = "wasm32"))]
    pub git_setup: Option<crate::cloud_settings::git::Wizard>,
    pub operations: operations::Operations,
    startup: startup::State,
    notices: BTreeMap<String, (u64, Option<String>)>,
    error: Option<String>,
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if app.services.cloud_host.is_none() {
        return;
    }
    let statuses = match poll(app) {
        Ok(statuses) => statuses,
        Err(error) => {
            app.cloud.error = Some(error);
            return;
        }
    };
    app.cloud.error = None;
    app.cloud.notices.retain(|id, _| app.session.cloud_settings().connection(id).is_some());
    for status in statuses.iter().filter(|s| !matches!(s.phase, Phase::Working | Phase::Syncing)) {
        // Background preview reads cannot acknowledge a failed push. Catalog and
        // opening errors have their own presentation caches; tracked actions clear
        // their notices only when a subsequent test/publication actually succeeds.
        let action = app.cloud.operations.tracked(&status.operation);
        if app.session.cloud_settings().connection(&status.connection).is_none()
            || (status.phase != Phase::Synced && !(action && (status.message.is_some() || status.phase == Phase::Ready)))
        {
            continue;
        }
        let operation = status.operation.strip_prefix("cloud-").and_then(|id| id.parse::<u64>().ok()).unwrap_or(0);
        let notice = app.cloud.notices.entry(status.connection.clone()).or_default();
        if operation >= notice.0 {
            *notice = (operation, status.message.clone());
        }
    }
    app.cloud.operations.update(&statuses);
    crate::cloud_browser::tick(app, &statuses);
    crate::cloud_browser::tabs::request_timeout(app, ctx);
    if statuses.iter().any(|status| matches!(status.phase, Phase::Working | Phase::Syncing)) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if !app.session.cloud_save_pending() && !crate::cloud_browser::choosing_destination(app) {
        crate::documents::saved_for_close(app);
    }
    startup::tick(app, ctx, &statuses);
}

pub(crate) fn notices(app: &SolveApp) -> Vec<String> {
    let mut messages: Vec<_> = app.cloud.error.iter().cloned().collect();
    for connection in &app.session.cloud_settings().connections {
        if let Some((_, Some(message))) = app.cloud.notices.get(&connection.id) {
            messages.push(format!("{}\n{message}", connection.repository_label()));
        }
    }
    messages
}

fn poll(app: &mut SolveApp) -> Result<Vec<JobStatus>, String> {
    app.session.poll_cloud().map_err(|e| e.to_string())?;
    let mut changed = app.session.take_cloud_settings_change().is_some();
    for slot in app.docs.background_mut() {
        slot.session.poll_cloud().map_err(|e| e.to_string())?;
        if slot.session.take_cloud_settings_change().is_some() {
            app.session.inherit_cloud_services(&slot.session);
            changed = true;
        }
    }
    if changed {
        for slot in app.docs.background_mut() {
            slot.session.inherit_cloud_services(&app.session);
        }
    }
    let mut statuses = app.session.cloud_operations().map_err(|e| e.to_string())?;
    for slot in app.docs.background_mut() {
        statuses.extend(slot.session.cloud_operations().map_err(|e| e.to_string())?);
    }
    let publications: Vec<_> = statuses.iter().flat_map(|status| status.publications.iter().cloned()).collect();
    let managed: Vec<_> = statuses.iter().flat_map(|status| status.managed.iter().cloned()).collect();
    if !managed.is_empty() {
        app.session.poll_cloud_management(&managed).map_err(|e| e.to_string())?;
        for slot in app.docs.background_mut() {
            slot.session.poll_cloud_management(&managed).map_err(|e| e.to_string())?;
        }
    }
    if !publications.is_empty() {
        app.session.poll_cloud_with_publications(&publications).map_err(|e| e.to_string())?;
        for slot in app.docs.background_mut() {
            slot.session.poll_cloud_with_publications(&publications).map_err(|e| e.to_string())?;
        }
    }
    Ok(statuses)
}

#[cfg(test)]
mod tests;
