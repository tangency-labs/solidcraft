//! Plugin updates (docs/plugins.md, Updates). A plugin whose manifest names
//! `"updates": {"github": "owner/repo"}` is checked at every start, and once a day while the app
//! stays open, by one background thread asking GitHub for that repository's latest release. A newer
//! stable release with a `<id>.zip` or `<id>-<version>.zip` asset puts Update to <version> on the
//! plugin's card, whether the plugin is on or off. Update downloads the `.zip`, checks it against
//! the release's sha256 digest, and opens the same review as Install plugin…, which keeps the
//! installed version's folders as for any update. The install's own checks and its atomic replace
//! do the rest; a failure leaves the installed version.
//!
//! The requests carry nothing but the repository names. The app's update check setting (and the
//! organization's policy over it) turns the checks off, as for the app's own update.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::Value;
use solidcraft_ui_text::format;

use crate::SolveApp;
use crate::plugins::Plugin;
use crate::update::{CHECK_INTERVAL_S, FetchError, is_newer, parse_version, unix_now};

/// The most plugins one round asks about: GitHub allows 60 anonymous requests an hour.
const MAX_CHECKS: usize = 20;
const MAX_ASSETS: usize = 400;

/// A newer release of one plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// The release's version, without the `v`.
    pub version: String,
    pub url: String,
    /// The asset's sha256 from GitHub's `digest`, lowercase hex.
    pub sha256: String,
    /// The `owner/repo` it came from.
    pub repo: String,
}

/// What one round found, and how long GitHub asked to wait when it hit the rate limit.
type Round = (BTreeMap<String, Offer>, Option<u64>);

struct Download {
    id: String,
    name: String,
    version: String,
    done: Receiver<Result<PathBuf, String>>,
}

/// The update under review or being installed: its `.zip` is deleted once nothing uses it.
pub(crate) struct Pending {
    pub(crate) id: String,
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

/// The `releases/latest` request for `repo`.
pub fn latest_url(repo: &str) -> String {
    std::format!("https://api.github.com/repos/{repo}/releases/latest")
}

/// A `releases/latest` answer for plugin `id` from `repo`: a stable release whose tag is a version,
/// with the plugin's `.zip` among its assets, downloaded from that repository's releases only, and
/// a sha256 digest to check the download against.
pub fn parse_release(json: &[u8], repo: &str, id: &str) -> Result<Offer, FetchError> {
    let v: Value = serde_json::from_slice(json).map_err(|e| FetchError::Failed(e.to_string()))?;
    if v.get("draft").and_then(Value::as_bool).unwrap_or(false) || v.get("prerelease").and_then(Value::as_bool).unwrap_or(false) {
        return Err(FetchError::Failed("the latest release is a prerelease".into()));
    }
    let tag = v.get("tag_name").and_then(Value::as_str).unwrap_or_default();
    if !parse_version(tag).is_some_and(|version| version.pre.is_empty()) {
        return Err(FetchError::Failed("the latest release's tag isn't a version".into()));
    }
    let version = tag.trim().trim_start_matches('v').to_string();
    let names = [std::format!("{id}.zip"), std::format!("{id}-{version}.zip")];
    let prefix = std::format!("https://github.com/{repo}/releases/download/").to_ascii_lowercase();
    v.get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ASSETS)
        .find_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let url = asset.get("browser_download_url")?.as_str()?;
            (names.iter().any(|n| n == name) && url.to_ascii_lowercase().starts_with(&prefix)).then_some((asset, url))
        })
        .ok_or_else(|| FetchError::Failed(std::format!("the latest release has no {id}.zip")))
        .and_then(|(asset, url)| {
            let sha256 = asset
                .get("digest")
                .and_then(Value::as_str)
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .filter(|hex| hex.len() == 64 && hex.bytes().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| FetchError::Failed(std::format!("the latest release lists no sha256 digest for {id}.zip")))?;
            Ok(Offer { version, url: url.to_string(), sha256, repo: repo.to_string() })
        })
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
    let same_repo = plugin.updates.as_ref().is_some_and(|repo| repo.eq_ignore_ascii_case(&offer.repo));
    (same_repo && !busy && is_newer(&offer.version, &plugin.version)).then_some(offer)
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
pub(crate) fn start(app: &mut SolveApp, ctx: &egui::Context, plugin: &Plugin) {
    let Some(offer) = offered(app, plugin).cloned() else { return };
    if app.plugin_updates.download.is_some() || app.plugin_updates.pending.is_some() || crate::plugins::busy(app) {
        app.set_status(app.tr("prefs.plugin_busy").to_owned(), true);
        return;
    }
    let (Some(download), Some(dir)) = (app.services.http_download.clone(), app.session.plugin_state.plugins_dir()) else { return };
    let path = dir.join(".updates").join(std::format!("{}-{}.zip", plugin.id, offer.version));
    let version = offer.version.clone();
    let (send, done) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("plugin-update-download".into()).spawn(move || {
        let _ = send.send(fetch(&download, &offer, path));
        ctx.request_repaint();
    });
    if spawned.is_ok() {
        app.set_status(format("plugin_update.downloading", &[("plugin", &plugin.name), ("version", &version)]), false);
        app.plugin_updates.download = Some(Download { id: plugin.id.clone(), name: plugin.name.clone(), version, done });
    }
}

/// Downloads `offer` to `path` and checks its digest; nothing is left behind on failure.
fn fetch(download: &crate::update::install::HttpDownload, offer: &Offer, path: PathBuf) -> Result<PathBuf, String> {
    let fetched = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .map_err(|e| e.to_string())
        .and_then(|()| download(&offer.url, &path, &|_, _| true).map_err(|e| e.to_string()))
        .and_then(|()| match crate::update::install::sha256_file(&path)? == offer.sha256 {
            true => Ok(()),
            false => Err("the download does not match the release's checksum".to_string()),
        });
    match fetched {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
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
            app.plugin_updates.pending = Some(Pending { id: download.id, version: download.version, path: path.clone() });
            crate::plugin_install::review(app, path.to_string_lossy().into_owned());
        }
        Err(error) => app.set_status(format("plugin_update.failed", &[("plugin", &download.name), ("error", &error)]), true),
    }
}

/// For the review of a downloaded update: the package must be the plugin and version offered.
/// The review fills in the installed version's folders as for any replacement. `None`: not an update.
pub(crate) fn prepare(app: &SolveApp, path: &str, plugin: &Value) -> Option<Result<(), String>> {
    let pending = app.plugin_updates.pending.as_ref().filter(|pending| pending.path.to_string_lossy() == path)?;
    let (id, version) = (plugin.get("id").and_then(Value::as_str), plugin.get("version").and_then(Value::as_str));
    if id != Some(pending.id.as_str()) || version.map(|v| v.trim_start_matches('v')) != Some(pending.version.as_str()) {
        let found = std::format!("{} {}", id.unwrap_or_default(), version.unwrap_or_default());
        return Some(Err(std::format!("the download is {found}, not {} {}", pending.id, pending.version)));
    }
    Some(Ok(()))
}

#[cfg(test)]
#[path = "plugin_updates_tests.rs"]
mod tests;
