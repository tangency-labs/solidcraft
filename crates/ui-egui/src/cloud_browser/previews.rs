//! Bounded preview cache. Visible rows request images through registered commands;
//! results are consumed once, and connection/revision changes discard stale pictures.
use super::*;

#[derive(Default)]
pub(super) struct State {
    entries: BTreeMap<String, Entry>,
    used: u64,
}
struct Entry {
    connection: Connection,
    revision: Option<String>,
    pending: Option<String>,
    image: Option<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
    used: u64,
}
fn key(destination: &Destination) -> Option<String> {
    serde_json::to_string(destination).ok()
}
fn remove(app: &mut SolveApp, key: &str) {
    if let Some(entry) = app.cloud.browser.previews.entries.remove(key)
        && let Some(operation) = entry.pending
    {
        let _ = app.run("cloud.cancel", json!({"operation":operation}));
    }
}
pub(super) fn clear(app: &mut SolveApp, connection: &str) {
    let keys: Vec<_> = app.cloud.browser.previews.entries.iter().filter(|(_, e)| e.connection.id == connection).map(|(key, _)| key.clone()).collect();
    for key in keys {
        remove(app, &key);
    }
}
pub(super) fn poll(app: &mut SolveApp, all: &[JobStatus]) {
    let stale: Vec<_> = app
        .cloud
        .browser
        .previews
        .entries
        .iter()
        .filter(|(_, e)| app.session.cloud_settings().connection(&e.connection.id) != Some(&e.connection))
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale {
        remove(app, &key);
    }
    let ready: Vec<_> = app
        .cloud
        .browser
        .previews
        .entries
        .iter()
        .filter_map(|(key, e)| {
            let id = e.pending.as_ref()?;
            let status = all.iter().find(|s| &s.operation == id)?;
            matches!(status.phase, Phase::Ready | Phase::Failed | Phase::Cancelled).then(|| (key.clone(), id.clone(), status.phase))
        })
        .collect();
    for (key, id, phase) in ready {
        let image = if phase == Phase::Ready {
            app.run("cloud.thumbnail.finish", json!({"operation":id}))
                .ok()
                .and_then(|v| solidcraft_cloud::Thumbnail::decode_response(&v).ok().flatten())
                .and_then(crate::home::decode_thumbnail)
        } else {
            None
        };
        if let Some(entry) = app.cloud.browser.previews.entries.get_mut(&key) {
            entry.pending = None;
            entry.image = image;
        }
    }
}
pub(super) fn textures(app: &mut SolveApp, ctx: &egui::Context) {
    for (key, entry) in &mut app.cloud.browser.previews.entries {
        if let Some(image) = entry.image.take() {
            entry.texture = Some(ctx.load_texture(format!("cloud-preview:{key}"), image, egui::TextureOptions::LINEAR));
        }
    }
}
pub(super) fn texture(app: &SolveApp, destination: &Destination) -> Option<egui::TextureHandle> {
    app.cloud.browser.previews.entries.get(&key(destination)?)?.texture.clone()
}
pub(super) fn image(app: &SolveApp, destination: &Destination) -> Option<(egui::TextureId, egui::Vec2)> {
    let texture = app.cloud.browser.previews.entries.get(&key(destination)?)?.texture.as_ref()?;
    Some((texture.id(), texture.size_vec2()))
}
pub(super) fn failed(app: &SolveApp, destination: &Destination) -> bool {
    key(destination)
        .and_then(|key| app.cloud.browser.previews.entries.get(&key))
        .is_some_and(|entry| entry.pending.is_none() && entry.image.is_none() && entry.texture.is_none())
}
pub(super) fn touch(app: &mut SolveApp, destination: Destination) {
    let Some(key) = key(&destination) else { return };
    let Some(connection) = app.session.cloud_settings().connection(&destination.connection).cloned() else { return };
    if app
        .cloud
        .browser
        .catalogs
        .get(&destination.connection)
        .into_iter()
        .flat_map(|c| &c.stored)
        .any(|s| s.destination == destination && s.local_revisions.len() > 1)
    {
        remove(app, &key);
        return;
    }
    let revision = app
        .cloud
        .browser
        .statuses
        .iter()
        .chain(app.cloud.browser.catalogs.get(&destination.connection).into_iter().flat_map(|c| &c.stored))
        .filter(|s| s.destination == destination)
        .max_by_key(|s| s.operation)
        .and_then(|s| s.revision.clone());
    if app.cloud.browser.previews.entries.get(&key).is_some_and(|e| e.connection != connection || e.revision != revision) {
        remove(app, &key);
    }
    app.cloud.browser.previews.used = app.cloud.browser.previews.used.saturating_add(1);
    let used = app.cloud.browser.previews.used;
    if let Some(entry) = app.cloud.browser.previews.entries.get_mut(&key) {
        entry.used = used;
        return;
    }
    if app.file_picker_busy() || app.file_load.busy() || app.saving.is_some() || app.session.cloud_configuring() {
        return;
    }
    if app.cloud.browser.previews.entries.len() >= 64
        && let Some(old) = app.cloud.browser.previews.entries.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone())
    {
        remove(app, &old);
    }
    let pending = app
        .run("cloud.thumbnail", json!({"destination":destination}))
        .ok()
        .and_then(|r| r.get("operation").and_then(|v| v.as_str()).map(str::to_owned));
    app.cloud.browser.previews.entries.insert(key, Entry { connection, revision, pending, image: None, texture: None, used });
}
