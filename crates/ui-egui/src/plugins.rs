//! Preferences › Plugins: packages installed under the per-user data folder.
//!
//! A plugin is a directory of `{data}/plugins/<id>/plugin.json`. The page searches that list.
//! It reads `plugin.json` and records which ids are turned off. It does not load or run anything
//! from the folder. Explicit `plugin.run` calls use the native component host and honour
//! `disabled_plugins`; listing or enabling a package never executes its code.

use std::path::Path;

use egui::RichText;
use serde::Deserialize;

use crate::SolveApp;
use crate::ui_kit::BackingTooltipExt as _;

/// Route namespaced entries through the existing approval gate and dedicated transaction worker.
pub(crate) fn normalize_request(session: &solidcraft_engine::Session, method: &str, params: &mut serde_json::Value) -> Result<(), String> {
    fn normalize(session: &solidcraft_engine::Session, step: &mut serde_json::Value) -> Result<(), String> {
        let Some(command) = step.get("command").or_else(|| step.get("id")).and_then(serde_json::Value::as_str) else { return Ok(()) };
        let inputs = step.get("params").unwrap_or(&serde_json::Value::Null);
        let Some(resolved) = solidcraft_engine::plugin::resolve_call(session, command, inputs).map_err(|e| e.to_string())? else { return Ok(()) };
        if let Some(step) = step.as_object_mut() {
            step.remove("id");
            step.insert("command".into(), "plugin.run".into());
            step.insert("params".into(), resolved);
        }
        Ok(())
    }
    match method {
        "engine.execute" | "command" => normalize(session, params),
        "engine.batch" | "engine.script" => {
            solidcraft_engine::script_steps(params).map_err(|e| e.to_string())?;
            let steps = if params.is_array() { params.as_array_mut() } else { params.get_mut("commands").and_then(serde_json::Value::as_array_mut) };
            if let Some(steps) = steps {
                for step in steps.iter_mut() {
                    normalize(session, step)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Whether an agent's request runs a plugin's code: `plugin.run` (a `<plugin>.<entry>` call is
/// one once normalized), `file.import`, `doc.open` of a file an installed importer opens, or
/// `file.export` through an installed exporter, alone or as a batch step. The chat asks before
/// such a request in every mode, whatever was allowed before: running a plugin's code needs the
/// user's go each time.
pub(crate) fn runs_plugin_code(session: &solidcraft_engine::Session, method: &str, params: &serde_json::Value) -> bool {
    let step = |step: &serde_json::Value| {
        let Some(command) = step.get("command").or_else(|| step.get("id")).and_then(serde_json::Value::as_str) else { return false };
        let params = step.get("params").unwrap_or(&serde_json::Value::Null);
        command.eq_ignore_ascii_case("plugin.run")
            || solidcraft_engine::cmd::runs_plugin_importer(session, command, params)
            || solidcraft_engine::cmd::runs_plugin_exporter(session, command, params)
    };
    match method {
        "engine.execute" | "command" => step(params),
        "engine.batch" | "engine.script" => {
            let steps = if params.is_array() { params.as_array() } else { params.get("commands").and_then(serde_json::Value::as_array) };
            steps.is_some_and(|steps| steps.iter().take(10_000).any(step))
        }
        _ => false,
    }
}

/// How many turned-off ids a preferences file may name.
pub const MAX_DISABLED: usize = 256;
/// `plugin.json` larger than this is refused.
const MAX_MANIFEST_BYTES: u64 = solidcraft_engine::plugin::MAX_MANIFEST_BYTES as u64;

/// One installed plugin, as its manifest describes it.
#[derive(Clone, Debug)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: String,
    /// The package hash checked by `plugin.run`.
    pub approval: String,
    pub disabled: bool,
    pub groups: std::collections::BTreeSet<String>,
    pub entries: Vec<solidcraft_engine::plugin::Entry>,
    /// The images its entries name, read from the package (`plugin_images`): each relative path
    /// with the icon name it is drawn by.
    pub images: std::collections::BTreeMap<String, String>,
    pub(crate) swatches: crate::plugin_images::Swatches,
    /// Its view backgrounds (`shaders@0.1`): the preference's key, `id/shader`, and the name.
    pub shaders: Vec<(String, String)>,
}

/// A folder under the plugins directory that is not a plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub folder: String,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
}

/// An id a folder and a manifest may share: 1–64 characters, lowercase letters, digits and
/// hyphens, not starting or ending with a hyphen.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=64).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && *b.last().unwrap_or(&b'-') != b'-'
}

fn plain(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max && text.chars().all(|c| !c.is_control())
}

fn check(m: Manifest, folder: &str) -> Result<Plugin, String> {
    if m.id != folder {
        return Err(format!(
            "plugin.json id `{}` does not match the folder `{folder}`: a package folder is named after its plugin's id, so rename the folder to `{}`",
            m.id, m.id
        ));
    }
    if !valid_id(&m.id) {
        return Err("the id must be 1 to 64 lowercase letters, digits and hyphens".into());
    }
    let name = m.name.trim();
    if !plain(name, 80) {
        return Err("the name must be 1 to 80 characters with no line breaks".into());
    }
    let version = m.version.trim();
    if !plain(version, 32) || !version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
        return Err("the version must be 1 to 32 letters, digits, dots, hyphens or plus signs".into());
    }
    let description = m.description.trim();
    if description.chars().count() > 240 || description.chars().any(|c| c.is_control()) {
        return Err("the description must be at most 240 characters with no line breaks".into());
    }
    Ok(Plugin {
        id: m.id,
        name: name.to_string(),
        version: version.to_string(),
        description: description.to_string(),
        path: String::new(),
        approval: String::new(),
        disabled: false,
        groups: Default::default(),
        entries: Vec::new(),
        images: Default::default(),
        swatches: Default::default(),
        shaders: Vec::new(),
    })
}

/// What a listed package's approval hash was worked out from: its manifest's bytes and its
/// component's size and modification time.
type ListedKey = (Vec<u8>, Option<(u64, std::time::SystemTime)>);

/// A package's manifest and approval hash, as `read_package` gives them, for the listing only
/// (Preferences, menus, ribbon slots). Hashing a large component stalls the thread that does it
/// (#21228), so an unchanged manifest and component reuse the hash worked out before. Running a
/// plugin never takes this path: it reads and hashes the package again (`read_package`).
fn read_package_listed(dir: &Path, folder: &str) -> Result<(solidcraft_engine::plugin::Manifest, String), String> {
    static LISTED: std::sync::Mutex<Option<std::collections::HashMap<std::path::PathBuf, (ListedKey, String)>>> = std::sync::Mutex::new(None);
    let key = || -> Option<(solidcraft_engine::plugin::Manifest, ListedKey)> {
        let bytes = std::fs::read(dir.join("plugin.json")).ok()?;
        let manifest = solidcraft_engine::plugin::parse_manifest(&bytes, folder).ok()?;
        let component = match &manifest.runtime {
            Some(runtime) => {
                let meta = std::fs::metadata(dir.join(&runtime.module)).ok()?;
                Some((meta.len(), meta.modified().ok()?))
            }
            None => None,
        };
        Some((manifest, (bytes, component)))
    };
    let known = key();
    if let Some((manifest, key)) = &known
        && let Ok(listed) = LISTED.lock()
        && let Some((seen, approval)) = listed.as_ref().and_then(|m| m.get(dir))
        && seen == key
    {
        return Ok((manifest.clone(), approval.clone()));
    }
    let package = solidcraft_engine::plugin::read_package(&dir.to_string_lossy()).map_err(|e| e.to_string())?;
    if let Some((_, key)) = known
        && let Ok(mut listed) = LISTED.lock()
    {
        listed.get_or_insert_with(Default::default).insert(dir.to_path_buf(), (key, package.approval.clone()));
    }
    Ok((package.manifest, package.approval))
}

fn read_plugin(dir: &Path, folder: &str) -> Result<(Plugin, bool), String> {
    let path = dir.join("plugin.json");
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "no plugin.json".to_string())?;
    if !meta.is_file() || meta.len() > MAX_MANIFEST_BYTES {
        return Err("plugin.json is missing or larger than 64 KiB".into());
    }
    let (parsed, approval) = read_package_listed(dir, folder)?;
    let shaders = parsed.shaders.iter().map(|shader| (format!("{}/{}", parsed.id, shader.id), shader.name.clone())).collect();
    let mut plugin = check(Manifest { id: parsed.id, name: parsed.name, version: parsed.version, description: parsed.description }, folder)?;
    plugin.path = dir.to_string_lossy().into_owned();
    plugin.approval = approval;
    plugin.shaders = shaders;
    let executable = parsed.runtime.is_some();
    if let Some(runtime) = parsed.runtime {
        plugin.entries = runtime.entries;
        plugin.groups = runtime.permissions.command_groups;
    }
    plugin.images = crate::plugin_images::load(dir, &plugin.id, &plugin.entries);
    plugin.swatches = crate::plugin_images::load_swatches(dir, &plugin.entries);
    Ok((plugin, executable))
}

/// The plugins in `dir`, and the folders that were not. A missing directory is an empty list.
fn scan_packages(dir: &Path) -> (Vec<(Plugin, bool)>, Vec<Skipped>) {
    let mut plugins = Vec::new();
    let mut skipped = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (plugins, skipped);
    };
    let mut folders: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()) && !entry.file_name().to_string_lossy().starts_with('.'))
        .take(1024)
        .collect();
    folders.sort_by_key(|entry| entry.path());
    for entry in folders {
        let Some(folder) = entry.file_name().into_string().ok() else {
            skipped.push(Skipped { folder: String::new(), reason: "the folder name is not text".into() });
            continue;
        };
        match read_plugin(&entry.path(), &folder) {
            Ok(plugin) => plugins.push(plugin),
            Err(reason) => skipped.push(Skipped { folder, reason }),
        }
    }
    skipped.sort_by(|a, b| a.folder.cmp(&b.folder));
    (plugins, skipped)
}

/// How many times each plugins folder was read for a listing; the tests watch it to prove that
/// drawing frames reads nothing (#21228, #21594). Keyed by folder, as tests run side by side.
#[cfg(test)]
pub(crate) static INVENTORY_READS: std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// How many listings read `dir` so far (tests).
#[cfg(test)]
pub(crate) fn inventory_reads(dir: &Path) -> usize {
    INVENTORY_READS.lock().unwrap().get(dir).copied().unwrap_or(0)
}

/// Installed and inspected packages share one reader across Preferences, menus and ribbon slots.
pub(crate) fn inventory<'a>(dir: Option<&Path>, inspected: impl Iterator<Item = &'a String>) -> (Vec<Plugin>, Vec<Skipped>) {
    #[cfg(test)]
    if let Some(dir) = dir {
        *INVENTORY_READS.lock().unwrap().entry(dir.to_path_buf()).or_default() += 1;
    }
    let mut packages = std::collections::BTreeMap::new();
    // Explicitly inspected packages win over installed folders with the same id, as in plugin.run.
    let mut found = Vec::new();
    for path in inspected {
        let dir = Path::new(path);
        if let Some(folder) = dir.file_name().and_then(|f| f.to_str())
            && let Ok(plugin) = read_plugin(dir, folder)
        {
            found.push(plugin);
        }
    }
    let (installed, skipped) = dir.map(scan_packages).unwrap_or_default();
    for (plugin, executable) in found.into_iter().chain(installed) {
        match packages.entry(plugin.id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((plugin, executable));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                // The Plugins menu previously ignored metadata-only packages before deduping.
                // Retain them for Preferences without hiding an executable copy of that id.
                if executable && !entry.get().1 {
                    entry.insert((plugin, executable));
                }
            }
        }
    }
    let mut plugins: Vec<_> = packages.into_values().map(|(plugin, _)| plugin).collect();
    plugins.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    (plugins, skipped)
}

/// Exact integer controls retain all u64 fuel values, without a float slider conversion.
pub(crate) struct LimitInputs {
    applied: solidcraft_engine::plugin::HostLimits,
    fuel: String,
    memory: String,
}
impl From<solidcraft_engine::plugin::HostLimits> for LimitInputs {
    fn from(applied: solidcraft_engine::plugin::HostLimits) -> Self {
        Self { fuel: applied.max_fuel.to_string(), memory: applied.max_memory_mib.to_string(), applied }
    }
}
pub(crate) fn host_limits(p: &crate::prefs::Prefs) -> solidcraft_engine::plugin::HostLimits {
    solidcraft_engine::plugin::HostLimits { max_fuel: p.plugin_max_fuel, max_memory_mib: p.plugin_max_memory_mib }
}
pub(crate) fn sync_policy(app: &mut SolveApp) {
    // A failed write is retried by the next sync; the command that changed the grants reports it.
    if let Err(e) = sync_store(app) {
        log::warn!("plugins: {e}");
    }
    app.session.disabled_plugins.clone_from(&app.preferences.disabled_plugins);
    app.session.plugin_limits = host_limits(&app.preferences);
    app.session.plugin_state.grants.clone_from(&app.preferences.plugin_grants);
}
/// Mirror successful install/remove command security state into persisted user preferences.
/// This also covers commands from the control channel, without opening Preferences.
/// An error: the shared grant store still holds what it held, so the command line and headless MCP
/// may still use a grant taken back here; the next sync tries again.
pub(crate) fn persist_grants(app: &mut SolveApp) -> Result<(), String> {
    for grants in app.session.plugin_state.grants.values_mut() {
        grants.folders.sort_by(|a, b| a.name.cmp(&b.name));
    }
    app.preferences.plugin_grants.clone_from(&app.session.plugin_state.grants);
    let saved = sync_store(app);
    app.session.plugin_state.grants.clone_from(&app.preferences.plugin_grants);
    saved
}

/// Start sharing grants through the plugins folder's grant store ([`SolveApp::share_plugin_grants`]).
pub(crate) fn share_grants(app: &mut SolveApp) {
    let Some(dir) = app.session.plugin_state.plugins_dir() else { return };
    let mut synced = solidcraft_engine::plugin::grant_store::Synced::default();
    if let Err(e) = synced.adopt(&dir, &mut app.preferences.plugin_grants) {
        log::warn!("plugins: approvals could not be shared with the command line: {e}");
    }
    app.preferences.plugin_grants.retain(|id, grants| valid_id(id) && valid_grants(grants));
    app.plugins.grant_store = Some(synced);
    sync_policy(app);
}

/// Write the grants changed here to the shared store, then take in what the command line or
/// headless MCP changed there. Nothing to do unless the app shares its grants.
fn sync_store(app: &mut SolveApp) -> Result<(), String> {
    let Some(synced) = app.plugins.grant_store.as_mut() else { return Ok(()) };
    let Some(dir) = app.session.plugin_state.plugins_dir() else { return Ok(()) };
    let saved = synced.persist(&dir, &app.preferences.plugin_grants);
    // A record that could not be written or deleted reads back as last seen: no change here.
    synced.refresh(&dir, &mut app.preferences.plugin_grants);
    saved.map_err(|e| format!("approvals could not be shared with the command line: {e}"))
}

/// Bound persisted grants as strictly as manifest permissions, including host folder paths.
pub(crate) fn valid_grants(grants: &solidcraft_engine::plugin::Grants) -> bool {
    solidcraft_engine::plugin::grant_store::valid_record(grants)
}

pub(crate) enum PackageAction {
    Describe { path: String },
    Install,
    Remove,
    Control { request: crate::control::ControlRequest, level: Option<solidcraft_engine::schema::ParamCheck> },
}

type PackageReply = Result<(serde_json::Value, solidcraft_engine::plugin::AppState), String>;

/// How long a package job may run before the app gives up on it and takes the next one (#20814).
/// Reading, checking and unpacking even the largest package takes seconds; a job still silent after
/// this long is stuck, and it must not leave the app refusing every install and removal for good.
pub(crate) const PACKAGE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(300);

/// What the control channel says about a running package job (`control.pending`, `control.running`).
struct JobInfo {
    key: u64,
    command: &'static str,
    subject: Option<String>,
    started: web_time::Instant,
}

static RUNNING_JOBS: std::sync::Mutex<Vec<JobInfo>> = std::sync::Mutex::new(Vec::new());
static JOB_KEYS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The running package jobs as the control channel lists them: never their parameters, only the
/// command and the archive or plugin it works on.
pub(crate) fn running_jobs() -> Vec<serde_json::Value> {
    let Ok(jobs) = RUNNING_JOBS.lock() else { return Vec::new() };
    jobs.iter()
        .map(|job| {
            let running = job.started.elapsed();
            let mut row = serde_json::json!({
                "job": "plugin_package",
                "command": job.command,
                "running_s": running.as_secs(),
                "ends_by_itself_in_s": PACKAGE_DEADLINE.saturating_sub(running).as_secs(),
                "note": "holds plugin.install and plugin.remove; it is abandoned by itself when its time is up",
            });
            if let Some(subject) = &job.subject {
                row["subject"] = serde_json::json!(subject);
            }
            row
        })
        .collect()
}

/// The archive or plugin a package command works on, short enough to repeat in a message.
#[cfg(not(target_arch = "wasm32"))]
fn job_subject(command: &str, params: &serde_json::Value) -> Option<String> {
    let name = if command == "plugin.remove" {
        params.get("id")?.as_str()?.to_owned()
    } else {
        let path = params.get("path")?.as_str()?;
        Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
    };
    Some(name.chars().take(80).collect())
}

/// Package I/O keeps running when Preferences closes; only its application policy is adopted.
pub(crate) struct PackageJob {
    command: &'static str,
    action: PackageAction,
    grants_before: std::collections::BTreeMap<String, solidcraft_engine::plugin::Grants>,
    /// `PrefsWindow::grants_revision` when the job started.
    revision: u64,
    done: std::sync::mpsc::Receiver<PackageReply>,
    started: web_time::Instant,
    /// Its entry in [`RUNNING_JOBS`], removed when the job ends.
    key: u64,
}

impl Drop for PackageJob {
    fn drop(&mut self) {
        if let Ok(mut jobs) = RUNNING_JOBS.lock() {
            jobs.retain(|job| job.key != self.key);
        }
    }
}

impl PackageJob {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(
        command: &'static str,
        subject: Option<String>,
        action: PackageAction,
        grants_before: std::collections::BTreeMap<String, solidcraft_engine::plugin::Grants>,
        revision: u64,
        done: std::sync::mpsc::Receiver<PackageReply>,
    ) -> Self {
        let key = JOB_KEYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = web_time::Instant::now();
        if let Ok(mut jobs) = RUNNING_JOBS.lock() {
            jobs.push(JobInfo { key, command, subject, started });
        }
        PackageJob { command, action, grants_before, revision, done, started, key }
    }
}

/// What refuses a new install or removal while a package job runs: which, on what, for how long, and
/// that it ends by itself.
pub(crate) fn busy_message(app: &SolveApp) -> String {
    busy_message_at(app, web_time::Instant::now())
}

fn busy_message_at(app: &SolveApp, now: web_time::Instant) -> String {
    let Some(job) = app.prefs_window.plugin_job.as_ref() else { return tr!(app, "prefs.plugin_busy").to_owned() };
    let subject = RUNNING_JOBS.lock().ok().and_then(|jobs| jobs.iter().find(|info| info.key == job.key).and_then(|info| info.subject.clone()));
    let what = match subject {
        Some(subject) => format!("{} ({subject})", job.command),
        None => job.command.to_owned(),
    };
    let running = now.saturating_duration_since(job.started).as_secs().to_string();
    // The saved language, as `SolveApp::tr` reads it: the control channel has no render scope.
    solidcraft_ui_text::Locale::for_language(&app.preferences.language)
        .format("prefs.plugin_busy_running", &[("what", &what), ("seconds", &running), ("limit", &PACKAGE_DEADLINE.as_secs().to_string())])
}

pub(crate) fn busy(app: &SolveApp) -> bool {
    app.prefs_window.plugin_job.is_some()
}

pub(crate) fn start_package(app: &mut SolveApp, command: &'static str, params: serde_json::Value, action: PackageAction) -> Result<(), String> {
    spawn_package(app, command, params, solidcraft_engine::io::vfs::current_scope(), None, action)
}

fn spawn_package(
    app: &mut SolveApp,
    command: &'static str,
    params: serde_json::Value,
    scope: Option<solidcraft_engine::io::vfs::PathScope>,
    level: Option<solidcraft_engine::schema::ParamCheck>,
    action: PackageAction,
) -> Result<(), String> {
    if busy(app) {
        return Err(busy_message(app));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sync_policy(app);
        let grants_before = app.session.plugin_state.grants.clone();
        let mut copy = app.session.scratch();
        if let Some(level) = level {
            copy.param_check = level;
        }
        let subject = job_subject(command, &params);
        let (send, done) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("plugin-package".into())
            .spawn(move || {
                let reply = solidcraft_engine::io::vfs::scoped(scope, || {
                    copy.execute(command, &params).map(|reply| (reply, copy.plugin_state)).map_err(|e| e.to_string())
                });
                let _ = send.send(reply);
            })
            .map_err(|e| e.to_string())?;
        app.prefs_window.plugin_job = Some(PackageJob::new(command, subject, action, grants_before, app.prefs_window.grants_revision, done));
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (command, params, scope, level, action);
        Err(tr!(app, "ui.plugins_desktop").to_owned())
    }
}

/// Poll package I/O on every frame, including after Preferences closes. Tests drain deterministically.
/// It runs in the browser too, so its clock is `web_time`: `std::time::Instant::now` panics on wasm.
pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context, wait: bool) {
    tick_at(app, ctx, wait, web_time::Instant::now());
}

/// [`tick`] at the time `now`. A job still silent after [`PACKAGE_DEADLINE`] is abandoned: it fails
/// like any other, and its reply, should it ever come, has nowhere to go (the job worked on a copy of
/// the session, so nothing of it reaches this one).
fn tick_at(app: &mut SolveApp, ctx: &egui::Context, wait: bool, now: web_time::Instant) {
    let Some(job) = app.prefs_window.plugin_job.take() else { return };
    let result = if wait {
        job.done.recv().unwrap_or_else(|e| Err(e.to_string()))
    } else {
        match job.done.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) if now.saturating_duration_since(job.started) < PACKAGE_DEADLINE => {
                app.prefs_window.plugin_job = Some(job);
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Err(solidcraft_ui_text::Locale::for_language(&app.preferences.language)
                .format("prefs.plugin_job_abandoned", &[("command", job.command), ("seconds", &PACKAGE_DEADLINE.as_secs().to_string())])),
            Err(error) => Err(error.to_string()),
        }
    };
    let result = result.and_then(|(reply, state)| {
        if job.command == "plugin.remove"
            || (job.command == "plugin.install" && reply.get("installed").and_then(serde_json::Value::as_bool) == Some(true))
        {
            let id = reply["id"].as_str().ok_or_else(|| tr!(app, "prefs.plugin_reply_invalid").to_owned())?;
            if let Some(path) = state.packages.get(id) {
                app.session.plugin_state.packages.insert(id.into(), path.clone());
            } else {
                app.session.plugin_state.packages.remove(id);
            }
            // Take in grants another program changed or revoked in the shared store since the job
            // started: a revoked record must not come back with this job's grant.
            sync_policy(app);
            // Preferences can change while package I/O runs. Keep a newer decision for this id.
            // A write to the stored grants during the job counts as a newer decision, unless the record this
            // job started from is still exactly as it was (an unrelated write).
            let live = app.session.plugin_state.grants.get(id);
            let undecided = if app.prefs_window.grants_revision == job.revision {
                live == job.grants_before.get(id)
            } else {
                live.is_some() && live == job.grants_before.get(id)
            };
            if undecided {
                if let Some(grants) = state.grants.get(id) {
                    // A record added meanwhile may have filled the map: keep what is stored.
                    if app.session.plugin_state.grants.contains_key(id) || app.session.plugin_state.grants.len() < MAX_DISABLED {
                        app.session.plugin_state.grants.insert(id.into(), grants.clone());
                    }
                } else {
                    app.session.plugin_state.grants.remove(id);
                }
            }
            if job.command == "plugin.remove" {
                app.preferences.disabled_plugins.remove(id);
                app.session.disabled_plugins.remove(id);
            }
            // A removed or replaced plugin's background is read again from its folder, or not at all.
            #[cfg(not(target_arch = "wasm32"))]
            crate::plugin_shaders::forget(app, id);
            persist_grants(app)?;
        }
        if let PackageAction::Describe { path } = &job.action {
            crate::plugin_install::described(app, path.clone(), reply.clone());
        }
        if matches!(job.action, PackageAction::Install) {
            crate::plugin_install::installed(app, &reply);
        }
        Ok(reply)
    });
    let result = app.command_completed(job.command, result);
    if let PackageAction::Control { request, level } = &job.action {
        let mut reply = match result {
            Ok(value) => serde_json::json!({"ok":true,"result":value}),
            Err(error) => serde_json::json!({"ok":false,"error":error}),
        };
        if let Some(level) = level {
            reply["param_check"] = serde_json::json!(level.name());
        }
        let _ = request.reply.send(reply);
    } else if let Err(error) = result {
        app.set_status(error, true);
    }
    ctx.request_repaint();
}

/// Native control calls use the same command worker and retain their exact path confinement.
/// A filesystem mutation remains running until its final reply; it is never offered as cancellable.
pub(crate) fn offload_control(
    app: &mut SolveApp,
    ctx: &egui::Context,
    request: crate::control::ControlRequest,
) -> Option<crate::control::ControlRequest> {
    if cfg!(target_arch = "wasm32") || !matches!(request.method.as_str(), "engine.execute" | "command") {
        return Some(request);
    }
    let Ok(execute) = crate::control_params::parse::<crate::control_params::Execute>(&request.params) else { return Some(request) };
    let Some(command) =
        solidcraft_engine::find_command(&execute.command).map(|spec| spec.id).filter(|id| matches!(*id, "plugin.install" | "plugin.remove"))
    else {
        return Some(request);
    };
    if request.refuse_cancelled_chat() {
        return None;
    }
    let scope = match crate::control::request_scope(app, &request.params) {
        Ok(scope) => scope,
        Err(error) => {
            let _ = request.reply.send(serde_json::json!({"ok":false,"error":error}));
            return None;
        }
    };
    let level = match crate::control::request_param_check(&request.method, &request.params) {
        Ok(level) => level,
        Err(error) => {
            let _ = request.reply.send(serde_json::json!({"ok":false,"error":error}));
            return None;
        }
    };
    let reply = request.reply.clone();
    if let Err(error) = spawn_package(app, command, execute.params(), scope, level, PackageAction::Control { request, level }) {
        let _ = reply.send(serde_json::json!({"ok":false,"error":error}));
    }
    ctx.request_repaint();
    None
}

fn remove_confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((id, name)) = app.prefs_window.plugin_remove.clone() else { return };
    let title = tr!("prefs.plugin_remove_title", name = &name);
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::unique("plugin_remove"), "delete", &title)
        .tone(crate::ui_kit::Tone::Danger)
        .actions(tr!(app, "prefs.plugin_remove"), tr!(app, "prefs.plugin_cancel"), None)
        .selected(crate::ui_kit::Action::Cancel)
        .show(ctx, true, |ui| {
            ui.add(egui::Label::new(tr!(app, "prefs.plugin_remove_note")).wrap());
        });
    if closed || action == crate::ui_kit::Action::Cancel {
        app.prefs_window.plugin_remove = None;
    }
    if action == crate::ui_kit::Action::Ok {
        app.prefs_window.plugin_remove = None;
        if let Err(error) = start_package(app, "plugin.remove", serde_json::json!({"id":id}), PackageAction::Remove) {
            app.set_status(error, true);
        }
        ctx.request_repaint();
    }
}

fn parsed_limits(input: &LimitInputs) -> Option<solidcraft_engine::plugin::HostLimits> {
    let limits =
        solidcraft_engine::plugin::HostLimits { max_fuel: input.fuel.trim().parse().ok()?, max_memory_mib: input.memory.trim().parse().ok()? };
    limits.validate().ok()?;
    Some(limits)
}
pub(crate) fn resource_pending(app: &SolveApp) -> bool {
    let current = host_limits(&app.preferences);
    app.prefs_window.plugin_limit_inputs.as_ref().is_some_and(|input| {
        input.applied == current && (input.fuel != current.max_fuel.to_string() || input.memory != current.max_memory_mib.to_string())
    })
}
/// Enter, Apply and OK commit both exact integers together. An incomplete edit cannot save a prefix.
pub(crate) fn commit_resource_inputs(app: &mut SolveApp) -> Result<(), String> {
    let Some(input) = app.prefs_window.plugin_limit_inputs.take() else { return Ok(()) };
    // A control client has already changed the preferences: never overwrite that newer policy.
    if input.applied != host_limits(&app.preferences) {
        return Ok(());
    }
    let Some(limits) = parsed_limits(&input) else {
        app.prefs_window.plugin_limit_inputs = Some(input);
        return Err(tr!(app, "prefs.plugin_limits_invalid").to_string());
    };
    app.preferences.plugin_max_fuel = limits.max_fuel;
    app.preferences.plugin_max_memory_mib = limits.max_memory_mib;
    app.prefs_window.plugin_limit_inputs = Some(limits.into());
    sync_policy(app);
    Ok(())
}
fn resource_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let current = host_limits(&app.preferences);
    let mut input = app.prefs_window.plugin_limit_inputs.take().filter(|v| v.applied == current).unwrap_or_else(|| current.into());
    let (fuel_enter, _) = crate::ui_kit::text_row(ui, tr!(app, "prefs.plugin_max_fuel"), &mut input.fuel, "");
    let (memory_enter, _) = crate::ui_kit::text_row(ui, tr!(app, "prefs.plugin_max_memory"), &mut input.memory, "");
    if parsed_limits(&input).is_none() {
        crate::ui_kit::text(ui, tr!(app, "prefs.plugin_limits_invalid"), crate::ui_kit::Text::Error("plugin_limits"));
    }
    crate::ui_kit::text(ui, tr!(app, "prefs.plugin_limits_note"), crate::ui_kit::Text::Hint("plugin_limits"));
    app.prefs_window.plugin_limit_inputs = Some(input);
    if (fuel_enter || memory_enter)
        && let Err(error) = commit_resource_inputs(app)
    {
        app.set_status(error, true);
    }
    ui.separator();
}

/// Whether an installed plugin is offered. Unknown ids are on; an id in `disabled_plugins` is off.
pub fn enabled(app: &SolveApp, id: &str) -> bool {
    !app.preferences.disabled_plugins.contains(id)
}

fn matches_query(plugin: &Plugin, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    [plugin.name.as_str(), plugin.id.as_str(), plugin.description.as_str(), plugin.version.as_str()]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
}

/// The Plugins page. Desktop only: the browser build has no per-user plugin folder.
pub fn page(app: &mut SolveApp, ui: &mut egui::Ui) {
    let dir = if cfg!(target_arch = "wasm32") { None } else { app.session.plugin_state.plugins_dir() };
    page_in_dir(app, ui, dir.as_deref());
}

fn page_in_dir(app: &mut SolveApp, ui: &mut egui::Ui, dir: Option<&Path>) {
    crate::ui_kit::settings_page(ui, |ui| {
        crate::ui_kit::settings_heading(ui, tr!("ui.plugins"), tr!("prefs.plugins_intro"));
        let Some(dir) = dir else {
            ui.label(tr!("ui.plugins_desktop"));
            return;
        };
        let plugins = crate::plugin_slots::preferences_inventory(app, dir);
        if plugins.is_empty() {
            // First-time users need the package format and a way to find the guide before
            // global host resource limits, which are rarely useful without a plugin (#18764).
            let intro = crate::ui_kit::text(ui, tr!("prefs.plugins_get_started"), crate::ui_kit::Text::Hint("plugins_get_started"));
            crate::scenario::publish_rect("prefs:plugins:get_started", intro.rect);
            let guide =
                ui.hyperlink_to(RichText::new(tr!("prefs.plugins_guide")), "https://github.com/tangency-labs/solidcraft/blob/main/docs/plugins.md");
            crate::scenario::publish_target("prefs:plugins:guide", guide.rect);
            ui.add_space(8.0);
        }
        // Install plugin…: pick a .zip, review what it asks for, install it.
        let install = crate::ui_kit::icon_label_button(ui, tr!("plugin_install.menu"), "import", !busy(app), false);
        crate::scenario::publish_target("prefs:plugins:install", install.rect);
        if install.clicked() {
            crate::plugin_install::start(app);
        }
        ui.add_space(8.0);
        let t = solidcraft_theme::Tokens::get();
        let path = dir.display().to_string();
        let searching = crate::ui_kit::settings_search::querying(ui);
        // The page's own box is for browsing Plugins. The dialog search is the filter while it is active.
        if !searching {
            crate::ui_kit::search_field(ui, ui.scope_id().with("plugins_search"), &mut app.prefs_window.plugin_query, tr!("ui.plugins_search"));
            ui.add_space(8.0);
        }
        let skipped = crate::plugin_slots::skipped(app).to_vec();
        let query = app.prefs_window.plugin_query.clone();
        crate::ui_kit::settings_list_section(ui, "plugins_installed", tr!("prefs.plugins_installed"), Some(&plugins.len().to_string()), |ui| {
            if plugins.is_empty() && (!searching || crate::ui_kit::settings_search::shows_all(ui)) {
                crate::ui_kit::text(ui, tr!("ui.plugins_empty"), crate::ui_kit::Text::Hint("plugins_empty"));
            }
            ui.spacing_mut().item_spacing.y = t.space_8;
            let mut shown = 0usize;
            for plugin in plugins.iter().filter(|plugin| searching || matches_query(plugin, &query)) {
                if searching
                    && !crate::ui_kit::settings_search::shows_all(ui)
                    && !crate::ui_kit::settings_search::texts_hit(ui, &[&plugin.name, &plugin.id, &plugin.description, &plugin.version])
                {
                    continue;
                }
                shown += 1;
                if searching {
                    crate::ui_kit::settings_search::reveal(ui);
                    crate::ui_kit::settings_search::push_force(ui);
                }
                plugin_card(app, ui, dir, plugin);
                if searching {
                    crate::ui_kit::settings_search::pop_force(ui);
                }
            }
            if !searching && !plugins.is_empty() && shown == 0 {
                ui.label(tr!("ui.plugins_none"));
            }
        });
        if !searching && query.trim().is_empty() && !skipped.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(tr!("ui.plugins_skipped", count = &skipped.len().to_string())).size(t.font_small).color(t.text_dim));
            for skip in skipped.iter().take(8) {
                let who = if skip.folder.is_empty() { "folder" } else { skip.folder.as_str() };
                ui.label(RichText::new(format!("{who}: {}", skip.reason)).size(t.font_small).color(t.text_dim));
            }
        }
        ui.add_space(8.0);
        let was_expanded = app.preferences.plugin_limits_expanded;
        let advanced = crate::ui_kit::checkbox(ui, tr!("prefs.plugins_advanced_limits"), &mut app.preferences.plugin_limits_expanded);
        crate::scenario::publish_target("prefs:plugins:advanced_limits", advanced.rect);
        if was_expanded && !app.preferences.plugin_limits_expanded {
            // Do not leave a hidden, unfinished fuel or memory change pending for Apply.
            app.prefs_window.plugin_limit_inputs = None;
        }
        if app.preferences.plugin_limits_expanded {
            resource_controls(app, ui);
        }
        crate::ui_kit::checkbox(ui, tr!("prefs.plugin_remember_dialog_position"), &mut app.preferences.plugin_remember_dialog_position)
            .on_hover_text(tr!("prefs.plugin_remember_dialog_position_help"));
        ui.add_space(8.0);
        if !searching {
            ui.horizontal(|ui| {
                let button = crate::ui_kit::icon_label_button(ui, tr!("prefs.plugins_show_folder"), "folder", true, false)
                    .backing_on_hover_text(tr!("ui.plugins_note", path = &path));
                crate::scenario::publish_target("prefs:plugins:folder", button.rect);
                if button.clicked() {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        app.set_status(format!("Could not create the plugins folder: {e}"), true);
                    } else if let Some(show) = &app.services.show_in_folder {
                        // The service opens the parent of the supplied file, even if it is absent.
                        show(&dir.join("plugin.json").display().to_string());
                    }
                }
            });
        }
    });
    remove_confirmation(app, ui.ctx());
}

/// What was approved at install for `plugin`'s current package, in plain words: command groups,
/// folders with their access, fuel and memory. Grants made for another version count for nothing.
pub(crate) fn approved_text(app: &SolveApp, plugin: &Plugin) -> String {
    use solidcraft_ui_text::tr_by_id;
    let Some(grants) = app.session.plugin_state.grants.get(&plugin.id).filter(|g| g.package_hash.as_deref() == Some(plugin.approval.as_str())) else {
        return tr!("prefs.plugin_approved_nothing").to_string();
    };
    let mut parts: Vec<String> = grants
        .command_groups
        .iter()
        .map(|group| match group.as_str() {
            "app" | "doc" | "file" | "plugin" => tr_by_id(&std::format!("prefs.plugin_group_short_{group}")).to_string(),
            other => other.to_string(),
        })
        .collect();
    for folder in &grants.folders {
        let access = tr_by_id(match (folder.read, folder.write) {
            (true, true) => "prefs.plugin_folder_read_write",
            (false, true) => "prefs.plugin_folder_write",
            _ => "prefs.plugin_folder_read",
        });
        parts.push(tr!("prefs.plugin_folder", name = &folder.name, access = access));
    }
    if let Some(fuel) = grants.fuel {
        parts.push(tr!("prefs.plugin_approved_fuel", fuel = &fuel.to_string()));
    }
    if let Some(memory) = grants.memory_mib {
        parts.push(tr!("prefs.plugin_approved_memory", memory = &memory.to_string()));
    }
    if parts.is_empty() {
        return tr!("prefs.plugin_approved_nothing").to_string();
    }
    tr!("prefs.plugin_approved", list = &parts.join(", "))
}

fn plugin_card(app: &mut SolveApp, ui: &mut egui::Ui, dir: &Path, plugin: &Plugin) {
    use crate::ui_kit;
    let mut on = enabled(app, &plugin.id);
    let was_on = on;
    let labels = ui_kit::PluginCardLabels { show_in_folder: solidcraft_ui_text::tr_label("Show in folder"), enabled: tr!("prefs.plugins_enabled") };
    let remove_label = tr!(app, "prefs.plugin_remove").to_owned();
    let can_remove = !busy(app) && Path::new(&plugin.path) == dir.join(&plugin.id);
    let card = ui
        .push_id(&plugin.id, |ui| {
            ui_kit::settings_plugin_card_with_actions(
                ui,
                ui_kit::PluginCardContent { key: &plugin.id, name: &plugin.name, version: &plugin.version, description: &plugin.description },
                &mut on,
                &labels,
                |ui| {
                    ui.add_space(solidcraft_theme::Tokens::get().space_8);
                    ui_kit::icon_label_button(ui, &remove_label, "delete", can_remove, false)
                },
            )
        })
        .inner;
    let response = card.response;
    let ((folder, check), remove) = card.inner;
    // What the user approved for this exact package, under its card.
    ui.add_space(solidcraft_theme::Tokens::get().space_2);
    let approved = approved_text(app, plugin);
    let t = solidcraft_theme::Tokens::get();
    ui.horizontal_wrapped(|ui| {
        ui.add_space(t.space_12);
        ui.label(RichText::new(approved).size(t.font_small).color(t.text_dim));
    });
    if let Some(error) = app.session.plugin_state.listener_failures.get(&plugin.id) {
        let text = tr!("prefs.plugin_listener_failed", error = error);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(t.space_12);
            ui.label(RichText::new(text).size(t.font_small).color(t.text_dim));
        });
    }
    crate::scenario::publish_target(&format!("prefs:plugin:{}:folder", plugin.id), folder.rect);
    crate::scenario::publish_target(&format!("prefs:plugin:{}", plugin.id), check.rect);
    if folder.clicked()
        && let Some(show) = &app.services.show_in_folder
    {
        show(&Path::new(&plugin.path).join("plugin.json").display().to_string());
    }
    if was_on != on {
        if on {
            app.preferences.disabled_plugins.remove(&plugin.id);
        } else if app.preferences.disabled_plugins.len() < MAX_DISABLED {
            app.preferences.disabled_plugins.insert(plugin.id.clone());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    plugin_settings(app, ui, plugin, on);
    crate::scenario::publish_target(&format!("prefs:plugin:{}:remove", plugin.id), remove.rect);
    if remove.clicked() {
        app.prefs_window.plugin_remove = Some((plugin.id.clone(), plugin.name.clone()));
    }
    crate::scenario::publish_rect(&format!("prefs:plugin:{}:card", plugin.id), response.rect);
    let texts = [&plugin.name, &plugin.id, &plugin.description, &plugin.version];
    if let Some(text) = texts.iter().find(|text| crate::ui_kit::settings_search::texts_hit(ui, &[*text])) {
        crate::ui_kit::settings_search::remember(ui, text, response.rect);
    }
    for text in texts {
        crate::ui_kit::settings_search::follow(ui, text, response.rect);
    }
}

/// The plugin's own settings, under its card. A background (`shaders@0.1`) has a switch; one
/// background is on at a time, and only while its plugin is on.
#[cfg(not(target_arch = "wasm32"))]
fn plugin_settings(app: &mut SolveApp, ui: &mut egui::Ui, plugin: &Plugin, on: bool) {
    if plugin.shaders.is_empty() {
        return;
    }
    let label = tr!(app, "prefs.background_shader").to_owned();
    ui.indent(("plugin_settings", &plugin.id), |ui| {
        ui.add_enabled_ui(on, |ui| {
            for (key, name) in &plugin.shaders {
                let active = on && app.preferences.background_shader == *key;
                let mut value = active;
                let response = crate::ui_kit::checkbox(ui, &format!("{label}: {name}"), &mut value);
                crate::scenario::publish_target(&format!("prefs:background:{key}"), response.rect);
                if value != active {
                    app.preferences.background_shader = if value { key.clone() } else { String::new() };
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn package(h: &crate::scenario::Harness, id: &str) -> PathBuf {
        let dir = h.dir.join("packages").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::to_vec(&serde_json::json!({
                "id":id,"name":"Workshop helper","version":"1.2.3","description":"Original plugin fixture for reviewing host permissions.",
                "capabilities":["commands@0.1"],
                "runtime":{"module":"plugin.wasm","entries":[{"name":"inspect","effect":"read","slots":[
                    {"kind":"menu","name":"Manufacturing"},{"kind":"ribbon","name":"Manufacturing"}
                ]}],"commands":[],"permissions":{"command_groups":["file"],"files":[{"name":"parts","read":true}],"clock":true}}
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("plugin.wasm"), b"\0asm\x0d\0\x01\0").unwrap();
        dir
    }

    fn zip_package(path: &Path) -> PathBuf {
        let destination = path.with_extension("zip");
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in ["plugin.json", "plugin.wasm"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, &std::fs::read(path.join(name)).unwrap()).unwrap();
        }
        std::fs::write(&destination, zip.finish().unwrap().into_inner()).unwrap();
        destination
    }

    fn capture_page(app: &mut SolveApp, dir: &Path, name: &str) {
        capture_page_sized(app, dir, name, egui::vec2(1280.0, 720.0));
    }

    fn capture_page_sized(app: &mut SolveApp, dir: &Path, name: &str, size: egui::Vec2) {
        let Ok(output_dir) = std::env::var("SOLIDCRAFT_PLUGINS_EVIDENCE") else { return };
        let ctx = crate::fonts::test_context();
        let mut textures = std::collections::HashMap::new();
        for frame in 0..6 {
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
                crate::ui_kit::scope(ui, |ui| page_in_dir(app, ui, Some(dir)))
            });
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            if frame == 5 {
                let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                std::fs::create_dir_all(&output_dir).unwrap();
                image.save(Path::new(&output_dir).join(name)).unwrap();
            }
        }
    }

    fn hand_install(path: &Path, root: &Path, id: &str) {
        let target = root.join(id);
        std::fs::create_dir_all(&target).unwrap();
        for name in ["plugin.json", "plugin.wasm"] {
            std::fs::copy(path.join(name), target.join(name)).unwrap();
        }
    }

    #[test]
    fn design_notifications_report_handler_failure_in_plugin_preferences() {
        fn has_text(shape: &egui::Shape, expected: &str) -> bool {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_text(shape, expected)),
                egui::Shape::Text(text) => text.galley.job.text.contains(expected),
                _ => false,
            }
        }
        fn text_bounds(shape: &egui::Shape, expected: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_bounds(shape, expected)),
                egui::Shape::Text(text) if text.galley.job.text.contains(expected) => Some(text.visual_bounding_rect()),
                _ => None,
            }
        }
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("installed");
        let path = package(&h, "workshop-helper");
        hand_install(&path, &root, "workshop-helper");
        h.app.session.plugin_state.listener_failures.insert("workshop-helper".into(), "read entries cannot edit the design".into());
        for size in [egui::vec2(1280.0, 720.0), egui::vec2(640.0, 480.0)] {
            let ctx = crate::fonts::test_context();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                crate::ui_kit::scope(ui, |ui| page_in_dir(&mut h.app, ui, Some(&root)))
            });
            output.textures_delta.clear();
            assert!(
                output.shapes.iter().any(|shape| has_text(&shape.shape, "read entries cannot edit the design")),
                "diagnostic remains visible at {size:?}"
            );
            let (painted, clip) = output
                .shapes
                .iter()
                .find_map(|shape| text_bounds(&shape.shape, "read entries cannot edit the design").map(|rect| (rect, shape.clip_rect)))
                .unwrap();
            assert!(bounds.contains_rect(painted) && clip.contains_rect(painted), "diagnostic is fully painted at {size:?}: {painted:?}");
        }
        capture_page(&mut h.app, &root, "plugin-design-listener-failure.png");
        capture_page_sized(&mut h.app, &root, "plugin-design-listener-failure-small.png", egui::vec2(640.0, 480.0));
    }

    #[test]
    fn plugins_remove_keeps_grants_revoked_after_preferences_cancel() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("installed");
        h.app.session.plugin_state.plugins_dir = Some(root.clone());
        let path = package(&h, "workshop-helper");
        hand_install(&path, &root, "workshop-helper");
        let package = solidcraft_engine::plugin::install::unpack(&std::fs::read(zip_package(&path)).unwrap()).unwrap();
        let grants = solidcraft_engine::plugin::Grants { command_groups: ["file".into()].into(), ..Default::default() };
        solidcraft_engine::plugin::store_grants(&mut h.app.session, &package, grants).unwrap();
        let grants = h.app.session.plugin_state.grants["workshop-helper"].clone();
        persist_grants(&mut h.app).unwrap();
        let ctx = crate::fonts::test_context();
        crate::prefs::control(&mut h.app, &json!({"open":"Plugins"})).unwrap();
        let persisted: serde_json::Value = serde_json::from_str(&h.app.prefs()).unwrap();
        assert_eq!(persisted["preferences"]["plugin_grants"]["workshop-helper"]["command_groups"], json!(["file"]));
        assert_eq!(persisted["preferences"]["plugin_grants"]["workshop-helper"]["package_hash"], package.approval);
        crate::prefs::control(&mut h.app, &json!({"button":"defaults"})).unwrap();
        assert_eq!(h.app.session.plugin_state.grants["workshop-helper"], grants);
        crate::prefs::control(&mut h.app, &json!({"button":"cancel"})).unwrap();
        assert_eq!(h.app.preferences.plugin_grants["workshop-helper"], grants);
        capture_page(&mut h.app, &root, "plugin-list-remove.png");
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.session.plugin_state.grants["workshop-helper"], grants);
        crate::prefs::control(&mut h.app, &json!({"open":"Plugins"})).unwrap();
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &root, vec![]);
        }
        click_page(&mut h.app, &ctx, &root, "prefs:plugin:workshop-helper:remove");
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &root, vec![]);
        }
        capture_page(&mut h.app, &root, "plugin-remove-confirm.png");
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &root, vec![]);
        }
        click_page(&mut h.app, &ctx, &root, "dialog:cancel");
        assert!(root.join("workshop-helper").exists() && h.app.prefs_window.plugin_remove.is_none());
        // The modal's previous-frame input shield must retire before clicking the card again.
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &root, vec![]);
        }
        click_page(&mut h.app, &ctx, &root, "prefs:plugin:workshop-helper:remove");
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &root, vec![]);
        }
        click_page(&mut h.app, &ctx, &root, "dialog:ok");
        // Closing Preferences cannot cancel an already confirmed filesystem mutation.
        crate::prefs::control(&mut h.app, &json!({"button":"cancel"})).unwrap();
        tick(&mut h.app, &ctx, true);
        assert!(!root.join("workshop-helper").exists() && h.app.preferences.plugin_grants.is_empty());
    }

    #[test]
    fn plugins_programmatic_describe_and_remove_preserve_scope_without_opening_ui() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("controlled-packages");
        h.app.session.plugin_state.plugins_dir = Some(root.clone());
        let path = package(&h, "controlled-plugin");
        let (request, reply) = crate::control::ControlRequest::new(
            "engine.execute",
            json!({
                "command":"plugin.install","params":{"path":zip_package(&path),"dry_run":true},"allow_paths":[h.dir],"param_check":"strict"
            }),
        );
        assert!(request.ticket.take());
        assert!(offload_control(&mut h.app, &h.ctx, request).is_none());
        assert!(reply.try_recv().is_err());
        tick(&mut h.app, &h.ctx, true);
        let reviewed = reply.recv().unwrap();
        assert_eq!(reviewed["ok"], true, "{reviewed}");
        assert_eq!(reviewed["param_check"], "strict");
        assert!(h.app.plugin_install.ask.is_none() && !h.app.prefs_window.shell.open);
        let (request, installed) = crate::control::ControlRequest::new(
            "engine.execute",
            json!({
                "command":"plugin.install","params":{"path":zip_package(&path),"approve":{"command_groups":["file"]}},"allow_paths":[h.dir]
            }),
        );
        assert!(request.ticket.take());
        assert!(offload_control(&mut h.app, &h.ctx, request).is_none());
        tick(&mut h.app, &h.ctx, true);
        let installed = installed.recv().unwrap();
        assert_eq!(installed["ok"], true, "{installed}");
        assert!(h.app.preferences.plugin_grants["controlled-plugin"].command_groups.contains("file"));
        assert!(h.app.session.plugin_state.packages.contains_key("controlled-plugin"));
        assert!(h.app.plugin_install.ask.is_none() && !h.app.prefs_window.shell.open);
        let (request, denied) = crate::control::ControlRequest::new(
            "engine.execute",
            json!({
                "command":"plugin.remove","params":{"id":"controlled-plugin"},"allow_paths":[]
            }),
        );
        assert!(offload_control(&mut h.app, &h.ctx, request).is_none());
        tick(&mut h.app, &h.ctx, true);
        assert_eq!(denied.recv().unwrap()["ok"], false);
        assert!(root.join("controlled-plugin").exists(), "empty scope cannot widen on the worker");
        let (request, removed) = crate::control::ControlRequest::new(
            "command",
            json!({
                "command":"plugin.remove","params":{"id":"controlled-plugin"},"allow_paths":[h.dir]
            }),
        );
        assert!(request.ticket.take());
        let ticket = request.ticket.clone();
        assert!(offload_control(&mut h.app, &h.ctx, request).is_none());
        assert!(matches!(ticket.cancel(), solidcraft_control::Cancel::Running));
        let (concurrent, refused) =
            crate::control::ControlRequest::new("engine.execute", json!({"command":"plugin.remove","params":{"id":"controlled-plugin"}}));
        assert!(offload_control(&mut h.app, &h.ctx, concurrent).is_none());
        assert_eq!(refused.recv().unwrap()["ok"], false);
        let revision = h.app.session.revision + 1;
        h.app.session.revision = revision;
        tick(&mut h.app, &h.ctx, true);
        assert_eq!(removed.recv().unwrap()["ok"], true);
        assert_eq!(h.app.session.revision, revision);
        assert!(!root.join("controlled-plugin").exists() && h.app.preferences.plugin_grants.is_empty());
        assert!(h.app.plugin_install.ask.is_none());
    }

    #[test]
    fn plugins_install_keeps_grant_changes_made_while_package_job_runs() {
        use serde_json::json;
        for change in ["unchanged", "changed", "revoked", "revoked_first", "unrelated"] {
            let mut h = crate::scenario::Harness::new();
            let root = h.dir.join("installed");
            h.app.session.plugin_state.plugins_dir = Some(root.clone());
            let path = package(&h, "workshop-helper");
            let zip = zip_package(&path);
            let package = solidcraft_engine::plugin::install::unpack(&std::fs::read(&zip).unwrap()).unwrap();
            if change != "revoked_first" {
                crate::prefs::set(&mut h.app, &json!({"plugin_grants":{"workshop-helper":{"command_groups":["file"]}}})).unwrap();
            }
            start_package(
                &mut h.app,
                "plugin.install",
                json!({"path":zip,"approval":package.approval,"approve":{"command_groups":["file"]}}),
                PackageAction::Install,
            )
            .unwrap();
            // The worker can finish at any point; only tick may publish its copied grants.
            match change {
                "changed" => crate::prefs::set(&mut h.app, &json!({"plugin_grants":{"workshop-helper":{"command_groups":[]}}})).map(drop).unwrap(),
                "revoked" | "revoked_first" => crate::prefs::set(&mut h.app, &json!({"plugin_grants":{}})).map(drop).unwrap(),
                "unrelated" => crate::prefs::set(
                    &mut h.app,
                    &json!({"plugin_grants":{"workshop-helper":{"command_groups":["file"]},"other-plugin":{"command_groups":[]}}}),
                )
                .map(drop)
                .unwrap(),
                _ => {}
            }
            let before = h.app.session.plugin_state.grants.clone();
            tick(&mut h.app, &h.ctx, true);
            assert!(root.join("workshop-helper").exists(), "{change}: installation finishes");
            assert!(h.app.session.plugin_state.packages.contains_key("workshop-helper"));
            if matches!(change, "changed" | "revoked" | "revoked_first") {
                assert_eq!(h.app.session.plugin_state.grants, before, "{change}: keep the live decision");
            } else {
                let installed = &h.app.session.plugin_state.grants["workshop-helper"];
                assert_eq!(installed.package_hash, Some(package.approval.clone()), "{change}: adopt worker grants for an unchanged id");
                assert!(installed.command_groups.contains("file"));
            }
            assert_eq!(h.app.session.plugin_state.grants.get("other-plugin"), before.get("other-plugin"));
            assert_eq!(h.app.preferences.plugin_grants, h.app.session.plugin_state.grants, "{change}: persist the merged grants");
            let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            restored.load_prefs(&h.app.prefs());
            assert_eq!(restored.session.plugin_state.grants, h.app.session.plugin_state.grants, "{change}: reload preserves the merge");
        }
    }

    /// #20814: the refusal follows the saved language even where no render scope is set, as the control
    /// channel has none.
    #[test]
    fn the_busy_message_follows_the_saved_language_without_a_render_scope() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "de".into();
        let (request, _reply) = crate::control::ControlRequest::new("engine.execute", serde_json::json!({"command":"plugin.remove"}));
        let (_send, done) = std::sync::mpsc::channel();
        let job =
            PackageJob::new("plugin.remove", Some("gears".into()), PackageAction::Control { request, level: None }, Default::default(), 0, done);
        let started = job.started;
        h.app.prefs_window.plugin_job = Some(job);
        let message = busy_message_at(&h.app, started + std::time::Duration::from_secs(7));
        assert!(message.contains("Plugin-Paketauftrag") && message.contains("plugin.remove (gears)"), "{message}");
        h.app.preferences.language = "en".into();
        assert!(busy_message_at(&h.app, started).contains("plugin package job"), "English still reads English");
    }

    /// #20814: a package job whose thread never answers must not leave the app refusing installs for good.
    #[test]
    fn a_package_job_that_never_replies_is_abandoned_and_the_next_install_runs() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.app.session.plugin_state.plugins_dir = Some(h.dir.join("installed"));
        let (request, reply) = crate::control::ControlRequest::new("engine.execute", json!({"command":"plugin.install"}));
        let (send, done) = std::sync::mpsc::channel();
        let job = PackageJob::new(
            "plugin.install",
            Some("stuck-20814.zip".into()),
            PackageAction::Control { request, level: None },
            Default::default(),
            0,
            done,
        );
        let started = job.started;
        h.app.prefs_window.plugin_job = Some(job);
        assert!(busy(&h.app));

        // The refusal says what runs, on what, for how long, and that it ends by itself.
        let message = busy_message_at(&h.app, started + std::time::Duration::from_secs(42));
        for expected in ["plugin.install", "stuck-20814.zip", "42", &PACKAGE_DEADLINE.as_secs().to_string()] {
            assert!(message.contains(expected), "{expected}: {message}");
        }
        let refused = start_package(&mut h.app, "plugin.install", json!({"path":"x.zip","dry_run":true}), PackageAction::Install).unwrap_err();
        assert!(refused.contains("plugin.install") && refused.contains("stuck-20814.zip"), "{refused}");

        // `control.pending` and `control.running` list it.
        let listed = |jobs: Vec<serde_json::Value>| jobs.into_iter().any(|j| j["subject"] == "stuck-20814.zip" && j["command"] == "plugin.install");
        assert!(listed(running_jobs()));

        // Before its time it keeps running; at its time it is abandoned and reported as failed.
        tick_at(&mut h.app, &h.ctx, false, started + PACKAGE_DEADLINE - std::time::Duration::from_secs(1));
        assert!(busy(&h.app));
        assert!(reply.try_recv().is_err());
        tick_at(&mut h.app, &h.ctx, false, started + PACKAGE_DEADLINE);
        assert!(!busy(&h.app));
        assert!(!listed(running_jobs()));
        let reply = reply.try_recv().unwrap();
        assert_eq!(reply["ok"], false, "{reply}");
        let error = reply["error"].as_str().unwrap();
        assert!(
            error.contains("plugin.install") && error.contains("abandoned") && error.contains(&PACKAGE_DEADLINE.as_secs().to_string()),
            "{error}"
        );

        // A late reply has nowhere to go: nothing is applied and the next install runs.
        assert!(send.send(Ok((json!({"installed":true,"id":"late-plugin"}), h.app.session.plugin_state.clone()))).is_err());
        assert!(!h.app.session.plugin_state.packages.contains_key("late-plugin"));
        let path = package(&h, "workshop-helper");
        let zip = zip_package(&path);
        let archive = zip.to_string_lossy().into_owned();
        start_package(&mut h.app, "plugin.install", json!({"path":zip,"dry_run":true}), PackageAction::Describe { path: archive }).unwrap();
        tick(&mut h.app, &h.ctx, true);
        assert!(!busy(&h.app));
    }

    #[test]
    fn a_scoped_caller_cannot_grant_a_folder_outside_its_scope() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let (inside, outside) = (h.dir.join("inside"), h.dir.join("outside"));
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let grants = |folder: &std::path::Path| json!({"plugin_grants":{"workshop-helper":{"folders":[{"name":"work","folder":folder,"read":true,"write":false}]}}});
        let scope = || Some(solidcraft_engine::io::vfs::PathScope::new([inside.clone()]));
        let refused = solidcraft_engine::io::vfs::scoped(scope(), || crate::prefs::set(&mut h.app, &grants(&outside)));
        assert!(refused.is_err(), "a folder outside the caller's scope is refused");
        assert!(h.app.preferences.plugin_grants.is_empty());
        solidcraft_engine::io::vfs::scoped(scope(), || crate::prefs::set(&mut h.app, &grants(&inside))).unwrap();
        assert_eq!(h.app.preferences.plugin_grants.len(), 1, "a folder inside it is accepted");
    }

    #[test]
    fn plugins_remove_keeps_grant_changes_made_while_package_job_runs() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("installed");
        h.app.session.plugin_state.plugins_dir = Some(root.clone());
        let path = package(&h, "workshop-helper");
        hand_install(&path, &root, "workshop-helper");
        crate::prefs::set(&mut h.app, &json!({"plugin_grants":{"workshop-helper":{"command_groups":["file"]}}})).unwrap();
        start_package(&mut h.app, "plugin.remove", json!({"id":"workshop-helper"}), PackageAction::Remove).unwrap();
        crate::prefs::set(&mut h.app, &json!({"plugin_grants":{"workshop-helper":{"command_groups":[]}}})).unwrap();
        let before = h.app.session.plugin_state.grants.clone();
        tick(&mut h.app, &h.ctx, true);
        assert!(!root.join("workshop-helper").exists());
        assert!(!h.app.session.plugin_state.packages.contains_key("workshop-helper"));
        assert_eq!(h.app.session.plugin_state.grants, before, "removal keeps a newer live grant decision");
        assert_eq!(h.app.preferences.plugin_grants, before, "persist the live grants");
    }

    #[test]
    fn plugins_stored_grants_are_bounded_sorted_and_reject_unusable_paths() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let folder = h.dir.display().to_string();
        let grants = json!({"command_groups":["file"],"folders":[
            {"name":"zeta","folder":folder,"read":true,"write":false},
            {"name":"alpha","folder":folder,"read":true,"write":true}
        ]});
        crate::prefs::set(&mut h.app, &json!({"plugin_grants":{"test":grants}})).unwrap();
        assert_eq!(h.app.preferences.plugin_grants["test"].folders[0].name, "alpha");
        for bad in [
            json!({"command_groups":["net"],"folders":[]}),
            json!({"command_groups":[],"folders":[{"name":"parts","folder":"relative","read":true,"write":false}]}),
        ] {
            assert!(crate::prefs::set(&mut h.app, &json!({"plugin_grants":{"test":bad}})).is_err());
            assert!(crate::prefs::lenient(&json!({"plugin_grants":{"test":bad}})).plugin_grants.is_empty());
        }
        let many: serde_json::Map<String, serde_json::Value> = (0..257).map(|i| (format!("plugin-{i}"), grants.clone())).collect();
        assert!(crate::prefs::set(&mut h.app, &json!({"plugin_grants":many})).is_err());
        assert_eq!(crate::prefs::lenient(&json!({"plugin_grants":many})).plugin_grants.len(), MAX_DISABLED);
    }

    #[test]
    fn shared_approvals_move_to_the_grant_store_once_and_then_follow_it() {
        use serde_json::json;
        use solidcraft_engine::plugin::{Grants, grant_store};
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("installed");
        h.app.session.plugin_state.plugins_dir = Some(root.clone());
        let approved = |groups: &[&str]| Grants {
            package_hash: Some("b".repeat(64)),
            command_groups: groups.iter().map(|g| g.to_string()).collect(),
            ..Default::default()
        };
        // Approved here before there was a store: they move to it.
        h.app.preferences.plugin_grants.insert("gears".into(), approved(&["file"]));
        h.app.share_plugin_grants();
        assert_eq!(grant_store::read_all(&root), h.app.preferences.plugin_grants);
        assert_eq!(h.app.session.plugin_state.grants, h.app.preferences.plugin_grants);
        // The command line approves one plugin and revokes the other: the app follows.
        grant_store::write(&root, "bolts", &approved(&["doc"])).unwrap();
        grant_store::delete(&root, "gears").unwrap();
        let listed = h.app.run("plugin.grants", json!({})).unwrap();
        assert_eq!(listed["grants"][0]["id"], "bolts");
        assert_eq!(h.app.preferences.plugin_grants.keys().collect::<Vec<_>>(), ["bolts"]);
        // Revoked here: gone from the store too.
        assert_eq!(h.app.run("plugin.revoke", json!({"id":"bolts"})).unwrap()["revoked"], true);
        assert!(grant_store::read_all(&root).is_empty() && h.app.preferences.plugin_grants.is_empty());
        // Once the store exists it is the whole truth: a record left in older preferences stays gone.
        let mut restarted = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restarted.session.plugin_state.plugins_dir = Some(root.clone());
        restarted.preferences.plugin_grants.insert("gears".into(), approved(&["file"]));
        restarted.share_plugin_grants();
        assert!(restarted.preferences.plugin_grants.is_empty() && restarted.session.plugin_state.grants.is_empty());
    }

    #[test]
    fn a_shared_revocation_holds_through_a_running_install_and_a_failed_shared_write_fails_the_revoke() {
        use serde_json::json;
        use solidcraft_engine::plugin::grant_store;
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("installed");
        h.app.session.plugin_state.plugins_dir = Some(root.clone());
        let zip = zip_package(&package(&h, "workshop-helper"));
        let package = solidcraft_engine::plugin::install::unpack(&std::fs::read(&zip).unwrap()).unwrap();
        let approved = json!({"plugin_grants":{"workshop-helper":{"command_groups":["file"]}}});
        crate::prefs::set(&mut h.app, &approved).unwrap();
        h.app.share_plugin_grants();
        assert!(grant_store::read_all(&root).contains_key("workshop-helper"));
        start_package(
            &mut h.app,
            "plugin.install",
            json!({"path":zip,"approval":package.approval,"approve":{"command_groups":["file"]}}),
            PackageAction::Install,
        )
        .unwrap();
        // The command line revokes it while the install runs: the revocation holds.
        grant_store::delete(&root, "workshop-helper").unwrap();
        tick(&mut h.app, &h.ctx, true);
        assert!(root.join("workshop-helper").exists(), "installation finishes");
        assert!(!h.app.session.plugin_state.grants.contains_key("workshop-helper"));
        assert!(grant_store::read_all(&root).is_empty(), "the revoked record is not written back");
        // A shared record the app cannot delete: the revoke fails instead of reporting success.
        crate::prefs::set(&mut h.app, &approved).unwrap();
        sync_policy(&mut h.app);
        assert!(grant_store::read_all(&root).contains_key("workshop-helper"));
        let elsewhere = h.dir.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let scope = Some(solidcraft_engine::io::vfs::PathScope::new([elsewhere]));
        let refused = solidcraft_engine::io::vfs::scoped(scope, || h.app.run("plugin.revoke", json!({"id":"workshop-helper"})));
        assert!(refused.unwrap_err().to_string().contains("shared"));
        assert!(grant_store::read_all(&root).contains_key("workshop-helper"));
    }

    fn page_frame(app: &mut SolveApp, ctx: &egui::Context, dir: &Path, events: Vec<egui::Event>) {
        crate::scenario::clear_handles();
        let mut output = crate::test_support::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 800.0))),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| crate::ui_kit::scope(ui, |ui| page_in_dir(app, ui, Some(dir))),
        );
        output.textures_delta.clear();
    }

    fn click_page(app: &mut SolveApp, ctx: &egui::Context, dir: &Path, handle: &str) {
        let at = crate::scenario::handle_at(handle).unwrap_or_else(|| {
            panic!(
                "missing {handle}; review={}, remove={:?}, busy={}, available={:?}",
                app.plugin_install.ask.is_some(),
                app.prefs_window.plugin_remove,
                busy(app),
                crate::scenario::handles("")
            )
        });
        page_frame(app, ctx, dir, crate::test_support::pointer(at, true));
        page_frame(app, ctx, dir, crate::test_support::pointer(at, false));
    }

    #[test]
    fn plugins_production_composition_publishes_page_heading_and_section_geometry() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let mut h = crate::scenario::Harness::new();
            let dir = h.dir.join("page-geometry");
            let ctx = crate::fonts::test_context();
            for _ in 0..3 {
                page_frame(&mut h.app, &ctx, &dir, vec![]);
            }
            // The injected path goes through the production composition, including its
            // heading and settings_page scope; the fixture adds only the window's form style.
            let heading = crate::scenario::rect_of("prefs:page_heading").unwrap();
            let section = crate::scenario::rect_of("prefs:section:plugins_installed").unwrap();
            let body = crate::scenario::rect_of("prefs:section:plugins_installed:body").unwrap();
            let t = solidcraft_theme::Tokens::get();
            assert!(heading.height() >= t.space_24 && heading.bottom() < section.top());
            assert!((section.left() - heading.left()).abs() <= 1.0 && (section.right() - heading.right()).abs() <= 1.0);
            assert!((section.height() - (t.control_height + t.space_2)).abs() <= 1.0);
            assert!((body.top() - section.bottom() - t.space_10).abs() <= 1.0);
            assert!(!dir.exists(), "production layout does not create the plugin folder");
        }
        solidcraft_theme::set_dark(previous);
    }

    /// A new user's Plugins page leads with installation help. Resource controls remain
    /// available under Advanced, and the saved preference restores their earlier layout (#18764).
    #[test]
    fn empty_plugins_page_leads_with_install_help_and_folds_advanced_limits() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("no-plugins");
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &dir, vec![]);
        }
        let help = crate::scenario::rect_of("prefs:plugins:get_started").expect("how to install a plugin");
        let guide = crate::scenario::rect_of("prefs:plugins:guide").expect("link to plugin guide");
        let install = crate::scenario::rect_of("prefs:plugins:install").expect("Install plugin button");
        assert!(help.bottom() < guide.top(), "help comes before documentation");
        assert!(guide.bottom() < install.top(), "the page leads with how to get plugins");
        assert!(!h.app.preferences.plugin_limits_expanded);
        assert!(h.app.prefs_window.plugin_limit_inputs.is_none(), "global resource fields start folded");
        assert!(!dir.exists(), "looking at an empty page creates no plugin folder");

        click_page(&mut h.app, &ctx, &dir, "prefs:plugins:advanced_limits");
        assert!(h.app.preferences.plugin_limits_expanded);
        assert!(h.app.prefs_window.plugin_limit_inputs.is_some(), "Advanced shows the old resource fields");

        let mut reopened = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        reopened.load_prefs(&h.app.prefs());
        assert!(reopened.preferences.plugin_limits_expanded, "the old layout remains a saved preference");
        click_page(&mut h.app, &ctx, &dir, "prefs:plugins:advanced_limits");
        assert!(!h.app.preferences.plugin_limits_expanded, "the controls can be folded again");
        assert!(h.app.prefs_window.plugin_limit_inputs.is_none(), "folding clears hidden draft edits");
        capture_page(&mut h.app, &dir, "plugins-get-started.png");
    }

    #[test]
    fn plugin_card_actions_share_a_row_at_normal_width_and_stay_inside_narrow_cards() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for language in ["en", "de"] {
                let _language = solidcraft_ui_text::scope(language);
                for width in [646.0, 200.0] {
                    let mut h = crate::scenario::Harness::new();
                    let dir = h.dir.join("plugin-action-layout");
                    write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Public fixture."}"#);
                    let ctx = crate::fonts::test_context();
                    let mut clip = egui::Rect::NOTHING;
                    let mut painted = Vec::new();
                    for _ in 0..3 {
                        crate::scenario::clear_handles();
                        let mut output = crate::test_support::frame(
                            &ctx,
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width + 16.0, 800.0))),
                                ..Default::default()
                            },
                            |ui| {
                                ui.set_width(width);
                                clip = ui.clip_rect();
                                crate::ui_kit::scope(ui, |ui| page_in_dir(&mut h.app, ui, Some(&dir)));
                            },
                        );
                        output.textures_delta.clear();
                        painted = output.shapes;
                    }
                    let card = crate::scenario::rect_of("prefs:plugin:public-one:card").unwrap();
                    let folder = crate::scenario::rect_of("prefs:plugin:public-one:folder").unwrap();
                    let checkbox = crate::scenario::rect_of("prefs:plugin:public-one").unwrap();
                    let remove = crate::scenario::rect_of("prefs:plugin:public-one:remove").unwrap();
                    assert!(card.width() <= width + 1.0, "{language}, {width}: card stays within the page");
                    for action in [folder, checkbox, remove] {
                        assert!(card.expand(1.0).contains_rect(action), "{language}, {width}: action stays inside its card");
                        assert!(clip.contains_rect(action), "{language}, {width}: action is not clipped");
                    }
                    if width == 646.0 {
                        assert!((folder.center().y - checkbox.center().y).abs() <= 1.0, "{language}: normal-width actions share one row");
                        assert!(folder.right() <= checkbox.left(), "actions do not overlap");
                        assert!(card.height() > 62.0 && remove.top() > folder.bottom(), "host removal action stays in its own row inside the card");
                        assert_eq!(folder.height(), 24.0);
                        assert_eq!(checkbox.size(), egui::vec2(16.0, 16.0));
                        let t = solidcraft_theme::Tokens::get();
                        let icon = crate::scenario::rect_of("prefs:plugin:public-one:icon").unwrap();
                        assert_eq!(icon.size(), egui::vec2(36.0, 36.0));
                        assert!((icon.left() - card.left() - 13.0).abs() <= 1.0, "12 pt padding inside the border");
                        for control in [icon, folder, checkbox] {
                            assert!((control.center().y - icon.center().y).abs() <= 1.0, "original card row remains vertically centred");
                        }
                        assert!((checkbox.right() - card.right() + 13.0).abs() <= 1.0, "right padding matches left");
                        let mut quiet = true;
                        let mut semibold_title = false;
                        for shape in &painted {
                            crate::test_support::each_shape(&shape.shape, &mut |shape| match shape {
                                egui::Shape::Rect(rect) if rect.rect == folder && rect.fill != egui::Color32::TRANSPARENT => quiet = false,
                                egui::Shape::Text(text) if text.galley.job.text == "Public one" => {
                                    semibold_title = text
                                        .galley
                                        .job
                                        .sections
                                        .iter()
                                        .all(|section| section.format.font_id == crate::ui_kit::type_face::semibold(t.font_button));
                                }
                                _ => {}
                            });
                        }
                        assert!(quiet, "Show in folder has no button background");
                        assert!(semibold_title, "the title uses the actual 13 pt semibold face");
                    }
                    assert!(enabled(&h.app, "public-one"), "layout does not change plugin state");
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn a_plugin_card_turns_one_background_on_at_a_time() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("public-plugins");
        let sky = |id: &str, shaders: &str| {
            format!(r#"{{"id":"{id}","name":"{id}","version":"1","description":"","capabilities":["shaders@0.1"],"shaders":[{shaders}]}}"#)
        };
        let shader = |id: &str| format!(r#"{{"id":"{id}","name":"{id}","kind":"background","source":"{id}.wgsl"}}"#);
        write(&dir, "skies", &sky("skies", &format!("{},{}", shader("dusk"), shader("dawn"))));
        write(&dir, "ripple", &sky("ripple", &shader("ripple")));
        crate::prefs::control(&mut h.app, &serde_json::json!({"open":"Plugins"})).unwrap();
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &dir, vec![]);
        }
        click_page(&mut h.app, &ctx, &dir, "prefs:background:skies/dusk");
        assert_eq!(h.app.preferences.background_shader, "skies/dusk");
        click_page(&mut h.app, &ctx, &dir, "prefs:background:ripple/ripple");
        assert_eq!(h.app.preferences.background_shader, "ripple/ripple", "turning one on turns the other off");
        click_page(&mut h.app, &ctx, &dir, "prefs:background:ripple/ripple");
        assert_eq!(h.app.preferences.background_shader, "", "and off again");
        click_page(&mut h.app, &ctx, &dir, "prefs:background:skies/dawn");
        click_page(&mut h.app, &ctx, &dir, "prefs:plugin:skies");
        assert!(!enabled(&h.app, "skies"));
        click_page(&mut h.app, &ctx, &dir, "prefs:background:skies/dusk");
        assert_eq!(h.app.preferences.background_shader, "skies/dawn", "a turned-off plugin's switches do nothing");
        crate::prefs::control(&mut h.app, &serde_json::json!({"button":"defaults"})).unwrap();
        assert_eq!(h.app.preferences.background_shader, "", "Restore Defaults on Plugins turns it off");
    }

    #[test]
    fn plugin_cards_open_their_own_folder_and_cancel_restores_enabled_state() {
        use std::{cell::RefCell, rc::Rc};
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let mut h = crate::scenario::Harness::new();
            let dir = h.dir.join("public-plugins");
            write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Public fixture."}"#);
            write(&dir, "public-two", r#"{"id":"public-two","name":"Public two","version":"2","description":"Another fixture."}"#);
            let opened = Rc::new(RefCell::new(Vec::new()));
            let seen = opened.clone();
            h.app.services.show_in_folder = Some(Box::new(move |path| seen.borrow_mut().push(PathBuf::from(path))));
            crate::prefs::control(&mut h.app, &serde_json::json!({"open":"Plugins"})).unwrap();
            let ctx = crate::fonts::test_context();
            for _ in 0..3 {
                page_frame(&mut h.app, &ctx, &dir, vec![]);
            }
            assert!(opened.borrow().is_empty(), "rendering never opens a folder");
            click_page(&mut h.app, &ctx, &dir, "prefs:plugin:public-two:folder");
            assert_eq!(*opened.borrow(), [dir.join("public-two/plugin.json")]);
            click_page(&mut h.app, &ctx, &dir, "prefs:plugin:public-one");
            assert!(!enabled(&h.app, "public-one") && enabled(&h.app, "public-two"));
            crate::prefs::control(&mut h.app, &serde_json::json!({"button":"defaults"})).unwrap();
            assert!(enabled(&h.app, "public-one"));
            page_frame(&mut h.app, &ctx, &dir, vec![]);
            click_page(&mut h.app, &ctx, &dir, "prefs:plugin:public-one");
            crate::prefs::control(&mut h.app, &serde_json::json!({"button":"cancel"})).unwrap();
            assert!(enabled(&h.app, "public-one") && enabled(&h.app, "public-two"));
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn show_plugins_folder_creates_only_the_requested_fixture_root() {
        use std::{cell::RefCell, rc::Rc};
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("not-created-yet");
        let opened = Rc::new(RefCell::new(Vec::new()));
        let seen = opened.clone();
        h.app.services.show_in_folder = Some(Box::new(move |path| seen.borrow_mut().push(PathBuf::from(path))));
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &dir, vec![]);
        }
        assert!(!dir.exists() && opened.borrow().is_empty());
        click_page(&mut h.app, &ctx, &dir, "prefs:plugins:folder");
        assert!(dir.is_dir());
        assert_eq!(*opened.borrow(), [dir.join("plugin.json")]);
        assert!(!dir.join("plugin.json").exists(), "the reveal service needs only a sentinel path");
    }

    #[test]
    fn plugin_metadata_search_owns_one_navigation_target_and_reveals_the_real_card() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("public-plugin-search");
        write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Unique description."}"#);
        let ctx = crate::fonts::test_context();
        for query in ["Public one", "public-one", "1.2.3", "Unique description"] {
            for _ in 0..3 {
                crate::scenario::clear_handles();
                let mut output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                    ui.set_width(646.0);
                    let _search = crate::ui_kit::settings_search::begin(ui, query, "prefs");
                    crate::ui_kit::settings_search::begin_page(ui, crate::prefs::PAGE_PLUGINS, "Extend > Plugins");
                    crate::ui_kit::scope(ui, |ui| page_in_dir(&mut h.app, ui, Some(&dir)));
                    crate::ui_kit::settings_search::store_nav(ui);
                });
                output.textures_delta.clear();
            }
            let (_, _, hits) = crate::ui_kit::settings_search::nav(&ctx);
            assert_eq!(hits.len(), 1, "{query}: one real plugin result");
            assert_eq!(hits[0].page, crate::prefs::PAGE_PLUGINS);
            crate::ui_kit::settings_search::set_reveal(&ctx, Some(crate::ui_kit::settings_search::reveal_for(hits[0].page, &hits[0].label, "prefs")));
            page_frame(&mut h.app, &ctx, &dir, vec![]);
            let card = crate::scenario::rect_of("prefs:plugin:public-one:card").unwrap();
            assert!(crate::scenario::handle_at("prefs:revealed").unwrap().distance(card.center()) <= 1.0);
            assert!(enabled(&h.app, "public-one"), "searching and revealing do not toggle the plugin");
        }
    }

    fn write(dir: &Path, folder: &str, body: &str) {
        let d = dir.join(folder);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.json"), body).unwrap();
    }

    #[test]
    fn a_folder_with_plugin_json_is_listed_and_a_bad_one_is_skipped() {
        let dir = std::env::temp_dir().join(format!("solidcraft-plugins-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir,
            "ftc-frc-parts",
            r#"{"id":"ftc-frc-parts","name":"FTC and FRC parts","version":"0.1.0","description":"Commercial robot parts."}"#,
        );
        write(&dir, "broken", r#"{"id":"other","name":"Nope","version":"1"}"#);
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        let (plugins, skipped) = inventory(Some(&dir), std::iter::empty());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "ftc-frc-parts");
        assert_eq!(plugins[0].name, "FTC and FRC parts");
        assert!(skipped.iter().any(|s| s.folder == "broken" && s.reason.contains("does not match")));
        assert!(skipped.iter().any(|s| s.folder == "notes" && s.reason.contains("no plugin.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plugin_inventory_prefers_inspected_ids_and_sorts_by_name_then_id() {
        let h = crate::scenario::Harness::new();
        let installed = h.dir.join("installed");
        let inspected = h.dir.join("inspected");
        for (root, id, name) in [
            (&installed, "alpha", "Old name"),
            (&installed, "beta", "Helper"),
            (&installed, "zeta", "Another helper"),
            (&inspected, "alpha", "Helper"),
        ] {
            write(root, id, &serde_json::json!({"id":id,"name":name,"version":"1"}).to_string());
        }
        let path = inspected.join("alpha").to_string_lossy().into_owned();
        let (plugins, skipped) = inventory(Some(&installed), [&path, &path].into_iter());
        assert!(skipped.is_empty());
        assert_eq!(plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["zeta", "alpha", "beta"]);
        assert_eq!(plugins[1].path, path, "the first inspected package wins over the installed copy");
        let (plugins, _) = inventory(None, [&path].into_iter());
        assert_eq!(plugins.len(), 1, "inspection works without an installed directory");
    }

    #[test]
    fn plugin_folder_cap_counts_only_visible_directories() {
        let h = crate::scenario::Harness::new();
        let dir = h.dir.join("installed");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..1025 {
            std::fs::write(dir.join(format!("note-{i}")), []).unwrap();
            std::fs::create_dir(dir.join(format!(".hidden-{i}"))).unwrap();
            let id = format!("helper-{i}");
            write(&dir, &id, &serde_json::json!({"id":id,"name":"Helper","version":"1"}).to_string());
        }
        let (plugins, skipped) = inventory(Some(&dir), std::iter::empty());
        assert_eq!(plugins.len(), 1024, "files and hidden folders do not consume the directory cap");
        assert!(skipped.is_empty(), "hidden folders are not reported as invalid plugins");
        assert!(plugins.windows(2).all(|pair| pair[0].id < pair[1].id));
    }

    #[test]
    fn plugin_inventory_keeps_executable_entries_when_an_inspected_copy_has_only_metadata() {
        let h = crate::scenario::Harness::new();
        let installed = h.dir.join("installed");
        let executable = package(&h, "workshop-helper");
        hand_install(&executable, &installed, "workshop-helper");
        let inspected = h.dir.join("inspected");
        write(&inspected, "workshop-helper", r#"{"id":"workshop-helper","name":"Metadata","version":"1"}"#);
        let path = inspected.join("workshop-helper").to_string_lossy().into_owned();
        let (plugins, _) = inventory(Some(&installed), [&path].into_iter());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].path, installed.join("workshop-helper").to_string_lossy());
        assert_eq!(plugins[0].entries[0].name, "inspect", "metadata-only inspection does not hide main's executable menu entries");
        let (plugins, _) = inventory(None, [&path].into_iter());
        assert_eq!(plugins[0].path, path, "Preferences still lists metadata-only inspected packages");
    }

    /// #21228: listing the installed plugins, about once a second, reads and hashes a component
    /// only when it changed. Same-size bytes written back with the old modification time are not
    /// read again (the approval stays), and a component of another size is hashed again.
    #[test]
    fn listing_plugins_hashes_a_component_only_when_it_changed() {
        let h = crate::scenario::Harness::new();
        let installed = h.dir.join("installed");
        hand_install(&package(&h, "workshop-helper"), &installed, "workshop-helper");
        let approval = || inventory(Some(&installed), std::iter::empty()).0[0].approval.clone();
        let first = approval();
        let component = installed.join("workshop-helper").join("plugin.wasm");
        let modified = std::fs::metadata(&component).unwrap().modified().unwrap();
        std::fs::write(&component, b"\0asm\x0d\0\x01\x01").unwrap();
        std::fs::File::options().write(true).open(&component).unwrap().set_modified(modified).unwrap();
        assert_eq!(approval(), first, "an unchanged size and time is not read again");
        std::fs::write(&component, b"\0asm\x0d\0\x01\0\0").unwrap();
        assert_ne!(approval(), first, "a changed component is hashed again");
    }

    #[test]
    fn ids_and_manifests_refuse_what_would_not_fit_on_the_page() {
        assert!(valid_id("a"));
        assert!(valid_id("ftc-frc-parts"));
        assert!(!valid_id(""));
        assert!(!valid_id("-a"));
        assert!(!valid_id("a-"));
        assert!(!valid_id("FTC"));
        assert!(!valid_id(&"a".repeat(65)));
        let bad = Manifest { id: "gear".into(), name: "Gear\nbox".into(), version: "1".into(), description: String::new() };
        assert!(check(bad, "gear").is_err());
        let big = Manifest { id: "gear".into(), name: "Gear".into(), version: "1".into(), description: "x".repeat(241) };
        assert!(check(big, "gear").unwrap_err().contains("240"));
    }

    #[test]
    fn turning_a_plugin_off_is_saved_and_restore_defaults_turns_it_back_on() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert!(enabled(&app, "ftc-frc-parts"));
        crate::prefs::set(&mut app, &json!({"disabled_plugins": ["ftc-frc-parts"]})).unwrap();
        assert!(!enabled(&app, "ftc-frc-parts"));
        let err = crate::prefs::set(&mut app, &json!({"disabled_plugins": ["Bad Id"]})).unwrap_err();
        assert!(err.contains("disabled_plugins"), "{err}");
        assert!(!enabled(&app, "ftc-frc-parts"), "a refused list leaves the previous choice");
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&app.prefs());
        assert!(!enabled(&other, "ftc-frc-parts"));
        crate::prefs::control(&mut app, &json!({"open": "Plugins", "button": "defaults"})).unwrap();
        assert_eq!(app.prefs_window.shell.section, crate::prefs::PAGE_PLUGINS);
        assert!(enabled(&app, "ftc-frc-parts"));
        assert!(crate::prefs::control(&mut app, &json!({"open": "NoSuch"})).is_err());
    }

    #[test]
    fn resource_ceilings_preserve_defaults_precision_persistence_and_page_transactions() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let initial = host_limits(&h.app.preferences);
        assert_eq!(initial, solidcraft_engine::plugin::HostLimits::default());
        let before = h.app.session.doc.to_json();
        let changed = json!({"plugin_max_fuel":u64::MAX,"plugin_max_memory_mib":512});
        let reply = h.call("ui.prefs", changed.clone());
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(h.app.preferences.plugin_max_fuel, u64::MAX);
        assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
        for bad in [
            json!({"plugin_max_fuel":0}),
            json!({"plugin_max_fuel":-1}),
            json!({"plugin_max_fuel":1.5}),
            json!({"plugin_max_memory_mib":4097}),
            json!({"plugin_max_memory_mib":0}),
        ] {
            assert_eq!(h.call("ui.prefs", bad)["ok"], false);
            assert_eq!(h.app.preferences.plugin_max_fuel, u64::MAX);
        }
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(host_limits(&restored.preferences), host_limits(&h.app.preferences));
        assert_eq!(restored.session.plugin_limits.max_fuel, u64::MAX);
        h.call("ui.prefs", json!({"open":"Plugins"}));
        h.call("ui.prefs", json!({"plugin_max_memory_mib":1024}));
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.plugin_max_memory_mib, 512);
        h.call("ui.prefs", json!({"open":"Plugins","button":"defaults"}));
        assert_eq!(host_limits(&h.app.preferences), initial);
        if let Some(input) = &h.app.prefs_window.plugin_limit_inputs {
            assert_eq!(input.applied, initial);
        }
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.plugin_max_memory_mib, 512);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
        let read = crate::prefs::lenient(&json!({"plugin_max_fuel":0,"plugin_max_memory_mib":99999,"language":"en"}));
        assert_eq!(host_limits(&read), initial);
        assert_eq!(read.language, "en");
    }
    #[test]
    fn resource_text_edits_require_complete_valid_confirmation() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Plugins"}));
        let initial = host_limits(&h.app.preferences);
        h.app.prefs_window.plugin_limit_inputs = Some(LimitInputs { applied: initial, fuel: u64::MAX.to_string(), memory: "512".into() });
        assert!(resource_pending(&h.app));
        assert_eq!(h.app.session.plugin_limits, initial, "typing alone does not authorize a higher ceiling");
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["apply_enabled"], true);
        assert_eq!(h.call("ui.prefs", json!({"button":"apply"}))["ok"], true);
        assert_eq!(h.app.session.plugin_limits.max_fuel, u64::MAX);
        assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
        let applied = h.app.session.plugin_limits;
        for text in ["", "0", "18446744073709551616", "1.5"] {
            h.app.prefs_window.plugin_limit_inputs = Some(LimitInputs { applied, fuel: text.into(), memory: "1024".into() });
            assert_eq!(h.call("ui.prefs", json!({"button":"ok"}))["ok"], false);
            assert!(h.app.prefs_window.shell.open);
            assert_eq!(h.app.session.plugin_limits, applied);
        }
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.session.plugin_limits, applied, "Cancel retains the last Apply");
        assert!(h.app.prefs_window.plugin_limit_inputs.is_none());
        h.call("ui.prefs", json!({"open":"Plugins","button":"defaults"}));
        assert_eq!(h.app.session.plugin_limits, initial);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.session.plugin_limits, applied);
    }
    #[test]
    fn resource_drafts_survive_preferences_navigation() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Plugins"}));
        let initial = host_limits(&h.app.preferences);
        h.app.prefs_window.plugin_limit_inputs = Some(LimitInputs { applied: initial, fuel: "90000000".into(), memory: "512".into() });
        assert_eq!(h.call("ui.prefs", json!({"open":"General"}))["ok"], true);
        let draft = h.app.prefs_window.plugin_limit_inputs.as_ref().unwrap();
        assert_eq!((&draft.fuel, &draft.memory), (&"90000000".to_string(), &"512".to_string()));
        assert_eq!(h.call("ui.prefs", json!({"button":"apply"}))["ok"], true);
        assert_eq!(h.app.session.plugin_limits.max_fuel, 90000000);
        assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
    }

    #[test]
    fn search_matches_the_name_the_id_and_the_description() {
        let plugin = Plugin {
            path: String::new(),
            approval: String::new(),
            disabled: false,
            groups: Default::default(),
            entries: Vec::new(),
            images: Default::default(),
            swatches: Default::default(),
            shaders: Vec::new(),
            id: "ftc-frc-parts".into(),
            name: "FTC and FRC parts".into(),
            version: "0.1.0".into(),
            description: "Commercial robot parts.".into(),
        };
        assert!(matches_query(&plugin, ""));
        assert!(matches_query(&plugin, " frc "));
        assert!(matches_query(&plugin, "FTC-FRC"));
        assert!(matches_query(&plugin, "commercial"));
        assert!(matches_query(&plugin, "0.1"));
        assert!(!matches_query(&plugin, "bearing"));
    }
}
