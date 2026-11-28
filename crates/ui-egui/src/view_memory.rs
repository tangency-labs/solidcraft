//! What each saved design's views were when it was last closed, kept for the user (never in the
//! design file) so "Reopen a design's views next time it opens" can bring them back. A small map
//! from a design's path to its [`ViewSet`] saved form, newest last. The desktop app keeps it in a
//! file in the user's data folder; the browser has no such folder, so it keeps it in the browser's
//! own storage ([`Store::Host`], #23271).

use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use solidcraft_views::{ViewKind, ViewSet};

/// The file in the data folder.
const FILE: &str = "view-memory.json";
/// How many designs are remembered; the least recently closed go first.
const CAP: usize = 200;
/// A file larger than this is ignored (it is not ours).
const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Where the remembered views are kept.
#[derive(Default)]
pub enum Store {
    /// The file in the user's data folder ([`solidcraft_engine::recovery::data_dir`]): the desktop
    /// app's. Without that folder nothing is remembered.
    #[default]
    DataFolder,
    /// The file in this folder (tests).
    #[cfg(test)]
    Folder(PathBuf),
    /// In memory, for the host to keep (the browser's storage).
    Host(Host),
}

impl Store {
    fn folder(&self) -> Option<PathBuf> {
        match self {
            Store::DataFolder => solidcraft_engine::recovery::data_dir(),
            #[cfg(test)]
            Store::Folder(dir) => Some(dir.clone()),
            Store::Host(_) => None,
        }
    }

    /// Remember `views` (a [`ViewSet::to_json`]) for the design saved at `path`. A file is written
    /// off the window thread.
    pub fn remember(&mut self, path: &str, views: Value) -> std::io::Result<()> {
        if let Store::Host(host) = self {
            put(&mut host.entries, path, views.clone());
            host.changed.push((path.to_owned(), views));
            host.failed = false;
            return Ok(());
        }
        match self.folder() {
            Some(dir) => remember_async(dir, path.to_owned(), views),
            None => Ok(()),
        }
    }

    /// What was remembered for the design saved at `path`.
    pub fn recall(&self, path: &str) -> Option<Value> {
        match self {
            Store::Host(host) => find(&host.entries, path),
            _ => recall(&self.folder()?, path),
        }
    }
}

/// The remembered views a host keeps itself: it loads them with [`Host::load`], saves what
/// [`Host::pending_text`] gives and says with [`Host::saved`] whether its storage took it.
#[derive(Default)]
pub struct Host {
    entries: Vec<(String, Value)>,
    /// What was remembered and not yet taken by the host's storage, oldest first.
    changed: Vec<(String, Value)>,
    /// The storage refused the last save (full, or not there): it is tried again with the next
    /// change, not every frame.
    failed: bool,
}

impl Host {
    /// The remembered views from what the host saved (none, damaged or foreign: none).
    pub fn load(saved: Option<&str>) -> Host {
        Host { entries: parse_saved(saved), changed: Vec::new(), failed: false }
    }

    /// Whether there is something to save now.
    pub fn changed(&self) -> bool {
        !self.changed.is_empty() && !self.failed
    }

    /// The text to save: what is saved now (`saved_now`, which another page of the app may have
    /// changed since) with every change of this page on top that the storage hasn't taken yet.
    pub fn pending_text(&mut self, saved_now: Option<&str>) -> Option<String> {
        if self.changed.is_empty() {
            return None;
        }
        self.entries = parse_saved(saved_now);
        for (path, views) in &self.changed {
            put(&mut self.entries, path, views.clone());
        }
        Some(to_text(&self.entries))
    }

    /// Whether the storage took the text [`Self::pending_text`] gave. If it did, those changes are
    /// kept; if not, they stay for the next try, so the latest camera of a closed design isn't
    /// lost.
    pub fn saved(&mut self, taken: bool) {
        if taken {
            self.changed.clear();
        }
        self.failed = !taken;
    }
}

fn parse_saved(saved: Option<&str>) -> Vec<(String, Value)> {
    saved.filter(|text| text.len() as u64 <= MAX_BYTES).map(|text| parse(text.as_bytes())).unwrap_or_default()
}

/// The remembered designs in a saved list, oldest first. Anything unreadable gives none.
fn parse(bytes: &[u8]) -> Vec<(String, Value)> {
    let Ok(Value::Array(items)) = serde_json::from_slice(bytes) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, Value)> =
        items.into_iter().filter_map(|item| Some((item.get("path")?.as_str()?.to_owned(), item.get("views")?.clone()))).collect();
    let skip = entries.len().saturating_sub(CAP);
    entries.drain(..skip);
    entries
}

/// `views` become the newest for `path`; the least recently remembered go past [`CAP`].
fn put(entries: &mut Vec<(String, Value)>, path: &str, views: Value) {
    entries.retain(|(known, _)| known != path);
    entries.push((path.to_owned(), views));
    let skip = entries.len().saturating_sub(CAP);
    entries.drain(..skip);
}

fn find(entries: &[(String, Value)], path: &str) -> Option<Value> {
    entries.iter().rev().find(|(known, _)| known == path).map(|(_, views)| views.clone())
}

fn to_text(entries: &[(String, Value)]) -> String {
    let list: Vec<Value> = entries.iter().map(|(path, views)| json!({"path": path, "views": views})).collect();
    Value::Array(list).to_string()
}

/// The remembered designs, oldest first. Anything unreadable gives none.
fn read(dir: &Path) -> Vec<(String, Value)> {
    let file = dir.join(FILE);
    if std::fs::metadata(&file).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        return Vec::new();
    }
    std::fs::read(&file).map(|bytes| parse(&bytes)).unwrap_or_default()
}

/// Remember `views` (a [`ViewSet::to_json`]) for the design saved at `path`, in the folder `dir`.
pub fn remember(dir: &Path, path: &str, views: Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(not(target_arch = "wasm32"))]
    let _lock = lock_memory(dir)?;
    let mut entries = read(dir);
    put(&mut entries, path, views);
    solidcraft_engine::io::safe_write::write_atomic(&dir.join(FILE), to_text(&entries).as_bytes(), false)
}

/// Keep the entire read-modify-write transaction exclusive across application processes.
/// The lockfile stays in place: deleting it would let two processes lock different files.
/// A busy lock is waited for until its holder releases it (or its process ends, which releases
/// it too), however long that takes: giving up on a wall-clock limit dropped a design's views
/// whenever the holder was slow, as on a loaded machine (#135). This runs on the
/// `remember-views` worker, never the window thread; quitting bounds its own wait
/// ([`quit_may_proceed`]).
#[cfg(not(target_arch = "wasm32"))]
fn lock_memory(dir: &Path) -> std::io::Result<std::fs::File> {
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("view-memory.lock"))?;
    match lock.try_lock() {
        Ok(()) => return Ok(lock),
        Err(std::fs::TryLockError::Error(error)) => return Err(error),
        Err(std::fs::TryLockError::WouldBlock) => {
            #[cfg(test)]
            tests::saw_busy(dir);
        }
    }
    lock.lock()?;
    Ok(lock)
}

/// How long quitting waits for queued view memories to reach disk before closing anyway.
/// A transaction is a small read and an atomic write, done in well under a second even on a
/// loaded machine; 10 s covers a slow disk or another SolidCraft process finishing its own
/// transaction, yet a holder that never lets go (a stopped process) cannot keep the app open.
#[cfg(not(target_arch = "wasm32"))]
pub const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The paths of the designs whose views are queued but not yet on disk, oldest first.
#[cfg(not(target_arch = "wasm32"))]
static PENDING_PATHS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The designs whose views are queued but not yet on disk.
#[cfg(not(target_arch = "wasm32"))]
pub fn unsaved() -> Vec<String> {
    PENDING_PATHS.lock().map(|paths| paths.clone()).unwrap_or_default()
}

/// What quitting has waited for so far: when it started waiting, and whether it gave up.
#[derive(Clone, Copy, Debug, Default)]
pub struct QuitWait {
    #[cfg(not(target_arch = "wasm32"))]
    since: Option<web_time::Instant>,
    #[cfg(not(target_arch = "wasm32"))]
    gave_up: bool,
}

/// Whether quitting may close now: no view memory is still queued, or quitting has already
/// waited [`QUIT_WAIT`] for them. On giving up it logs, once, which designs' views were not
/// saved; those designs reopen as they would have without remembered views.
pub fn quit_may_proceed(wait: &std::cell::Cell<QuitWait>) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        quit_may_proceed_at(wait, web_time::Instant::now())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = wait;
        true
    }
}

/// [`quit_may_proceed`] at the time `now`.
#[cfg(not(target_arch = "wasm32"))]
fn quit_may_proceed_at(wait: &std::cell::Cell<QuitWait>, now: web_time::Instant) -> bool {
    let mut state = wait.get();
    if !pending() {
        wait.set(QuitWait::default());
        return true;
    }
    if state.gave_up {
        return true;
    }
    let since = *state.since.get_or_insert(now);
    let gave_up = now.saturating_duration_since(since) >= QUIT_WAIT;
    if gave_up {
        state.gave_up = true;
        log::warn!("quitting without saving the remembered views of {} after waiting {} s", unsaved().join(", "), QUIT_WAIT.as_secs());
    }
    wait.set(state);
    gave_up
}

#[cfg(not(target_arch = "wasm32"))]
static PENDING: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(target_arch = "wasm32"))]
struct PendingWrite(String);

#[cfg(not(target_arch = "wasm32"))]
impl PendingWrite {
    fn start(path: &str) -> Self {
        if let Ok(mut paths) = PENDING_PATHS.lock() {
            paths.push(path.to_owned());
        }
        PENDING.fetch_add(1, Ordering::AcqRel);
        PendingWrite(path.to_owned())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for PendingWrite {
    fn drop(&mut self) {
        if let Ok(mut paths) = PENDING_PATHS.lock()
            && let Some(at) = paths.iter().position(|path| *path == self.0)
        {
            paths.remove(at);
        }
        PENDING.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct WriteRequest {
    dir: PathBuf,
    path: String,
    views: Value,
    _pending: PendingWrite,
}

#[cfg(not(target_arch = "wasm32"))]
enum Request {
    Write(WriteRequest),
    #[cfg(test)]
    Barrier(std::sync::mpsc::Sender<()>),
}

#[cfg(not(target_arch = "wasm32"))]
static WRITER: std::sync::Mutex<Option<std::sync::mpsc::Sender<Request>>> = std::sync::Mutex::new(None);

/// Schedule the disk transaction off the window thread. A single worker preserves enqueue
/// order, including two snapshots of the same design. Quit waits for `pending` to clear.
pub fn remember_async(dir: PathBuf, path: String, views: Value) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut writer = WRITER.lock().map_err(|_| std::io::Error::other("remembered views worker unavailable"))?;
        if writer.is_none() {
            let (sender, receiver) = std::sync::mpsc::channel::<Request>();
            std::thread::Builder::new().name("remember-views".into()).spawn(move || {
                for request in receiver {
                    match request {
                        Request::Write(request) => {
                            if let Err(error) = remember(&request.dir, &request.path, request.views) {
                                log::warn!("could not remember the views of {}: {error}", request.path);
                            }
                        }
                        #[cfg(test)]
                        Request::Barrier(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })?;
            *writer = Some(sender);
        }
        let Some(sender) = writer.as_ref() else { return Err(std::io::Error::other("remembered views worker unavailable")) };
        let pending = PendingWrite::start(&path);
        let request = WriteRequest { dir, path, views, _pending: pending };
        if sender.send(Request::Write(request)).is_err() {
            *writer = None;
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "remembered views worker stopped"));
        }
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        remember(&dir, &path, views)
    }
}

/// Whether a scheduled native cache write still needs the application process to remain alive.
/// Native only: on the web a write happens at once, so quitting never waits for one.
#[cfg(not(target_arch = "wasm32"))]
pub fn pending() -> bool {
    PENDING.load(Ordering::Acquire) != 0
}

/// Tests enqueue a barrier behind their writes, then wait for its acknowledgement.
/// This uses the same production worker without timing assumptions or alternate writes.
#[cfg(test)]
pub fn wait_for_pending() -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (done, finished) = std::sync::mpsc::channel();
        {
            let writer = WRITER.lock().map_err(|_| std::io::Error::other("remembered views worker unavailable"))?;
            let Some(sender) = writer.as_ref() else { return Ok(()) };
            sender
                .send(Request::Barrier(done))
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "remembered views worker stopped"))?;
        }
        finished.recv().map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "remembered views worker stopped"))?;
    }
    Ok(())
}

/// What was remembered for the design saved at `path` in the folder `dir`.
pub fn recall(dir: &Path, path: &str) -> Option<Value> {
    find(&read(dir), path)
}

/// The views of a remembered `saved` set that `can_show` accepts (given each one's kind and
/// subject), as a set of their own; the closed ones are not brought back. None when the saved form
/// is not a set or no view is left.
pub fn restorable(saved: &Value, can_show: impl Fn(ViewKind, Option<&str>) -> bool) -> Option<ViewSet> {
    // Keep legacy Sketch tabs until migration has chosen their Model fallback. Continue to
    // ignore unavailable or foreign kinds before validating the remembered layout.
    let mut readable = saved.clone();
    readable.get_mut("views")?.as_array_mut()?.retain(|view| {
        let kind = view.get("kind").and_then(Value::as_str);
        kind == Some("sketch") || kind.and_then(ViewKind::from_key).is_some_and(|kind| can_show(kind, view.get("subject").and_then(Value::as_str)))
    });
    readable.as_object_mut()?.remove("closed");
    let mut set = ViewSet::from_json(&readable)?.to_json();
    let views = set.get_mut("views")?.as_array_mut()?;
    views.retain(|view| {
        let kind = view.get("kind").and_then(Value::as_str).and_then(ViewKind::from_key);
        kind.is_some_and(|kind| can_show(kind, view.get("subject").and_then(Value::as_str)))
    });
    if views.is_empty() {
        return None;
    }
    set.as_object_mut()?.remove("closed");
    ViewSet::from_json(&set)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-view-memory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn set_with_render() -> ViewSet {
        let mut set = ViewSet::new();
        set.open(ViewKind::Render, Some("7".into()), true);
        set.open(ViewKind::Sheet, Some("d1".into()), true);
        set
    }

    /// Folders whose next refused lock attempt a test is waiting for (the waiter is then known
    /// to be queued behind the holder, with no timing guess).
    #[cfg(not(target_arch = "wasm32"))]
    static BUSY_WATCH: std::sync::Mutex<Vec<(std::path::PathBuf, std::sync::mpsc::Sender<()>)>> = std::sync::Mutex::new(Vec::new());

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn saw_busy(dir: &Path) {
        if let Ok(mut watch) = BUSY_WATCH.lock() {
            watch.retain(|(watched, signal)| watched != dir || signal.send(()).is_err());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn watch_busy(dir: &Path) -> std::sync::mpsc::Receiver<()> {
        let (signal, seen) = std::sync::mpsc::channel();
        BUSY_WATCH.lock().unwrap().push((dir.to_owned(), signal));
        seen
    }

    /// #135: a transaction queued behind a slow holder (a loaded machine, another SolidCraft
    /// process) waits for that holder's release, however long it takes, and loses no design.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_transaction_waits_for_a_slow_holder_and_keeps_both_designs() {
        let dir = folder("slow-holder");
        remember(&dir, "a.sc", ViewSet::new().to_json()).unwrap();
        let busy = watch_busy(&dir);
        let held = lock_memory(&dir).unwrap();
        let waiter = {
            let dir = dir.clone();
            std::thread::spawn(move || remember(&dir, "b.sc", ViewSet::new().to_json()))
        };
        busy.recv().unwrap();
        // Longer than any wall-clock budget the waiter used to give the holder (2 s).
        std::thread::sleep(std::time::Duration::from_millis(2500));
        drop(held);
        waiter.join().unwrap().unwrap();
        assert!(recall(&dir, "a.sc").is_some() && recall(&dir, "b.sc").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Quitting waits for a queued view memory held up by another holder, but only for
    /// [`QUIT_WAIT`]: then it closes and names the design whose views were not saved. The write
    /// itself still completes once the holder lets go.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quitting_waits_for_a_held_view_memory_only_for_the_quit_bound() {
        let dir = folder("quit-bound");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("held.sc").to_string_lossy().into_owned();
        let busy = watch_busy(&dir);
        let held = lock_memory(&dir).unwrap();
        remember_async(dir.clone(), path.clone(), ViewSet::new().to_json()).unwrap();
        busy.recv().unwrap();
        let wait = std::cell::Cell::new(QuitWait::default());
        let start = web_time::Instant::now();
        assert!(!quit_may_proceed_at(&wait, start), "a queued write holds quitting at first");
        assert!(!quit_may_proceed_at(&wait, start + QUIT_WAIT - std::time::Duration::from_millis(1)));
        assert!(unsaved().contains(&path));
        assert!(quit_may_proceed_at(&wait, start + QUIT_WAIT), "quitting never waits past the bound");
        assert!(quit_may_proceed_at(&wait, start + QUIT_WAIT), "and stays released");
        drop(held);
        wait_for_pending().unwrap();
        assert!(recall(&dir, &path).is_some() && !unsaved().contains(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn concurrent_cache_transactions_preserve_both_designs() {
        let dir = folder("concurrent");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let jobs: Vec<_> = ["a.sc", "b.sc"]
            .into_iter()
            .map(|path| {
                let (dir, barrier) = (dir.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    remember(&dir, path, ViewSet::new().to_json()).unwrap();
                })
            })
            .collect();
        barrier.wait();
        for job in jobs {
            job.join().unwrap();
        }
        assert!(recall(&dir, "a.sc").is_some() && recall(&dir, "b.sc").is_some());
        assert!(dir.join("view-memory.lock").is_file(), "the persistent lock is never unlinked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_busy_cache_write_waits_off_thread_and_remains_pending_until_persisted() {
        let dir = folder("async");
        std::fs::create_dir_all(&dir).unwrap();
        let held = lock_memory(&dir).unwrap();
        remember_async(dir.clone(), "a.sc".into(), ViewSet::new().to_json()).unwrap();
        remember_async(dir.clone(), "b.sc".into(), ViewSet::new().to_json()).unwrap();
        assert!(pending(), "the caller returned while the file is still held");
        assert!(recall(&dir, "a.sc").is_none());
        drop(held);
        wait_for_pending().unwrap();
        assert!(recall(&dir, "a.sc").is_some() && recall(&dir, "b.sc").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_last_queued_snapshot_of_the_same_design_wins() {
        let dir = folder("queued-order");
        let first = ViewSet::new();
        let mut last = ViewSet::new();
        last.open(ViewKind::Render, None, false);
        remember_async(dir.clone(), "a.sc".into(), first.to_json()).unwrap();
        remember_async(dir.clone(), "a.sc".into(), last.to_json()).unwrap();
        wait_for_pending().unwrap();
        assert_eq!(recall(&dir, "a.sc"), Some(last.to_json()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_design_gets_back_the_views_it_was_closed_with_and_the_newest_wins() {
        let dir = folder("roundtrip");
        assert_eq!(recall(&dir, "a.scd"), None, "nothing yet");
        remember(&dir, "a.scd", set_with_render().to_json()).unwrap();
        remember(&dir, "b.scd", ViewSet::new().to_json()).unwrap();
        assert_eq!(ViewSet::from_json(&recall(&dir, "a.scd").unwrap()), Some(set_with_render()));
        remember(&dir, "a.scd", ViewSet::new().to_json()).unwrap();
        assert_eq!(ViewSet::from_json(&recall(&dir, "a.scd").unwrap()), Some(ViewSet::new()));
        assert_eq!(recall(&dir, "c.scd"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_two_hundred_latest_designs_are_kept() {
        let dir = folder("cap");
        for n in 0..CAP + 5 {
            remember(&dir, &format!("{n}.scd"), ViewSet::new().to_json()).unwrap();
        }
        assert_eq!(read(&dir).len(), CAP);
        assert_eq!(recall(&dir, "0.scd"), None, "the oldest went");
        assert!(recall(&dir, &format!("{}.scd", CAP + 4)).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn host_of(store: &mut Store) -> Option<&mut Host> {
        match store {
            Store::Host(host) => Some(host),
            _ => None,
        }
    }

    /// #23271: the browser keeps the remembered views in its own storage. What one page remembers
    /// is saved on top of what another page of the app stored meanwhile, and stays to be saved
    /// until the storage takes it.
    #[test]
    fn a_host_keeps_the_views_and_another_pages_too() {
        let mut store = Store::Host(Host::load(None));
        assert_eq!(store.recall("/opfs/a.sc"), None, "nothing yet");
        let host = host_of(&mut store).unwrap();
        assert!(!host.changed() && host.pending_text(None).is_none(), "nothing to keep");
        store.remember("/opfs/a.sc", set_with_render().to_json()).unwrap();
        assert_eq!(ViewSet::from_json(&store.recall("/opfs/a.sc").unwrap()), Some(set_with_render()), "shown again in this page at once");
        // Another page stored its own design meanwhile, and the storage is full.
        let other = to_text(&[("/opfs/b.sc".to_owned(), ViewSet::new().to_json())]);
        let host = host_of(&mut store).unwrap();
        assert!(host.changed());
        assert!(host.pending_text(Some(&other)).is_some());
        host.saved(false);
        assert!(!host.changed(), "not tried again every frame");
        // The next change tries again, with the one the storage refused.
        store.remember("/opfs/c.sc", ViewSet::new().to_json()).unwrap();
        let host = host_of(&mut store).unwrap();
        assert!(host.changed());
        let kept = host.pending_text(Some(&other)).unwrap();
        host.saved(true);
        assert!(!host.changed() && host.pending_text(Some(&kept)).is_none(), "kept once");
        let next = Store::Host(Host::load(Some(&kept)));
        assert_eq!(ViewSet::from_json(&next.recall("/opfs/a.sc").unwrap()), Some(set_with_render()), "the next page gets them back");
        assert!(next.recall("/opfs/c.sc").is_some(), "with the later change");
        assert!(next.recall("/opfs/b.sc").is_some(), "the other page's stay");
        for hostile in ["", "not json", "{\"path\": 1}", "[1, null]"] {
            assert_eq!(Store::Host(Host::load(Some(hostile))).recall("/opfs/a.sc"), None, "{hostile}");
        }
    }

    #[test]
    fn a_damaged_or_foreign_file_is_ignored_and_replaced() {
        let dir = folder("hostile");
        std::fs::create_dir_all(&dir).unwrap();
        for text in ["", "not json", "{\"path\": 1}", "[1, null, {\"path\": 3}, {\"path\": \"a.scd\"}]"] {
            std::fs::write(dir.join(FILE), text).unwrap();
            assert_eq!(recall(&dir, "a.scd"), None, "{text}");
        }
        remember(&dir, "a.scd", ViewSet::new().to_json()).unwrap();
        assert!(recall(&dir, "a.scd").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restoring_migrates_legacy_sketch_tabs_before_filtering_workspaces() {
        let saved = json!({"views":[
            {"id":1,"kind":"sheet","name":"Drawing"},
            {"id":2,"kind":"sketch","name":"Old sketch"}
        ], "active":2});
        let restored = restorable(&saved, |kind, _| kind == ViewKind::Model).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.active().kind, ViewKind::Model);
        assert!(restored.active().id > 2);
        assert!(!restored.can_reopen());
        assert!(restorable(&saved, |_, _| false).is_none());
    }

    #[test]
    fn restoring_drops_views_that_cannot_be_shown_and_stale_json() {
        let saved = set_with_render().to_json();
        let all = restorable(&saved, |_, _| true).unwrap();
        assert_eq!(all.len(), 3);
        let some = restorable(&saved, |kind, subject| kind == ViewKind::Model || (kind == ViewKind::Render && subject == Some("7"))).unwrap();
        assert_eq!(some.views().iter().map(|view| view.kind).collect::<Vec<_>>(), [ViewKind::Model, ViewKind::Render]);
        assert_eq!(some.active().kind, ViewKind::Model, "the active view was dropped: the first one shows");
        assert!(restorable(&saved, |_, _| false).is_none(), "nothing left");
        let foreign = json!({"views":[
            {"id":1,"kind":"model","name":"Model"},
            {"id":2,"kind":"unknown","name":"Foreign"},
            {"id":3,"kind":"sheet","name":""}
        ]});
        assert_eq!(restorable(&foreign, |kind, _| kind == ViewKind::Model).unwrap().len(), 1, "unavailable entries cannot poison a usable Model");
        for hostile in [json!(null), json!(5), json!({"views": 3}), json!({"views": [{"kind": 4}]}), json!({"views": []})] {
            assert!(restorable(&hostile, |_, _| true).is_none(), "{hostile}");
        }
    }
}
