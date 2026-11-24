//! Anonymous usage statistics (docs/privacy.md): how many installations there are, how many are
//! used each day, for how long, and how often each command runs.
//!
//! A report is one day of one installation and holds exactly the fields of [`Report`]: a random
//! installation id, the version, operating system, architecture and platform, the minutes with
//! input in the window, and per source (`ui`, `agent`, `script`) how often each command changed the
//! design (`operations`) and each tool or dialog was started (`tools`), by registered command id
//! (`solidcraft_engine::usage` says what counts), how often each failed by kind of error, how long
//! each took and how long each tool or dialog stayed open, all in a few fixed buckets. Never
//! parameters, error messages, design names, file paths, geometry or plugin data.
//!
//! - **Off** with the `usage_statistics` preference, `DO_NOT_TRACK=1` or
//!   `SOLIDCRAFT_NO_TELEMETRY=1` ([`ENV_OFF`]), or the organization's `UsageStatistics` policy
//!   (`policy`). Off, nothing is counted, kept or sent, and what was kept is deleted.
//! - **Nothing before the notice.** Nothing is counted until the notice that says so has been
//!   answered with OK ([`Store::notice_ok`]); until then the counter doesn't run at all.
//! - **Sending** happens in the background through the host's HTTP ([`crate::Services::usage_http`]):
//!   a finished day is posted once, to [`ENDPOINT`] only. The window never waits on it. A day
//!   that could not be sent is kept for at most [`KEEP_DAYS`] days, and the app tries again once
//!   per start and then once every [`RETRY_HOURS`] hours, never in a loop.
//! - The whole module is compiled only with the `usage-stats` cargo feature; a build without it
//!   has no counting, no network code and no endpoint.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::SolveApp;
use crate::agent_chat::backend::HttpRequest;

/// Where reports go: the one address the app ever posts them to.
pub const ENDPOINT: &str = "https://metrics.solidcraft.io/v1/report";

/// Where Delete my data goes: this followed by the installation id, sent as `DELETE`.
pub const INSTALL_ENDPOINT: &str = "https://metrics.solidcraft.io/v1/install/";

/// The most deletions waiting to be sent.
pub const MAX_FORGET: usize = 10;

/// The only addresses the host's HTTP for usage statistics may reach: the report endpoint, and
/// an installation's delete address.
pub fn allowed_url(url: &str) -> bool {
    url == ENDPOINT || url.strip_prefix(INSTALL_ENDPOINT).is_some_and(valid_id)
}

/// The version of the report's fields ([`Report::v`]).
pub const SCHEMA: u32 = 1;

/// The host's storage key for [`Store`] (next to `prefs::STORAGE_KEY`).
pub const STORAGE_KEY: &str = "usage_statistics";

/// Environment variables that turn usage statistics off: set to anything but empty, `0` or
/// `false`.
pub const ENV_OFF: [&str; 2] = ["DO_NOT_TRACK", "SOLIDCRAFT_NO_TELEMETRY"];

/// A day not sent is dropped after this many days.
pub const KEEP_DAYS: i64 = 7;

/// After a failed try, the next one waits this long (a start always tries once).
pub const RETRY_HOURS: f64 = 24.0;

/// The first try waits this long after start, so it never competes with opening the window.
pub const FIRST_TRY_SECONDS: f64 = 60.0;

/// The most commands one day keeps per bucket and source; there are fewer registered commands.
pub const MAX_COMMANDS: usize = 2000;

/// The most kinds of error one command keeps per day.
pub const MAX_KINDS: usize = 200;

/// Counts by source (`ui`, `agent`, `script`), then by command id.
pub type Counts = BTreeMap<String, BTreeMap<String, u32>>;

/// Failures by source, then command id, then kind of error.
pub type Failures = BTreeMap<String, BTreeMap<String, BTreeMap<String, u32>>>;

/// Counts by command id, then time bucket.
pub type Buckets = BTreeMap<String, BTreeMap<String, u32>>;

/// Tool times by tool id, then `applied` or `cancelled`, then time bucket.
pub type Dialogs = BTreeMap<String, BTreeMap<String, BTreeMap<String, u32>>>;

/// Minutes in a day: the most active minutes one day can have.
const DAY_MINUTES: u32 = 24 * 60;

/// One day of one installation, as it is sent: these fields and nothing else.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub v: u32,
    /// [`Store::install_id`].
    pub install_id: String,
    /// The UTC day, `YYYY-MM-DD`.
    pub day: String,
    /// The app's version.
    pub version: String,
    /// `windows`, `macos`, `linux`, `freebsd`, `web` or `other`.
    pub os: String,
    /// `x86_64`, `aarch64`, `x86`, `wasm32` or `other`.
    pub arch: String,
    /// `desktop` or `web`.
    pub platform: String,
    /// Minutes in which there was input in the window.
    pub active_minutes: u32,
    /// Commands that changed the design (made an undo step), by source and id.
    pub operations: Counts,
    /// Tools and dialogs started, by source and id.
    pub tools: Counts,
    /// Commands that failed, by source, id and kind of error.
    pub failed: Failures,
    /// How long operations and failures took (`solidcraft_engine::usage::compute_bucket`).
    pub compute_ms: Buckets,
    /// How long tools and dialogs stayed open, until applied or cancelled ([`dialog_bucket`]).
    pub dialog_s: Dialogs,
}

/// What one day counted so far.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    pub active_minutes: u32,
    pub operations: Counts,
    pub tools: Counts,
    /// Commands that failed, by source, id and kind of error.
    pub failed: Failures,
    /// How long operations and failures took (`solidcraft_engine::usage::compute_bucket`).
    pub compute_ms: Buckets,
    /// How long tools and dialogs stayed open, until applied or cancelled ([`dialog_bucket`]).
    pub dialog_s: Dialogs,
}

/// What the app keeps between starts (the host stores it under [`STORAGE_KEY`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    /// Random, made on first use and by Reset ID; never derived from the computer.
    pub install_id: String,
    /// The person chose OK on the notice (or switched sharing on in Preferences › Privacy): only
    /// from then on is anything counted or sent. Until then the notice shows at every start.
    /// (0.46 kept `notice_shown` and `notice_dismissed` instead, which recorded only that its
    /// notice had shown or been closed; they are not read, so those installations see the new
    /// notice and count nothing until OK.)
    pub notice_ok: bool,
    /// Days counted and not sent yet, by `YYYY-MM-DD` (UTC).
    pub days: BTreeMap<String, Day>,
    /// Earlier installation ids whose reports the server is still to delete (Delete my data).
    pub forget: Vec<String>,
}

impl Store {
    /// The stored text as a store; anything unreadable is a new one.
    pub fn from_stored(text: Option<&str>) -> Store {
        let mut store: Store = text.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default();
        if !valid_id(&store.install_id) {
            store.install_id.clear();
        }
        store.days.retain(|day, _| day_number(day).is_some());
        store.forget.retain(|id| valid_id(id));
        store.forget.truncate(MAX_FORGET);
        store
    }

    /// The text the host stores.
    pub fn to_stored(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Why usage statistics are off, if they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Off {
    /// The organization's policy.
    Policy,
    /// [`ENV_OFF`].
    Environment,
    /// The person's preference.
    Preference,
}

/// The app's usage statistics: what is kept, and the report on its way.
#[derive(Default)]
pub struct UsageStats {
    pub store: Store,
    /// The host keeps this app's usage statistics between runs (`SolveApp::load_usage_stats`).
    /// Until then (tests, scenarios, an app that keeps nothing) the notice never shows and nothing
    /// is counted.
    pub(crate) hosted: bool,
    /// Preferences › Privacy shows the next report.
    pub show_report: bool,
    /// The notice was closed with its × this run: it shows again at the next start.
    notice_hidden: bool,
    /// [`ENV_OFF`] said off when the app started.
    pub env_off: bool,
    /// The minute (since 1970) of the last input counted.
    last_minute: Option<i64>,
    /// The day posted and the answer to come.
    sending: Option<(String, crate::tickets::Call)>,
    /// When the last try started (seconds on the app's clock); None: none since start.
    last_try: Option<f64>,
    /// When this run's clock started.
    started: Option<f64>,
    /// A tool or dialog the person started, until it is applied or cancelled.
    open_tool: Option<OpenTool>,
    /// The deletion on its way: the id and the answer to come.
    deleting: Option<(String, crate::tickets::Call)>,
    /// When the last deletion was tried (the app's clock); None: none since the last request.
    last_delete_try: Option<f64>,
    /// A tool just started (`SolveApp::start`), stamped at the next tick.
    pub(crate) started_tool: Option<&'static str>,
}

/// A tool or dialog being timed.
#[derive(Clone, Copy, Debug)]
struct OpenTool {
    id: &'static str,
    /// When it started, on the app's clock.
    since: f64,
    /// It has shown (a tool or a dialog was open on a tick).
    shown: bool,
    /// An operation was counted while it was open: it was applied.
    applied: bool,
}

/// The bucket of a tool's or dialog's time open: `0-5s`, `5-30s`, `30s-2m` or `2m+`.
pub fn dialog_bucket(seconds: f64) -> &'static str {
    match seconds {
        s if s < 5.0 => "0-5s",
        s if s < 30.0 => "5-30s",
        s if s < 120.0 => "30s-2m",
        _ => "2m+",
    }
}

impl UsageStats {
    /// What the host stored, with the environment read now.
    pub fn new(stored: Option<&str>) -> UsageStats {
        UsageStats { store: Store::from_stored(stored), env_off: env_off(|name| std::env::var(name).ok()), ..UsageStats::default() }
    }
}

/// Whether the environment turns usage statistics off; `get` reads a variable.
pub fn env_off(get: impl Fn(&str) -> Option<String>) -> bool {
    ENV_OFF.iter().any(|name| get(name).is_some_and(|value| !matches!(value.trim().to_ascii_lowercase().as_str(), "" | "0" | "false")))
}

/// Why usage statistics are off, or None when they are on.
pub fn off(app: &SolveApp) -> Option<Off> {
    if !crate::policy::usage_statistics_allowed(app) {
        Some(Off::Policy)
    } else if app.usage.env_off {
        Some(Off::Environment)
    } else if !app.preferences.usage_statistics {
        Some(Off::Preference)
    } else {
        None
    }
}

/// Usage statistics are on and the person has been told: count and send.
pub fn active(app: &SolveApp) -> bool {
    app.usage.hosted && off(app).is_none() && app.usage.store.notice_ok
}

/// A new random installation id (a version 4 UUID). None when the system has no randomness.
fn new_id() -> Option<String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).ok()?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
    Some(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

/// A version 4 UUID in lower case.
fn valid_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|g| g.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        && id.as_bytes()[14] == b'4'
}

/// Reset ID: a new installation id, and what was kept under the old one is deleted.
pub fn reset_id(app: &mut SolveApp) {
    app.usage.store.install_id = new_id().unwrap_or_default();
    app.usage.store.days.clear();
}

/// Delete my data: the server is asked to delete everything it has for this installation id
/// (sent in the background, retried like a report until the server answers), and a new id is
/// used from now on. False when there is nothing to delete or the organization or the environment
/// turned statistics off (then nothing is sent at all).
pub fn delete_my_data(app: &mut SolveApp) -> bool {
    if matches!(off(app), Some(Off::Policy | Off::Environment)) {
        return false;
    }
    let old = std::mem::take(&mut app.usage.store.install_id);
    if valid_id(&old) && !app.usage.store.forget.contains(&old) {
        app.usage.store.forget.push(old);
        let extra = app.usage.store.forget.len().saturating_sub(MAX_FORGET);
        app.usage.store.forget.drain(..extra);
    }
    reset_id(app);
    app.usage.last_delete_try = None;
    !app.usage.store.forget.is_empty()
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date; None when it isn't one.
fn day_number(day: &str) -> Option<i64> {
    let mut parts = day.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (date(days) == day).then_some(days)
}

/// The `YYYY-MM-DD` date of `days` since 1970-01-01.
fn date(days: i64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Seconds since 1970 now.
fn unix_seconds() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// This build's operating system, architecture and platform.
fn system() -> (&'static str, &'static str, &'static str) {
    let os = if cfg!(target_arch = "wasm32") {
        "web"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        "other"
    };
    let arch = match std::env::consts::ARCH {
        arch @ ("x86_64" | "aarch64" | "x86" | "wasm32") => arch,
        _ => "other",
    };
    (os, arch, if cfg!(target_arch = "wasm32") { "web" } else { "desktop" })
}

/// The report of `day` as it would be sent.
pub fn report(store: &Store, day: &str, counted: &Day) -> Report {
    let (os, arch, platform) = system();
    Report {
        v: SCHEMA,
        install_id: store.install_id.clone(),
        day: day.to_string(),
        version: crate::update::current_version().to_string(),
        os: os.into(),
        arch: arch.into(),
        platform: platform.into(),
        active_minutes: counted.active_minutes.min(DAY_MINUTES),
        operations: counted.operations.clone(),
        tools: counted.tools.clone(),
        failed: counted.failed.clone(),
        compute_ms: counted.compute_ms.clone(),
        dialog_s: counted.dialog_s.clone(),
    }
}

/// Show what's sent: the exact JSON of the next report (the oldest day kept, else today so far),
/// or None while usage statistics are off.
pub fn next_report_json(app: &SolveApp) -> Option<String> {
    off(app).is_none().then_some(())?;
    let today = date(unix_seconds().div_euclid(86_400));
    let (day, counted) = app.usage.store.days.iter().next().map_or((today, Day::default()), |(day, counted)| (day.clone(), counted.clone()));
    let mut store = app.usage.store.clone();
    if store.install_id.is_empty() {
        store.install_id = "(made when the first report is sent)".into();
    }
    serde_json::to_string_pretty(&report(&store, &day, &counted)).ok()
}

/// Whether there was input in the window this frame: a key, text, the pointer or a touch.
fn had_input(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.focused
            && i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Touch { .. }
                        | egui::Event::Zoom(_)
                )
            })
    })
}

/// Once a frame: count, keep and send while usage statistics are on; while they are off,
/// nothing is counted and what was kept is deleted.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let input = had_input(ctx);
    tick_at(app, unix_seconds(), now, input);
}

/// [`tick`] at `unix` seconds since 1970, `now` on the app's clock, with or without input.
pub fn tick_at(app: &mut SolveApp, unix: i64, now: f64, input: bool) {
    let on = active(app);
    app.session.count_usage = on;
    // The counts are the process's: only the app that keeps them takes them, so another app in
    // the same process (a test's) never takes this one's.
    if !app.usage.hosted {
        return;
    }
    forget(app, now);
    let counted = solidcraft_engine::usage::take();
    if !on {
        // Off, or no OK yet: nothing is kept to send later.
        app.usage.store.days.clear();
        app.usage.sending = None;
        return;
    }
    if app.usage.store.install_id.is_empty() {
        let Some(id) = new_id() else { return };
        app.usage.store.install_id = id;
    }
    let today_number = unix.div_euclid(86_400);
    let today = date(today_number);
    // A dialog applying off the window thread is still open until its result is in.
    let tool_open = app.tool.is_some() || app.dialog.is_some() || app.applying.is_some();
    let operations = counted.counts.keys().any(|(bucket, ..)| *bucket == solidcraft_engine::usage::Bucket::Operations);
    let timed = time_tools(&mut app.usage, now, tool_open, operations);
    let day = app.usage.store.days.entry(today.clone()).or_default();
    for (tool, outcome, bucket) in timed {
        let count = day.dialog_s.entry(tool.to_string()).or_default().entry(outcome.to_string()).or_default().entry(bucket.to_string()).or_default();
        *count = count.saturating_add(1);
    }
    for ((source, id, kind), n) in counted.failures {
        let ids = day.failed.entry(source.name().to_string()).or_default();
        if ids.len() < MAX_COMMANDS || ids.contains_key(id) {
            let kinds = ids.entry(id.to_string()).or_default();
            if kinds.len() < MAX_KINDS || kinds.contains_key(&kind) {
                let count = kinds.entry(kind).or_default();
                *count = count.saturating_add(n);
            }
        }
    }
    for ((id, bucket), n) in counted.compute {
        if day.compute_ms.len() < MAX_COMMANDS || day.compute_ms.contains_key(id) {
            let count = day.compute_ms.entry(id.to_string()).or_default().entry(bucket.to_string()).or_default();
            *count = count.saturating_add(n);
        }
    }
    for ((bucket, source, id), n) in counted.counts {
        let counts = match bucket {
            solidcraft_engine::usage::Bucket::Operations => &mut day.operations,
            solidcraft_engine::usage::Bucket::Tools => &mut day.tools,
        };
        let ids = counts.entry(source.name().to_string()).or_default();
        if ids.len() < MAX_COMMANDS || ids.contains_key(id) {
            let count = ids.entry(id.to_string()).or_default();
            *count = count.saturating_add(n);
        }
    }
    let minute = unix.div_euclid(60);
    if input && app.usage.last_minute != Some(minute) {
        app.usage.last_minute = Some(minute);
        day.active_minutes = (day.active_minutes + 1).min(DAY_MINUTES);
    }
    // A day not sent in time is dropped, and so is one from the future (a clock set back).
    app.usage.store.days.retain(|day, _| day_number(day).is_some_and(|n| n <= today_number && today_number - n < KEEP_DAYS));
    send(app, &today, now);
}

/// Time the tool or dialog the person started: it shows on a tick, and when it is gone it was
/// applied (an operation was counted meanwhile) or cancelled. Its `(tool, outcome, bucket)` once
/// finished; a start that showed nothing (a command that just ran) is not timed.
fn time_tools(usage: &mut UsageStats, now: f64, open: bool, operations: bool) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut done = Vec::new();
    let mut finish = |tool: OpenTool| {
        if tool.shown {
            done.push((tool.id, if tool.applied { "applied" } else { "cancelled" }, dialog_bucket(now - tool.since)));
        }
    };
    if let Some(started) = usage.started_tool.take() {
        // Another tool replaces the one open: that one ends here.
        if let Some(tool) = usage.open_tool.take() {
            finish(tool);
        }
        usage.open_tool = Some(OpenTool { id: started, since: now, shown: false, applied: false });
    }
    if let Some(tool) = usage.open_tool.as_mut() {
        tool.applied |= operations && tool.shown;
        if open {
            tool.shown = true;
        } else if let Some(tool) = usage.open_tool.take() {
            finish(tool);
        }
    }
    done
}

/// Send the deletions asked for (Delete my data), one at a time, whether or not statistics are on;
/// none when the organization or the environment turned them off. A deletion the server didn't
/// take is tried again an hour later, never in a loop.
fn forget(app: &mut SolveApp, now: f64) {
    if matches!(off(app), Some(Off::Policy | Off::Environment)) {
        app.usage.store.forget.clear();
        app.usage.deleting = None;
        return;
    }
    if let Some((id, call)) = &app.usage.deleting {
        let Some(reply) = call.poll() else { return };
        let id = id.clone();
        app.usage.deleting = None;
        match reply {
            Ok((status, _)) if done(status) => {
                app.usage.store.forget.retain(|old| *old != id);
                // The next one waiting goes at once.
                app.usage.last_delete_try = None;
                if app.usage.store.forget.is_empty() {
                    let text = app.tr("prefs.privacy.delete_done").to_string();
                    app.set_status(text, false);
                }
            }
            Ok((status, _)) => log::info!("usage statistics: delete answered {status}; it waits"),
            Err(why) => log::info!("usage statistics: delete not sent ({why}); it waits"),
        }
        return;
    }
    if app.usage.last_delete_try.is_some_and(|at| now - at < 3600.0) || !may_send() {
        return;
    }
    let Some(id) = app.usage.store.forget.first().cloned() else { return };
    let Some(http) = app.services.usage_http.clone() else { return };
    let request = HttpRequest { method: "DELETE", url: format!("{INSTALL_ENDPOINT}{id}"), headers: Vec::new(), body: None, timeout_s: 30 };
    app.usage.last_delete_try = Some(now);
    app.usage.deleting = Some((id, crate::tickets::Call::start(&http, request)));
}

/// Reports go only from release builds (`update::release_build`): a development build counts but
/// never sends.
fn may_send() -> bool {
    cfg!(test) || crate::update::release_build()
}

/// Post the oldest finished day, or take the answer to the one on its way.
fn send(app: &mut SolveApp, today: &str, now: f64) {
    if let Some((day, call)) = &app.usage.sending {
        let Some(reply) = call.poll() else { return };
        let day = day.clone();
        app.usage.sending = None;
        match reply {
            // Received, or refused for good (a malformed report is never accepted later): the
            // next finished day goes at once.
            Ok((status, _)) if done(status) => {
                app.usage.store.days.remove(&day);
            }
            Ok((status, _)) => {
                log::info!("usage statistics: the server answered {status}; the report waits");
                return;
            }
            Err(why) => {
                log::info!("usage statistics: not sent ({why}); the report waits");
                return;
            }
        }
    } else {
        let started = *app.usage.started.get_or_insert(now);
        let due = match app.usage.last_try {
            None => now - started >= FIRST_TRY_SECONDS,
            Some(at) => now - at >= RETRY_HOURS * 3600.0,
        };
        if !due {
            return;
        }
    }
    if !may_send() {
        return;
    }
    let Some(http) = app.services.usage_http.clone() else { return };
    let Some((day, counted)) = app.usage.store.days.iter().find(|(day, _)| day.as_str() < today) else { return };
    let Ok(body) = serde_json::to_vec(&report(&app.usage.store, day, counted)) else { return };
    let request = HttpRequest {
        method: "POST",
        url: ENDPOINT.into(),
        // Plain text, so a browser posts it without asking the server first (no CORS preflight).
        headers: vec![("content-type".into(), "text/plain;charset=UTF-8".into())],
        body: Some(body),
        timeout_s: 30,
    };
    app.usage.last_try = Some(now);
    app.usage.sending = Some((day.clone(), crate::tickets::Call::start(&http, request)));
}

/// The server took the report (2xx) or will never take it (4xx other than 429).
fn done(status: u16) -> bool {
    (200..300).contains(&status) || ((400..500).contains(&status) && status != 429)
}

/// What the notice's buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    /// Sharing is fine: counting starts now.
    Ok,
    /// Preferences › Privacy opens, where the switch is. No answer: the notice stays.
    Settings,
    /// The ×: hidden until the next start, still no answer, nothing counted.
    Close,
}

/// The person agreed to sharing (OK on the notice, or the switch turned on in Preferences ›
/// Privacy): counting starts.
pub fn agree(app: &mut SolveApp) {
    app.usage.store.notice_ok = true;
    app.preferences.usage_statistics = true;
}

/// A button of the notice was pressed.
pub fn act(app: &mut SolveApp, action: NoticeAction) {
    match action {
        NoticeAction::Ok => agree(app),
        NoticeAction::Settings => crate::prefs::open_at(app, crate::prefs::PAGE_PRIVACY),
        NoticeAction::Close => app.usage.notice_hidden = true,
    }
}

/// The notice as one of the shared toasts in `free`; its rect when shown.
pub fn notice_toast(app: &mut SolveApp, ui: &egui::Ui, free: egui::Rect) -> Option<egui::Rect> {
    if !notice_pending(app) {
        return None;
    }
    use crate::ui_kit::toast::Action;
    const ACTIONS: [NoticeAction; 2] = [NoticeAction::Settings, NoticeAction::Ok];
    let labels = [app.tr("usage.settings").to_string(), app.tr("usage.ok").to_string()];
    let actions = [(labels[0].as_str(), Action::Secondary), (labels[1].as_str(), Action::Primary)];
    // No countdown: it stays until OK, or until sharing is turned off.
    let extras = crate::ui_kit::toast::Extras { close: Some("toast:usage_stats:close"), ..Default::default() };
    let text = app.tr("usage.notice").to_string();
    let toast = crate::viewport::paint_toast_ext(ui, free, text, crate::ui_kit::toast::Lead::Dot(false), &actions, extras)?;
    crate::scenario::publish_rect("toast:usage_stats", toast.rect);
    for (index, button) in toast.buttons.iter().enumerate() {
        crate::scenario::publish_rect(&format!("toast:usage_stats:button:{index}"), *button);
    }
    let action = if toast.closed { Some(NoticeAction::Close) } else { toast.clicked.and_then(|i| ACTIONS.get(i).copied()) };
    if let Some(action) = action {
        act(app, action);
    }
    Some(toast.rect)
}

/// Whether the notice shows: at every start until the person chooses OK, or turns sharing off
/// (in Preferences, by policy or by the environment). Closed with ×, it is back at the next start.
pub fn notice_pending(app: &SolveApp) -> bool {
    app.usage.hosted && off(app).is_none() && !app.usage.store.notice_ok && !app.usage.notice_hidden
}

#[cfg(test)]
#[path = "usage_stats_tests.rs"]
mod tests;
