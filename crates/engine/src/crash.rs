//! Crash reports: enough to reproduce a crash and fix it, without slowing the app down or
//! filling the disk (docs/crash-reports.md).
//!
//! - **Panics** are caught by a hook. One inside a command becomes a report with the design as
//!   it was before the command and the command itself, so `solidcraft-cli run replay.json --in
//!   design.sc` reproduces it. Panics the third-party kernel raises (and
//!   `solidcraft_kernel::guard` turns into errors) are reported the same way, marked recovered.
//! - **Crashes no hook sees** (an abort, a stack overflow, a segfault, the process being killed)
//!   are found at the next start: every running app keeps a small, locked command journal; a
//!   journal nobody holds any more belongs to an app that died. Its report carries the design
//!   from the last autosave (or the saved file) and the commands run since, ending with the one
//!   in flight.
//! - **Cost**: nothing until [`install`] is called (tests, the CLI and the web build never do);
//!   after that, two small unbuffered appends per top-level command of the app's own design, and
//!   work only when something panics.
//! - **Bounded**: the same crash (its signature: place + message, digits ignored) is counted, not
//!   written again; at most [`MAX_NEW_PER_SESSION`] new reports per run, [`MAX_REPORTS`]
//!   reports, [`MAX_TOTAL_BYTES`] on disk, nothing older than [`MAX_AGE_DAYS`] days.
//!
//! - **Startup steps** ([`phase`]): the journal keeps the step the app is in while it gets its
//!   window and graphics device up, so a crash inside a graphics driver still says where it
//!   happened, and the renderer ([`set_environment`]) once it is known.
//! - **Native crashes** (Windows, macOS): the app's exception filter or signal handler writes what
//!   it caught (exception code or signal, faulting module and offset, on macOS a walk up the stack)
//!   and on Windows a minidump next to the journal ([`native_paths`]); the next start puts them
//!   into the unclean exit's report.
//!
//! Reports are folders `crashes/<time>-<signature>/` holding `report.json`, and when known
//! `design.sc` (`design.solidcraft` in reports from earlier releases), `replay.json` and `minidump.dmp`. Nothing is ever sent anywhere.

use std::backtrace::Backtrace;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};
use solidcraft_doc::{Document, drawing};

use crate::Session;

/// Reports kept (the oldest, and recovered kernel panics, go first).
pub const MAX_REPORTS: usize = 20;
/// Bytes all reports may take together.
pub const MAX_TOTAL_BYTES: u64 = 50 << 20;
/// Reports older than this are deleted.
pub const MAX_AGE_DAYS: u64 = 90;
/// New reports one run of the app may write (repeats are only counted).
pub const MAX_NEW_PER_SESSION: usize = 5;
/// A design bigger than this is not copied into a report.
pub const MAX_DESIGN_BYTES: usize = 10 << 20;
/// A journal stops recording commands at this size (until the next autosave restarts it).
const MAX_JOURNAL_BYTES: u64 = 512 << 10;
/// Command parameters recorded per command.
const MAX_PARAMS_BYTES: usize = 4 << 10;
/// Recent commands shown in a report.
const RECENT: usize = 32;
const MAX_FRAMES: usize = 60;
const MAX_MESSAGE: usize = 2000;
/// The report format.
pub const FORMAT: &str = "solidcraft-crash/1";

/// What the app is (shown in every report).
#[derive(Clone, Debug, Default)]
pub struct Build {
    pub version: &'static str,
    pub commit: &'static str,
    pub built: &'static str,
}

/// One report, as listed.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Summary {
    pub id: String,
    /// `command_panic`, `panic`, `kernel_panic_recovered` or `unclean_exit`.
    pub kind: String,
    /// When the report was first recorded (UTC). Unclean exits are recorded at a later launch,
    /// which may be days after the session ended, rather than at the exact exit time.
    pub time: String,
    pub message: String,
    /// The command running when it happened, if one was.
    pub command: Option<String>,
    /// Times it happened.
    pub count: u64,
    pub version: String,
    /// The report's folder.
    pub path: String,
    /// `replay.json` and the design (`design.sc`) are there: the crash can be replayed.
    pub replayable: bool,
}

struct State {
    dir: PathBuf,
    build: Build,
    /// Renderer, GPU and the like (set by the UI).
    environment: Mutex<String>,
    journal: Mutex<Journal>,
    /// Signatures already reported (this run's count; the report's folder name).
    known: Mutex<HashMap<u64, (u64, String)>>,
    written: AtomicUsize,
    started: u64,
    /// When the app started, in milliseconds (for how far into the start a step came).
    started_ms: u64,
}

/// The startup step that ends startup: the app is up and drawing.
pub const RUNNING: &str = "running";

static STATE: OnceLock<State> = OnceLock::new();
/// The hook is writing a report (a panic inside it must not write another).
static IN_HOOK: AtomicBool = AtomicBool::new(false);
static SEQ: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Commands running on this thread (their panics are reported when the outermost ends).
    static IN_COMMAND: Cell<u32> = const { Cell::new(0) };
    /// Panics caught inside the running command, waiting for it to end.
    static PENDING: RefCell<Vec<Panic>> = const { RefCell::new(Vec::new()) };
}

/// Crash reporting is on in this process.
pub fn enabled() -> bool {
    STATE.get().is_some()
}

/// The crash folder, next to the recovery folder. `SOLIDCRAFT_CRASH_DIR` overrides it.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(s) = STATE.get() {
        return Some(s.dir.clone());
    }
    // Tests never touch the user's real crash folder.
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("solidcraft-test-crashes-{}", std::process::id())));
    }
    if let Some(d) = solidcraft_geom::env::var_os("CRASH_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    crate::recovery::default_dir().and_then(|d| d.parent().map(|p| p.join("crashes")))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// `secs` since 1970 as (year, month, day, hour, minute, second) in UTC.
fn civil(secs: u64) -> (i64, i64, i64, u64, u64, u64) {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// `2026-10-12T14:03:22Z`.
pub fn utc(secs: u64) -> String {
    let (y, m, d, h, mi, s) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn stamp(secs: u64) -> String {
    let (y, m, d, h, mi, s) = civil(secs);
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// A crash's identity: where it happened and what it said, numbers ignored (ids and sizes vary).
pub fn signature(location: &str, message: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for b in location.bytes() {
        eat(b);
    }
    eat(b'|');
    let mut digit = false;
    for b in message.bytes().take(300) {
        if b.is_ascii_digit() {
            if !digit {
                eat(b'#');
            }
            digit = true;
        } else {
            digit = false;
            eat(b);
        }
    }
    h
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn limited(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes)", s.get(..end).unwrap_or_default(), s.len())
}

/// Serialise `v`, giving up past `limit` bytes.
fn bounded_json(v: &Value, limit: usize) -> Option<String> {
    struct W(Vec<u8>, usize);
    impl Write for W {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.0.len() + b.len() > self.1 {
                return Err(std::io::Error::other("too long"));
            }
            self.0.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut w = W(Vec::new(), limit);
    serde_json::to_writer(&mut w, v).ok()?;
    String::from_utf8(w.0).ok()
}

// ---------------------------------------------------------------------------------------------
// Panics

/// What a panic said.
#[derive(Clone, Debug)]
struct Panic {
    message: String,
    location: String,
    thread: String,
    backtrace: Vec<String>,
    recovered: bool,
    signature: u64,
}

/// A backtrace as frames `symbol (file:line)`, without the panic machinery, the user's home
/// folder or the toolchain's hash.
fn frames(bt: &str) -> Vec<String> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for line in bt.lines() {
        let t = line.trim();
        if let Some(at) = t.strip_prefix("at ") {
            let mut at = at.to_string();
            if !home.is_empty() {
                at = at.replace(&home, "~");
            }
            if let Some(i) = at.find("/rustc/")
                && let Some(j) = at.get(i + 7..).and_then(|r| r.find('/'))
            {
                at = format!("rustc{}", at.get(i + 7 + j..).unwrap_or_default());
            }
            if let Some(last) = out.last_mut()
                && !last.ends_with(')')
            {
                last.push_str(&format!(" ({at})"));
            }
        } else if let Some((n, sym)) = t.split_once(": ")
            && n.chars().all(|c| c.is_ascii_digit())
        {
            out.push(sym.to_string());
        }
    }
    // Drop the hook and the panic machinery on top (everything up to where the panic began),
    // everything below the thread's start, and the standard library's own frames between.
    let entry = ["panicking::begin_panic", "panic::panic_any", "rust_begin_unwind", "core::panicking::", "__rust_end_short_backtrace"];
    let skip = out.iter().take(40).rposition(|f| entry.iter().any(|e| f.contains(e))).map_or(0, |i| i + 1);
    let mut v: Vec<String> = out.into_iter().skip(skip).collect();
    if let Some(end) = v.iter().position(|f| f.contains("__rust_begin_short_backtrace")) {
        v.truncate(end);
    }
    v.retain(|f| !f.contains("(rustc/"));
    v.truncate(MAX_FRAMES);
    v
}

/// Install the panic hook and start this run's journal; reports the runs that crashed without a
/// report (unclean exits). Returns the reports written since the last time this was called,
/// newest first, for the app to tell the user about. Call once, early.
pub fn install(dir: PathBuf, build: Build) -> Vec<Summary> {
    let _ = fs::create_dir_all(dir.join("sessions"));
    let started = now();
    let started_ms = now_ms();
    for path in dead_journals(&dir) {
        unclean_exit(&dir, &build, &path);
        let _ = fs::remove_file(&path);
    }
    remove_stray_native_files(&dir);
    prune(&dir, started);
    // Reports the user has been told about (by id).
    let seen_file = dir.join(".seen");
    let mut seen = String::new();
    if let Ok(f) = File::open(&seen_file) {
        let _ = f.take(64 << 10).read_to_string(&mut seen);
    }
    let all = list(&dir);
    let fresh: Vec<Summary> = all.iter().filter(|s| !seen.lines().any(|l| l == s.id)).cloned().collect();
    let _ = fs::write(&seen_file, all.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join("\n"));
    let known = all.into_iter().filter_map(|s| sig_of(&s.id).map(|sig| (sig, (0, s.id)))).collect();
    let journal = Journal::open(&dir, &build, started);
    if STATE
        .set(State {
            dir,
            build,
            environment: Mutex::new(String::new()),
            journal: Mutex::new(journal),
            known: Mutex::new(known),
            written: AtomicUsize::new(0),
            started,
            started_ms,
        })
        .is_ok()
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !hook(info) {
                previous(info);
            }
        }));
    }
    fresh
}

/// Tell reports about the renderer, GPU and window (one short line).
pub fn set_environment(text: &str) {
    if let Some(s) = STATE.get()
        && let Ok(mut e) = s.environment.lock()
    {
        *e = limited(text, 500);
        if let Ok(mut j) = s.journal.lock() {
            let line = json!({ "environment": *e }).to_string();
            j.write(&line);
            j.environment = Some(line);
        }
    }
}

/// The app is about to start a step of getting up (`"window"`, `"device creation"`, …; [`RUNNING`]
/// once it draws): the journal keeps the latest, so a crash nothing catches (inside a graphics
/// driver, say) is still reported with the step it happened in.
pub fn phase(step: &str) {
    let Some(s) = STATE.get() else { return };
    if let Ok(mut j) = s.journal.lock() {
        if j.phase_step.as_deref() == Some(step) {
            return;
        }
        let line = json!({ "phase": limited(step, 200), "ms": now_ms().saturating_sub(s.started_ms) }).to_string();
        j.write(&line);
        j.phase = Some(line);
        j.phase_step = Some(step.to_string());
    }
}

/// Where a native crash handler writes what it caught for this run: a small JSON file
/// (`code`, `name`, `address`, `module`, `offset`, `detail`, `thread`, and from macOS
/// `thread_name` and `frames`) and a minidump. The next
/// start reports both with this run's unclean exit. `None` before [`install`] or without a journal.
pub fn native_paths() -> Option<(PathBuf, PathBuf)> {
    let s = STATE.get()?;
    let j = s.journal.lock().ok()?;
    j.path.as_ref().map(|p| native_paths_for(p))
}

fn native_paths_for(journal: &Path) -> (PathBuf, PathBuf) {
    (journal.with_extension("native.json"), journal.with_extension("dmp"))
}

/// Native crash files whose journal is gone (their report was written, or never will be).
fn remove_stray_native_files(dir: &Path) {
    for path in fs::read_dir(dir.join("sessions")).into_iter().flatten().flatten().map(|e| e.path()).take(200) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let stem = name.strip_suffix(".native.json").or_else(|| name.strip_suffix(".dmp"));
        if let Some(stem) = stem
            && !path.with_file_name(format!("{stem}.jsonl")).exists()
        {
            let _ = fs::remove_file(&path);
        }
    }
}

/// The app is closing normally: its journal goes, and repeat counts are written.
pub fn end_session() {
    let Some(s) = STATE.get() else { return };
    if let Ok(mut j) = s.journal.lock()
        && let Some(path) = j.path.take()
    {
        j.file = None;
        let _ = fs::remove_file(path);
    }
    let counts: Vec<(u64, String)> = s.known.lock().map(|k| k.values().filter(|(n, _)| *n > 0).cloned().collect()).unwrap_or_default();
    for (n, id) in counts {
        bump(&s.dir, &id, n);
    }
}

/// The panic hook. Returns true when the panic was recorded for a command or the kernel (which
/// will catch it), so the default "thread panicked" message is not printed.
fn hook(info: &std::panic::PanicHookInfo<'_>) -> bool {
    let Some(state) = STATE.get() else { return false };
    if IN_HOOK.swap(true, Ordering::SeqCst) {
        return false;
    }
    let message = limited(info.payload_as_str().unwrap_or("(not a text message)"), MAX_MESSAGE);
    let location = info.location().map_or_else(|| "(unknown)".to_string(), |l| format!("{}:{}", l.file(), l.line()));
    let sig = signature(&location, &message);
    let recovered = solidcraft_kernel::guarded();
    let in_command = IN_COMMAND.with(Cell::get) > 0;
    let quiet = recovered || in_command;
    // Already reported (this run or before): count it, nothing else. A panic outside commands
    // can end the app before `end_session` writes the counts, so its count goes to the report
    // now, and the journal says it was reported (or the next start reports an unclean exit).
    let known = state.known.lock().map(|mut k| {
        k.get_mut(&sig).map(|e| {
            if quiet {
                e.0 += 1;
            }
            e.1.clone()
        })
    });
    let repeat = match known {
        Ok(Some(id)) => {
            if !quiet {
                bump(&state.dir, &id, 1);
                if let Ok(mut j) = state.journal.try_lock() {
                    j.record(&json!({ "reported": id }));
                }
            }
            true
        }
        Ok(None) => false,
        Err(_) => true,
    };
    if repeat {
        IN_HOOK.store(false, Ordering::SeqCst);
        return quiet;
    }
    let thread = std::thread::current().name().unwrap_or("(unnamed)").to_string();
    let backtrace = frames(&Backtrace::force_capture().to_string());
    let p = Panic { message, location, thread, backtrace, recovered, signature: sig };
    if in_command {
        // Reported with the design and the command when the outermost command ends.
        PENDING.with(|q| {
            let mut q = q.borrow_mut();
            if q.len() < 8 {
                q.push(p);
            }
        });
        eprintln!("SolidCraft: internal error caught in a command; a crash report will be written");
    } else {
        // Outside any command: what the journal knows has to do.
        let kind = if recovered { "kernel_panic_recovered" } else { "panic" };
        if let Some(id) = write_from_journal(state, kind, Some(&p)) {
            eprintln!("SolidCraft: internal error; crash report {}", state.dir.join(&id).display());
            if !recovered && let Ok(mut j) = state.journal.try_lock() {
                j.record(&json!({ "reported": id }));
            }
        }
    }
    IN_HOOK.store(false, Ordering::SeqCst);
    quiet
}

/// Marks a command running on this thread; panics inside it wait for [`command_done`].
pub(crate) struct Scope(bool);

impl Scope {
    pub(crate) fn enter() -> Scope {
        if !enabled() {
            return Scope(false);
        }
        IN_COMMAND.with(|c| c.set(c.get() + 1));
        Scope(true)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.0 {
            IN_COMMAND.with(|c| c.set(c.get().saturating_sub(1)));
        }
    }
}

/// A top-level command is about to run: the journal records it (only for the app's own design,
/// `Session::crash_journal`).
pub(crate) fn command_begin(s: &Session, id: &str, params: &Value) {
    let Some(state) = STATE.get() else { return };
    if !s.crash_journal {
        return;
    }
    let Ok(mut j) = state.journal.lock() else { return };
    let ptr = std::sync::Arc::as_ptr(&s.doc) as usize;
    if ptr != j.last_doc {
        // The design changed outside commands (opened, a new tab, a dialog): what is it now?
        j.rebase(&journal_base(s), ptr, Some(s.ids.seed()));
    }
    j.run(id, params);
}

fn journal_base(s: &Session) -> Value {
    if s.is_dirty() {
        json!("unknown")
    } else if let Some(path) = &s.path {
        json!({ "file": path })
    } else if s.doc.features.is_empty() {
        json!("new")
    } else {
        json!("unknown")
    }
}

/// Record a successful isolated command only after its entire transaction is accepted.
pub(crate) fn accepted_command(
    s: &Session,
    before: &std::sync::Arc<Document>,
    after: &std::sync::Arc<Document>,
    id: &str,
    params: &Value,
    changed: bool,
) {
    let Some(state) = STATE.get() else { return };
    if !s.crash_journal {
        return;
    }
    let Ok(mut j) = state.journal.lock() else { return };
    let ptr = std::sync::Arc::as_ptr(before) as usize;
    if ptr != j.last_doc {
        let at_session = std::sync::Arc::ptr_eq(&s.doc, before);
        let base = if at_session { journal_base(s) } else { json!("unknown") };
        j.rebase(&base, ptr, at_session.then(|| s.ids.seed()));
    }
    j.run(id, params);
    j.done(&Ok(()), changed);
    j.last_doc = std::sync::Arc::as_ptr(after) as usize;
}

/// A top-level command ended: the journal records how, and panics caught inside it are reported
/// with `before` (the design it started from) and the command (its id, its parameters and the
/// session's id seed when it started), which together reproduce them.
pub(crate) fn command_done(
    journaled: bool,
    changed: bool,
    before: &Document,
    after: &std::sync::Arc<Document>,
    (id, params, ids): (&str, &Value, u64),
    outcome: std::result::Result<(), String>,
) {
    let Some(state) = STATE.get() else { return };
    if journaled && let Ok(mut j) = state.journal.lock() {
        j.done(&outcome, changed);
        j.last_doc = std::sync::Arc::as_ptr(after) as usize;
    }
    if IN_COMMAND.with(Cell::get) > 0 {
        return;
    }
    let pending: Vec<Panic> = PENDING.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for p in pending {
        let kind = if p.recovered { "kernel_panic_recovered" } else { "command_panic" };
        let (openable, left_out) = drawing::openable(before);
        let design = solidcraft_io::write_design(&openable);
        let replay = json!({ "commands": [{ "command": id, "params": params }], "ids": ids });
        let recent = if journaled { read_live_journal(state).recent } else { Vec::new() };
        let extra = Extra {
            command: Some(json!({ "command": id, "params": params })),
            design: Some(design),
            replay: Some(replay),
            recent,
            base: json!("the design as it was when the command started"),
            notes: left_out,
            fields: serde_json::Map::new(),
            attach: Vec::new(),
        };
        write_report(state, kind, Some(&p), extra);
    }
}

/// An autosave just wrote the design to `file` (or found nothing unsaved: `file` is then the
/// design's own file, or `None` for a new design or one that cannot be named): replays start
/// from there.
pub(crate) fn rebased(s: &Session, file: Option<&Path>, autosave: bool) {
    let Some(state) = STATE.get() else { return };
    if !s.crash_journal {
        return;
    }
    let Ok(mut j) = state.journal.lock() else { return };
    let base = match (file, autosave) {
        (Some(f), true) => json!({ "autosave": f.to_string_lossy() }),
        (Some(f), false) => json!({ "file": f.to_string_lossy() }),
        (None, _) if s.doc.features.is_empty() => json!("new"),
        _ => json!("unknown"),
    };
    j.rebase(&base, std::sync::Arc::as_ptr(&s.doc) as usize, Some(s.ids.seed()));
}

// ---------------------------------------------------------------------------------------------
// The journal

struct Journal {
    path: Option<PathBuf>,
    file: Option<File>,
    bytes: u64,
    full: bool,
    last_doc: usize,
    header: String,
    /// Where the running command's line starts (to take it back when it changed nothing).
    running_at: Option<u64>,
    /// The latest startup step's line and name, and the renderer's line (kept across rebases).
    phase: Option<String>,
    phase_step: Option<String>,
    environment: Option<String>,
}

impl Journal {
    fn open(dir: &Path, build: &Build, started: u64) -> Journal {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join("sessions").join(format!("{}-{}-{seq}.jsonl", stamp(started), std::process::id()));
        // Append-only handles cannot truncate on Windows; seek explicitly for each write.
        let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&path).ok().filter(|f| f.try_lock().is_ok());
        let header = json!({ "start": utc(started), "pid": std::process::id(), "version": build.version, "commit": build.commit }).to_string();
        let mut j = Journal {
            path: file.as_ref().map(|_| path),
            file,
            bytes: 0,
            full: false,
            last_doc: 0,
            header,
            running_at: None,
            phase: None,
            phase_step: None,
            environment: None,
        };
        j.rebase(&json!("new"), 0, None);
        j
    }

    fn write(&mut self, line: &str) {
        let Some(f) = &mut self.file else { return };
        if f.seek(SeekFrom::End(0)).and_then(|_| f.write_all(line.as_bytes())).and_then(|()| f.write_all(b"\n")).is_err() {
            self.file = None;
        }
        self.bytes += line.len() as u64 + 1;
    }

    fn record(&mut self, v: &Value) {
        self.write(&v.to_string());
    }

    /// A new base: `base`, the design the commands after it start from, and `ids`, the session's
    /// id seed there.
    fn rebase(&mut self, base: &Value, doc: usize, ids: Option<u64>) {
        if let Some(f) = &self.file
            && f.set_len(0).is_err()
        {
            self.file = None;
        }
        self.bytes = 0;
        self.full = false;
        self.running_at = None;
        self.last_doc = doc;
        let h = self.header.clone();
        self.write(&h);
        let mut line = json!({ "base": base });
        if let Some(ids) = ids {
            line["ids"] = json!(ids);
        }
        self.record(&line);
        for line in [self.phase.clone(), self.environment.clone()].into_iter().flatten() {
            self.write(&line);
        }
    }

    fn run(&mut self, id: &str, params: &Value) {
        if self.full {
            return;
        }
        if self.bytes > MAX_JOURNAL_BYTES {
            self.full = true;
            self.record(&json!({ "full": true }));
            return;
        }
        self.running_at = Some(self.bytes);
        let id = Value::String(id.to_string()).to_string();
        let line = match bounded_json(params, MAX_PARAMS_BYTES) {
            Some(p) => format!("{{\"run\":{id},\"params\":{p}}}"),
            None => format!("{{\"run\":{id},\"params_omitted\":true}}"),
        };
        self.write(&line);
    }

    fn done(&mut self, outcome: &std::result::Result<(), String>, changed: bool) {
        let at = self.running_at.take();
        if self.full {
            return;
        }
        match outcome {
            // Nothing changed: take the command's line back (one truncate, no write).
            Ok(()) if !changed => {
                if let (Some(at), Some(f)) = (at, &self.file) {
                    if f.set_len(at).is_ok() {
                        self.bytes = at;
                    } else {
                        self.file = None;
                    }
                }
            }
            Ok(()) => self.write("{\"ok\":true}"),
            Err(e) => self.record(&json!({ "ok": false, "error": limited(e, 300) })),
        }
    }
}

/// What a journal says.
#[derive(Default)]
struct JournalRead {
    header: Value,
    base: Value,
    /// The session's id seed at the base, when it was kept.
    ids: Option<u64>,
    /// Commands since the base that changed something or did nothing wrong, in order.
    replay: Vec<Value>,
    /// The command that was running, if one was.
    in_flight: Option<Value>,
    recent: Vec<Value>,
    complete: bool,
    reported: bool,
    /// The latest startup step and how many milliseconds after the start it began.
    phase: Option<(String, u64)>,
    /// The renderer, once known.
    environment: Option<String>,
}

// Read through the lock-owning file object. On Windows an independently opened
// handle cannot read a live journal's exclusively locked byte range.
fn read_live_journal(state: &State) -> JournalRead {
    state.journal.try_lock().ok().and_then(|j| j.file.as_ref()?.try_clone().ok().map(read_journal_file)).unwrap_or_default()
}

fn read_journal(path: &Path) -> JournalRead {
    File::open(path).ok().map(read_journal_file).unwrap_or_default()
}

fn read_journal_file(mut file: File) -> JournalRead {
    let mut text = String::new();
    if file.seek(SeekFrom::Start(0)).is_ok() {
        let _ = file.take(MAX_JOURNAL_BYTES + (64 << 10)).read_to_string(&mut text);
    }
    let mut r = JournalRead { complete: true, ..Default::default() };
    let mut running: Option<Value> = None;
    for (i, line) in text.lines().enumerate() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if i == 0 {
            r.header = v;
            continue;
        }
        // Startup steps and the renderer say nothing about whether the run was reported.
        if v.get("phase").is_none() && v.get("environment").is_none() {
            r.reported = v.get("reported").is_some();
        }
        if let Some(b) = v.get("base") {
            r.base = b.clone();
            r.ids = v.get("ids").and_then(Value::as_u64);
            r.replay.clear();
            r.recent.clear();
            running = None;
        } else if let Some(id) = v.get("run") {
            let c = match v.get("params") {
                Some(p) => json!({ "command": id, "params": p }),
                None => {
                    r.complete = false;
                    json!({ "command": id, "params_omitted": true })
                }
            };
            running = Some(c);
        } else if let Some(ok) = v.get("ok").and_then(Value::as_bool) {
            if let Some(c) = running.take() {
                let mut shown = c.clone();
                shown["ok"] = json!(ok);
                if let Some(e) = v.get("error") {
                    shown["error"] = e.clone();
                }
                r.recent.push(shown);
                if ok {
                    r.replay.push(c);
                }
            }
        } else if v.get("full").is_some() {
            r.complete = false;
        } else if let Some(p) = v.get("phase").and_then(Value::as_str) {
            r.phase = Some((p.to_string(), v.get("ms").and_then(Value::as_u64).unwrap_or(0)));
        } else if let Some(e) = v.get("environment").and_then(Value::as_str) {
            r.environment = Some(e.to_string());
        }
    }
    r.in_flight = running;
    let n = r.recent.len();
    r.recent.drain(..n.saturating_sub(RECENT));
    r
}

/// Journals of apps that are not running any more (nobody holds their lock).
fn dead_journals(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .take(100)
        .filter(|p| File::open(p).is_ok_and(|f| f.try_lock().is_ok()))
        .collect()
}

fn unclean_exit(dir: &Path, build: &Build, journal: &Path) {
    let j = read_journal(journal);
    let (native_file, dump_file) = native_paths_for(journal);
    let native = fs::read(&native_file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object);
    if j.reported {
        return;
    }
    let was = j.header.get("version").and_then(Value::as_str).unwrap_or("?");
    let when = j.header.get("start").and_then(Value::as_str).unwrap_or("?");
    let doing = j.in_flight.as_ref().and_then(|c| c.get("command")).and_then(Value::as_str);
    // A run that never got to drawing died while starting up: say at which step.
    let starting = j.phase.as_ref().filter(|(p, _)| p != RUNNING).map(|(p, ms)| (p.clone(), *ms));
    let during = match (&starting, doing) {
        (Some((p, ms)), _) => format!("during startup ({p}, {:.1} s after launch)", *ms as f64 / 1000.0),
        (None, Some(c)) => format!("while running `{c}`"),
        (None, None) => "between commands".to_string(),
    };
    let field = |k: &str| native.as_ref().and_then(|n| n.get(k)).and_then(Value::as_str).map(|v| limited(v, 300));
    let (message, location, signature_text, backtrace) = match &native {
        Some(_) => {
            let code = field("code").unwrap_or_else(|| "?".into());
            let name = field("name").unwrap_or_else(|| "exception".into());
            let module = field("module").unwrap_or_else(|| "(unknown module)".into());
            let short = module.rsplit(['\\', '/']).next().unwrap_or(&module).to_string();
            let offset = field("offset").unwrap_or_else(|| "?".into());
            let at = format!("{short}+{offset}");
            let mut frames = vec![format!("{code} {name}{}", field("detail").map(|d| format!(" ({d})")).unwrap_or_default())];
            frames.push(format!("at {module}+{offset} (address {})", field("address").unwrap_or_else(|| "?".into())));
            if let Some(t) = native.as_ref().and_then(|n| n.get("thread")).and_then(Value::as_u64) {
                frames.push(match field("thread_name").filter(|n| !n.is_empty()) {
                    Some(n) => format!("on thread {t} ({n})"),
                    None => format!("on thread {t}"),
                });
            }
            // The handler's own walk up the stack (macOS): `module+offset symbol+offset`.
            if let Some(walk) = native.as_ref().and_then(|n| n.get("frames")).and_then(Value::as_array) {
                frames.extend(walk.iter().filter_map(Value::as_str).take(MAX_FRAMES).map(|f| limited(f, 300)));
            }
            (
                format!("SolidCraft {was} (started {when}) crashed with {name} ({code}) in {at} {during}"),
                format!("native exception in {at}"),
                format!("{code} {short} {}", starting.as_ref().map_or(doing.unwrap_or("idle"), |(p, _)| p.as_str())),
                frames,
            )
        }
        None => (
            format!("SolidCraft {was} (started {when}) ended without a clean exit {during}"),
            match (&starting, doing) {
                (Some((p, _)), _) => format!("unclean exit during startup: {p}"),
                (None, c) => format!("unclean exit in {}", c.unwrap_or("(between commands)")),
            },
            match &starting {
                Some((p, _)) => format!("startup: {p}"),
                None => doing.unwrap_or("idle").to_string(),
            },
            Vec::new(),
        ),
    };
    // A temporary state just for this report (the app's own is not set up yet).
    let state = State {
        dir: dir.to_path_buf(),
        build: build.clone(),
        environment: Mutex::new(j.environment.clone().unwrap_or_default()),
        journal: Mutex::new(Journal {
            path: None,
            file: None,
            bytes: 0,
            full: true,
            last_doc: 0,
            header: String::new(),
            running_at: None,
            phase: None,
            phase_step: None,
            environment: None,
        }),
        known: Mutex::new(list(dir).into_iter().filter_map(|s| sig_of(&s.id).map(|sig| (sig, (0, s.id)))).collect()),
        written: AtomicUsize::new(0),
        started: now(),
        started_ms: now_ms(),
    };
    let p = Panic {
        message,
        location,
        thread: "(unknown)".into(),
        backtrace,
        recovered: false,
        signature: signature(if native.is_some() { "native exception" } else { "unclean exit" }, &signature_text),
    };
    if let Ok(mut k) = state.known.lock()
        && let Some(e) = k.get_mut(&p.signature)
    {
        e.0 += 1;
        let (n, id) = e.clone();
        drop(k);
        bump(dir, &id, n);
        return;
    }
    let mut extra = from_journal(j);
    if let Some(n) = native {
        extra.fields.insert("native".into(), n);
        if dump_file.is_file() {
            extra.attach.push(("minidump".into(), dump_file, "minidump.dmp".into()));
        }
    }
    write_report(&state, "unclean_exit", Some(&p), extra);
}

// ---------------------------------------------------------------------------------------------
// Reports

struct Extra {
    command: Option<Value>,
    design: Option<Vec<u8>>,
    replay: Option<Value>,
    recent: Vec<Value>,
    base: Value,
    notes: Vec<String>,
    /// More top-level fields for the report (`startup_phase`, `native`).
    fields: serde_json::Map<String, Value>,
    /// Files moved into the report's folder: (key in `files`, source, name in the folder).
    attach: Vec<(String, PathBuf, String)>,
}

/// Design and replay from a journal: the base design (autosave or saved file) and the commands
/// since, ending with the one in flight.
fn from_journal(j: JournalRead) -> Extra {
    let mut notes = Vec::new();
    let file = j.base.get("autosave").or_else(|| j.base.get("file")).and_then(Value::as_str).map(PathBuf::from);
    let design = match &file {
        Some(f) => match fs::metadata(f) {
            Ok(m) if m.len() <= MAX_DESIGN_BYTES as u64 => fs::read(f).ok(),
            Ok(_) => {
                notes.push(format!("the design ({}) is larger than {} MB and was not copied", f.display(), MAX_DESIGN_BYTES >> 20));
                None
            }
            Err(_) => {
                notes.push(format!("the design the commands start from ({}) is gone", f.display()));
                None
            }
        },
        None => None,
    };
    let fresh = j.base.as_str() == Some("new");
    let mut commands = j.replay;
    if let Some(c) = &j.in_flight {
        commands.push(c.clone());
    }
    if !j.complete {
        notes.push("the journal was cut short (too many or too large commands): the replay is incomplete".into());
    }
    let replay = (design.is_some() || fresh) && j.complete && !commands.is_empty();
    if j.base.as_str() == Some("unknown") {
        notes.push("the design changed outside commands before this; it cannot be rebuilt from the journal".into());
    }
    let mut fields = serde_json::Map::new();
    if let Some((p, ms)) = j.phase.filter(|(p, _)| p != RUNNING) {
        fields.insert("startup_phase".into(), json!({ "step": p, "ms": ms }));
    }
    Extra {
        command: j.in_flight,
        design,
        replay: replay.then(|| match j.ids {
            Some(ids) => json!({ "commands": commands, "ids": ids }),
            None => json!({ "commands": commands }),
        }),
        recent: j.recent,
        base: j.base,
        notes,
        fields,
        attach: Vec::new(),
    }
}

fn write_from_journal(state: &State, kind: &str, p: Option<&Panic>) -> Option<String> {
    let j = read_live_journal(state);
    let extra = from_journal(j);
    write_report(state, kind, p, extra)
}

fn write_report(state: &State, kind: &str, p: Option<&Panic>, extra: Extra) -> Option<String> {
    let sig = p.map_or(0, |p| p.signature);
    if let Ok(mut k) = state.known.lock() {
        if let Some(e) = k.get_mut(&sig) {
            e.0 += 1;
            return None;
        }
        if state.written.load(Ordering::SeqCst) >= MAX_NEW_PER_SESSION {
            return None;
        }
        state.written.fetch_add(1, Ordering::SeqCst);
        let t = now();
        let id = format!("{}-{sig:016x}", stamp(t));
        k.insert(sig, (0, id.clone()));
        drop(k);
        let folder = state.dir.join(&id);
        fs::create_dir_all(&folder).ok()?;
        let mut files = serde_json::Map::new();
        let mut notes = extra.notes;
        if let Some(d) = &extra.design {
            if d.len() <= MAX_DESIGN_BYTES {
                let name = format!("design.{}", solidcraft_io::DESIGN_EXT);
                if solidcraft_io::write_atomic(&folder.join(&name), d, false).is_ok() {
                    files.insert("design".into(), json!(name));
                }
            } else {
                notes.push(format!("the design is larger than {} MB and was not copied", MAX_DESIGN_BYTES >> 20));
            }
        }
        if let Some(r) = &extra.replay
            && let Ok(b) = serde_json::to_vec_pretty(r)
            && solidcraft_io::write_atomic(&folder.join("replay.json"), &b, false).is_ok()
        {
            files.insert("replay".into(), json!("replay.json"));
        }
        for (key, from, name) in &extra.attach {
            let to = folder.join(name);
            if fs::rename(from, &to).is_ok() || fs::copy(from, &to).is_ok() {
                files.insert(key.clone(), json!(name));
            }
        }
        let reproduce = match (files.contains_key("replay"), files.contains_key("design")) {
            (true, true) => Some(format!("solidcraft-cli crashes replay {id}")),
            (true, false) => Some(format!("solidcraft-cli crashes replay {id}  (starts from a new design)")),
            _ => None,
        };
        let environment = state.environment.lock().map(|e| e.clone()).unwrap_or_default();
        let mut report = json!({
            "format": FORMAT,
            "id": id,
            "kind": kind,
            "signature": format!("{sig:016x}"),
            "count": 1,
            "first_seen": utc(t),
            "last_seen": utc(t),
            "app": { "version": state.build.version, "commit": state.build.commit, "built": state.build.built },
            "system": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "environment": environment },
            "session": { "uptime_s": t.saturating_sub(state.started) },
            "message": p.map(|p| p.message.clone()),
            "location": p.map(|p| p.location.clone()),
            "thread": p.map(|p| p.thread.clone()),
            "backtrace": p.map(|p| p.backtrace.clone()).unwrap_or_default(),
            "command": extra.command,
            "base": extra.base,
            "recent_commands": extra.recent,
            "files": files,
            "reproduce": reproduce,
            "notes": notes,
        });
        if let Some(r) = report.as_object_mut() {
            for (k, v) in extra.fields {
                r.entry(k).or_insert(v);
            }
        }
        let bytes = serde_json::to_vec_pretty(&report).ok()?;
        solidcraft_io::write_atomic(&folder.join("report.json"), &bytes, false).ok()?;
        prune(&state.dir, t);
        return Some(id);
    }
    None
}

/// Add `n` repeats to a report.
fn bump(dir: &Path, id: &str, n: u64) {
    if !valid_id(id) || n == 0 {
        return;
    }
    let path = dir.join(id).join("report.json");
    let Some(mut v) = read_report_file(&path) else { return };
    let count = v.get("count").and_then(Value::as_u64).unwrap_or(1).saturating_add(n);
    v["count"] = json!(count);
    v["last_seen"] = json!(utc(now()));
    if let Ok(b) = serde_json::to_vec_pretty(&v) {
        let _ = solidcraft_io::write_atomic(&path, &b, false);
    }
}

fn read_report_file(path: &Path) -> Option<Value> {
    let f = File::open(path).ok()?;
    let mut s = String::new();
    f.take(4 << 20).read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok().filter(Value::is_object)
}

fn sig_of(id: &str) -> Option<u64> {
    id.rsplit('-').next().and_then(|h| u64::from_str_radix(h, 16).ok())
}

/// Seconds since 1970 at which report `id` was made (from its folder's time).
fn report_time(dir: &Path, id: &str) -> u64 {
    fs::metadata(dir.join(id).join("report.json"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn folder_bytes(p: &Path) -> u64 {
    fs::read_dir(p).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
}

/// Report folders in `dir`, oldest first.
fn report_ids(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| valid_id(n) && n != "sessions" && sig_of(n).is_some() && n.len() == 32)
        .take(1000)
        .collect();
    v.sort();
    v
}

/// Keep the reports within the limits: drop expired ones, then recovered kernel panics, then the
/// oldest, until at most [`MAX_REPORTS`] and [`MAX_TOTAL_BYTES`] remain.
pub fn prune(dir: &Path, now_secs: u64) {
    let ids = report_ids(dir);
    let mut keep: Vec<(String, bool, u64)> = Vec::new();
    for id in ids {
        let folder = dir.join(&id);
        let t = report_time(dir, &id);
        if (t > 0 && now_secs.saturating_sub(t) > MAX_AGE_DAYS * 86_400) || !folder.join("report.json").is_file() {
            let _ = fs::remove_dir_all(&folder);
            continue;
        }
        let recovered = read_report_file(&folder.join("report.json"))
            .and_then(|v| v.get("kind").and_then(Value::as_str).map(|k| k == "kernel_panic_recovered"))
            .unwrap_or(false);
        keep.push((id, recovered, folder_bytes(&folder)));
    }
    let over = |k: &[(String, bool, u64)]| k.len() > MAX_REPORTS || k.iter().map(|e| e.2).sum::<u64>() > MAX_TOTAL_BYTES;
    while over(&keep) {
        let i = keep.iter().position(|e| e.1).unwrap_or(0);
        if i >= keep.len() {
            break;
        }
        let (id, _, _) = keep.remove(i);
        let _ = fs::remove_dir_all(dir.join(id));
    }
}

/// The reports in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<Summary> {
    let mut out: Vec<Summary> = report_ids(dir)
        .into_iter()
        .rev()
        .filter_map(|id| {
            let folder = dir.join(&id);
            let v = read_report_file(&folder.join("report.json"))?;
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            Some(Summary {
                kind: s("kind"),
                time: s("first_seen"),
                message: limited(&s("message"), 300),
                command: v.get("command").and_then(|c| c.get("command")).and_then(Value::as_str).map(str::to_string),
                count: v.get("count").and_then(Value::as_u64).unwrap_or(1),
                version: v.get("app").and_then(|a| a.get("version")).and_then(Value::as_str).unwrap_or_default().to_string(),
                path: folder.to_string_lossy().into_owned(),
                replayable: folder.join("replay.json").is_file(),
                id,
            })
        })
        .collect();
    out.truncate(MAX_REPORTS * 2);
    out
}

/// One report's `report.json`, with the paths of its files.
pub fn read(dir: &Path, id: &str) -> std::result::Result<Value, String> {
    if !valid_id(id) {
        return Err(format!("no crash report `{id}`"));
    }
    let folder = dir.join(id);
    let mut v = read_report_file(&folder.join("report.json")).ok_or_else(|| format!("no crash report `{id}`"))?;
    v["path"] = json!(folder.to_string_lossy());
    // Reports from earlier releases hold `design.solidcraft`.
    if let Some(design) = solidcraft_io::DESIGN_EXTS.iter().map(|ext| folder.join(format!("design.{ext}"))).find(|p| p.is_file()) {
        v["design_path"] = json!(design.to_string_lossy());
    }
    for (k, f) in [("replay_path", "replay.json"), ("minidump_path", "minidump.dmp")] {
        if folder.join(f).is_file() {
            v[k] = json!(folder.join(f).to_string_lossy());
        }
    }
    Ok(v)
}

/// Delete one report, or all of them.
pub fn clear(dir: &Path, id: Option<&str>) -> usize {
    let ids = match id {
        Some(i) if valid_id(i) && report_ids(dir).iter().any(|r| r == i) => vec![i.to_string()],
        Some(_) => Vec::new(),
        None => report_ids(dir),
    };
    ids.iter().filter(|i| fs::remove_dir_all(dir.join(i)).is_ok()).count()
}

#[cfg(test)]
#[path = "crash_tests.rs"]
mod tests;
