//! Two writers on one design file (#5467): a design kept in a shared or synced folder can be open
//! on two computers at once, and the second Save would silently replace the first one's work.
//!
//! - **Lock files.** While the desktop app has a design open it keeps a small JSON file next to
//!   it, `.~<file>.lock` ([`lock_path`]): the computer, the user, the app version, the process
//!   and when it was opened and last refreshed. The app refreshes it every [`REFRESH_SECS`] and
//!   removes it on close, so a lock left by a crash or a lost connection goes stale
//!   ([`Lock::is_stale`]) and is ignored instead of blocking anyone. It is not a design file, so
//!   the file browser, Open and the recent lists never list it, and failing to write one (a
//!   read-only folder) never stops a design from opening.
//! - **File stamps.** A design remembers its file's size and modification time when it is opened
//!   or saved ([`FileStamp`]); a different stamp at the next Save means another program or
//!   computer wrote the file meanwhile.
//!
//! The browser build keeps neither: it has no shared folders, and no clock in `std`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How often an open design's lock is written again.
pub const REFRESH_SECS: u64 = 60;
/// A lock not refreshed for this long belongs to an app that is gone (three missed refreshes).
pub const STALE_SECS: u64 = 3 * REFRESH_SECS;
/// Largest lock file read (a real one is a few hundred bytes).
const MAX_LOCK_BYTES: u64 = 4096;

/// A file's size and modification time, to tell whether something else wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    /// Nanoseconds since the Unix epoch (`None` where the file system keeps no time).
    pub modified_ns: Option<u128>,
}

/// The stamp of the file at `path` now (`None`: it doesn't exist or can't be read; always `None`
/// in the browser). A symbolic link is followed, as a save follows it.
pub fn stamp(path: &str) -> Option<FileStamp> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
    let modified_ns = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos());
    Some(FileStamp { len: meta.len(), modified_ns })
}

/// The file at `path` is not the one stamped `known`: something else wrote it since. A file that
/// is gone counts as unchanged (saving puts it back and replaces nobody's work).
pub fn changed_since(path: &str, known: &FileStamp) -> bool {
    stamp(path).is_some_and(|now| now != *known)
}

/// The lock file of the design at `path`: `.~<file name>.lock` in the same folder.
pub fn lock_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "design".into());
    path.with_file_name(format!(".~{name}.lock"))
}

/// Seconds since the Unix epoch (0 in the browser, which keeps no lock files).
pub fn now_secs() -> u64 {
    if cfg!(target_arch = "wasm32") {
        return 0;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Who has a design open, as its lock file says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lock {
    /// The computer's name.
    pub host: String,
    pub user: String,
    /// The SolidCraft version that wrote it.
    pub app: String,
    pub pid: u32,
    /// When the design was opened, and when the lock was last written (Unix seconds).
    pub opened: u64,
    pub refreshed: u64,
}

impl Lock {
    /// This app's lock on a design opened at `opened`.
    pub fn mine(opened: u64) -> Lock {
        Lock { host: host_name(), user: user_name(), app: env!("CARGO_PKG_VERSION").into(), pid: std::process::id(), opened, refreshed: opened }
    }

    /// Not refreshed for [`STALE_SECS`] at `now`: the app that wrote it is gone. A lock dated
    /// further ahead than that (another computer's clock far off) is stale too, so a wrong clock
    /// can't keep a design locked.
    pub fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.refreshed) > STALE_SECS || self.refreshed.saturating_sub(now) > STALE_SECS
    }

    /// Written by this process.
    pub fn is_mine(&self) -> bool {
        self.pid == std::process::id() && self.host == host_name()
    }
}

/// The computer's name ("this computer" when it has none).
pub fn host_name() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    let name = gethostname::gethostname().to_string_lossy().trim().to_string();
    #[cfg(target_arch = "wasm32")]
    let name = String::new();
    if name.is_empty() { "this computer".into() } else { name }
}

fn user_name() -> String {
    ["USER", "USERNAME", "LOGNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())).unwrap_or_default()
}

/// The lock next to the design at `path`, if there is a readable one.
pub fn read(path: &Path) -> Option<Lock> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let file = lock_path(path);
    if std::fs::metadata(&file).ok()?.len() > MAX_LOCK_BYTES {
        return None;
    }
    serde_json::from_slice(&std::fs::read(&file).ok()?).ok()
}

/// Another app's fresh lock on the design at `path` at `now`: it is open elsewhere.
pub fn held_elsewhere(path: &Path, now: u64) -> Option<Lock> {
    read(path).filter(|l| !l.is_mine() && !l.is_stale(now))
}

/// Write `lock` next to the design at `path` (replacing whatever lock is there).
pub fn write(path: &Path, lock: &Lock) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(lock).map_err(std::io::Error::other)?;
    crate::safe_write::write_atomic(&lock_path(path), &bytes, false)
}

/// Remove this process's lock on the design at `path`; another app's lock is left alone.
pub fn remove(path: &Path) {
    if read(path).is_some_and(|l| l.is_mine()) {
        let _ = std::fs::remove_file(lock_path(path));
    }
}
