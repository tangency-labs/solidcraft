//! The app's log on disk: what SolidCraft's crates log at info and above, and other crates'
//! warnings, one line each, in `<data folder>/logs/solidcraft.log`. Each start moves the last
//! run's log to `solidcraft.previous.log`, so a report sent after a crash still has the lines of
//! the run that crashed. A problem report attaches the end of it ([`tail`]).
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

/// Start the log in `dir`: last run's log becomes [`PREVIOUS`], and this run writes [`FILE`].
/// Returns the file, or `None` when it could not be made or a logger was already installed.
pub fn install(dir: &Path) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let path = dir.join(FILE);
    // Another window's process may still hold it (Windows then refuses the move): this run then
    // adds to the same file.
    if path.is_file() {
        let _ = fs::rename(&path, dir.join(PREVIOUS));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
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

/// [`tail_in`] the installed log; nothing when there is none.
pub fn tail(max_lines: usize, max_bytes: u64) -> Vec<String> {
    dir().map(|d| tail_in(d, max_lines, max_bytes)).unwrap_or_default()
}
