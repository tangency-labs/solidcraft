//! Plugin updates (docs/plugins.md, Updates). A plugin whose manifest names
//! `"updates": {"github": "owner/repo"}` is checked at every start, and once a day while the app
//! stays open, by one background thread asking GitHub for that repository's latest release. A newer
//! stable release with a `<id>.zip` or `<id>-<version>.zip` asset puts Update to <version> on the
//! plugin's card, whether the plugin is on or off. Update downloads the `.zip`, checks it against
//! the release's sha256 digest, and opens the same review as Install plugin…, with the folders the
//! installed version was given filled in. The install's own checks and its atomic replace do the
//! rest; a failure leaves the installed version.
//!
//! The requests carry nothing but the repository names. The app's update check setting (and the
//! organization's policy over it) turns the checks off, as for the app's own update.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::Value;
use solidcraft_engine::plugin::github::fetch;
pub use solidcraft_engine::plugin::github::{Offer, latest_url, parse_release};
use solidcraft_ui_text::format;

use crate::SolveApp;
use crate::plugins::Plugin;
use crate::update::{CHECK_INTERVAL_S, FetchError, is_newer, unix_now};

/// The most plugins one round asks about: GitHub allows 60 anonymous requests an hour.
const MAX_CHECKS: usize = 20;

/// What one round found, and how long GitHub asked to wait when it hit the rate limit.
type Round = (BTreeMap<String, Offer>, Option<u64>);

struct Download {
    id: String,
    name: String,
    repository: String,
    version: String,
    done: Receiver<Result<PathBuf, String>>,
}

/// The update under review or being installed: its `.zip` is deleted once nothing uses it.
pub(crate) struct Pending {
    pub(crate) id: String,
    pub(crate) repository: String,
    pub(crate) version: String,
    pub(crate) path: PathBuf,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) offers: BTreeMap<String, Offer>,
    check: Option<Receiver<Round>>,
    /// The next automatic round; 0 until the first one of this launch.
    next_check: f64,
    download: Option<Download>,
    pub(crate) pending: Option<Pending>,
}

/// One round over `(id, repo)` pairs. A plugin that fails is skipped; a rate limit ends the round.
fn check_round(plugins: &[(String, String)], get: &dyn Fn(&str) -> Result<Vec<u8>, FetchError>) -> Round {
    let mut offers = BTreeMap::new();
    for (id, repo) in plugins.iter().take(MAX_CHECKS) {
        match get(&latest_url(repo)).and_then(|body| parse_release(&body, repo, id)) {
            Ok(offer) => {
                offers.insert(id.clone(), offer);
            }
            Err(FetchError::RateLimited { retry_after_s }) => return (offers, Some(retry_after_s)),
            Err(FetchError::Failed(_)) => {}
        }
    }
    (offers, None)
}

/// The installed plugins that name a repository, as `(id, repo)`.
fn subscribed(plugins: &[Plugin]) -> Vec<(String, String)> {
    plugins.iter().filter_map(|plugin| Some((plugin.id.clone(), plugin.updates.clone()?))).collect()
}

/// The Update button shows on `plugin`'s card: a newer release was found and nothing is updating it.
pub(crate) fn offered<'a>(app: &'a SolveApp, plugin: &Plugin) -> Option<&'a Offer> {
    let state = &app.plugin_updates;
    let busy = state.download.as_ref().is_some_and(|d| d.id == plugin.id) || state.pending.as_ref().is_some_and(|p| p.id == plugin.id);
    let offer = state.offers.get(&plugin.id)?;
    (plugin.updates.is_some() && !busy && is_newer(&offer.version, &plugin.version)).then_some(offer)
}

fn checks_allowed(app: &SolveApp) -> bool {
    crate::plugin_install::available()
        && app.services.http_get.is_some()
        && crate::policy::check_updates(app)
        && !std::env::var("SOLIDCRAFT_UPDATE_CHECK").is_ok_and(|v| v == "0")
        && unix_now() >= app.preferences.update.rate_limited_until
}

/// Per frame: start a round at launch and daily after, collect finished work, and delete a
/// downloaded `.zip` nothing uses any more.
pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    poll_check(app);
    poll_download(app);
    let in_use = app.plugin_updates.download.is_some() || app.plugin_install.ask.is_some() || crate::plugins::busy(app);
    if !in_use && let Some(pending) = app.plugin_updates.pending.take() {
        let _ = std::fs::remove_file(&pending.path);
    }
    let now = unix_now();
    if app.plugin_updates.check.is_some() || now < app.plugin_updates.next_check || !checks_allowed(app) {
        return;
    }
    let (Some(dir), Some(get)) = (app.session.plugin_state.plugins_dir(), app.services.http_get.clone()) else { return };
    app.plugin_updates.next_check = now + CHECK_INTERVAL_S;
    let (send, done) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("plugin-update-check".into()).spawn(move || {
        let (plugins, _) = crate::plugins::inventory(Some(&dir), std::iter::empty());
        let _ = send.send(check_round(&subscribed(&plugins), &|url: &str| get(url)));
        ctx.request_repaint();
    });
    if spawned.is_ok() {
        app.plugin_updates.check = Some(done);
    }
}

fn poll_check(app: &mut SolveApp) {
    let Some(done) = &app.plugin_updates.check else { return };
    let (offers, limited) = match done.try_recv() {
        Ok(round) => round,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => (BTreeMap::new(), None),
    };
    app.plugin_updates.check = None;
    app.plugin_updates.offers = offers;
    if let Some(wait) = limited {
        let until = unix_now() + (wait as f64).clamp(60.0, CHECK_INTERVAL_S);
        app.plugin_updates.next_check = until;
        let shared = &mut app.preferences.update.rate_limited_until;
        *shared = shared.max(until);
    }
}

/// Update: download the offered `.zip` beside the plugins, off the window thread.
pub(crate) fn start(app: &mut SolveApp, plugin: &Plugin) {
    let Some(offer) = offered(app, plugin).cloned() else { return };
    let Some(repository) = plugin.updates.clone() else { return };
    if app.plugin_updates.download.is_some() || app.plugin_updates.pending.is_some() || crate::plugins::busy(app) {
        app.set_status(app.tr("prefs.plugin_busy").to_owned(), true);
        return;
    }
    let (Some(download), Some(dir)) = (app.services.http_download.clone(), app.session.plugin_state.plugins_dir()) else { return };
    let path = dir.join(".updates").join(std::format!("{}-{}.zip", plugin.id, offer.version));
    let version = offer.version.clone();
    let (send, done) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("plugin-update-download".into()).spawn(move || {
        let _ = send.send(fetch(&download, &offer, path));
    });
    if spawned.is_ok() {
        app.set_status(format("plugin_update.downloading", &[("plugin", &plugin.name), ("version", &version)]), false);
        app.plugin_updates.download = Some(Download { id: plugin.id.clone(), name: plugin.name.clone(), repository, version, done });
    }
}

fn poll_download(app: &mut SolveApp) {
    let Some(download) = &app.plugin_updates.download else { return };
    let result = match download.done.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err("the download stopped".into()),
    };
    let Some(download) = app.plugin_updates.download.take() else { return };
    match result {
        Ok(path) => {
            app.plugin_updates.pending =
                Some(Pending { id: download.id, repository: download.repository, version: download.version, path: path.clone() });
            crate::plugin_install::review(app, path.to_string_lossy().into_owned());
        }
        Err(error) => app.set_status(format("plugin_update.failed", &[("plugin", &download.name), ("error", &error)]), true),
    }
}

/// The reviewed update's origin stays attached to its local archive through installation.
pub(crate) fn repository<'a>(app: &'a SolveApp, path: &str) -> Option<&'a str> {
    app.plugin_updates.pending.as_ref().filter(|pending| pending.path.to_string_lossy() == path).map(|pending| pending.repository.as_str())
}

/// For the review of a downloaded update: the package must be the plugin and version offered, and
/// the folders the installed version was given are filled in by name. `None`: not an update.
pub(crate) fn prepare(app: &SolveApp, path: &str, plugin: &Value, grants: &mut solidcraft_engine::plugin::Grants) -> Option<Result<(), String>> {
    let pending = app.plugin_updates.pending.as_ref().filter(|pending| pending.path.to_string_lossy() == path)?;
    let (id, version) = (plugin.get("id").and_then(Value::as_str), plugin.get("version").and_then(Value::as_str));
    if id != Some(pending.id.as_str()) || version.map(|v| v.strip_prefix('v').unwrap_or(v)) != Some(pending.version.as_str()) {
        let found = std::format!("{} {}", id.unwrap_or_default(), version.unwrap_or_default());
        return Some(Err(std::format!("the download is {found}, not {} {}", pending.id, pending.version)));
    }
    if let Some(installed) = app.session.plugin_state.grants.get(&pending.id) {
        for grant in &mut grants.folders {
            if let Some(old) = installed.folders.iter().find(|old| old.name == grant.name) {
                grant.folder.clone_from(&old.folder);
            }
        }
    }
    Some(Ok(()))
}

#[cfg(test)]
#[path = "plugin_updates_tests.rs"]
mod tests;
