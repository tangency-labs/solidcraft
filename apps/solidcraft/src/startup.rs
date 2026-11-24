//! Getting the window open, whatever state the settings folder or the graphics device is in
//! (docs/book/src/guide/troubleshooting.md).
//!
//! - **Settings that don't read** (`app.ron`, `graphics.json`): moved aside as
//!   `<name>.bad-<time>` and the app starts with defaults, instead of a newer or damaged file
//!   being dropped silently or stopping the start.
//! - **Launches that die before the window is up**: a marker file counts them; after two in a
//!   row the next launch starts in safe mode by itself.
//! - **Safe mode** (`--safe-mode` or `SOLIDCRAFT_SAFE_MODE=1`): default settings (the saved ones
//!   are left untouched), no recovery offer, the software renderer and CPU meshing.
//! - **Safe graphics mode** (`SafeGraphics`): after a launch that failed to come up or lost its
//!   graphics device, or when Preferences asks for it, the next launch draws the plain way v0.1.0
//!   did on the same card: wgpu's own backend choice, no anti-aliasing, no optional effects.
//! - **A renderer that cannot start**: on Windows, try the remaining graphics APIs in order,
//!   then the software renderer (`SOLIDCRAFT_RENDERER=cpu`). An explicit `WGPU_BACKEND` fails
//!   cleanly without switching. Exhausted retries show a message box with the error.

use std::path::{Path, PathBuf};

/// The environment variable that starts safe mode (any value but `0` or empty).
pub const SAFE_MODE_ENV: &str = "SOLIDCRAFT_SAFE_MODE";
/// `SOLIDCRAFT_RENDERER=cpu` picks the software renderer (WARP, lavapipe, llvmpipe).
pub const RENDERER_ENV: &str = "SOLIDCRAFT_RENDERER";
/// Consecutive launches that died before the window was up, after which safe mode starts.
pub const FAILED_STARTS_FOR_SAFE_MODE: u32 = 2;
const MARKER: &str = "starting";

/// Explicit application-settings isolation for side-by-side builds and native UI
/// acceptance. A supplied invalid path refuses startup rather than falling back
/// to the user's ordinary settings. No directory is read or created here.
pub fn settings_directory(override_path: Option<&std::ffi::OsStr>, default: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    match override_path {
        None => Ok(default),
        Some(raw) => {
            let path = PathBuf::from(raw);
            if raw.is_empty() || !path.is_absolute() {
                return Err("SOLIDCRAFT_SETTINGS_DIR must be a nonempty absolute directory path".into());
            }
            if path.exists() && !path.is_dir() {
                return Err("SOLIDCRAFT_SETTINGS_DIR names a file, not a directory".into());
            }
            Ok(Some(path))
        }
    }
}

/// Whether an environment value switches something on.
pub fn env_on(v: Option<&str>) -> bool {
    v.map(str::trim).is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// Whether `SOLIDCRAFT_RENDERER` asks for the software renderer.
pub fn cpu_renderer_env(v: Option<&str>) -> bool {
    v.map(str::trim).is_some_and(|v| v.eq_ignore_ascii_case("cpu") || v.eq_ignore_ascii_case("software"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Move `path` aside as `<name>.bad-<time>` (kept for a bug report); returns where it went.
pub fn back_up(path: &Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "settings".into());
    let mut to = path.with_file_name(format!("{name}.bad-{}", unix_now()));
    let mut n = 1;
    while to.exists() && n < 100 {
        to = path.with_file_name(format!("{name}.bad-{}-{n}", unix_now()));
        n += 1;
    }
    std::fs::rename(path, &to)?;
    Ok(to)
}

/// Whether eframe's `app.ron` reads, with the preferences inside it.
fn app_ron_reads(text: &str, prefs_keys: &[&str]) -> bool {
    let Ok(kv) = ron::from_str::<std::collections::HashMap<String, String>>(text) else { return false };
    prefs_keys.iter().filter_map(|k| kv.get(*k)).all(|p| serde_json::from_str::<serde_json::Value>(p).is_ok_and(|v| v.is_object()))
}

/// Check the settings files in `dir` before anything reads them: one that doesn't read is moved
/// aside (the app then starts with defaults for it). Returns a line per file moved.
pub fn check_settings(dir: &Path, prefs_keys: &[&str]) -> Vec<String> {
    let mut notes = Vec::new();
    let mut check = |name: &str, reads: &dyn Fn(&str) -> bool| {
        let path = dir.join(name);
        let Ok(bytes) = std::fs::read(&path) else { return };
        let ok = std::str::from_utf8(&bytes).is_ok_and(|t| t.trim().is_empty() || reads(t));
        if ok {
            return;
        }
        match back_up(&path) {
            Ok(to) => {
                log::warn!("settings: {} did not read; moved to {}", path.display(), to.display());
                notes.push(format!("{name} could not be read and was moved to {}; defaults are used.", to.display()));
            }
            Err(e) => log::warn!("settings: {} did not read and could not be moved: {e}", path.display()),
        }
    };
    check("app.ron", &|t| app_ron_reads(t, prefs_keys));
    check("graphics.json", &|t| serde_json::from_str::<serde_json::Value>(t).is_ok_and(|v| v.is_object()));
    notes
}

/// A launch begins: count it in the marker in `dir`. Returns how many launches before this one
/// died before their window was up.
pub fn begin(dir: &Path) -> u32 {
    let marker = dir.join(MARKER);
    let before = std::fs::read_to_string(&marker).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0).min(1000);
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(&marker, (before + 1).to_string());
    }
    before
}

/// The window is up: this launch did not fail to start.
pub fn started(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(MARKER));
}

/// The window closed while the app was still starting (behind its starting page, #1210). A close
/// the user asked for is not a failed start: the marker goes, so closing a couple of launches early
/// cannot put the next one in safe mode. A close the app made itself (its graphics device was lost
/// and it starts again) keeps the marker: that start did fail.
pub fn closed_while_starting(dir: &Path, by_user: bool) {
    if by_user {
        started(dir);
    }
}

/// A saved backend change that this launch is trying. Keep this value until the first
/// frame is presented, so only this launch's matching attempt can be confirmed or reverted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendAttempt {
    pub requested: String,
    pub previous: String,
}

/// The backend to use before creating any graphics device, with any pending recovery notice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendLaunch {
    pub backend: Option<String>,
    pub attempt: Option<BackendAttempt>,
    pub note: Option<String>,
}

type GraphicsSettings = serde_json::Map<String, serde_json::Value>;
const BACKEND_TRIAL: &str = "backend_trial";
const MAX_BACKEND_PREFERENCE_BYTES: usize = 256;

/// Read only object-shaped settings. A missing or empty file means defaults; other failures
/// are reported so the caller can use its safe renderer without overwriting unreadable data.
fn read_backend_settings(path: &Path) -> Result<GraphicsSettings, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(GraphicsSettings::new()),
        Err(error) => return Err(format!("could not read graphics settings: {error}")),
    };
    if text.trim().is_empty() {
        return Ok(GraphicsSettings::new());
    }
    serde_json::from_str(&text).map_err(|error| format!("could not read graphics settings: {error}"))
}

/// The attempt marker and backend are written together: a crash must leave a whole old or
/// new record, never a selected API whose recovery data was only partly written.
fn write_backend_settings(path: &Path, graphics: &GraphicsSettings) -> Result<(), String> {
    let bytes = serde_json::to_vec(graphics).map_err(|error| format!("could not encode graphics settings: {error}"))?;
    solidcraft_engine::io::safe_write::write_atomic(path, &bytes, false).map_err(|error| format!("could not save graphics settings: {error}"))
}

/// UI choices are single API names. The previous value remains verbatim, including legacy
/// comma lists and blanks, because startup already knows how to interpret those preferences.
fn backend_trial(graphics: &GraphicsSettings) -> Option<(BackendAttempt, bool)> {
    let trial = graphics.get(BACKEND_TRIAL)?.as_object()?;
    let requested = trial.get("requested")?.as_str()?;
    if !matches!(requested, "auto" | "dx12" | "vulkan" | "gl" | "metal") {
        return None;
    }
    let previous = trial.get("previous")?.as_str()?;
    if previous.len() > MAX_BACKEND_PREFERENCE_BYTES {
        return None;
    }
    let started = trial.get("started")?.as_bool()?;
    Some((BackendAttempt { requested: requested.into(), previous: previous.into() }, started))
}

/// Describe a restored choice using the same catalog as the Graphics preferences page.
pub(super) fn backend_reverted_note(requested: &str, previous: &str) -> String {
    let requested = solidcraft_ui_egui::prefs::graphics_backend_label(requested);
    let previous = solidcraft_ui_egui::prefs::graphics_backend_label(previous);
    solidcraft_ui_text::format("prefs.graphics_api_start_reverted", &[("requested", &requested), ("previous", &previous)])
}

/// Prepare a UI-selected API before any adapter is opened. Safe mode, an explicit environment
/// override and process-local fallback launches pass `false` and leave the attempt untouched.
/// An interrupted attempt restores its previous API. A new attempt is returned only after its
/// started marker reaches disk; any I/O error must make the caller avoid the requested API.
pub fn prepare_backend_start(path: &Path, allow_trial: bool) -> Result<BackendLaunch, String> {
    prepare_backend_launch(read_backend_settings(path)?, allow_trial, |graphics| write_backend_settings(path, graphics))
}

/// Separate the state transition from persistence so a refused marker write is testable
/// without depending on the test user's filesystem permissions.
fn prepare_backend_launch(
    mut graphics: GraphicsSettings,
    allow_trial: bool,
    save: impl FnOnce(&GraphicsSettings) -> Result<(), String>,
) -> Result<BackendLaunch, String> {
    let backend = graphics.get("backend").and_then(serde_json::Value::as_str).map(str::to_string);
    let mut launch = BackendLaunch { backend, ..Default::default() };
    if !allow_trial || !graphics.contains_key(BACKEND_TRIAL) {
        return Ok(launch);
    }
    let parsed = backend_trial(&graphics);
    let requested = graphics.get(BACKEND_TRIAL).and_then(|trial| trial.get("requested")).and_then(serde_json::Value::as_str);
    if requested.is_some() && requested != launch.backend.as_deref() {
        // Another edit replaced the selected API; stale recovery data cannot undo that edit.
        graphics.remove(BACKEND_TRIAL);
    } else if let Some((attempt, started)) = parsed {
        if attempt.requested == attempt.previous {
            graphics.remove(BACKEND_TRIAL);
        } else if started {
            graphics.insert("backend".into(), serde_json::Value::String(attempt.previous.clone()));
            graphics.remove(BACKEND_TRIAL);
            launch.backend = Some(attempt.previous.clone());
            launch.note = Some(backend_reverted_note(&attempt.requested, &attempt.previous));
        } else {
            let trial = graphics
                .get_mut(BACKEND_TRIAL)
                .and_then(serde_json::Value::as_object_mut)
                .ok_or_else(|| "graphics backend attempt changed before it could start".to_string())?;
            trial.insert("started".into(), serde_json::Value::Bool(true));
            launch.attempt = Some(attempt);
        }
    } else {
        // A damaged guard must not turn a protected API into an unguarded attempt. Prefer its
        // intact previous value; otherwise use Automatic, preserving the other graphics keys.
        let previous = graphics
            .get(BACKEND_TRIAL)
            .and_then(|trial| trial.get("previous"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| value.len() <= MAX_BACKEND_PREFERENCE_BYTES)
            .unwrap_or("auto")
            .to_string();
        launch.note = Some(backend_reverted_note(launch.backend.as_deref().unwrap_or("auto"), &previous));
        graphics.insert("backend".into(), serde_json::Value::String(previous.clone()));
        graphics.remove(BACKEND_TRIAL);
        launch.backend = Some(previous);
    }
    save(&graphics)?;
    Ok(launch)
}

/// Only a matching, started attempt still owning the saved backend may change recovery state.
/// A newer preference edit is left alone, including a newly pending attempt at the same API.
fn finish_backend_start(path: &Path, attempt: &BackendAttempt, revert: bool) -> Result<(), String> {
    let mut graphics = read_backend_settings(path)?;
    let matches = backend_trial(&graphics).is_some_and(|(saved, started)| started && saved == *attempt)
        && graphics.get("backend").and_then(serde_json::Value::as_str) == Some(attempt.requested.as_str());
    if !matches {
        return Ok(());
    }
    graphics.remove(BACKEND_TRIAL);
    if revert {
        graphics.insert("backend".into(), serde_json::Value::String(attempt.previous.clone()));
    }
    write_backend_settings(path, &graphics)
}

/// Keep a backend only after the window has presented its first frame. Handled startup
/// failures must call `rollback_backend_start` instead, even if the ordinary crash marker clears.
pub fn confirm_backend_start(path: &Path, attempt: &BackendAttempt) -> Result<(), String> {
    finish_backend_start(path, attempt, false)
}

/// A handled window-start failure restores the previous backend before the host retries.
/// Reading fresh settings and matching the attempt avoids erasing a later preference edit.
pub fn rollback_backend_start(path: &Path, attempt: &BackendAttempt) -> Result<(), String> {
    finish_backend_start(path, attempt, true)
}

/// `graphics.json`'s record of how full-graphics launches went (safe graphics mode).
pub const SAFE_GRAPHICS: &str = "safe_graphics";
/// Full-graphics launches in a row that failed, after which safe graphics mode stays on until
/// the user asks for full graphics again.
pub const FAILED_FULL_LAUNCHES_FOR_STICKY: u32 = 2;
/// How long a full-graphics launch runs after "app up" without losing its device before the
/// failures before it are forgotten.
pub const FULL_GRAPHICS_PROOF: std::time::Duration = std::time::Duration::from_secs(60);

/// Safe graphics mode: what the next launches do after a launch that failed to come up or lost
/// its graphics device (`graphics.json`'s `safe_graphics`). Safe graphics is the plain path
/// v0.1.0 had: wgpu's own backend choice with no API order or retries, no anti-aliasing,
/// 24-bit depth, CPU meshing, and no ambient occlusion, blur, studio shading or path tracer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SafeGraphics {
    /// Full-graphics launches in a row that failed. Cleared by a full launch that runs
    /// [`FULL_GRAPHICS_PROOF`] after "app up" without losing its device.
    pub failures: u32,
    /// A full-graphics launch is under way and has neither proved itself nor closed normally.
    pub trial: bool,
    /// [`FAILED_FULL_LAUNCHES_FOR_STICKY`] failures: safe until the user asks for full graphics.
    pub sticky: bool,
    /// The user asked for safe graphics for the next launch only (Preferences › Graphics).
    pub next: bool,
    /// The last launch ran in safe graphics mode: its failure is not a full-graphics one.
    pub last_safe: bool,
    /// The graphics API the last full-graphics launch started (`vulkan`, `dx12`, `gl`, `metal`),
    /// when it started one alone: the safe launch after its failure avoids it ([`safe_backends`]).
    pub backend: Option<String>,
}

/// Why a launch is in safe graphics mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeGraphicsReason {
    /// Preferences › Graphics › Start in safe graphics mode (this launch only), or `--safe-mode`.
    Asked,
    /// The last launch failed to come up or lost its graphics device.
    Failed,
    /// Two full-graphics launches in a row failed.
    Sticky,
}

impl SafeGraphics {
    /// The record in `graphics`; anything missing or of the wrong type reads as its default.
    pub fn read(graphics: &GraphicsSettings) -> Self {
        let Some(record) = graphics.get(SAFE_GRAPHICS).and_then(serde_json::Value::as_object) else { return Self::default() };
        let flag = |key: &str| record.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
        let failures = record.get("failures").and_then(serde_json::Value::as_u64).map_or(0, |n| n.min(1000) as u32);
        let backend = record
            .get("backend")
            .and_then(serde_json::Value::as_str)
            .filter(|b| matches!(*b, "vulkan" | "dx12" | "gl" | "metal"))
            .map(str::to_string);
        Self { failures, trial: flag("trial"), sticky: flag("sticky"), next: flag("next"), last_safe: flag("last_safe"), backend }
    }

    /// Put the record in `graphics`, leaving every other setting as it was.
    pub fn write(&self, graphics: &mut GraphicsSettings) {
        graphics.insert(
            SAFE_GRAPHICS.into(),
            serde_json::json!({
                "failures": self.failures,
                "trial": self.trial,
                "sticky": self.sticky,
                "next": self.next,
                "last_safe": self.last_safe,
                "backend": self.backend,
            }),
        );
    }

    /// A launch begins. `previous_failed`: the last launch did not reach "app up" (the start
    /// marker) or closed because its graphics device was lost. `tracked`: this launch's outcome
    /// counts (not one driven over the control channel, which tools end at will). `forced`:
    /// safe mode was asked for on the command line or by the environment.
    ///
    /// Returns why this launch is in safe graphics mode, or `None` for full graphics; the
    /// record is updated for the launch (save it before any graphics device is created).
    pub fn begin(&mut self, previous_failed: bool, tracked: bool, forced: bool) -> Option<SafeGraphicsReason> {
        let failed = tracked && (self.trial || previous_failed);
        if failed && !self.last_safe {
            self.failures = self.failures.saturating_add(1).min(1000);
        }
        if self.failures >= FAILED_FULL_LAUNCHES_FOR_STICKY && failed {
            self.sticky = true;
        }
        let reason = if self.sticky {
            Some(SafeGraphicsReason::Sticky)
        } else if failed {
            Some(SafeGraphicsReason::Failed)
        } else if self.next || forced {
            Some(SafeGraphicsReason::Asked)
        } else {
            None
        };
        // Asked for once: the launch after this one is full graphics again.
        self.next = false;
        self.last_safe = reason.is_some();
        self.trial = reason.is_none() && tracked;
        reason
    }

    /// A full-graphics launch ran [`FULL_GRAPHICS_PROOF`] after "app up" without losing its
    /// device: the failures before it are forgotten.
    pub fn proved(&mut self) {
        self.failures = 0;
        self.trial = false;
    }

    /// A full-graphics launch closed normally before proving itself: not a failure, but the
    /// failures before it are kept.
    pub fn closed_normally(&mut self) {
        self.trial = false;
    }

    /// A full-graphics launch lost its graphics device: whenever the next launch comes, it
    /// counts this one as failed, even after the proof.
    pub fn lost_device(&mut self) {
        self.trial = true;
    }
}

/// The record's name for `backends` when they are one API alone (what a full launch starts).
pub fn backend_name(backends: eframe::wgpu::Backends) -> Option<&'static str> {
    use eframe::wgpu::Backends;
    [(Backends::VULKAN, "vulkan"), (Backends::DX12, "dx12"), (Backends::GL, "gl"), (Backends::METAL, "metal")]
        .into_iter()
        .find(|(b, _)| *b == backends)
        .map(|(_, name)| name)
}

/// The backends a safe graphics launch starts: wgpu's own (`default`), less the API the failed
/// full launch used (`failed`, `None` when it was asked for rather than after a failure), so a
/// driver that crashed is not loaded again. After a Vulkan failure on Windows that leaves D3D12,
/// then OpenGL (then the software renderer, D3D12's WARP, if the window still can't open);
/// after a D3D12 failure, Vulkan, then OpenGL. The APIs that crashed the process while starting
/// (`crashed`, [`BackendGuard`]) are left out as well, so their drivers are never loaded. Never
/// empty: an API that is all there is stays.
pub fn safe_backends(default: eframe::wgpu::Backends, failed: Option<&str>, crashed: eframe::wgpu::Backends) -> eframe::wgpu::Backends {
    let avoid = failed.map(eframe::wgpu::Backends::from_comma_list).unwrap_or(eframe::wgpu::Backends::empty());
    let rest = default - avoid - crashed;
    if rest.intersects(native_backends()) { rest } else { without_crashed(default, crashed) }
}

/// The crash guard (#18683): a graphics driver that crashes the process while its API is started
/// leaves this file behind, naming the API, so the next launch knows which one it was and never
/// loads it again on this machine. Written (and flushed) before a backend is started; removed
/// once the window presented its first frame, or once startup returned without crashing.
pub const GRAPHICS_ATTEMPT: &str = "graphics-attempt.json";
/// `graphics.json`'s record of the APIs that crashed the process while starting, each with the
/// app version it crashed on. Only the current version's entries count: a new version (which may
/// come with a new driver) tries each API once more.
pub const CRASHED_BACKENDS: &str = "crashed_backends";
/// `graphics.json`'s record of the APIs that could not open the window ([`failed_backends`]).
pub const FAILED_BACKENDS: &str = "failed_backends";
/// The order wgpu brings its backends up in; with several APIs in one attempt, the first one is
/// blamed, so repeated crashes take one API away per launch.
const GUARD_ORDER: [(eframe::wgpu::Backends, &str); 4] = [
    (eframe::wgpu::Backends::VULKAN, "vulkan"),
    (eframe::wgpu::Backends::METAL, "metal"),
    (eframe::wgpu::Backends::DX12, "dx12"),
    (eframe::wgpu::Backends::GL, "gl"),
];

/// The attempt file's contents: the APIs about to be loaded, and this app version.
pub fn attempt_text(backends: eframe::wgpu::Backends, version: &str) -> String {
    let names: Vec<&str> = GUARD_ORDER.iter().filter(|(b, _)| backends.contains(*b)).map(|(_, n)| *n).collect();
    serde_json::json!({ "backends": names, "version": version }).to_string()
}

/// The API a leftover attempt blames for the crash, if the attempt is from `version` and names
/// one; an attempt from another version is forgotten (that version is not running now).
pub fn blamed_backend(leftover: &str, version: &str) -> Option<&'static str> {
    let value: serde_json::Value = serde_json::from_str(leftover).ok()?;
    if value.get("version")?.as_str()? != version {
        return None;
    }
    let names: Vec<&str> = value.get("backends")?.as_array()?.iter().filter_map(serde_json::Value::as_str).collect();
    GUARD_ORDER.iter().find(|(_, n)| names.contains(n)).map(|(_, n)| *n)
}

/// The APIs that crashed this version of the app (`graphics.json`'s `crashed_backends`).
pub fn crashed_backends(graphics: &GraphicsSettings, version: &str) -> eframe::wgpu::Backends {
    recorded_backends(graphics, CRASHED_BACKENDS, version)
}

/// The APIs that could not open the window on this version without crashing: no hardware adapter
/// for them, or a device that would not start (`graphics.json`'s `failed_backends`). The automatic
/// order skips them; unlike crashed ones, the software renderer may still use them (WARP is D3D12).
pub fn failed_backends(graphics: &GraphicsSettings, version: &str) -> eframe::wgpu::Backends {
    recorded_backends(graphics, FAILED_BACKENDS, version)
}

fn recorded_backends(graphics: &GraphicsSettings, key: &str, version: &str) -> eframe::wgpu::Backends {
    let Some(record) = graphics.get(key).and_then(serde_json::Value::as_object) else { return eframe::wgpu::Backends::empty() };
    GUARD_ORDER
        .iter()
        .filter(|(_, n)| record.get(*n).and_then(serde_json::Value::as_str) == Some(version))
        .fold(eframe::wgpu::Backends::empty(), |all, (b, _)| all | *b)
}

/// Add `name` to `graphics`' record `key` for `version`, dropping other versions' entries.
/// Returns whether `graphics` changed.
fn note_backend(graphics: &mut GraphicsSettings, key: &str, name: Option<&str>, version: &str) -> bool {
    let before = graphics.get(key).cloned();
    let mut record: serde_json::Map<String, serde_json::Value> = before
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .map(|r| r.iter().filter(|(_, v)| v.as_str() == Some(version)).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    if let Some(name) = name {
        record.insert(name.into(), serde_json::Value::String(version.into()));
    }
    let after = (!record.is_empty()).then_some(serde_json::Value::Object(record));
    let changed = after != before;
    match after {
        Some(record) => graphics.insert(key.into(), record),
        None => graphics.remove(key),
    };
    changed
}

/// The APIs a launch skips ([`BackendGuard::begin`]). Its default skips none (wgpu's
/// `Backends::default()` is every backend, so it is not derived).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkippedBackends {
    /// Crashed the process while starting: never loaded, safe mode and software rendering included.
    pub crashed: eframe::wgpu::Backends,
    /// Could not open the window (no hardware adapter): skipped by the automatic order only.
    pub failed: eframe::wgpu::Backends,
}

impl Default for SkippedBackends {
    fn default() -> Self {
        Self { crashed: eframe::wgpu::Backends::empty(), failed: eframe::wgpu::Backends::empty() }
    }
}

/// A launch begins with what the last one left (`leftover`, the attempt file's text): a crash
/// during an attempt adds the blamed API to `graphics`' record, and entries from other versions
/// go. Returns the APIs that crashed this version and whether `graphics` changed.
pub fn note_crashed_attempt(graphics: &mut GraphicsSettings, leftover: Option<&str>, version: &str) -> (eframe::wgpu::Backends, bool) {
    let blamed = leftover.and_then(|text| blamed_backend(text, version));
    let crashed = note_backend(graphics, CRASHED_BACKENDS, blamed, version);
    let failed = note_backend(graphics, FAILED_BACKENDS, None, version);
    (crashed_backends(graphics, version), crashed || failed)
}

/// The backends left once `crashed` are taken out of `backends`; never none at all (with every
/// API crashed there is nothing else to open a window with, so all of them are tried again).
pub fn without_crashed(backends: eframe::wgpu::Backends, crashed: eframe::wgpu::Backends) -> eframe::wgpu::Backends {
    let rest = backends - crashed;
    if rest.intersects(native_backends()) { rest } else { backends }
}

/// The APIs that can open a window on this system (wgpu's default set also names others, such as
/// Metal and WebGPU on Windows, which never can).
fn native_backends() -> eframe::wgpu::Backends {
    use eframe::wgpu::Backends;
    if cfg!(target_os = "windows") {
        Backends::VULKAN | Backends::DX12 | Backends::GL
    } else if cfg!(target_os = "macos") {
        Backends::METAL | Backends::GL
    } else {
        Backends::VULKAN | Backends::GL
    }
}

/// The window could not start on `failed` (one API) without crashing: record it
/// ([`failed_backends`]), so later launches of this version go straight to the next API in the
/// order instead of failing on this one first.
pub fn record_failed_backend(graphics_path: &Path, failed: eframe::wgpu::Backends, version: &str) {
    let name = GUARD_ORDER.iter().find(|(b, _)| failed.contains(*b)).map(|(_, n)| *n);
    let result = read_backend_settings(graphics_path).and_then(|mut graphics| {
        let changed = note_backend(&mut graphics, FAILED_BACKENDS, name, version);
        if changed { write_backend_settings(graphics_path, &graphics) } else { Ok(()) }
    });
    if let Err(reason) = result {
        log::warn!("graphics: failed API not saved: {reason}");
    }
}

/// The attempt file in the settings folder: [`GRAPHICS_ATTEMPT`].
#[derive(Clone, Debug)]
pub struct BackendGuard {
    marker: PathBuf,
    version: String,
}

impl BackendGuard {
    /// Read what the last launch left in `dir` and record a crash in `graphics_path`
    /// ([`note_crashed_attempt`]). Returns the guard and the APIs this launch skips. Unreadable
    /// graphics settings are left alone (nothing is skipped then).
    pub fn begin(dir: &Path, graphics_path: &Path, version: &str) -> (Self, SkippedBackends) {
        let guard = Self { marker: dir.join(GRAPHICS_ATTEMPT), version: version.into() };
        let leftover = std::fs::read_to_string(&guard.marker).ok();
        let skipped = match read_backend_settings(graphics_path) {
            Ok(mut graphics) => {
                let (crashed, changed) = note_crashed_attempt(&mut graphics, leftover.as_deref(), version);
                if changed && let Err(reason) = write_backend_settings(graphics_path, &graphics) {
                    log::warn!("graphics: crashed API not saved: {reason}");
                }
                SkippedBackends { crashed, failed: failed_backends(&graphics, version) }
            }
            Err(reason) => {
                log::warn!("graphics: {reason}");
                SkippedBackends::default()
            }
        };
        if let Some(name) = leftover.as_deref().and_then(|t| blamed_backend(t, version)) {
            log::warn!("graphics: the last launch crashed while starting {name}; it is skipped on this computer until the next version");
        }
        guard.clear();
        (guard, skipped)
    }

    /// `backends` are about to be started: say so on disk first, flushed.
    pub fn attempt(&self, backends: eframe::wgpu::Backends) {
        let text = attempt_text(backends, &self.version);
        if let Err(reason) = solidcraft_engine::io::safe_write::write_atomic_no_follow(&self.marker, text.as_bytes()) {
            log::warn!("graphics: attempt not saved: {reason}");
        }
    }

    /// The attempt is over without a crash: the device is up and drew a frame, or startup returned.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.marker);
    }
}

/// Read, change and save `graphics.json`'s safe graphics record at `path`. Unreadable settings
/// are left alone.
pub fn update_safe_graphics<T>(path: &Path, change: impl FnOnce(&mut SafeGraphics) -> T) -> Result<T, String> {
    let mut graphics = read_backend_settings(path)?;
    let mut record = SafeGraphics::read(&graphics);
    let out = change(&mut record);
    record.write(&mut graphics);
    write_backend_settings(path, &graphics)?;
    Ok(out)
}

/// The software adapter among `adapters` (WARP on Windows, lavapipe or llvmpipe elsewhere), else
/// the first one that can draw to `surface`.
pub fn pick_cpu_adapter(adapters: &[eframe::wgpu::Adapter], surface: Option<&eframe::wgpu::Surface<'_>>) -> Result<eframe::wgpu::Adapter, String> {
    let fits = |a: &&eframe::wgpu::Adapter| surface.is_none_or(|s| a.is_surface_supported(s));
    adapters
        .iter()
        .filter(fits)
        .find(|a| a.get_info().device_type == eframe::wgpu::DeviceType::Cpu)
        .or_else(|| adapters.iter().find(fits))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().backend)).collect();
            format!("no graphics adapter can draw the window (found: {})", if names.is_empty() { "none".into() } else { names.join(", ") })
        })
}

/// Adapter properties used to choose a card without opening a device.
#[derive(Clone, Debug)]
pub struct AdapterChoice {
    pub name: String,
    pub compatible: bool,
}

impl AdapterChoice {
    fn of(adapter: &eframe::wgpu::Adapter, surface: Option<&eframe::wgpu::Surface<'_>>) -> Self {
        let info = adapter.get_info();
        Self { name: info.name, compatible: surface.is_none_or(|s| adapter.is_surface_supported(s)) }
    }
}

/// Choose an adapter by name. The environment override follows wgpu's case-insensitive
/// substring convention among surface-compatible adapters. A saved preference prefers an
/// exact (then case-insensitive) name among compatible adapters, and if the named card is
/// only listed as incompatible with the surface still picks it (#8341: otherwise the
/// preference is kept in the dialog while wgpu silently uses another GPU).
/// No match leaves selection to the normal wgpu request.
pub fn choose_adapter(adapters: &[AdapterChoice], environment: Option<&str>, preference: Option<&str>) -> Option<usize> {
    // wgpu's environment helper panics on a missing name; an override must remain recoverable.
    let compatible = || adapters.iter().enumerate().filter(|(_, a)| a.compatible);
    if let Some(name) = environment.filter(|s| !s.is_empty()) {
        let name = name.to_lowercase();
        if let Some((index, _)) = compatible().find(|(_, a)| a.name.to_lowercase().contains(&name)) {
            return Some(index);
        }
    }
    let name = preference.filter(|s| !s.is_empty())?;
    let find = |pred: &dyn Fn(&AdapterChoice) -> bool, only_compatible: bool| {
        adapters.iter().enumerate().find(|(_, a)| (!only_compatible || a.compatible) && pred(a)).map(|(i, _)| i)
    };
    find(&|a| a.name == name, true)
        .or_else(|| find(&|a| a.name.eq_ignore_ascii_case(name), true))
        .or_else(|| find(&|a| a.name == name, false))
        .or_else(|| find(&|a| a.name.eq_ignore_ascii_case(name), false))
}

/// Leave Automatic on wgpu's normal request path; named choices may defer to it too.
pub fn configure_adapter(setup: &mut eframe::egui_wgpu::WgpuSetupCreateNew, cpu: bool, environment: Option<String>, preference: Option<String>) {
    if cpu {
        setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, surface| pick_cpu_adapter(adapters, surface).map(Some)));
    } else if environment.as_deref().is_some_and(|s| !s.is_empty()) || preference.as_deref().is_some_and(|s| !s.is_empty()) {
        setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| {
            let choices: Vec<_> = adapters.iter().map(|a| AdapterChoice::of(a, surface)).collect();
            Ok(choose_adapter(&choices, environment.as_deref(), preference.as_deref()).and_then(|i| adapters.get(i)).cloned())
        }));
    }
}

/// `WGPU_BACKEND`: when set, wgpu picks the backends from it and the order below is not used.
pub const WGPU_BACKEND_ENV: &str = "WGPU_BACKEND";

/// The graphics backends to start with, or `None` to leave it to wgpu (`WGPU_BACKEND`, or all
/// the backends it was built with).
///
/// On Windows, Vulkan comes first, then D3D12, then OpenGL, and only one of them is started at
/// all (never several APIs together). Vulkan is the open API the other systems use too; under
/// D3D12, AMD's Ryzen integrated graphics lost the device or froze the machine while opening a
/// design (#1789: 0.11.0, on Vulkan, worked; 0.12.0, on D3D12 first, did not). Where Vulkan has
/// no card (older Intel graphics, virtual machines, Remote Desktop) D3D12 is next, then OpenGL,
/// each only with a hardware adapter, then D3D12's software adapter (WARP), all in one process;
/// a device that fails to start falls back the same way ([`fallback_backend`]).
/// `has_hardware(backends)` says whether those backends have an adapter that isn't software
/// (it is only asked about the backends being tried, one at a time, each named on disk first by
/// the crash guard). An API that crashed the process while starting (`crashed`, [`BackendGuard`])
/// is skipped everywhere but in an explicit choice, and never probed; one whose window failed to
/// start (`failed`) is skipped by the automatic order.
///
/// - `WGPU_BACKEND` set (`env_set`): `None`, whatever it says wins.
/// - The software renderer (`cpu`, safe mode): D3D12 alone on Windows (WARP); `None` elsewhere.
/// - `preference` (`graphics.json`'s `backend`: `dx12`, `vulkan`, `gl` or a comma list; `auto`
///   or empty for the order above): those backends, on any system, crashed or not.
/// - Elsewhere than Windows: `None` (with crashed APIs, every backend but those).
/// - On Windows an AMD adapter on the OpenGL backend does not count as hardware - the scene
///   draws nothing there ([`counts_as_hardware`]) - so the automatic order ends at the
///   software renderer on such a machine.
pub fn choose_backends(
    windows: bool,
    env_set: bool,
    cpu: bool,
    preference: Option<&str>,
    crashed: eframe::wgpu::Backends,
    failed: eframe::wgpu::Backends,
    mut has_hardware: impl FnMut(eframe::wgpu::Backends) -> bool,
) -> Option<eframe::wgpu::Backends> {
    use eframe::wgpu::Backends;
    if env_set {
        return None;
    }
    let platform = if windows { Backends::VULKAN | Backends::DX12 | Backends::GL } else { Backends::VULKAN | Backends::GL | Backends::METAL };
    let others = || (!crashed.is_empty()).then(|| without_crashed(platform, crashed));
    if cpu {
        return if windows && !crashed.contains(Backends::DX12) { Some(Backends::DX12) } else { others() };
    }
    if let Some(p) = preference.map(str::trim).filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto")) {
        let chosen = Backends::from_comma_list(p);
        if !chosen.is_empty() {
            return Some(chosen);
        }
        log::warn!("graphics: unknown backend preference `{p}`; using the default order");
    }
    if !windows {
        return others();
    }
    let skip = crashed | failed;
    if let Some(found) = WINDOWS_ORDER.into_iter().filter(|b| !skip.contains(*b)).find(|b| has_hardware(*b)) {
        return Some(found);
    }
    // No hardware adapter: D3D12's software adapter (WARP), unless D3D12 crashed.
    Some(if crashed.contains(Backends::DX12) {
        WINDOWS_ORDER.into_iter().find(|b| !crashed.contains(*b)).unwrap_or(Backends::DX12)
    } else {
        Backends::DX12
    })
}

/// Internal, process-local retry choice; never written to the graphics preference.
pub const RETRY_BACKEND_ENV: &str = "SOLIDCRAFT_STARTUP_BACKEND";
/// Why this launch is using a fallback, carried to the new process for status and Preferences.
pub const FALLBACK_NOTE_ENV: &str = "SOLIDCRAFT_GRAPHICS_FALLBACK";

/// A handled startup failure tries each remaining Windows API once (after Vulkan, D3D12), then
/// software rendering (WARP, which is D3D12's).
/// An explicit environment choice is final, including when software rendering was requested.
/// An API that crashed the process while starting (`crashed`) is never the next one.
pub fn fallback_backend(
    windows: bool,
    explicit: bool,
    cpu: bool,
    failed: eframe::wgpu::Backends,
    crashed: eframe::wgpu::Backends,
) -> Option<&'static str> {
    if explicit || cpu {
        return None;
    }
    if windows {
        let last = WINDOWS_ORDER.iter().rposition(|backend| failed.intersects(*backend));
        if let Some(next) = last.and_then(|i| WINDOWS_ORDER[i + 1..].iter().find(|b| !crashed.contains(**b))) {
            return Some(if *next == eframe::wgpu::Backends::DX12 {
                "dx12"
            } else if *next == eframe::wgpu::Backends::VULKAN {
                "vulkan"
            } else {
                "gl"
            });
        }
    }
    Some("cpu")
}

/// Keep driver diagnostics on one bounded line, including across the finite retry chain.
pub fn fallback_note(previous: &str, failed: eframe::wgpu::Backends, next: &str, error: &str) -> String {
    let previous: String = previous.chars().take(2048).collect();
    let error: String = error.chars().take(1024).collect();
    let line = format!("{previous} Graphics fallback: {failed:?} failed ({error}); trying {next}.");
    line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(4096).collect()
}

/// The device feature for a 32-bit float depth buffer with a stencil (reversed depth, #1434).
pub const FLOAT_DEPTH_STENCIL: eframe::wgpu::Features = eframe::wgpu::Features::DEPTH32FLOAT_STENCIL8;

/// Timestamp queries: the GPU time of the view's passes in the frame-rate readout's details. Asked
/// for only where the adapter has them; without them the readout leaves that row out.
pub const TIMESTAMP_QUERY: eframe::wgpu::Features = eframe::wgpu::Features::TIMESTAMP_QUERY;

/// The order backends are tried in on Windows.
pub const WINDOWS_ORDER: [eframe::wgpu::Backends; 3] = [eframe::wgpu::Backends::VULKAN, eframe::wgpu::Backends::DX12, eframe::wgpu::Backends::GL];

/// Whether an adapter counts as usable hardware for the automatic Windows backend order.
///
/// On Windows, an AMD adapter that wgpu offers on the OpenGL backend starts a device, but
/// its scene passes draw nothing: the view shows only the egui-drawn origin dot, and the
/// screen flashes black on every repaint (#11357, #11834, #12994, #16453). Such an adapter
/// does not count as hardware, so the automatic order falls through to the next backend and
/// ends at the software renderer, which draws everything (`--safe-mode` showed the planes,
/// axes and grid on the reporters' machines). An explicit `WGPU_BACKEND` or a `graphics.json`
/// `backend` preference is never second-guessed; other backends and other systems are
/// untouched.
pub fn counts_as_hardware(windows: bool, backend: eframe::wgpu::Backend, name: &str) -> bool {
    let amd = ["amd", "radeon", "ati"].iter().any(|k| name.to_ascii_lowercase().contains(k));
    !(windows && backend == eframe::wgpu::Backend::Gl && amd)
}

/// Whether `backends` (started on their own, nothing else) have a hardware adapter. The crash
/// guard names them on disk before this loads their driver.
pub fn has_hardware_adapter(backends: eframe::wgpu::Backends) -> bool {
    let mut desc = eframe::wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    desc.flags = eframe::wgpu::InstanceFlags::from_build_config().with_env();
    desc.backend_options.dx12.shader_compiler = solidcraft_viewport_gpu::dx12_compiler();
    let instance = eframe::wgpu::Instance::new(desc);
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let found: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().device_type)).collect();
    log::info!("graphics: {backends:?} adapters: {found:?}");
    let windows = cfg!(target_os = "windows");
    adapters
        .iter()
        .any(|a| a.get_info().device_type != eframe::wgpu::DeviceType::Cpu && counts_as_hardware(windows, a.get_info().backend, &a.get_info().name))
}

/// The environment variable that switches wgpu's indirect-call validation on or off.
pub const INDIRECT_VALIDATION_ENV: &str = "WGPU_VALIDATION_INDIRECT_CALL";

/// The instance flags for the window. wgpu's indirect-call validation compiles a compute shader
/// when the device is created, and on Intel Macs (macOS 12 with Intel Iris graphics, #3024,
/// #3080, #4605) the Metal compiler refuses it: the device is lost and the window never opens.
/// So on an Intel Mac the validation is left off, unless the user turned it on or off with
/// `WGPU_VALIDATION_INDIRECT_CALL` themselves.
pub fn window_instance_flags(flags: eframe::wgpu::InstanceFlags, intel_mac: bool, env: Option<&str>) -> eframe::wgpu::InstanceFlags {
    if intel_mac && env.map(str::trim).is_none_or(str::is_empty) { flags - eframe::wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL } else { flags }
}

/// The environment variable that makes the macOS compatibility shim report an older OS
/// version to wgpu's Metal backend. On OCLP-patched machines the Metal compiler accepts
/// only up to the MSL standard of the original GPU, not the one the patched OS reports,
/// so wgpu picks a standard that the compiler refuses (`invalid value 'metal3.2'`).
/// Setting this variable before the process starts causes the OS to report the older
/// version and wgpu selects a supported MSL standard (#3080, #4605).
#[cfg(target_os = "macos")]
pub const SYSTEM_VERSION_COMPAT_ENV: &str = "SYSTEM_VERSION_COMPAT";

/// The NVRAM variable that OpenCore Legacy Patcher writes on every OCLP-booted system.
/// Its presence is the primary signal that `SYSTEM_VERSION_COMPAT=1` is needed.
#[cfg(target_os = "macos")]
pub const OCLP_NVRAM_KEY: &str = "4D1FDA02-38C7-4A6A-9CC6-4BCCA8B30102:OCLP-Version";

/// Where macOS keeps its `nvram` tool.
#[cfg(target_os = "macos")]
const NVRAM_TOOL: &str = "/usr/sbin/nvram";

/// `true` if the OCLP NVRAM key is present on this system.
///
/// Runs `/usr/sbin/nvram <key>` and treats exit 0 as present. The full path keeps a `PATH` that
/// lacks `/usr/sbin`, or lists a look-alike first, from changing the answer. Any error (missing
/// binary, permission denied, key absent) returns `false`: this is a best-effort probe.
#[cfg(target_os = "macos")]
pub fn oclp_nvram_present() -> bool {
    std::process::Command::new(NVRAM_TOOL).arg(OCLP_NVRAM_KEY).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Whether to re-launch with `SYSTEM_VERSION_COMPAT=1`: it is not set yet and OCLP shows through
/// the NVRAM key. The probe runs only when it is needed, so a re-launched process (variable
/// set) never starts `nvram`.
#[cfg(target_os = "macos")]
fn compat_wanted(compat_set: bool, nvram: impl FnOnce() -> bool) -> bool {
    !compat_set && nvram()
}

/// If this process is running under OCLP and `SYSTEM_VERSION_COMPAT` has not been set,
/// re-launch the current binary with `SYSTEM_VERSION_COMPAT=1` added to the environment,
/// then exit with the child's status code.
///
/// `SYSTEM_VERSION_COMPAT=1` must be present **before** the process starts: setting it
/// with `std::env::set_var` inside `main` is too late because the macOS frameworks have
/// already queried the OS version by then, so the program starts itself again with the variable
/// set and waits for that child.
///
/// Returns `Ok(false)` immediately when:
/// - `SYSTEM_VERSION_COMPAT` is already set (re-launched process, no loop), or
/// - the OCLP NVRAM key is absent.
///
/// Returns `Err` only when `std::env::current_exe()` fails or the child process cannot
/// be spawned; the caller should log the error and continue normally.
///
/// Never returns `Ok(true)`: a successful re-launch exits the current process.
#[cfg(target_os = "macos")]
pub fn relaunch_with_version_compat() -> Result<bool, String> {
    // A set variable means this is the re-launched process: no loop.
    if !compat_wanted(std::env::var_os(SYSTEM_VERSION_COMPAT_ENV).is_some(), oclp_nvram_present) {
        return Ok(false);
    }
    log::info!("startup: OCLP detected; re-launching with {}=1", SYSTEM_VERSION_COMPAT_ENV);
    solidcraft_engine::note!("SolidCraft: OCLP detected; re-launching with {}=1", SYSTEM_VERSION_COMPAT_ENV);
    let exe = std::env::current_exe().map_err(|e| format!("could not find own executable: {e}"))?;
    let status = std::process::Command::new(&exe)
        .args(std::env::args_os().skip(1))
        .env(SYSTEM_VERSION_COMPAT_ENV, "1")
        .status()
        .map_err(|e| format!("could not re-launch with {}: {e}", SYSTEM_VERSION_COMPAT_ENV))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Windows with OpenGL: the window keeps the system title bar (#2063). The window without OS
/// decorations keeps its caption style while having no frame, and some OpenGL drivers (older
/// Intel HD/UHD) then show the picture a caption's height lower than the window, so it no longer
/// lines up with the pointer. Direct3D 12 and Vulkan draw it in place and keep the application bar
/// as the title bar. `backends` are the ones wgpu starts: OpenGL alone is decided before the
/// window opens.
pub fn system_title_bar(windows: bool, backends: eframe::wgpu::Backends) -> bool {
    use eframe::wgpu::Backends;
    windows && backends.contains(Backends::GL) && !backends.intersects(Backends::DX12 | Backends::VULKAN)
}

/// The `--max-file-size` value (#4086): a size such as `2GB` or `1500MB`, at most 16 GB. It raises
/// the largest STEP/import and design file this run reads, over the saved preference, without
/// saving anything. A missing or bad value stops the start with this error.
pub fn max_file_size(value: Option<&str>) -> Result<u64, String> {
    let value = value.ok_or("--max-file-size needs a size, for example --max-file-size 2GB")?;
    solidcraft_engine::io::limits::parse_flag(value).map_err(|e| format!("--max-file-size: {e}"))
}

/// The message shown when no window could be opened at all.
pub fn no_window_message(error: &str, settings: Option<&Path>) -> String {
    let folder = settings.map(|d| d.display().to_string()).unwrap_or_else(|| "the SolidCraft settings folder".into());
    format!(
        "SolidCraft could not open its window.\n\n{error}\n\n\
         The software renderer was tried too. Things to try:\n\
         - Install or update the graphics driver (on Windows, from the GPU maker's site rather than Windows Update).\n\
         - Over Remote Desktop or in a virtual machine, enable 3D acceleration or run it on the machine itself.\n\
         - Start it once with `solidcraft --safe-mode`.\n\
         - Rename the settings folder: {folder}\n\n\
         More: https://docs.solidcraft.io/guide/troubleshooting.html"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards() -> Vec<AdapterChoice> {
        ["Integrated", "Discrete small", "Discrete large"].into_iter().map(|name| AdapterChoice { name: name.into(), compatible: true }).collect()
    }

    #[test]
    fn max_file_size_parses_sizes_and_refuses_bad_ones() {
        assert_eq!(max_file_size(Some("2GB")), Ok(2 << 30));
        assert_eq!(max_file_size(Some("1500MB")), Ok(1500 << 20));
        for bad in [None, Some(""), Some("2"), Some("17GB"), Some("lots"), Some("-1GB")] {
            let error = max_file_size(bad).unwrap_err();
            assert!(error.contains("--max-file-size"), "{bad:?}: {error}");
        }
    }

    #[test]
    fn adapter_names_environment_override_and_missing_card_fallback() {
        let cards = cards();
        assert_eq!(choose_adapter(&cards, None, Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, Some("LARGE"), Some("Discrete small")), Some(2));
        assert_eq!(choose_adapter(&cards, None, Some("removed")), None);
        assert_eq!(choose_adapter(&cards, Some("missing"), Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, Some(""), Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, None, Some("discrete small")), Some(1), "preference matches without regard to case");
        assert_eq!(choose_adapter(&cards, None, Some("Discrete")), None, "a preference is the full name, not a substring");
    }

    #[test]
    fn automatic_adapter_leaves_selection_to_wgpu() {
        let mut cards = cards();
        assert_eq!(choose_adapter(&cards, None, None), None);
        assert_eq!(choose_adapter(&cards, Some(""), Some("")), None);
        cards[2].compatible = false;
        // Environment still needs a compatible adapter; a preference keeps the named card (#8341).
        assert_eq!(choose_adapter(&cards, Some("large"), Some("Discrete large")), Some(2));
        assert_eq!(choose_adapter(&cards, Some("small"), None), Some(1));
        assert_eq!(choose_adapter(&[], None, None), None);
    }

    #[test]
    fn preference_keeps_a_named_card_even_when_surface_says_incompatible() {
        let mut cards = cards();
        cards[1].compatible = false;
        assert_eq!(choose_adapter(&cards, None, Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, None, Some("discrete SMALL")), Some(1));
    }

    #[test]
    fn automatic_adapter_preserves_the_previous_request_configuration() {
        use eframe::egui_wgpu::WgpuSetupCreateNew;
        let previous = WgpuSetupCreateNew::without_display_handle();
        for preference in [None, Some(String::new())] {
            let mut setup = WgpuSetupCreateNew::without_display_handle();
            configure_adapter(&mut setup, false, None, preference);
            assert!(setup.native_adapter_selector.is_none(), "Automatic must use the normal wgpu request");
            assert_eq!(setup.power_preference, previous.power_preference);
            assert_eq!(setup.instance_descriptor.backends, previous.instance_descriptor.backends);
        }
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        configure_adapter(&mut setup, false, Some("missing".into()), Some("removed".into()));
        assert!(setup.native_adapter_selector.unwrap()(&[], None).unwrap().is_none(), "missing names must use the normal request too");
    }

    /// One launch: `begin` on the record, then the launch's outcome. Returns why it was safe.
    fn launch(record: &mut SafeGraphics, previous_failed: bool, outcome: &str) -> Option<SafeGraphicsReason> {
        let reason = record.begin(previous_failed, true, false);
        if reason.is_none() {
            match outcome {
                "proved" => record.proved(),
                "closed" => record.closed_normally(),
                "lost" => record.lost_device(),
                _ => {}
            }
        }
        reason
    }

    /// A working machine never sees safe graphics, whether it quits early or runs on.
    #[test]
    fn safe_graphics_never_triggers_on_launches_that_come_up() {
        let mut record = SafeGraphics::default();
        for outcome in ["proved", "closed", "proved", "closed", "closed"] {
            assert_eq!(launch(&mut record, false, outcome), None);
        }
        assert_eq!(record, SafeGraphics::default());
    }

    /// The launch after one that never reached "app up" (its trial still set, or the start marker
    /// counted it), or that lost its device, is safe; the one after that tries full graphics again.
    #[test]
    fn safe_graphics_triggers_after_a_failed_start_or_a_lost_device() {
        let mut record = SafeGraphics::default();
        assert_eq!(launch(&mut record, false, "crash"), None);
        assert_eq!(launch(&mut record, false, ""), Some(SafeGraphicsReason::Failed), "trial left set by a crash");
        assert_eq!(record.failures, 1);
        assert_eq!(launch(&mut record, false, "proved"), None, "one failure: full graphics is tried again");
        assert_eq!(record.failures, 0, "a proved launch forgets the failure");

        let mut record = SafeGraphics::default();
        assert_eq!(launch(&mut record, true, ""), Some(SafeGraphicsReason::Failed), "the start marker from an older version");
        let mut record = SafeGraphics::default();
        assert_eq!(launch(&mut record, false, "proved"), None);
        record.lost_device();
        assert_eq!(launch(&mut record, false, ""), Some(SafeGraphicsReason::Failed), "a loss after the proof still counts");

        // Launches driven over the control channel neither count nor are counted.
        let mut record = SafeGraphics { trial: true, ..SafeGraphics::default() };
        assert_eq!(record.begin(true, false, false), None);
        assert!(!record.trial);
    }

    /// Two failed full-graphics launches in a row make safe graphics stick, through any number
    /// of launches, until Try full graphics clears it; one more failure sticks again, and a full
    /// launch that proves itself clears everything.
    #[test]
    fn safe_graphics_sticks_after_two_failures_until_full_graphics_is_asked_for() {
        let mut record = SafeGraphics::default();
        assert_eq!(launch(&mut record, false, "crash"), None);
        assert_eq!(launch(&mut record, false, ""), Some(SafeGraphicsReason::Failed));
        assert_eq!(launch(&mut record, false, "crash"), None, "the retry");
        for _ in 0..4 {
            assert_eq!(launch(&mut record, false, ""), Some(SafeGraphicsReason::Sticky));
        }
        assert_eq!(record.failures, 2, "safe launches are not full-graphics failures");
        // A safe launch that failed itself does not count as a full-graphics failure either.
        assert_eq!(launch(&mut record, true, ""), Some(SafeGraphicsReason::Sticky));
        assert_eq!(record.failures, 2);
        // Try full graphics (the UI clears `sticky` and `next` in graphics.json).
        record.sticky = false;
        assert_eq!(launch(&mut record, false, "crash"), None);
        assert_eq!(launch(&mut record, false, ""), Some(SafeGraphicsReason::Sticky), "one more failure sticks again");
        record.sticky = false;
        assert_eq!(launch(&mut record, false, "proved"), None);
        assert_eq!(record.failures, 0);
        assert_eq!(launch(&mut record, false, "crash"), None);
        assert_eq!(launch(&mut record, false, ""), Some(SafeGraphicsReason::Failed), "a fresh first failure is not sticky");
    }

    /// Asked for in Preferences: the next launch only. `--safe-mode` is safe graphics too.
    #[test]
    fn safe_graphics_asked_for_applies_to_one_launch_and_the_record_survives_graphics_json() {
        let mut record = SafeGraphics { next: true, ..SafeGraphics::default() };
        assert_eq!(launch(&mut record, false, "closed"), Some(SafeGraphicsReason::Asked));
        assert_eq!(launch(&mut record, false, "closed"), None);
        assert_eq!(record.begin(false, true, true), Some(SafeGraphicsReason::Asked));

        let dir = scratch("safe-graphics");
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"backend":"vulkan","msaa":8,"safe_graphics":{"failures":"x","sticky":7}}"#).unwrap();
        assert_eq!(update_safe_graphics(&path, |r| r.clone()).unwrap(), SafeGraphics::default(), "wrong types read as defaults");
        update_safe_graphics(&path, |r| r.begin(false, true, false)).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["backend"], "vulkan");
        assert_eq!(saved["msaa"], 8);
        assert_eq!(saved["safe_graphics"]["trial"], true);
        std::fs::write(&path, "not json").unwrap();
        assert!(update_safe_graphics(&path, |r| r.begin(true, true, false)).is_err(), "unreadable settings are left alone");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The safe launch after a full launch that failed on Vulkan starts without Vulkan (D3D12
    /// first on Windows, so the crashed driver isn't loaded again), and after a D3D12 failure
    /// without D3D12; one asked for keeps wgpu's own order.
    #[test]
    fn safe_graphics_after_a_vulkan_failure_starts_on_d3d12() {
        use eframe::wgpu::Backends;
        let default = Backends::PRIMARY | Backends::GL;
        let mut record = SafeGraphics::default();
        assert_eq!(record.begin(false, true, false), None);
        record.backend = backend_name(Backends::VULKAN).map(str::to_string);
        let reason = record.begin(false, true, false);
        assert_eq!(reason, Some(SafeGraphicsReason::Failed));
        assert_eq!(record.backend.as_deref(), Some("vulkan"), "begin keeps the failed launch's API");
        let safe = safe_backends(default, record.backend.as_deref(), eframe::wgpu::Backends::empty());
        assert!(!safe.contains(Backends::VULKAN));
        assert!(safe.contains(Backends::DX12) && safe.contains(Backends::GL), "D3D12, then OpenGL");
        let safe = safe_backends(default, Some("dx12"), eframe::wgpu::Backends::empty());
        assert!(!safe.contains(Backends::DX12) && safe.contains(Backends::VULKAN) && safe.contains(Backends::GL));
        assert_eq!(safe_backends(default, None, eframe::wgpu::Backends::empty()), default, "asked for: wgpu's own order");
        assert_eq!(safe_backends(Backends::VULKAN, Some("vulkan"), eframe::wgpu::Backends::empty()), Backends::VULKAN, "never empty");
        assert_eq!(backend_name(Backends::VULKAN | Backends::GL), None, "several APIs name none");
        // The record survives graphics.json; junk reads as nothing.
        let mut graphics = GraphicsSettings::new();
        record.write(&mut graphics);
        assert_eq!(SafeGraphics::read(&graphics).backend.as_deref(), Some("vulkan"));
        graphics.insert(SAFE_GRAPHICS.into(), serde_json::json!({"backend": "glide"}));
        assert_eq!(SafeGraphics::read(&graphics).backend, None);
    }

    /// The crash guard (#18683): a driver that crashes the process while Vulkan starts leaves the
    /// attempt behind; the next start records Vulkan as crashed and never loads it.
    #[test]
    fn a_crash_while_vulkan_starts_makes_the_next_start_skip_vulkan() {
        use eframe::wgpu::Backends;
        let v = "0.44.2";
        let mut graphics = GraphicsSettings::new();
        graphics.insert("msaa".into(), serde_json::json!(8));
        let leftover = attempt_text(Backends::VULKAN, v);
        assert_eq!(note_crashed_attempt(&mut graphics, Some(&leftover), v), (Backends::VULKAN, true));
        assert_eq!(graphics["msaa"], 8, "other settings stay");
        assert_eq!(
            choose_backends(true, false, false, None, Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::DX12),
            "Vulkan is never loaded"
        );
        assert_eq!(
            choose_backends(true, false, false, Some("auto"), Backends::VULKAN | Backends::DX12, eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::GL),
            "then OpenGL"
        );
        assert_eq!(
            choose_backends(true, false, false, None, Backends::DX12, eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::VULKAN),
            "a crashed D3D12 alone: Vulkan as ever"
        );
        assert_eq!(
            choose_backends(true, false, false, None, Backends::VULKAN | Backends::DX12 | Backends::GL, eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::DX12),
            "all crashed: one is tried again"
        );
        // A handled failure never retries a crashed API either.
        assert_eq!(fallback_backend(true, false, false, Backends::VULKAN, Backends::DX12), Some("gl"));
        assert_eq!(fallback_backend(true, false, false, Backends::DX12, Backends::GL), Some("cpu"));
        // The record lasts: a later start with nothing left over still skips Vulkan.
        assert_eq!(note_crashed_attempt(&mut graphics, None, v), (Backends::VULKAN, false));
        // Several APIs in one attempt: the first one wgpu brings up is blamed, one per crash.
        assert_eq!(blamed_backend(&attempt_text(Backends::DX12 | Backends::GL, v), v), Some("dx12"));
        assert_eq!(
            note_crashed_attempt(&mut graphics, Some(&attempt_text(Backends::DX12 | Backends::GL, v)), v).0,
            Backends::VULKAN | Backends::DX12
        );
        assert_eq!(blamed_backend("not json", v), None);
        // The same on every system: elsewhere wgpu's own choice, less the crashed API.
        let linux = choose_backends(false, false, false, None, Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true).unwrap();
        assert!(!linux.contains(Backends::VULKAN) && linux.contains(Backends::GL));
        assert_eq!(choose_backends(false, false, false, None, Backends::empty(), eframe::wgpu::Backends::empty(), |_| true), None);
    }

    /// The attempt file is written before the driver loads and gone once the start went well: a start
    /// that presented its first frame (or returned) blames nothing; one that died does.
    #[test]
    fn the_attempt_file_is_cleared_by_a_good_start_and_blames_a_crashed_one() {
        use eframe::wgpu::Backends;
        let dir = scratch("backend-guard");
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"msaa":4}"#).unwrap();
        let (guard, skipped) = BackendGuard::begin(&dir, &path, "0.44.2");
        assert!(SkippedBackends::default().crashed.is_empty() && SkippedBackends::default().failed.is_empty(), "the default skips nothing");
        assert_eq!(skipped, SkippedBackends::default());
        guard.attempt(Backends::VULKAN);
        assert!(dir.join(GRAPHICS_ATTEMPT).is_file(), "on disk before the driver loads");
        guard.clear();
        let (guard, skipped) = BackendGuard::begin(&dir, &path, "0.44.2");
        assert_eq!(skipped, SkippedBackends::default(), "a start that presented a frame blames nothing");
        assert_eq!(backend_settings(&path), serde_json::json!({"msaa": 4}), "graphics.json untouched");
        // The process dies inside the Vulkan driver: nothing clears the file.
        guard.attempt(Backends::VULKAN);
        let (_, skipped) = BackendGuard::begin(&dir, &path, "0.44.2");
        assert_eq!(skipped.crashed, Backends::VULKAN);
        assert!(!dir.join(GRAPHICS_ATTEMPT).exists(), "the leftover is read once");
        assert_eq!(backend_settings(&path), serde_json::json!({"msaa": 4, "crashed_backends": {"vulkan": "0.44.2"}}));
        let (_, skipped) = BackendGuard::begin(&dir, &path, "0.44.2");
        assert_eq!(skipped.crashed, Backends::VULKAN, "skipped from then on");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A window that failed to start on an API (its device would not start) is kept, so the next
    /// launch of this version goes straight to the next API.
    #[test]
    fn a_backend_that_could_not_open_the_window_is_skipped_next_time() {
        use eframe::wgpu::Backends;
        let dir = scratch("backend-failed");
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"backend":"auto"}"#).unwrap();
        record_failed_backend(&path, Backends::VULKAN, "0.44.2");
        let (_, skipped) = BackendGuard::begin(&dir, &path, "0.44.2");
        assert_eq!(skipped, SkippedBackends { crashed: Backends::empty(), failed: Backends::VULKAN });
        assert_eq!(choose_backends(true, false, false, Some("auto"), skipped.crashed, skipped.failed, |_| true), Some(Backends::DX12));
        // Not crashed: the software renderer still uses D3D12 (WARP).
        assert_eq!(choose_backends(true, false, true, None, skipped.crashed, skipped.failed, |_| true), Some(Backends::DX12));
        assert_eq!(backend_settings(&path)["backend"], "auto", "the preference stays");
        let (_, skipped) = BackendGuard::begin(&dir, &path, "0.45.0");
        assert_eq!(skipped, SkippedBackends::default(), "the next version tries Vulkan again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Windows machine whose only hardware API is OpenGL: Vulkan has no adapter, D3D12 only WARP.
    /// The automatic order takes only an API with a hardware adapter, so it lands on OpenGL in one
    /// process, never on WARP; with no hardware at all, WARP, still in one process.
    #[test]
    fn a_machine_with_only_opengl_hardware_lands_on_opengl_not_warp() {
        use eframe::wgpu::Backends;
        let none = Backends::empty();
        let mut asked = vec![];
        let chosen = choose_backends(true, false, false, None, none, none, |b| {
            asked.push(b);
            b == Backends::GL
        });
        assert_eq!((chosen, asked), (Some(Backends::GL), vec![Backends::VULKAN, Backends::DX12, Backends::GL]), "OpenGL hardware, not WARP");
        // No hardware adapter anywhere (a VM, a GPU-less CI runner): WARP at once, no restarts.
        assert_eq!(choose_backends(true, false, false, None, none, none, |_| false), Some(Backends::DX12));
        // A crashed API is never probed; a failed one is skipped too.
        let mut asked = vec![];
        let chosen = choose_backends(true, false, false, None, Backends::VULKAN, Backends::DX12, |b| {
            asked.push(b);
            true
        });
        assert_eq!((chosen, asked), (Some(Backends::GL), vec![Backends::GL]));
        // With D3D12 crashed and no hardware: not WARP (it is D3D12); the next API not crashed.
        assert_eq!(choose_backends(true, false, false, None, Backends::DX12, none, |_| false), Some(Backends::VULKAN));
    }

    /// A new app version (perhaps with a new driver) tries a crashed API once more; an attempt
    /// left by another version blames nothing.
    #[test]
    fn a_new_version_retries_a_crashed_backend() {
        use eframe::wgpu::Backends;
        let mut graphics = GraphicsSettings::new();
        note_crashed_attempt(&mut graphics, Some(&attempt_text(Backends::VULKAN, "0.44.1")), "0.44.1");
        assert_eq!(crashed_backends(&graphics, "0.44.1"), Backends::VULKAN);
        assert_eq!(note_crashed_attempt(&mut graphics, None, "0.44.2"), (Backends::empty(), true));
        assert!(!graphics.contains_key(CRASHED_BACKENDS), "the old version's record goes");
        assert_eq!(note_crashed_attempt(&mut graphics, Some(&attempt_text(Backends::VULKAN, "0.44.1")), "0.44.2"), (Backends::empty(), false));
    }

    /// Preferences › Graphics API and WGPU_BACKEND still win over the record.
    #[test]
    fn an_explicit_choice_wins_over_a_crashed_backend() {
        use eframe::wgpu::Backends;
        assert_eq!(
            choose_backends(true, false, false, Some("vulkan"), Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::VULKAN)
        );
        assert_eq!(
            choose_backends(false, false, false, Some("vulkan"), Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::VULKAN)
        );
        assert_eq!(
            choose_backends(true, true, false, None, Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true),
            None,
            "WGPU_BACKEND: wgpu reads it"
        );
        assert_eq!(choose_backends(true, true, true, None, Backends::DX12, eframe::wgpu::Backends::empty(), |_| true), None);
        assert_eq!(fallback_backend(true, true, false, Backends::VULKAN, Backends::DX12), None);
    }

    /// Safe graphics mode and the software renderer start an instance without the crashed APIs,
    /// so their drivers are never loaded; with every API crashed, all are tried again.
    #[test]
    fn safe_mode_and_the_software_renderer_leave_crashed_backends_out() {
        use eframe::wgpu::Backends;
        let default = Backends::PRIMARY | Backends::GL;
        let safe = safe_backends(default, None, Backends::VULKAN);
        assert!(!safe.contains(Backends::VULKAN) && safe.contains(Backends::DX12) && safe.contains(Backends::GL));
        assert_eq!(safe_backends(default, Some("dx12"), Backends::VULKAN) & (Backends::VULKAN | Backends::DX12), Backends::empty());
        assert_eq!(safe_backends(Backends::VULKAN | Backends::GL, Some("gl"), Backends::VULKAN), Backends::GL, "never empty");
        assert_eq!(safe_backends(Backends::VULKAN, None, Backends::VULKAN), Backends::VULKAN, "all there is");
        // The software renderer: WARP alone on Windows; without D3D12 (crashed), everything else.
        assert_eq!(choose_backends(true, false, true, None, Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true), Some(Backends::DX12));
        let cpu = choose_backends(true, false, true, None, Backends::DX12, eframe::wgpu::Backends::empty(), |_| true).unwrap();
        assert!(!cpu.contains(Backends::DX12) && cpu.contains(Backends::VULKAN));
        let cpu = choose_backends(false, false, true, None, Backends::VULKAN, eframe::wgpu::Backends::empty(), |_| true).unwrap();
        assert!(!cpu.contains(Backends::VULKAN) && cpu.contains(Backends::GL), "llvmpipe, not lavapipe");
        assert_eq!(choose_backends(false, false, true, None, Backends::empty(), eframe::wgpu::Backends::empty(), |_| true), None);
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-startup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn isolated_settings_are_explicit_absolute_and_never_fall_back() {
        let default = std::env::temp_dir().join("ordinary-settings");
        assert_eq!(settings_directory(None, Some(default.clone())).unwrap(), Some(default.clone()));
        let selected = std::env::temp_dir().join("solidcraft-isolated-settings-fixture");
        assert_eq!(settings_directory(Some(selected.as_os_str()), Some(default.clone())).unwrap(), Some(selected));
        for raw in ["", "relative/settings", "../ordinary"] {
            assert!(settings_directory(Some(std::ffi::OsStr::new(raw)), Some(default.clone())).is_err());
        }
        let file = std::env::current_exe().unwrap();
        assert!(settings_directory(Some(file.as_os_str()), Some(default)).is_err());
    }

    // These restart contracts apply to Windows and macOS as well as Linux.
    #[test]
    fn graphics_restart_applies_saved_choices_and_reopens_saved_designs() {
        use std::ffi::OsStr;
        let program = std::env::current_exe().unwrap();
        let designs = vec!["/saved/first design.sc".to_string(), "/saved/second.sc".to_string()];
        let command = crate::graphics_restart_command(&program, &[], &designs);
        assert_eq!(command.get_program(), program.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), designs.iter().map(OsStr::new).collect::<Vec<_>>());
        let changes: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        for variable in [
            crate::startup::RETRY_BACKEND_ENV,
            crate::startup::FALLBACK_NOTE_ENV,
            crate::startup::RENDERER_ENV,
            crate::startup::SAFE_MODE_ENV,
            crate::device_lost::LOST_ENV,
            crate::device_lost::REASON_ENV,
        ] {
            assert_eq!(changes.get(OsStr::new(variable)), Some(&None), "{variable}");
        }
        assert!(!changes.contains_key(OsStr::new(crate::startup::WGPU_BACKEND_ENV)));
        assert!(!changes.contains_key(OsStr::new("SOLIDCRAFT_SETTINGS_DIR")));
        assert!(!changes.contains_key(OsStr::new("WGPU_ADAPTER_NAME")));
    }

    #[test]
    fn graphics_restart_uses_an_appimage_only_from_its_own_existing_mount() {
        use crate::graphics_restart_program;
        use std::ffi::OsStr;
        let root = std::env::temp_dir().join("solidcraft-graphics-restart-fixture");
        let appdir = root.join("mount");
        let mounted = appdir.join("usr").join("bin").join("solidcraft");
        let image = root.join("SolidCraft.AppImage");
        let outside = root.join("source").join("solidcraft");
        let looks_similar = root.join("mount-other").join("solidcraft");
        let exists = |path: &std::path::Path| path == image;
        assert_eq!(graphics_restart_program(Some(&mounted), Some(image.as_os_str()), Some(appdir.as_os_str()), exists), Some(image.clone()));
        for executable in [&outside, &looks_similar] {
            assert_eq!(
                graphics_restart_program(Some(executable), Some(image.as_os_str()), Some(appdir.as_os_str()), exists),
                Some(executable.to_path_buf()),
                "a launcher inherited from another process is ignored"
            );
        }
        for (appimage, directory) in [
            (None, Some(appdir.as_os_str())),
            (Some(image.as_os_str()), None),
            (Some(OsStr::new("")), Some(appdir.as_os_str())),
            (Some(image.as_os_str()), Some(OsStr::new(""))),
            (Some(OsStr::new("relative.AppImage")), Some(appdir.as_os_str())),
        ] {
            assert_eq!(graphics_restart_program(Some(&mounted), appimage, directory, exists), Some(mounted.clone()));
        }
        assert_eq!(
            graphics_restart_program(Some(&mounted), Some(image.as_os_str()), Some(appdir.as_os_str()), |_| false),
            Some(mounted.clone()),
            "a missing outer image cannot replace the running executable"
        );
        assert_eq!(graphics_restart_program(None, Some(image.as_os_str()), Some(appdir.as_os_str()), exists), None);
    }

    #[test]
    fn graphics_restart_keeps_launch_options_and_uses_the_current_saved_tabs() {
        let original = [
            "--safe-mode",
            "--control",
            "7878",
            "original design.sc",
            "--theme-file",
            "a theme.json",
            "--sample",
            "--max-file-size",
            "2GB",
            "other-original.sc",
            "--max-file-size=3GB",
        ];
        let args = crate::graphics_restart_args(original.into_iter().map(str::to_string));
        assert_eq!(args, ["--control", "7878", "--theme-file", "a theme.json", "--max-file-size", "2GB", "--max-file-size=3GB"]);
        let program = std::env::current_exe().unwrap();
        let designs = ["current saved.sc".to_string()];
        let command = crate::graphics_restart_command(&program, &args, &designs);
        assert_eq!(
            command.get_args().map(|value| value.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            ["--control", "7878", "--theme-file", "a theme.json", "--max-file-size", "2GB", "--max-file-size=3GB", "current saved.sc"]
        );
        for missing_value in ["--control", "--theme-file", "--max-file-size"] {
            assert!(crate::graphics_restart_args([missing_value.to_string()]).is_empty());
        }
    }

    #[test]
    fn startup_graphics_api_is_authoritative_over_stale_ui_preferences() {
        use serde_json::json;
        for choice in ["auto", "vulkan", "dx12", "metal", "gl", "dx12,vulkan"] {
            for safe in [false, true] {
                let mut app = crate::SolveApp::new(crate::Session::default(), crate::Services::default());
                let stored = json!({"preferences":{"graphics_backend":"gl"}}).to_string();
                let saved = json!({"backend":choice});
                crate::restore_preferences(&mut app, Some(&stored), Some(&saved), safe);
                assert_eq!(app.preferences.graphics_backend, choice);
            }
        }
        let mut app = crate::SolveApp::new(crate::Session::default(), crate::Services::default());
        app.preferences.graphics_backend = "vulkan".into();
        crate::restore_graphics_preferences(&mut app, None);
        assert_eq!(app.preferences.graphics_backend, "auto");
    }
    fn backend_settings(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn pending_backend(previous: &str) -> serde_json::Value {
        serde_json::json!({
            "backend": "vulkan",
            "backend_trial": {"requested": "vulkan", "previous": previous, "started": false},
            "msaa": 8,
            "adapter": "Saved GPU",
            "window_system": "x11",
            "future_setting": {"enabled": true}
        })
    }

    #[test]
    fn backend_start_marks_the_attempt_before_selection_and_keeps_it_after_a_presented_frame() {
        let dir = scratch("backend-confirm");
        let path = dir.join("graphics.json");
        let original = pending_backend("auto");
        std::fs::write(&path, original.to_string()).unwrap();

        let launch = prepare_backend_start(&path, true).unwrap();
        assert_eq!(launch.backend.as_deref(), Some("vulkan"));
        assert!(launch.note.is_none());
        let attempt = launch.attempt.unwrap();
        assert_eq!(attempt, BackendAttempt { requested: "vulkan".into(), previous: "auto".into() });
        let mut started = original.clone();
        started["backend_trial"]["started"] = serde_json::json!(true);
        assert_eq!(backend_settings(&path), started, "selection is returned only after the started marker is durable");

        confirm_backend_start(&path, &attempt).unwrap();
        let mut kept = original;
        kept.as_object_mut().unwrap().remove(BACKEND_TRIAL);
        assert_eq!(backend_settings(&path), kept, "confirmation changes no other graphics preferences");
        let next = prepare_backend_start(&path, true).unwrap();
        assert_eq!(next.backend.as_deref(), Some("vulkan"));
        assert!(next.attempt.is_none() && next.note.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn interrupted_backend_start_restores_the_exact_previous_choice_once() {
        solidcraft_ui_egui::i18n::install();
        let _language = solidcraft_ui_text::scope("en");
        let dir = scratch("backend-interrupted");
        let path = dir.join("graphics.json");
        for (previous, label) in [("auto", "Automatic"), (" dx12, vulkan ", " dx12, vulkan "), ("", "Automatic")] {
            let mut expected = pending_backend(previous);
            std::fs::write(&path, expected.to_string()).unwrap();
            assert!(prepare_backend_start(&path, true).unwrap().attempt.is_some());

            // A process crash leaves the started marker; the next normal launch rolls back.
            let recovered = prepare_backend_start(&path, true).unwrap();
            assert_eq!(recovered.backend.as_deref(), Some(previous));
            assert!(recovered.attempt.is_none());
            assert_eq!(
                recovered.note,
                Some(format!("The previous start with Vulkan did not open the window. SolidCraft restored {label}.")),
                "recovery describes both APIs using their displayed names"
            );
            expected["backend"] = serde_json::json!(previous);
            expected.as_object_mut().unwrap().remove(BACKEND_TRIAL);
            assert_eq!(backend_settings(&path), expected, "only the attempted API is reverted");
            assert!(prepare_backend_start(&path, true).unwrap().note.is_none(), "the notice is not repeated");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn safe_override_and_fallback_launches_leave_backend_attempts_untouched() {
        let dir = scratch("backend-bypass");
        let path = dir.join("graphics.json");
        assert_eq!(prepare_backend_start(&path, true).unwrap(), BackendLaunch::default());
        assert!(!path.exists(), "default startup does not create a settings file");
        for started in [false, true] {
            let mut saved = pending_backend("dx12");
            saved["backend_trial"]["started"] = serde_json::json!(started);
            let bytes = serde_json::to_vec_pretty(&saved).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            let launch = prepare_backend_start(&path, false).unwrap();
            assert_eq!(launch.backend.as_deref(), Some("vulkan"));
            assert!(launch.attempt.is_none() && launch.note.is_none());
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "even an interrupted attempt waits for a normal launch");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn backend_start_rollback_and_confirmation_cannot_overwrite_a_newer_choice() {
        let dir = scratch("backend-ownership");
        let path = dir.join("graphics.json");
        let saved = pending_backend("dx12");
        std::fs::write(&path, saved.to_string()).unwrap();
        let attempt = prepare_backend_start(&path, true).unwrap().attempt.unwrap();
        rollback_backend_start(&path, &attempt).unwrap();
        let mut reverted = saved.clone();
        reverted["backend"] = serde_json::json!("dx12");
        reverted.as_object_mut().unwrap().remove(BACKEND_TRIAL);
        assert_eq!(backend_settings(&path), reverted, "handled failure restores the last working preference");

        for newer in [
            serde_json::json!({"backend": "gl"}),
            serde_json::json!({"backend": "vulkan", "backend_trial": {"previous": "dx12", "requested": "vulkan", "started": false}}),
            serde_json::json!({"backend": "vulkan", "backend_trial": {"previous": "auto", "requested": "vulkan", "started": true}}),
            serde_json::json!({"backend": "gl", "backend_trial": {"previous": "dx12", "requested": "vulkan", "started": true}}),
        ] {
            let bytes = serde_json::to_vec_pretty(&newer).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            confirm_backend_start(&path, &attempt).unwrap();
            rollback_backend_start(&path, &attempt).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "stale launch state cannot consume another preference");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_backend_guards_use_the_previous_api_or_automatic_without_touching_other_settings() {
        let dir = scratch("backend-malformed");
        let path = dir.join("graphics.json");
        for (trial, expected_backend) in [
            (serde_json::json!(null), "auto"),
            (serde_json::json!(42), "auto"),
            (serde_json::json!({}), "auto"),
            (serde_json::json!({"requested": "vulkan", "previous": "dx12"}), "dx12"),
            (serde_json::json!({"requested": "vulkan", "previous": "dx12", "started": "yes"}), "dx12"),
            (serde_json::json!({"requested": "vulkan", "previous": "x".repeat(257), "started": false}), "auto"),
        ] {
            let mut saved = pending_backend("auto");
            saved["backend_trial"] = trial;
            std::fs::write(&path, saved.to_string()).unwrap();
            let launch = prepare_backend_start(&path, true).unwrap();
            assert_eq!(launch.backend.as_deref(), Some(expected_backend));
            assert!(launch.attempt.is_none());
            saved["backend"] = serde_json::json!(expected_backend);
            saved.as_object_mut().unwrap().remove(BACKEND_TRIAL);
            assert_eq!(backend_settings(&path), saved);
        }

        let mut saved = pending_backend("auto");
        saved["backend"] = serde_json::json!("gl");
        std::fs::write(&path, saved.to_string()).unwrap();
        let launch = prepare_backend_start(&path, true).unwrap();
        assert_eq!(launch.backend.as_deref(), Some("gl"), "a stale marker cannot replace a newer API");
        assert!(launch.attempt.is_none() && launch.note.is_none());
        saved.as_object_mut().unwrap().remove(BACKEND_TRIAL);
        assert_eq!(backend_settings(&path), saved);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_backend_attempt_is_never_returned_when_its_marker_cannot_be_saved() {
        for started in [false, true] {
            let mut saved = pending_backend("dx12");
            saved["backend_trial"]["started"] = serde_json::json!(started);
            let graphics = saved.as_object().unwrap().clone();
            let error = prepare_backend_launch(graphics, true, |_| Err("fixture: write refused".into())).unwrap_err();
            assert_eq!(error, "fixture: write refused", "no selected backend escapes a failed transition");
        }

        let dir = scratch("backend-unreadable");
        let path = dir.join("graphics.json");
        for damaged in ["{", "[]", "null"] {
            std::fs::write(&path, damaged).unwrap();
            assert!(prepare_backend_start(&path, true).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), damaged, "unreadable settings are never overwritten");
        }
        assert!(prepare_backend_start(&dir, true).is_err(), "a filesystem read failure is reported");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn intel_macs_start_without_indirect_call_validation() {
        use eframe::wgpu::InstanceFlags as F;
        let on = F::VALIDATION | F::VALIDATION_INDIRECT_CALL;
        assert_eq!(window_instance_flags(on, true, None), F::VALIDATION);
        assert_eq!(window_instance_flags(on, true, Some("  ")), F::VALIDATION);
        // The user's own choice stands, on or off.
        assert_eq!(window_instance_flags(on, true, Some("1")), on);
        assert_eq!(window_instance_flags(F::VALIDATION, true, Some("0")), F::VALIDATION);
        // Every other machine keeps what it had.
        assert_eq!(window_instance_flags(on, false, None), on);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn oclp_relaunch_needs_the_signal_and_no_compat_variable() {
        use super::compat_wanted;
        let probed = std::cell::Cell::new(false);
        let probe = |found: bool| {
            let probed = &probed;
            move || {
                probed.set(true);
                found
            }
        };
        // The re-launched process has the variable: no probe, no second re-launch.
        assert!(!compat_wanted(true, probe(true)));
        assert!(!probed.get());
        // The OCLP NVRAM key is the signal.
        assert!(compat_wanted(false, probe(true)));
        // No signal: an ordinary Mac starts as it is.
        assert!(!compat_wanted(false, probe(false)));
    }

    #[test]
    fn switches_read_from_the_environment() {
        assert!(env_on(Some("1")) && env_on(Some("yes")) && !env_on(Some("0")) && !env_on(Some(" ")) && !env_on(None) && !env_on(Some("false")));
        assert!(cpu_renderer_env(Some("cpu")) && cpu_renderer_env(Some("Software")) && !cpu_renderer_env(Some("gpu")) && !cpu_renderer_env(None));
    }

    #[test]
    fn settings_that_do_not_read_are_moved_aside_and_good_ones_stay() {
        let keys = ["solidcraft.prefs"];
        let d = scratch("check");
        // Good files (and an empty one) stay where they are.
        std::fs::write(d.join("app.ron"), r#"{"solidcraft.prefs": "{\"grid\":true}", "egui": "(anything)"}"#).unwrap();
        std::fs::write(d.join("graphics.json"), r#"{"msaa":4}"#).unwrap();
        assert!(check_settings(&d, &keys).is_empty());
        assert!(d.join("app.ron").is_file() && d.join("graphics.json").is_file());
        // Damaged RON, preferences that aren't JSON, a truncated graphics.json, non-UTF-8 bytes.
        for (name, text) in [
            ("app.ron", b"{\"solidcraft.prefs\": \"{\\\"gr".as_slice()),
            ("app.ron", br#"{"solidcraft.prefs": "not json"}"#.as_slice()),
            ("graphics.json", br#"{"msaa":"#.as_slice()),
            ("graphics.json", b"\xff\xfe\x00".as_slice()),
        ] {
            std::fs::write(d.join(name), text).unwrap();
            let notes = check_settings(&d, &keys);
            assert_eq!(notes.len(), 1, "{name}: {notes:?}");
            assert!(!d.join(name).exists(), "{name} moved");
            let kept: Vec<_> =
                std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("{name}.bad-"))).collect();
            assert!(kept.iter().any(|e| std::fs::read(e.path()).unwrap() == text), "{name}: the bad file is kept");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// #4164: two windows saving at once leave `app.ron` holding one whole save, never a short
    /// save with a longer one's tail behind it, and no temporary files. A window starting
    /// meanwhile reads one whole save too.
    #[test]
    fn settings_saved_at_once_from_two_windows_stay_whole() {
        let keys = ["solidcraft.prefs"];
        let d = scratch("concurrent");
        let path = d.join("app.ron");
        let save = |size: usize| {
            std::collections::HashMap::from([
                ("solidcraft.prefs".to_string(), format!("{{\"grid\":{size}}}")),
                ("window".to_string(), "x".repeat(size)),
            ])
        };
        let (long, short) = (save(200_000), save(10));
        let whole = |text: &str| ron::from_str::<std::collections::HashMap<String, String>>(text).is_ok_and(|kv| kv == long || kv == short);
        eframe::save_to_disk(&path, &short);
        let saving = std::sync::atomic::AtomicBool::new(true);
        let damaged = std::thread::scope(|s| {
            let reader = s.spawn(|| {
                let mut damaged = 0;
                while saving.load(std::sync::atomic::Ordering::Relaxed) {
                    damaged += usize::from(!std::fs::read_to_string(&path).is_ok_and(|t| whole(&t)));
                }
                damaged
            });
            let writers: Vec<_> =
                [&long, &short, &long, &short].into_iter().map(|kv| s.spawn(|| (0..40).for_each(|_| eframe::save_to_disk(&path, kv)))).collect();
            writers.into_iter().for_each(|w| w.join().unwrap());
            saving.store(false, std::sync::atomic::Ordering::Relaxed);
            reader.join().unwrap()
        });
        assert_eq!(damaged, 0, "every read while saving found one whole save");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(whole(&text), "one whole save: {} bytes", text.len());
        assert!(check_settings(&d, &keys).is_empty(), "the file reads");
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["app.ron"], "no temporary files are left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_settings_save_the_system_refuses_leaves_no_temporary_file() {
        let d = scratch("refused-save");
        let path = d.join("app.ron");
        // A folder with something in it can't be replaced by a file, so every rename fails.
        std::fs::create_dir_all(path.join("inside")).unwrap();
        eframe::save_to_disk(&path, &std::collections::HashMap::from([("k".to_string(), "v".to_string())]));
        assert!(path.join("inside").is_dir(), "what was there stays");
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["app.ron"], "no temporary files are left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn logic_only_ticks_and_extra_ui_passes_do_not_finish_startup() {
        use eframe::App as _;

        let d = scratch("logic-only");
        assert_eq!(begin(&d), 0);
        let path = d.join("graphics.json");
        std::fs::write(&path, pending_backend("auto").to_string()).unwrap();
        let attempt = prepare_backend_start(&path, true).unwrap().attempt.unwrap();
        let ctx = egui::Context::default();
        let solve = solidcraft_ui_egui::SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
        let mut app = crate::App {
            prefs_writer: crate::PrefsWriter::read(solve.prefs()),
            solve,
            setup: Default::default(),
            gpu_job: None,
            window_frames: 0,
            starting: Some(d.clone()),
            backend_attempt: Some((path.clone(), attempt)),
            backend_guard: None,
            safe_graphics_record: None,
            full_graphics_up: None,
            frames: 0,
            up: false,
            take_focus_unwanted: true,
            come_forward: crate::come_forward::ComeForward::default(),
            gpu: None,
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            space_mouse: Some(crate::space_mouse::SpaceMouse::inactive()),
            #[cfg(target_os = "macos")]
            native_menu: None,
            nav_bridge: crate::nav_bridge::NavBridge::inactive(),
            dialog_folder: crate::DialogFolder::default(),
            ctx: ctx.clone(),
            #[cfg(target_os = "windows")]
            window_move: crate::native_frame::WindowMove::default(),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let input = egui::RawInput::default();
        for _ in 0..3 {
            let _ = ctx.run_logic(&input, |ctx| app.logic(ctx, &mut frame));
            assert_eq!(app.frames, 0, "a logic-only update has not drawn a frame");
            assert_eq!(app.window_frames, 0, "logic alone cannot show or focus the starting window");
            assert!(app.starting.is_some());
            assert!(app.backend_attempt.is_some());
            assert!(backend_settings(&path).get(BACKEND_TRIAL).is_some());
            assert!(d.join(MARKER).exists(), "a launch without a drawn window still counts as failed");
        }

        // A staged startup can remain busy across control updates and painted starting pages.
        let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let setup_ready = ready.clone();
        app.setup = solidcraft_ui_egui::starting::Starting::default().until("", move |_, _| setup_ready.load(std::sync::atomic::Ordering::SeqCst));
        ctx.run_ui(input.clone(), |ui| {
            app.logic(ui.ctx(), &mut frame);
            app.ui(ui, &mut frame);
        })
        .drop_without_applying_deltas();
        assert_eq!(app.window_frames, 1);
        assert_eq!(app.frames, 0, "a starting page is not an app frame");
        assert!(app.backend_attempt.is_some());
        ready.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = ctx.run_logic(&input, |ctx| app.logic(ctx, &mut frame));
        assert!(app.setup.done(), "control updates can finish background setup");
        assert_eq!(app.frames, 0, "finishing setup does not present an app frame");
        assert!(d.join(MARKER).exists());

        // Layout may discard its first pass: two UI calls still make only one drawn frame.
        ctx.options_mut(|options| options.max_passes = 2.try_into().unwrap());
        let mut passes = 0;
        ctx.run_ui(input.clone(), |ui| {
            app.logic(ui.ctx(), &mut frame);
            app.ui(ui, &mut frame);
            passes += 1;
            ui.ctx().request_discard("exercise another layout pass during startup");
        })
        .drop_without_applying_deltas();
        assert_eq!(passes, 2);
        assert_eq!(app.frames, 1);
        assert!(d.join(MARKER).exists(), "the first frame must be presented before startup completes");
        assert!(app.backend_attempt.is_some());
        assert!(backend_settings(&path).get(BACKEND_TRIAL).is_some());

        for _ in 0..3 {
            let _ = ctx.run_logic(&input, |ctx| app.logic(ctx, &mut frame));
            assert_eq!(app.frames, 1);
            assert!(d.join(MARKER).exists());
        }
        ctx.run_ui(input, |ui| {
            app.logic(ui.ctx(), &mut frame);
            app.ui(ui, &mut frame);
        })
        .drop_without_applying_deltas();
        assert_eq!(app.frames, 2);
        assert!(app.starting.is_none());
        assert!(app.backend_attempt.is_none());
        assert!(backend_settings(&path).get(BACKEND_TRIAL).is_none());
        assert!(!d.join(MARKER).exists(), "the second drawn frame completes startup");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn launches_that_never_got_a_window_are_counted_until_one_does() {
        let d = scratch("marker");
        assert_eq!(begin(&d), 0);
        assert_eq!(begin(&d), 1);
        assert_eq!(begin(&d), FAILED_STARTS_FOR_SAFE_MODE);
        started(&d);
        assert_eq!(begin(&d), 0);
        // A damaged marker counts as none.
        std::fs::write(d.join(MARKER), "zz").unwrap();
        assert_eq!(begin(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Closing the window from the starting page twice is not two failed starts (#1210); a close
    /// after a lost device still counts toward safe mode.
    #[test]
    fn closing_while_starting_is_not_a_failed_start_unless_the_app_closed_itself() {
        let d = scratch("closed-while-starting");
        for _ in 0..FAILED_STARTS_FOR_SAFE_MODE + 1 {
            assert_eq!(begin(&d), 0, "a user's close never counts");
            closed_while_starting(&d, true);
        }
        assert_eq!(begin(&d), 0);
        closed_while_starting(&d, false);
        assert_eq!(begin(&d), 1, "the device was lost: that start failed");
        closed_while_starting(&d, false);
        assert_eq!(begin(&d), FAILED_STARTS_FOR_SAFE_MODE);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn windows_starts_vulkan_first_and_d3d12_only_when_vulkan_has_no_gpu() {
        use eframe::wgpu::Backends;
        let none = Backends::empty();
        let probe = |with: &'static [Backends]| {
            move |b: Backends| {
                assert!(b == Backends::DX12 || b == Backends::VULKAN || b == Backends::GL, "one backend at a time: {b:?}");
                with.contains(&b)
            }
        };
        // Vulkan works: D3D12 and OpenGL are never even asked about (one API's drivers loaded).
        let mut asked = Vec::new();
        let chosen = choose_backends(true, false, false, None, none, none, |b| {
            asked.push(b);
            b == Backends::VULKAN
        });
        assert_eq!((chosen, asked), (Some(Backends::VULKAN), vec![Backends::VULKAN]));
        assert_eq!(
            choose_backends(true, false, false, None, none, none, probe(&[Backends::DX12, Backends::VULKAN, Backends::GL])),
            Some(Backends::VULKAN)
        );
        // No Vulkan hardware adapter (older Intel, VMs, Remote Desktop): D3D12, else OpenGL, else
        // D3D12's software adapter (WARP).
        assert_eq!(choose_backends(true, false, false, None, none, none, probe(&[Backends::DX12, Backends::GL])), Some(Backends::DX12));
        assert_eq!(choose_backends(true, false, false, None, none, none, probe(&[Backends::GL])), Some(Backends::GL));
        assert_eq!(choose_backends(true, false, false, None, none, none, probe(&[])), Some(Backends::DX12));
        // `auto` and blanks are the default order; a chosen API always wins.
        assert_eq!(
            choose_backends(true, false, false, Some(" auto "), none, none, probe(&[Backends::VULKAN, Backends::DX12])),
            Some(Backends::VULKAN)
        );
        assert_eq!(choose_backends(true, false, false, Some(""), none, none, probe(&[Backends::GL])), Some(Backends::GL));
        assert_eq!(choose_backends(true, false, false, Some("dx12"), none, none, probe(&[Backends::VULKAN, Backends::DX12])), Some(Backends::DX12));
    }

    #[test]
    fn failed_windows_backends_fall_back_in_order_and_stop_after_software() {
        use eframe::wgpu::Backends as B;
        assert_eq!(
            fallback_backend(true, false, false, B::VULKAN, eframe::wgpu::Backends::empty()),
            Some("dx12"),
            "a Vulkan device that fails: D3D12"
        );
        assert_eq!(fallback_backend(true, false, false, B::DX12, eframe::wgpu::Backends::empty()), Some("gl"));
        assert_eq!(fallback_backend(true, false, false, B::GL, eframe::wgpu::Backends::empty()), Some("cpu"));
        assert_eq!(fallback_backend(true, false, true, B::VULKAN, eframe::wgpu::Backends::empty()), None);
        assert_eq!(fallback_backend(true, false, false, B::DX12 | B::VULKAN, eframe::wgpu::Backends::empty()), Some("gl"));
        assert_eq!(fallback_backend(false, false, false, B::VULKAN, eframe::wgpu::Backends::empty()), Some("cpu"));
        let note = fallback_note("", B::DX12, "vulkan", "egui_pipeline\n0x80070057");
        assert!(note.contains("egui_pipeline 0x80070057") && note.contains("vulkan") && !note.contains('\n'));
        let long = fallback_note(&"x".repeat(5000), B::VULKAN, "gl", &"y".repeat(5000));
        assert!(long.chars().count() <= 4096 && long.ends_with("trying gl."));
    }

    #[test]
    fn explicit_backend_failure_never_switches_api_or_retries_software() {
        use eframe::wgpu::Backends as B;
        for windows in [false, true] {
            for cpu in [false, true] {
                for backend in [B::DX12, B::VULKAN, B::GL, B::all()] {
                    assert_eq!(fallback_backend(windows, true, cpu, backend, eframe::wgpu::Backends::empty()), None);
                    assert_eq!(
                        choose_backends(windows, true, cpu, Some("vulkan"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| {
                            true
                        }),
                        None
                    );
                }
            }
        }
    }

    #[test]
    fn only_opengl_on_windows_gets_the_system_title_bar() {
        use eframe::wgpu::Backends;
        assert!(system_title_bar(true, Backends::GL));
        for b in [Backends::DX12, Backends::VULKAN, Backends::DX12 | Backends::GL, Backends::VULKAN | Backends::GL, Backends::all()] {
            assert!(!system_title_bar(true, b), "{b:?}");
        }
        for b in [Backends::GL, Backends::VULKAN, Backends::METAL] {
            assert!(!system_title_bar(false, b), "{b:?}");
        }
    }

    #[test]
    fn wgpu_backend_the_preference_and_the_software_renderer_override_the_order() {
        use eframe::wgpu::Backends;
        // WGPU_BACKEND: wgpu reads it itself.
        assert_eq!(choose_backends(true, true, false, Some("gl"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true), None);
        assert_eq!(choose_backends(true, true, true, None, eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true), None);
        // The software renderer (safe mode, the retry): WARP through D3D12 alone, never Vulkan.
        assert_eq!(
            choose_backends(true, false, true, Some("vulkan"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::DX12)
        );
        assert_eq!(choose_backends(false, false, true, None, eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true), None);
        // The preference, on any system.
        assert_eq!(
            choose_backends(true, false, false, Some("vulkan"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::VULKAN)
        );
        assert_eq!(
            choose_backends(true, false, false, Some("GL"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::GL)
        );
        assert_eq!(
            choose_backends(true, false, false, Some("dx12,vulkan"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::DX12 | Backends::VULKAN)
        );
        assert_eq!(
            choose_backends(false, false, false, Some("vulkan"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::VULKAN)
        );
        // An unknown preference is the default order.
        assert_eq!(
            choose_backends(true, false, false, Some("glide"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            Some(Backends::VULKAN)
        );
        // Elsewhere: wgpu's own choice.
        assert_eq!(choose_backends(false, false, false, None, eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true), None);
        assert_eq!(
            choose_backends(false, false, false, Some("auto"), eframe::wgpu::Backends::empty(), eframe::wgpu::Backends::empty(), |_| true),
            None
        );
    }

    #[test]
    fn an_amd_opengl_adapter_on_windows_is_not_usable_hardware() {
        use eframe::wgpu::{Backend, Backends};
        // The reporters' cards: the device starts, but the scene draws nothing (#11357,
        // #11834, #12994, #16453).
        for name in ["AMD Radeon HD 5450", "AMD Radeon HD 7500 Series", "AMD Radeon HD 5700 Series", "ATI Radeon HD 2600 XT"] {
            assert!(!counts_as_hardware(true, Backend::Gl, name), "{name}");
        }
        // Other backends draw fine, and other systems never had the fault.
        assert!(counts_as_hardware(true, Backend::Dx12, "AMD Radeon HD 5450"));
        assert!(counts_as_hardware(true, Backend::Vulkan, "AMD Radeon HD 5450"));
        assert!(counts_as_hardware(false, Backend::Gl, "AMD Radeon HD 5450"));
        // Non-AMD OpenGL adapters are untouched.
        assert!(counts_as_hardware(true, Backend::Gl, "Intel(R) HD Graphics 530"));
        assert!(counts_as_hardware(true, Backend::Gl, "NVIDIA Quadro P2200"));
        // The automatic order on such a machine ends at the software renderer: the only
        // hardware adapter it could enumerate is the AMD one on GL, the filter passes it over,
        // and no backend with a usable GPU is left. The probe models that machine: GL alone
        // enumerates hardware, and the filter is applied to it, so GL is probed but not taken.
        let probe = |b: Backends| b == Backends::GL && counts_as_hardware(true, Backend::Gl, "AMD Radeon HD 5450");
        assert_eq!(choose_backends(true, false, false, None, Backends::empty(), Backends::empty(), probe), Some(Backends::DX12));
        // Without the filter the same machine would have started on GL and drawn nothing.
        let unfiltered = |b: Backends| b == Backends::GL;
        assert_eq!(choose_backends(true, false, false, None, Backends::empty(), Backends::empty(), unfiltered), Some(Backends::GL));
        // The name-based filter excludes every AMD adapter on GL, modern cards included.
        // That is safe: a machine where GL alone enumerates hardware has no working D3D12 or
        // Vulkan device, which is exactly the TeraScale-class situation where the GL scene
        // draws nothing. A modern AMD card's D3D12 or Vulkan wins first, so GL is never asked
        // for; the filter only changes anything on machines where GL was the broken last
        // resort. The old start is kept behind WGPU_BACKEND and the backend preference.
        assert!(!counts_as_hardware(true, Backend::Gl, "AMD Radeon RX 6800"));
        let amd_gl_only = |b: Backends| b == Backends::GL && counts_as_hardware(true, Backend::Gl, "AMD Radeon RX 6800");
        assert_eq!(choose_backends(true, false, false, None, Backends::empty(), Backends::empty(), amd_gl_only), Some(Backends::DX12));
    }

    #[test]
    fn the_message_names_the_settings_folder_and_safe_mode() {
        let m = no_window_message("no adapter", Some(Path::new("/x/SolidCraft")));
        assert!(m.contains("no adapter") && m.contains("/x/SolidCraft") && m.contains("--safe-mode"));
    }

    #[test]
    fn the_software_adapter_is_preferred_and_its_absence_is_an_error_not_a_panic() {
        assert!(pick_cpu_adapter(&[], None).unwrap_err().contains("none"));
        // The adapters the software renderer chooses among: it starts D3D12 alone on Windows
        // (WARP), not every backend, so the test doesn't load the OpenGL driver either (#1156).
        let options = crate::wgpu_options(true, None, SkippedBackends::default(), None);
        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_setup else { panic!("the software renderer makes its own device") };
        let backends = setup.instance_descriptor.backends;
        if cfg!(target_os = "windows") && std::env::var_os(WGPU_BACKEND_ENV).is_none() {
            assert_eq!(backends, eframe::wgpu::Backends::DX12);
        }
        let mut desc = solidcraft_viewport_gpu::instance_descriptor_from_env();
        desc.backends = backends;
        let instance = eframe::wgpu::Instance::new(desc);
        let adapters = pollster::block_on(instance.enumerate_adapters(backends));
        if adapters.is_empty() {
            return;
        }
        let picked = pick_cpu_adapter(&adapters, None).unwrap();
        if adapters.iter().any(|a| a.get_info().device_type == eframe::wgpu::DeviceType::Cpu) {
            assert_eq!(picked.get_info().device_type, eframe::wgpu::DeviceType::Cpu);
        }
    }
}
