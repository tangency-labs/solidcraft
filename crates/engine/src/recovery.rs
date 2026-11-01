//! Autosave and crash recovery.
//!
//! Each open design keeps its own recovery entry in the recovery folder: `<id>.sc` (the design
//! as of the last autosave), `<id>.json` (its file path, name and time) and `<id>.lock`, which
//! the app holds locked while it runs. Autosaves are written atomically, only when the design
//! has unsaved changes it hasn't written yet; a design with nothing unsaved has no entry. An
//! entry whose lock nobody holds belongs to an app that crashed (or closed with unsaved changes):
//! the next launch offers to recover it.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use solidcraft_doc::Document;

use crate::{EngineError, Result, Session};

/// The recovery folder: `SOLIDCRAFT_RECOVERY_DIR`, else the platform's per-user data folder.
/// Spelled with long names (`vfs::long_names`): an agent's commands reach it by this path, and
/// short (8.3) names are refused there.
pub fn default_dir() -> Option<PathBuf> {
    use solidcraft_io::vfs::long_names;
    // Tests never touch the user's real recovery folder.
    if cfg!(test) {
        return Some(solidcraft_io::vfs::temp_dir().join(format!("solidcraft-test-recovery-{}", std::process::id())));
    }
    if let Some(d) = solidcraft_geom::env::var_os("RECOVERY_DIR").filter(|d| !d.is_empty()) {
        return Some(long_names(Path::new(&d)));
    }
    let env = |k: &str| std::env::var_os(k).filter(|d| !d.is_empty()).map(PathBuf::from);
    let root = if cfg!(windows) {
        env("LOCALAPPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))?
    };
    let (dir, legacy) = dirs_under(&long_names(&root), cfg!(windows) || cfg!(target_os = "macos"));
    // Compatibility shim (docs/compatibility.md): autosaves made before the rename (or, in the
    // Flatpak sandbox, under an earlier app id) are copied to the new folder on first launch; the
    // old folder is left as it was.
    for old in solidcraft_io::legacy_flatpak_paths(&dir).into_iter().chain([legacy]) {
        if let Err(e) = solidcraft_io::adopt_legacy_dir(&dir, &old) {
            log::warn!("recovery: could not copy {} to {}: {e}", old.display(), dir.display());
        }
    }
    Some(dir)
}

/// SolidCraft's per-user data folder (the recovery folder's parent unless
/// `SOLIDCRAFT_RECOVERY_DIR` moves that): `%LOCALAPPDATA%\SolidCraft`,
/// `~/Library/Application Support/SolidCraft`, or `$XDG_DATA_HOME/solidcraft`
/// (`~/.local/share/solidcraft`).
pub fn data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|d| !d.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        Some(env("LOCALAPPDATA")?.join("SolidCraft"))
    } else if cfg!(target_os = "macos") {
        Some(env("HOME")?.join("Library/Application Support/SolidCraft"))
    } else {
        Some(env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))?.join("solidcraft"))
    }
}

/// The recovery folder under the per-user data `root`, and the folder earlier releases used.
/// Windows and macOS name the app's folder in title case, other systems in lower case.
fn dirs_under(root: &Path, title_case: bool) -> (PathBuf, PathBuf) {
    let (name, legacy) = if title_case { ("SolidCraft", "SolveCraft") } else { ("solidcraft", "solvecraft") };
    (root.join(name).join("recovery"), root.join(legacy).join("recovery"))
}

/// What an entry's `<id>.json` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// The design's file, if it had one.
    pub path: Option<String>,
    pub name: String,
    /// When it was autosaved (seconds since 1970).
    pub saved_at: u64,
    pub features: usize,
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Recovery metadata is small; a damaged or hostile entry must not exhaust memory.
const MAX_ENTRY_BYTES: usize = 1 << 20;

/// Read only bounded, regular-file metadata. The second type check and bounded read
/// also protect against files changed while the recovery folder is being scanned.
fn read_entry(path: &Path) -> Option<Entry> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ENTRY_BYTES as u64 {
        return None;
    }
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_ENTRY_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > MAX_ENTRY_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// An entry's design: `<id>.sc`, or `<id>.solidcraft` / `<id>.solvecraft` for an autosave an
/// earlier release wrote (compatibility shim, docs/compatibility.md).
fn design_file(dir: &Path, id: &str) -> PathBuf {
    solidcraft_io::DESIGN_EXTS.iter().map(|ext| file(dir, id, ext)).find(|p| p.is_file()).unwrap_or_else(|| file(dir, id, solidcraft_io::DESIGN_EXT))
}

fn file(dir: &Path, id: &str, ext: &str) -> PathBuf {
    dir.join(format!("{id}.{ext}"))
}

/// Entries left by apps that are no longer running, newest first.
pub fn orphans(dir: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                return None;
            }
            let id = p.file_stem().and_then(|s| s.to_str())?;
            let entry = read_entry(&p)?;
            if !valid_id(id) || entry.id != id || !design_file(dir, id).is_file() {
                return None;
            }
            // Held by a running app: not an orphan.
            let lock = File::open(file(dir, &entry.id, "lock"));
            if let Ok(l) = &lock
                && l.try_lock().is_err()
            {
                return None;
            }
            Some(entry)
        })
        .take(1000)
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.saved_at));
    out
}

/// Read an entry's design.
pub fn load(dir: &Path, id: &str) -> Result<(Document, Entry)> {
    if !valid_id(id) {
        return Err(EngineError::Other(format!("no recovered design `{id}`")));
    }
    let entry = read_entry(&file(dir, id, "json")).filter(|e| e.id == id).ok_or_else(|| EngineError::Other(format!("no recovered design `{id}`")))?;
    let path = design_file(dir, id);
    let bytes = solidcraft_io::limits::read(&path.to_string_lossy(), solidcraft_io::limits::Kind::Design)
        .map_err(|e| EngineError::Other(format!("recovered design `{id}`: {e}")))?;
    Ok((solidcraft_io::read_design(&bytes)?, entry))
}

/// Delete an entry. Only a real one: `<id>.json` must describe entry `id` and its design must
/// be there, so a folder and id that merely name someone's `part.sc` delete nothing.
pub fn remove(dir: &Path, id: &str) -> Result<()> {
    let is_entry = valid_id(id) && read_entry(&file(dir, id, "json")).is_some_and(|e| e.id == id) && design_file(dir, id).is_file();
    if !is_entry {
        return Err(EngineError::Other(format!("no recovered design `{id}`")));
    }
    for ext in ["json", "lock"].into_iter().chain(solidcraft_io::DESIGN_EXTS) {
        let _ = fs::remove_file(file(dir, id, ext));
    }
    Ok(())
}

/// #5064: marker file in the recovery folder listing the designs a launch is reopening.
const REOPEN_MARKER: &str = "reopening.json";

/// A launch that reopens designs marks them here. The marker is removed as each design
/// finishes opening; one still there at the next launch means the last session died (crashed,
/// hung, or was force-quit) while opening those designs — blindly reopening them would brick
/// the app the same way, so the next launch skips them instead (#5064).
pub fn begin_reopen(dir: &Path, paths: &[String]) {
    if paths.is_empty() || fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec(paths) {
        let _ = fs::write(dir.join(REOPEN_MARKER), bytes);
    }
}

/// The designs the previous session was reopening when it died (empty: nothing is stuck).
pub fn stuck_reopen(dir: &Path) -> Vec<String> {
    read_reopen_marker(dir).unwrap_or_default()
}

/// The skip has been said: forget the marker so the launch after the skip reopens normally.
pub fn clear_reopen(dir: &Path) {
    let _ = fs::remove_file(dir.join(REOPEN_MARKER));
}

/// One reopened design finished (opened or failed to): take it off the marker. When the last
/// one is gone the marker goes with it, so a healthy session leaves nothing behind.
pub fn reopen_finished(dir: &Path, path: &str) {
    let Some(mut left) = read_reopen_marker(dir) else { return };
    left.retain(|p| p != path);
    if left.is_empty() {
        clear_reopen(dir);
    } else if let Ok(bytes) = serde_json::to_vec(&left) {
        let _ = fs::write(dir.join(REOPEN_MARKER), bytes);
    }
}

/// The marker's paths, bounded like every other file read from the folder.
fn read_reopen_marker(dir: &Path) -> Option<Vec<String>> {
    let path = dir.join(REOPEN_MARKER);
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_ENTRY_BYTES as u64 {
        return None;
    }
    serde_json::from_slice(&fs::read(&path).ok()?).ok()
}

/// One open design's autosaver. Keep it with that design when switching tabs.
pub struct Autosaver {
    pub dir: PathBuf,
    pub id: String,
    /// Autosave this often while there are unsaved changes.
    pub interval: Duration,
    lock: Option<File>,
    /// The design and what was hidden at the last autosave.
    last: Option<(Arc<Document>, crate::Visibility)>,
    last_at: Instant,
    has_entry: bool,
    /// The autosave being written on a worker thread: the design and what was hidden as it
    /// started, and the outcome once the thread ends. A large design takes long enough to write
    /// (and longer with its evaluated model) that the window must not wait for it.
    writing: Option<Writing>,
}

/// An autosaver going away with a write still running (the app quit without closing it) lets the
/// write end first, so the entry it leaves is whole.
impl Drop for Autosaver {
    fn drop(&mut self) {
        if let Some(w) = self.writing.take() {
            let _ = w.done.recv();
        }
    }
}

/// An autosave a worker thread is writing.
struct Writing {
    doc: Arc<Document>,
    visibility: crate::Visibility,
    done: std::sync::mpsc::Receiver<Result<()>>,
}

/// What one autosave writes, taken from the session in an instant.
struct Snapshot {
    dir: PathBuf,
    id: String,
    doc: Arc<Document>,
    visibility: crate::Visibility,
    path: Option<String>,
}

impl Snapshot {
    /// Write the design and its entry (the slow part: serialising and compressing the design).
    fn write(&self) -> Result<()> {
        let io = |e: std::io::Error| EngineError::Other(format!("autosave: {e}"));
        let bytes = if self.visibility.is_empty() {
            solidcraft_io::write_design_container(&self.doc)?
        } else {
            let mut doc = (*self.doc).clone();
            doc.visibility = self.visibility.clone();
            solidcraft_io::write_design_container(&doc)?
        };
        solidcraft_io::write_atomic(&file(&self.dir, &self.id, solidcraft_io::DESIGN_EXT), &bytes, false).map_err(io)?;
        let entry = Entry {
            id: self.id.clone(),
            path: self.path.clone(),
            name: self.doc.name.clone(),
            saved_at: unix_now(),
            features: self.doc.features.len(),
        };
        let meta = serde_json::to_vec_pretty(&entry).map_err(|e| EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(&file(&self.dir, &self.id, "json"), &meta, false).map_err(io)?;
        Ok(())
    }
}

impl Autosaver {
    /// Start autosaving into `dir` (created if needed), holding this design's lock.
    pub fn new(dir: &Path, interval: Duration) -> Result<Autosaver> {
        let io = |e: std::io::Error| EngineError::Other(format!("{}: {e}", dir.display()));
        fs::create_dir_all(dir).map_err(io)?;
        // Locks of apps that ended with nothing to recover.
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("lock")
                && !p.with_extension("json").exists()
                && File::open(&p).is_ok_and(|l| l.try_lock().is_ok())
            {
                let _ = fs::remove_file(&p);
            }
        }
        // Unique within the process too: an app started again in the same second (tests that
        // "crash" and restart) must not take over the crashed app's entry.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("{}-{}-{seq}", unix_now(), std::process::id());
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(file(dir, &id, "lock")).map_err(io)?;
        lock.lock().map_err(io)?;
        Ok(Autosaver { dir: dir.to_path_buf(), id, interval, lock: Some(lock), last: None, last_at: Instant::now(), has_entry: false, writing: None })
    }

    /// Autosave now if the design has unsaved changes not written yet (and drop the entry when
    /// nothing is unsaved), waiting for the write. Returns whether it wrote.
    pub fn save(&mut self, s: &Session) -> Result<bool> {
        self.finish(s, true)?;
        let Some(snapshot) = self.begin(s) else { return Ok(false) };
        let written = snapshot.write();
        self.written(s, snapshot.doc, snapshot.visibility, written)?;
        Ok(true)
    }

    /// Autosave when the interval has passed, or right away after a big operation. The design
    /// is written on a worker thread; true when this call started a write.
    pub fn tick(&mut self, s: &Session, big: bool) -> Result<bool> {
        // A write still running: nothing new starts until it has ended.
        if !self.finish(s, false)? {
            return Ok(false);
        }
        if !big && self.last_at.elapsed() < self.interval {
            return Ok(false);
        }
        let Some(snapshot) = self.begin(s) else { return Ok(false) };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let (doc, visibility) = (snapshot.doc.clone(), snapshot.visibility.clone());
            let started = std::thread::Builder::new().name("autosave".into()).spawn(move || {
                let _ = tx.send(snapshot.write());
            });
            match started {
                Ok(_) => {
                    self.writing = Some(Writing { doc, visibility, done: rx });
                    Ok(true)
                }
                // No thread to write on: write here, as before.
                Err(_) => {
                    let snapshot = Snapshot { dir: self.dir.clone(), id: self.id.clone(), doc, visibility, path: s.path.clone() };
                    let written = snapshot.write();
                    self.written(s, snapshot.doc, snapshot.visibility, written)?;
                    Ok(true)
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let written = snapshot.write();
            self.written(s, snapshot.doc, snapshot.visibility, written)?;
            Ok(true)
        }
    }

    /// What the next autosave writes: `None` when there is nothing to write (and, with nothing
    /// unsaved, the entry has been dropped).
    fn begin(&mut self, s: &Session) -> Option<Snapshot> {
        self.last_at = Instant::now();
        if !s.is_dirty() {
            if self.has_entry {
                for ext in ["json"].into_iter().chain(solidcraft_io::DESIGN_EXTS) {
                    let _ = fs::remove_file(file(&self.dir, &self.id, ext));
                }
                self.has_entry = false;
            }
            self.last = None;
            // Crash replays start from the saved file (or a new design) from here.
            crate::crash::rebased(s, s.path.as_deref().map(Path::new), false);
            return None;
        }
        if self.last.as_ref().is_some_and(|(l, v)| (Arc::ptr_eq(l, &s.doc) || **l == *s.doc) && *v == s.visibility) {
            return None;
        }
        Some(Snapshot { dir: self.dir.clone(), id: self.id.clone(), doc: s.doc.clone(), visibility: s.visibility.clone(), path: s.path.clone() })
    }

    /// The running write's outcome, once it has one (`wait`: wait for it). True when no write
    /// is running any more.
    fn finish(&mut self, s: &Session, wait: bool) -> Result<bool> {
        let Some(w) = self.writing.take() else { return Ok(true) };
        let outcome = if wait {
            w.done.recv().unwrap_or_else(|_| Err(EngineError::Other("autosave: the writer stopped".into())))
        } else {
            match w.done.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.writing = Some(w);
                    return Ok(false);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(EngineError::Other("autosave: the writer stopped".into())),
            }
        };
        self.written(s, w.doc, w.visibility, outcome)?;
        Ok(true)
    }

    /// A write of `doc` (with `visibility`) ended with `outcome`.
    fn written(&mut self, s: &Session, doc: Arc<Document>, visibility: crate::Visibility, outcome: Result<()>) -> Result<()> {
        outcome?;
        self.has_entry = true;
        // Crash replays start from this autosave from here, unless the design changed while it
        // was written: the journal then keeps the commands since its older start.
        if Arc::ptr_eq(&s.doc, &doc) {
            crate::crash::rebased(s, Some(&file(&self.dir, &self.id, solidcraft_io::DESIGN_EXT)), true);
        }
        self.last = Some((doc, visibility));
        Ok(())
    }

    /// The user closed this design (saving it or choosing Don't Save): discard only its entry.
    pub fn discard(mut self) {
        // A write still running would recreate the entry, without a lock, after it is removed:
        // the next launch would offer a design the user closed.
        if let Some(w) = self.writing.take() {
            let _ = w.done.recv();
        }
        for ext in ["json"].into_iter().chain(solidcraft_io::DESIGN_EXTS) {
            let _ = fs::remove_file(file(&self.dir, &self.id, ext));
        }
        self.lock = None;
        let _ = fs::remove_file(file(&self.dir, &self.id, "lock"));
    }

    /// Closing the app: with nothing unsaved the entry goes; otherwise it stays for the next
    /// launch to offer. The lock is released either way.
    pub fn close(mut self, s: &Session) {
        let _ = self.save(s);
        if !self.has_entry {
            let _ = fs::remove_file(file(&self.dir, &self.id, "lock"));
        }
        self.lock = None;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn autosave_from_a_legacy_folder_is_listed_and_loads() {
        // Autosaves written by earlier releases: `.solvecraft` (before the rename) and
        // `.solidcraft` (before designs were saved as `.sc`).
        for ext in solidcraft_io::LEGACY_DESIGN_EXTS {
            let d = std::env::temp_dir().join(format!("solidcraft-recovery-legacy-{ext}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            let id = "1-2-3";
            let doc = Document::default();
            fs::write(d.join(format!("{id}.{ext}")), solidcraft_io::write_design(&doc)).unwrap();
            let entry = Entry { id: id.into(), path: Some(format!("/d/part.{ext}")), name: "part".into(), saved_at: 1, features: 0 };
            fs::write(d.join(format!("{id}.json")), serde_json::to_vec(&entry).unwrap()).unwrap();
            assert_eq!(orphans(&d).iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [id], ".{ext}");
            let (_, loaded) = load(&d, id).unwrap();
            assert_eq!(loaded, entry);
            remove(&d, id).unwrap();
            assert!(orphans(&d).is_empty() && !d.join(format!("{id}.{ext}")).exists(), ".{ext}");
            let _ = fs::remove_dir_all(&d);
        }
    }

    /// New autosaves are written as `<id>.sc`.
    #[test]
    fn autosaves_are_written_as_sc() {
        let d = dir("sc-ext");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::ZERO).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(a.save(&s).unwrap());
        assert!(d.join(format!("{}.sc", a.id)).is_file());
        assert!(!d.join(format!("{}.solidcraft", a.id)).exists());
        a.discard();
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn recovery_dirs_and_their_legacy_names() {
        let (dir, legacy) = super::dirs_under(Path::new("/data"), false);
        assert_eq!((dir.as_path(), legacy.as_path()), (Path::new("/data/solidcraft/recovery"), Path::new("/data/solvecraft/recovery")));
        let (dir, legacy) = super::dirs_under(Path::new("/data"), true);
        assert_eq!((dir.as_path(), legacy.as_path()), (Path::new("/data/SolidCraft/recovery"), Path::new("/data/SolveCraft/recovery")));
    }

    use super::*;
    use serde_json::json;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-recovery-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn autosave_only_unsaved_changes_and_recover_after_a_crash() {
        let d = dir("crash");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::from_secs(3600)).unwrap();
        assert!(!a.save(&s).unwrap(), "nothing unsaved");
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(!a.tick(&s, false).unwrap(), "not due yet");
        assert!(a.tick(&s, true).unwrap(), "after a big operation");
        assert!(!a.save(&s).unwrap(), "unchanged since the last autosave");
        // Our own entry is locked: not offered.
        assert!(orphans(&d).is_empty());
        // The app dies without closing (the lock goes with the process).
        drop(a);
        let found = orphans(&d);
        assert_eq!(found.len(), 1);
        let (doc, entry) = load(&d, &found[0].id).unwrap();
        assert_eq!(doc, *s.doc);
        assert_eq!(entry.features, 1);
        remove(&d, &entry.id).unwrap();
        assert!(orphans(&d).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// A tick doesn't wait for the autosave (#3044): it starts the write on a worker and
    /// returns; the outcome is taken up by a later tick, and nothing new starts meanwhile. The
    /// entry is the design as it was when the write started, even when it changed since.
    #[test]
    fn autosave_writes_on_a_worker_and_a_later_tick_takes_it_up() {
        let d = dir("worker");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::ZERO).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let written = s.doc.clone();
        assert!(a.tick(&s, false).unwrap(), "a write started");
        assert!(a.writing.is_some(), "and runs on its own thread");
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "center": [20, 0, 2.5], "operation": "new"})).unwrap();
        // Ticks while it runs start nothing; the one that finds it ended takes it up and,
        // the design having changed since, starts the next write.
        let began = Instant::now();
        while !a.tick(&s, false).unwrap() {
            assert!(a.writing.is_some(), "nothing new starts while the write runs");
            assert!(began.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(a.has_entry);
        assert!(a.last.as_ref().is_some_and(|(doc, _)| Arc::ptr_eq(doc, &written)), "the first write was the design as it started");
        assert!(a.writing.as_ref().is_some_and(|w| Arc::ptr_eq(&w.doc, &s.doc)), "the next one writes the changed design");
        drop(a);
        let found = orphans(&d);
        assert_eq!(found.len(), 1);
        assert_eq!(load(&d, &found[0].id).unwrap().0, *s.doc, "dropping the autosaver lets its write end");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_app_restarted_in_the_same_second_still_offers_the_crashed_ones_work() {
        let d = dir("restart");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::from_secs(3600)).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(a.save(&s).unwrap());
        drop(a);
        let b = Autosaver::new(&d, Duration::from_secs(3600)).unwrap();
        let found = orphans(&d);
        assert_eq!(found.len(), 1, "the crashed app's entry is offered");
        assert_ne!(found[0].id, b.id);
        let _ = fs::remove_dir_all(&d);
    }

    /// `doc.recovery_discard` deletes recovery entries only (#205): a folder and id that merely
    /// name someone's design, or a `<id>.json` describing another entry, delete nothing.
    #[test]
    fn discarding_refuses_what_is_not_a_recovery_entry() {
        let d = dir("discard");
        fs::create_dir_all(&d).unwrap();
        let part = d.join("part.solidcraft");
        fs::write(&part, solidcraft_io::write_design(&Document::default())).unwrap();
        let mut s = Session::default();
        let discard = |s: &mut Session, id: &str| s.execute("doc.recovery_discard", &json!({"dir": d.to_string_lossy(), "id": id}));
        let e = discard(&mut s, "part").unwrap_err();
        assert!(e.to_string().contains("no recovered design `part`"), "{e}");
        let other = Entry { id: "other".into(), path: None, name: "x".into(), saved_at: 1, features: 0 };
        fs::write(d.join("part.json"), serde_json::to_vec(&other).unwrap()).unwrap();
        assert!(discard(&mut s, "part").is_err());
        assert!(discard(&mut s, "../part").is_err());
        assert!(part.exists() && d.join("part.json").exists(), "nothing deleted");
        // A real entry still goes.
        let mine = Entry { id: "part".into(), ..other };
        fs::write(d.join("part.json"), serde_json::to_vec(&mine).unwrap()).unwrap();
        assert_eq!(discard(&mut s, "part").unwrap()["discarded"], json!(["part"]));
        assert!(!part.exists());
        let _ = fs::remove_dir_all(&d);
    }

    /// `doc.recover {discard: true}` opens a real entry and then deletes it; given a folder and
    /// id that only name someone's design, it deletes nothing (#205 review).
    #[test]
    fn recover_with_discard_deletes_only_the_entry_it_opened() {
        let d = dir("recover-discard");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::from_secs(3600)).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(a.save(&s).unwrap());
        let id = a.id.clone();
        drop(a);
        let mut t = Session::default();
        let r = t.execute("doc.recover", &json!({"dir": d.to_string_lossy(), "id": id, "discard": true})).unwrap();
        assert_eq!(r["features"], 1, "{r}");
        assert!(orphans(&d).is_empty() && !file(&d, &id, "json").exists() && !file(&d, &id, solidcraft_io::DESIGN_EXT).exists());
        let part = d.join("part.solidcraft");
        fs::write(&part, solidcraft_io::write_design(&Document::default())).unwrap();
        assert!(t.execute("doc.recover", &json!({"dir": d.to_string_lossy(), "id": "part", "discard": true})).is_err());
        assert!(part.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_drops_the_entry_and_a_clean_close_leaves_nothing() {
        let d = dir("clean");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::ZERO).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(a.tick(&s, false).unwrap());
        s.mark_saved();
        a.tick(&s, false).unwrap();
        assert!(!file(&d, &a.id, solidcraft_io::DESIGN_EXT).exists());
        a.close(&s);
        assert_eq!(fs::read_dir(&d).unwrap().count(), 0);
        // Closing with unsaved changes keeps them for the next launch.
        let b = Autosaver::new(&d, Duration::ZERO).unwrap();
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap();
        b.close(&s);
        assert_eq!(orphans(&d).len(), 1);
        assert!(load(&d, "../etc").is_err());
        let _ = fs::remove_dir_all(&d);
    }

    /// A corrupted or hostile recovery entry must not consume unbounded memory at startup.
    /// A metadata file exactly at the limit is still accepted.
    #[test]
    fn oversized_recovery_metadata_does_not_hide_valid_entries() {
        let d = dir("bounded-metadata");
        fs::create_dir_all(&d).unwrap();
        let good = Entry { id: "good".into(), path: None, name: "good".into(), saved_at: 1, features: 0 };
        let mut boundary = Entry { id: "boundary".into(), path: None, name: String::new(), saved_at: 2, features: 0 };
        let base_len = serde_json::to_vec(&boundary).unwrap().len();
        boundary.name = "x".repeat((1 << 20) - base_len);
        let oversized = Entry { id: "oversized".into(), name: "x".repeat(1 << 20), ..good.clone() };
        for entry in [&good, &boundary, &oversized] {
            fs::write(file(&d, &entry.id, solidcraft_io::DESIGN_EXT), solidcraft_io::write_design(&Document::default())).unwrap();
            fs::write(file(&d, &entry.id, "json"), serde_json::to_vec(entry).unwrap()).unwrap();
        }
        assert_eq!(fs::metadata(file(&d, "boundary", "json")).unwrap().len(), 1 << 20);
        assert!(fs::metadata(file(&d, "oversized", "json")).unwrap().len() > 1 << 20);
        let mut ids: Vec<String> = orphans(&d).into_iter().map(|e| e.id).collect();
        ids.sort();
        assert_eq!(ids, ["boundary", "good"]);
        assert_eq!(load(&d, "good").unwrap().1, good);
        assert_eq!(load(&d, "boundary").unwrap().1, boundary);
        assert!(load(&d, "oversized").is_err());
        assert!(remove(&d, "oversized").is_err());
        assert!(file(&d, "oversized", "json").exists(), "invalid metadata must not delete a design");
        fs::remove_dir_all(&d).unwrap();
    }

    /// A mismatched metadata ID must not redirect the recovered design's save path.
    #[test]
    fn recovery_metadata_id_must_match_its_filename() {
        let d = dir("metadata-id");
        fs::create_dir_all(&d).unwrap();
        let wrong = Entry { id: "other".into(), path: Some("/unrelated-design.solidcraft".into()), name: "wrong".into(), saved_at: 1, features: 0 };
        fs::write(file(&d, "actual", "json"), serde_json::to_vec(&wrong).unwrap()).unwrap();
        for id in ["actual", "other"] {
            fs::write(file(&d, id, solidcraft_io::DESIGN_EXT), solidcraft_io::write_design(&Document::default())).unwrap();
        }
        assert!(orphans(&d).is_empty(), "metadata under another ID must not be offered");
        assert!(load(&d, "actual").is_err(), "a mismatched path must not become the next save target");
        assert!(remove(&d, "actual").is_err());
        assert!(file(&d, "actual", solidcraft_io::DESIGN_EXT).exists());
        fs::remove_dir_all(&d).unwrap();
    }

    /// A device posing as a JSON file must not block the startup recovery scan.
    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_scan_skips_non_regular_metadata_files() {
        use std::os::unix::fs::symlink;

        let d = dir("device-metadata");
        fs::create_dir_all(&d).unwrap();
        symlink("/dev/zero", file(&d, "device", "json")).unwrap();
        assert!(orphans(&d).is_empty());
        assert!(load(&d, "device").is_err());
        assert!(remove(&d, "device").is_err());
        fs::remove_dir_all(&d).unwrap();
    }

    /// #5064: a design that kills the app while it is being reopened must not be blindly
    /// reopened every launch. The marker survives the death, the next launch sees what the
    /// last one died on, and a healthy session clears it by itself.
    #[test]
    fn reopen_guard_survives_a_session_that_died_mid_reopen() {
        let d = dir("reopen-guard");
        assert!(stuck_reopen(&d).is_empty(), "no marker: nothing is stuck");
        begin_reopen(&d, &[]);
        assert!(!d.join(REOPEN_MARKER).exists(), "nothing to reopen writes no marker");

        let paths = vec!["/d/one.solidcraft".to_string(), "/d/two.solidcraft".to_string()];
        begin_reopen(&d, &paths);
        assert_eq!(stuck_reopen(&d), paths);
        // The first design opens; the session dies on the second.
        reopen_finished(&d, "/d/one.solidcraft");
        assert_eq!(stuck_reopen(&d), vec!["/d/two.solidcraft".to_string()]);
        // The skip clears the marker so the launch after it reopens normally.
        clear_reopen(&d);
        assert!(stuck_reopen(&d).is_empty());
        // A healthy session: every design finishes and the marker goes with the last one.
        begin_reopen(&d, &paths);
        reopen_finished(&d, "/d/one.solidcraft");
        reopen_finished(&d, "/d/two.solidcraft");
        assert!(stuck_reopen(&d).is_empty() && !d.join(REOPEN_MARKER).exists());
        // A damaged marker is ignored, not fatal.
        fs::write(d.join(REOPEN_MARKER), b"not json").unwrap();
        assert!(stuck_reopen(&d).is_empty());
        reopen_finished(&d, "/d/one.solidcraft");
        fs::remove_dir_all(&d).unwrap();
    }
}
