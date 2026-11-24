//! What each saved design's views were when it was last closed, kept in the user's own data
//! folder (never in the design file) so "Reopen a design's views next time it opens" can bring
//! them back. A small map from a design's path to its [`ViewSet`] saved form, newest last.

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

/// The remembered designs, oldest first. Anything unreadable gives none.
fn read(dir: &Path) -> Vec<(String, Value)> {
    let file = dir.join(FILE);
    if std::fs::metadata(&file).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        return Vec::new();
    }
    let Ok(Value::Array(items)) = std::fs::read(&file).map_err(drop).and_then(|bytes| serde_json::from_slice(&bytes).map_err(drop)) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, Value)> =
        items.into_iter().filter_map(|item| Some((item.get("path")?.as_str()?.to_owned(), item.get("views")?.clone()))).collect();
    let skip = entries.len().saturating_sub(CAP);
    entries.drain(..skip);
    entries
}

/// Remember `views` (a [`ViewSet::to_json`]) for the design saved at `path`, in the folder `dir`.
pub fn remember(dir: &Path, path: &str, views: Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(not(target_arch = "wasm32"))]
    let _lock = lock_memory(dir)?;
    let mut entries = read(dir);
    entries.retain(|(known, _)| known != path);
    entries.push((path.to_owned(), views));
    let skip = entries.len().saturating_sub(CAP);
    entries.drain(..skip);
    let list: Vec<Value> = entries.into_iter().map(|(path, views)| json!({"path": path, "views": views})).collect();
    solidcraft_engine::io::safe_write::write_atomic(&dir.join(FILE), Value::Array(list).to_string().as_bytes(), false)
}

/// Keep the entire read-modify-write transaction exclusive across application processes.
/// The lockfile stays in place: deleting it would let two processes lock different files.
#[cfg(not(target_arch = "wasm32"))]
fn lock_memory(dir: &Path) -> std::io::Result<std::fs::File> {
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("view-memory.lock"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
            Err(std::fs::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= deadline {
                    return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "remembered views are busy"));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
static PENDING: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(target_arch = "wasm32"))]
struct PendingWrite;

#[cfg(not(target_arch = "wasm32"))]
impl Drop for PendingWrite {
    fn drop(&mut self) {
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
        PENDING.fetch_add(1, Ordering::AcqRel);
        let request = WriteRequest { dir, path, views, _pending: PendingWrite };
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
pub fn pending() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        PENDING.load(Ordering::Acquire) != 0
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
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
    read(dir).into_iter().rev().find(|(known, _)| known == path).map(|(_, views)| views)
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
