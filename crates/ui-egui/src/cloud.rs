//! Platform workers own storage; sessions own captured saves. The UI only polls and commands.
use std::collections::{BTreeMap, BTreeSet};

use solidcraft_cloud::jobs::{JobStatus, Phase};

use crate::SolveApp;
pub(crate) mod configuration;

#[derive(Default)]
pub(crate) struct State {
    retry_at: f64,
    messages: BTreeMap<String, String>,
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if app.services.cloud_host.is_none() {
        return;
    }
    let statuses = match poll(app) {
        Ok(statuses) => statuses,
        Err(error) => {
            app.set_status(error, true);
            return;
        }
    };
    let live: BTreeSet<_> = statuses.iter().map(|status| status.operation.as_str()).collect();
    app.cloud.messages.retain(|id, _| live.contains(id.as_str()));
    for status in &statuses {
        if let Some(message) = &status.message
            && app.cloud.messages.get(&status.operation) != Some(message)
        {
            app.cloud.messages.insert(status.operation.clone(), message.clone());
            app.set_status(message.clone(), true);
        }
    }
    if statuses.iter().any(|status| matches!(status.phase, Phase::Working | Phase::Syncing)) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if !app.session.cloud_save_pending() {
        crate::documents::saved_for_close(app);
    }
    // Retry after startup and once a minute. Queues and their failures remain independent.
    let now = ctx.input(|input| input.time);
    let now = if now.is_finite() { now } else { 0.0 };
    let settings = app.session.cloud_settings();
    if settings.retry_pending && !app.session.cloud_configuring() && now >= app.cloud.retry_at {
        let connections: Vec<_> = settings
            .connections
            .iter()
            .filter(|connection| {
                !statuses.iter().any(|status| status.connection == connection.id && matches!(status.phase, Phase::Working | Phase::Syncing))
            })
            .map(|connection| connection.id.clone())
            .collect();
        app.cloud.retry_at = now + 60.0;
        for connection in connections {
            let _ = app.run("cloud.sync", serde_json::json!({"connection":connection}));
        }
    }
    if app.session.cloud_settings().retry_pending && !app.session.cloud_settings().connections.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((app.cloud.retry_at - now).clamp(1.0, 60.0)));
    }
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
        app.cloud.retry_at = 0.0;
    }
    let mut statuses = app.session.cloud_operations().map_err(|e| e.to_string())?;
    for slot in app.docs.background_mut() {
        statuses.extend(slot.session.cloud_operations().map_err(|e| e.to_string())?);
    }
    let publications: Vec<_> = statuses.iter().flat_map(|status| status.publications.iter().cloned()).collect();
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
