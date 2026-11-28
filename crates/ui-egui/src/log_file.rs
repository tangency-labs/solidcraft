//! The app's log on disk: what SolidCraft's crates log at info and above, and other crates'
//! warnings, one line each, in `<data folder>/logs/solidcraft.log`. Each start moves the last
//! run's log to `solidcraft.previous.log`, and that one's to `solidcraft.previous.2.log`, keeping
//! [`KEEP`] earlier runs within [`MAX_KEPT_BYTES`]: a report sent after a crash still has the lines
//! of the run that crashed, even after a few more starts that failed too. A problem report attaches
//! the end of the log ([`tail`]); a crash report's, the end of the crashed run's own ([`run_tail_in`]),
//! which each log names on its first line ([`run_header`]).
//!
//! Only the desktop app installs it ([`install`]); tests, the CLI and the web build never do,
//! and [`tail`] then has nothing to read.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// This run's log.
pub const FILE: &str = "solidcraft.log";
/// The run before's.
pub const PREVIOUS: &str = "solidcraft.previous.log";
/// Earlier runs' logs kept, [`PREVIOUS`] included (`solidcraft.previous.<n>.log` for n ≥ 2).
pub const KEEP: usize = 5;
/// Bytes the logs older than [`PREVIOUS`] may take together: the oldest go first.
pub const MAX_KEPT_BYTES: u64 = 8 << 20;
/// The first line of each run's log says which run it is ([`run_header`]).
const RUN_HEADER: &str = "SolidCraft log of process ";
/// A log stops growing at this size; the line that reaches it says so.
const MAX_BYTES: u64 = 4 << 20;
/// The longest line written.
const MAX_LINE: usize = 4000;
/// Said between the run before's lines and this run's in [`tail`].
pub const RUN_MARK: &str = "--- the run before ---";

/// The log's folder, once installed.
static DIR: OnceLock<PathBuf> = OnceLock::new();

struct FileLog {
    file: Mutex<Option<File>>,
    written: AtomicU64,
}

fn ours(target: &str) -> bool {
    target.starts_with("solidcraft") || target.starts_with("craft_plugin_wasm")
}

impl Log for FileLog {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= if ours(m.target()) { Level::Info } else { Level::Warn }
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut line = format!("{} [{} {}] {}", solidcraft_engine::crash::utc(secs), r.level().as_str().to_ascii_lowercase(), r.target(), r.args());
        if line.len() > MAX_LINE {
            let mut end = MAX_LINE;
            while !line.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            line.truncate(end);
            line.push('…');
        }
        // One line per record: a message with line breaks stays one line of the log.
        let mut line = line.replace(['\r', '\n'], " ⏎ ");
        line.push('\n');
        let Ok(mut file) = self.file.lock() else { return };
        let Some(f) = file.as_mut() else { return };
        let before = self.written.fetch_add(line.len() as u64, Ordering::Relaxed);
        if before >= MAX_BYTES {
            return;
        }
        if before + line.len() as u64 >= MAX_BYTES {
            line.push_str("[the log is full; later lines are not kept]\n");
        }
        let _ = f.write_all(line.as_bytes());
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock()
            && let Some(f) = file.as_mut()
        {
            let _ = f.flush();
        }
    }
}

/// The log an earlier run wrote: `1` is [`PREVIOUS`], the run before this one.
pub fn previous_name(n: usize) -> String {
    if n <= 1 { PREVIOUS.to_string() } else { format!("solidcraft.previous.{n}.log") }
}

/// Move each earlier run's log one place older (the oldest of [`KEEP`] goes), this run's last
/// one to [`PREVIOUS`], then drop the oldest until those older than [`PREVIOUS`] fit in
/// [`MAX_KEPT_BYTES`]. Another window's process may still hold [`FILE`] (Windows then refuses the
/// move): this run then adds to the same file, and the others stay as they are.
pub fn rotate(dir: &Path) {
    let path = dir.join(FILE);
    if !path.is_file() {
        return;
    }
    let _ = fs::remove_file(dir.join(previous_name(KEEP)));
    for n in (1..KEEP).rev() {
        let from = dir.join(previous_name(n));
        if from.is_file() {
            let _ = fs::rename(&from, dir.join(previous_name(n + 1)));
        }
    }
    if fs::rename(&path, dir.join(PREVIOUS)).is_err() {
        // Held by another window: the older logs go back where they were.
        for n in 2..=KEEP {
            let from = dir.join(previous_name(n));
            if from.is_file() {
                let _ = fs::rename(&from, dir.join(previous_name(n - 1)));
            }
        }
        return;
    }
    let mut total = 0;
    for n in 2..=KEEP {
        let file = dir.join(previous_name(n));
        let Ok(meta) = fs::metadata(&file) else { continue };
        total += meta.len();
        if total > MAX_KEPT_BYTES {
            let _ = fs::remove_file(file);
        }
    }
}

/// The line each run's log starts with: its process and when it started, which a crash report
/// records too (`run`), so a report finds the crashed run's own lines ([`run_tail_in`]).
pub fn run_header(pid: u32, start: &str) -> String {
    format!("{RUN_HEADER}{pid}, started {start}")
}

/// Start the log in `dir`: last run's log becomes [`PREVIOUS`] ([`rotate`]), and this run writes
/// [`FILE`], starting with [`run_header`]. Returns the file, or `None` when it could not be made or
/// a logger was already installed.
pub fn install(dir: &Path) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let path = dir.join(FILE);
    rotate(dir);
    let mut file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    // The crash journal's start, to the second (`crash::install` runs just after this).
    let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = writeln!(file, "{}", run_header(std::process::id(), &solidcraft_engine::crash::utc(secs)));
    let written = file.metadata().map_or(0, |m| m.len());
    let logger = FileLog { file: Mutex::new(Some(file)), written: AtomicU64::new(written) };
    log::set_boxed_logger(Box::new(logger)).ok()?;
    log::set_max_level(LevelFilter::Info);
    let _ = DIR.set(dir.to_path_buf());
    Some(path)
}

/// The installed log's folder.
pub fn dir() -> Option<&'static Path> {
    DIR.get().map(PathBuf::as_path)
}

/// The last whole lines of `path`, at most `max_bytes` read from its end.
fn last_lines(path: &Path, max_bytes: u64) -> Vec<String> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let len = f.metadata().map_or(0, |m| m.len());
    let from = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if f.take(max_bytes).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    // Started inside a line: that part of it goes.
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines.retain(|l| !l.trim().is_empty());
    lines
}

/// The last `max_lines` lines of the log in `dir`, at most `max_bytes` of them: this run's, and
/// when it has fewer, the end of the run before's first, after it [`RUN_MARK`].
pub fn tail_in(dir: &Path, max_lines: usize, max_bytes: u64) -> Vec<String> {
    let mut lines = last_lines(&dir.join(FILE), max_bytes);
    let n = lines.len();
    lines.drain(..n.saturating_sub(max_lines));
    let used: u64 = lines.iter().map(|l| l.len() as u64 + 1).sum();
    let (room_lines, room_bytes) = (max_lines.saturating_sub(lines.len() + 1), max_bytes.saturating_sub(used + RUN_MARK.len() as u64 + 1));
    if room_lines > 0 && room_bytes > 0 {
        let mut before = last_lines(&dir.join(PREVIOUS), room_bytes);
        let n = before.len();
        before.drain(..n.saturating_sub(room_lines));
        if !before.is_empty() {
            before.push(RUN_MARK.to_string());
            before.append(&mut lines);
            lines = before;
        }
    }
    lines
}

/// The last `max_lines` lines, at most `max_bytes`, that the run of process `pid` started at
/// `start` (a crash report's `run`) wrote, from whichever of the kept logs holds them: this run's
/// or one of the [`KEEP`] before. The run is found by its [`run_header`]; one started within a
/// few minutes of `start` counts (the log starts a moment before the crash journal). None when no
/// kept log has it.
pub fn run_tail_in(dir: &Path, pid: u32, start: &str, max_lines: usize, max_bytes: u64) -> Option<Vec<String>> {
    let want = parse_utc(start)?;
    let head = format!("{RUN_HEADER}{pid}, started ");
    let files = std::iter::once(FILE.to_string()).chain((1..=KEEP).map(previous_name));
    for name in files {
        let Ok(f) = File::open(dir.join(&name)) else { continue };
        let mut text = String::new();
        if f.take(MAX_BYTES + (64 << 10)).read_to_string(&mut text).is_err() {
            continue;
        }
        let mut found = false;
        let mut out: Vec<&str> = Vec::new();
        for line in text.lines() {
            if line.strip_prefix(&head).and_then(parse_utc).is_some_and(|at| at.abs_diff(want) <= 300) {
                found = true;
                out.clear();
                continue;
            }
            if found {
                if line.starts_with(RUN_HEADER) {
                    break;
                }
                if !line.trim().is_empty() {
                    out.push(line);
                }
            }
        }
        if !found {
            continue;
        }
        let mut kept: Vec<String> = Vec::new();
        let mut bytes = 0u64;
        for line in out.iter().rev().take(max_lines) {
            bytes += line.len() as u64 + 1;
            if bytes > max_bytes {
                break;
            }
            kept.push((*line).to_string());
        }
        kept.reverse();
        return Some(kept);
    }
    None
}

/// Seconds since 1970 of a UTC time as `crash::utc` writes it (`2026-11-27T22:37:48Z`).
fn parse_utc(s: &str) -> Option<u64> {
    let s = s.trim().strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.splitn(3, '-').map(str::parse::<i64>);
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.splitn(3, ':').map(str::parse::<i64>);
    let (h, min, sec) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    // Days from 1970 (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + min * 60 + sec).ok()
}

/// [`tail_in`] the installed log; nothing when there is none.
pub fn tail(max_lines: usize, max_bytes: u64) -> Vec<String> {
    dir().map(|d| tail_in(d, max_lines, max_bytes)).unwrap_or_default()
}
