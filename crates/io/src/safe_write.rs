//! Crash-safe file writes: the new content goes to a temporary file next to the target, is
//! flushed to disk, and replaces the target in one rename. A crash or power cut at any moment
//! leaves the old file or the new one, never a torn mix. The previous version can be kept as
//! `<file>.bak` (itself written the same way).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The backup of a file written with `backup: true`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

fn temp_beside(path: &Path, tag: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    path.with_file_name(format!(".{name}.{tag}.{}.{nanos}.tmp", std::process::id()))
}

/// Bytes written between checks for Cancel and reports of how far the write is.
const CHUNK: usize = 1 << 20;

/// What a write its watch cancelled returns: the file it was to replace, and its backup, are as
/// they were.
pub fn cancelled_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "save cancelled")
}

/// The watch over this thread's work asked it to stop.
pub(crate) fn cancelled() -> bool {
    solidcraft_kernel::watch::current().is_some_and(|w| w.cancelled())
}

/// Write `bytes` to a new file at `path` and flush it, a piece at a time: a watched write
/// ([`solidcraft_kernel::watch`]) reports how far it is (`report`) and stops when cancelled.
fn write_synced(path: &Path, bytes: &[u8], report: bool) -> std::io::Result<()> {
    let watch = solidcraft_kernel::watch::current();
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut done = 0usize;
    for chunk in bytes.chunks(CHUNK) {
        if watch.as_ref().is_some_and(|w| w.cancelled()) {
            return Err(cancelled_error());
        }
        f.write_all(chunk)?;
        done = done.saturating_add(chunk.len());
        if report && let Some(w) = &watch {
            w.progress("writing", done, bytes.len());
        }
    }
    flush(&f)
}

/// Flush a written file to the disk. On Apple systems `sync_all` asks the drive itself to flush
/// (`F_FULLFSYNC`), which a network share refuses (an SMB share: "Operation not supported", os
/// error 45), so a save there failed before it replaced anything. The plain `fsync` the share
/// does offer is used then, as SQLite and git do.
fn flush(f: &fs::File) -> std::io::Result<()> {
    flush_with(f, fs::File::sync_all)
}

fn flush_with(f: &fs::File, full: impl FnOnce(&fs::File) -> std::io::Result<()>) -> std::io::Result<()> {
    match full(f) {
        #[cfg(target_vendor = "apple")]
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported || e.raw_os_error() == Some(45) => rustix::fs::fsync(f).map_err(std::io::Error::from),
        r => r,
    }
}

/// Flush the directory entry (the rename) to disk where the platform allows it.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Steps of a save, so tests can stop one part way (as a crash would).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Step {
    TempWritten,
    BackupWritten,
    #[allow(dead_code)] // stopping after the last step is a whole save (tests name it)
    Replaced,
}

/// Most links followed before a path is called a loop.
const MAX_LINK_HOPS: usize = 40;

/// The file a save has to replace: `path`, or where a symbolic link at `path` (or a chain of
/// them) leads, so a design linked into a project from a synced folder is saved there and the
/// link survives. A link whose target is missing resolves to the place the target would be.
fn resolve_links(path: &Path) -> std::io::Result<PathBuf> {
    let mut at = path.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        if !fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Ok(at);
        }
        let to = fs::read_link(&at)?;
        at = if to.is_absolute() { to } else { at.parent().map(|d| d.join(&to)).unwrap_or(to) };
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{}: too many levels of symbolic links", path.display())))
}

/// Give the file that replaces `old` the permissions `old` had (a private `0600` design must not
/// come back readable by everyone, a group's `0664` must not lose the group's write access).
fn keep_permissions(old: &Option<fs::Permissions>, new: &Path) {
    if let Some(p) = old {
        let _ = fs::set_permissions(new, p.clone());
    }
}

// Retrying only the already-synced atomic rename avoids rewriting a large image
// or deleting/truncating its existing target. ACL/read-only/other permanent
// refusals still fail. No file lock is broken and no security setting is changed.
fn replace_synced(source: &Path, destination: &Path) -> std::io::Result<()> {
    // A read-only target is refused for good: fail at once rather than wait for it.
    let read_only = fs::metadata(destination).is_ok_and(|m| m.permissions().readonly());
    let windows = cfg!(windows);
    rename_with_retry(|| fs::rename(source, destination), windows && !read_only, std::thread::sleep)
        .map_err(|error| explain_refusal(error, windows, read_only))
}

/// The waits between attempts at a rename Windows refuses: doubling from 1 ms, then 250 ms at a
/// time, about 2 s in all. An antivirus or indexer scanning a file written a moment earlier can
/// hold it for several hundred milliseconds when the machine is busy (#8104); 127 ms was too short.
const RENAME_WAITS_MS: [u64; 15] = [1, 2, 4, 8, 16, 32, 64, 128, 250, 250, 250, 250, 250, 250, 250];

/// Whether `error` is Windows refusing a rename (access denied, os error 5, or a sharing
/// violation, os error 32): another program has the file open, or it is read-only or denied.
fn refused_by_windows(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32))
}

/// Say why Windows would not replace the file rather than only the bare "Access is denied", and
/// keep the system's own words.
fn explain_refusal(error: std::io::Error, windows: bool, read_only: bool) -> std::io::Error {
    if !windows || !refused_by_windows(&error) {
        return error;
    }
    let why =
        if read_only { "the file is read-only" } else { "Windows would not replace the file: another program may have it open, or it is read-only" };
    std::io::Error::new(error.kind(), format!("{why} ({error})"))
}

fn rename_with_retry(
    mut rename: impl FnMut() -> std::io::Result<()>,
    windows: bool,
    mut wait: impl FnMut(std::time::Duration),
) -> std::io::Result<()> {
    // Windows can refuse rename while an independently opened handle withholds
    // FILE_SHARE_DELETE. Access denied may also be permanent, so keep an attempt
    // bound and the last error. Requested sleeps total 2005 ms; scheduling/OS
    // operation time is not a wall-clock latency guarantee.
    let mut result = rename();
    if !windows {
        return result;
    }
    for milliseconds in RENAME_WAITS_MS {
        if !result.as_ref().is_err_and(refused_by_windows) {
            return result;
        }
        wait(std::time::Duration::from_millis(milliseconds));
        result = rename();
    }
    result
}

fn write_steps(path: &Path, bytes: &[u8], backup: bool, stop: Option<Step>) -> std::io::Result<()> {
    let permissions = fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.permissions());
    let tmp = temp_beside(path, "new");
    if let Err(e) = write_synced(&tmp, bytes, true) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    keep_permissions(&permissions, &tmp);
    if stop == Some(Step::TempWritten) {
        return Ok(());
    }
    if backup && path.is_file() {
        // Copy (not move) the old file, so the target exists at every moment.
        let old = fs::read(path);
        let bak_tmp = temp_beside(path, "bak");
        // The last moment Cancel stops it: nothing has been replaced yet.
        let r = old
            .and_then(|old| write_synced(&bak_tmp, &old, false))
            .map(|_| keep_permissions(&permissions, &bak_tmp))
            .and_then(|_| if cancelled() { Err(cancelled_error()) } else { Ok(()) })
            .and_then(|_| replace_synced(&bak_tmp, &backup_path(path)));
        if let Err(e) = r {
            let _ = fs::remove_file(&bak_tmp);
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if stop == Some(Step::BackupWritten) {
        return Ok(());
    }
    // Without a backup to keep, the last moment Cancel stops it; once the backup is replaced the
    // save goes through to the end.
    if !(backup && path.is_file()) && cancelled() {
        let _ = fs::remove_file(&tmp);
        return Err(cancelled_error());
    }
    if let Err(e) = replace_synced(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(path);
    // Temporary files of saves that crashed part way (a minute old or more) go.
    for t in stale_temps(path) {
        if fs::metadata(&t).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age.as_secs() >= 60) {
            let _ = fs::remove_file(t);
        }
    }
    Ok(())
}

/// Write `bytes` to `path` atomically; with `backup`, the file being replaced is kept as
/// `<path>.bak` first. A symbolic link at `path` is followed (the file it points to is replaced,
/// the link stays), and the file keeps the permissions it had. The app's own folders (autosave,
/// crash reports) write through this without a [`crate::vfs::PathScope`]; a command writing a
/// path it was given calls [`crate::vfs::check`] first, which judges where the link leads.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: bool) -> std::io::Result<()> {
    // In the browser the host stores files (one write is already atomic there).
    if cfg!(target_arch = "wasm32") {
        let _ = backup;
        return crate::vfs::write(&path.to_string_lossy(), bytes);
    }
    write_steps(&resolve_links(path)?, bytes, backup, None)
}

/// Publish a complete new file without replacing an existing destination, even if another
/// writer creates it after this call starts. Unsupported filesystems return an error.
pub fn create_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if cfg!(target_arch = "wasm32") {
        let name = path.to_string_lossy();
        if crate::vfs::exists(&name) {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "destination already exists"));
        }
        return crate::vfs::write(&name, bytes);
    }
    let tmp = temp_beside(path, "create");
    // Only remove a temporary file that this invocation successfully created.
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let result = file.write_all(bytes).and_then(|_| flush(&file));
    drop(file);
    let result = result.and_then(|_| fs::hard_link(&tmp, path));
    let _ = fs::remove_file(&tmp);
    if result.is_ok() {
        sync_dir(path);
    }
    result
}

/// Leftover temporary files of saves that never finished (a crash part way), next to `path`
/// (next to the file a link at `path` points to, where a save puts them).
pub fn stale_temps(path: &Path) -> Vec<PathBuf> {
    let resolved = resolve_links(path).unwrap_or_else(|_| path.to_path_buf());
    let path = resolved.as_path();
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else { return Vec::new() };
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let prefix = format!(".{name}.");
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".tmp")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #5467: a lock is someone else's while it is refreshed; one left by a crash goes stale, and
    /// this app's own lock never counts as the design being open elsewhere. Writing a design's
    /// file changes its stamp.
    #[test]
    fn a_fresh_lock_from_another_app_holds_the_design_and_a_stale_or_own_one_does_not() {
        use crate::design_lock as lock;
        let d = dir("design-lock");
        let design = d.join("Bracket.solidcraft");
        std::fs::write(&design, b"one").unwrap();
        assert_eq!(lock::lock_path(&design), d.join(".~Bracket.solidcraft.lock"));
        assert!(!crate::is_design_path(&lock::lock_path(&design).to_string_lossy()), "never listed as a design");

        let now = lock::now_secs();
        let theirs = lock::Lock { host: "WORKSHOP-PC".into(), pid: std::process::id().wrapping_add(1), ..lock::Lock::mine(now - 600) };
        lock::write(&design, &lock::Lock { refreshed: now - 30, ..theirs.clone() }).unwrap();
        assert_eq!(lock::held_elsewhere(&design, now).map(|l| l.host), Some("WORKSHOP-PC".into()));
        // Not refreshed for longer than three refreshes: left behind, ignored.
        lock::write(&design, &lock::Lock { refreshed: now - lock::STALE_SECS - 1, ..theirs.clone() }).unwrap();
        assert!(lock::held_elsewhere(&design, now).is_none());
        lock::remove(&design);
        assert!(lock::lock_path(&design).exists(), "another app's lock is left alone");
        // This app's own lock: not "elsewhere", and removed on close.
        lock::write(&design, &lock::Lock::mine(now)).unwrap();
        assert!(lock::held_elsewhere(&design, now).is_none());
        lock::remove(&design);
        assert!(!lock::lock_path(&design).exists());

        let known = lock::stamp(&design.to_string_lossy()).unwrap();
        assert!(!lock::changed_since(&design.to_string_lossy(), &known));
        std::fs::write(&design, b"written elsewhere").unwrap();
        assert!(lock::changed_since(&design.to_string_lossy(), &known));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn create_does_not_replace_an_existing_destination() {
        let d = dir("create");
        let p = d.join("render.png");
        create_atomic(&p, b"first complete image").unwrap();
        assert_eq!(create_atomic(&p, b"replacement").unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&p).unwrap(), b"first complete image");
        assert!(stale_temps(&p).is_empty());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn concurrent_creators_publish_exactly_one_whole_file() {
        let d = dir("create-race");
        let p = d.join("render.png");
        let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [b"first image".as_slice(), b"second image".as_slice()]
            .into_iter()
            .map(|bytes| {
                let gate = gate.clone();
                let p = p.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    create_atomic(&p, bytes)
                })
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(results.iter().filter_map(|r| r.as_ref().err()).next().unwrap().kind(), std::io::ErrorKind::AlreadyExists);
        let bytes = fs::read(&p).unwrap();
        assert!(bytes == b"first image" || bytes == b"second image");
        assert!(stale_temps(&p).is_empty());
        fs::remove_dir_all(d).unwrap();
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-safe-write-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A watch for the tests: how far the write said it is, and Cancel once `cancel_at` bytes are
    /// reported written.
    struct TestWatch {
        reported: std::sync::Mutex<Vec<(&'static str, usize, usize)>>,
        cancel_at: usize,
        cancelled: std::sync::atomic::AtomicBool,
    }

    impl solidcraft_kernel::watch::Watch for TestWatch {
        fn cancelled(&self) -> bool {
            self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
        }
        fn progress(&self, stage: &'static str, done: usize, total: usize) {
            if done >= self.cancel_at {
                self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(mut r) = self.reported.lock() {
                r.push((stage, done, total));
            }
        }
    }

    fn watched(cancel_at: usize) -> std::sync::Arc<TestWatch> {
        std::sync::Arc::new(TestWatch { reported: Default::default(), cancel_at, cancelled: Default::default() })
    }

    /// A watched save reports how far it has written, a piece at a time, up to the whole file;
    /// cancelled part way, it stops: the file and its backup are as they were, and no temporary
    /// file is left behind.
    #[test]
    fn a_watched_write_reports_its_progress_and_cancelled_leaves_the_file_and_its_backup() {
        let d = dir("watched");
        let path = d.join("Plate.solidcraft");
        fs::write(&path, b"version one").unwrap();
        write_atomic(&path, b"version two", true).unwrap();
        let big = vec![7u8; 3 * CHUNK + 5];
        // Written to the end: each piece reported, the last the whole file.
        let watch = watched(usize::MAX);
        solidcraft_kernel::watch::watching(watch.clone(), || write_atomic(&path, &big, true)).unwrap();
        let reported = watch.reported.lock().unwrap().clone();
        assert_eq!(reported.len(), 4, "{reported:?}");
        assert_eq!(reported.last(), Some(&("writing", big.len(), big.len())));
        assert_eq!(fs::read(&path).unwrap(), big);
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"version two");
        // Cancelled after the first piece: refused as cancelled, nothing replaced.
        let watch = watched(1);
        let newer = vec![9u8; 3 * CHUNK];
        let e = solidcraft_kernel::watch::watching(watch.clone(), || write_atomic(&path, &newer, true)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(watch.reported.lock().unwrap().len(), 1, "stopped at the next piece");
        assert_eq!(fs::read(&path).unwrap(), big, "the file is as it was");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"version two", "and its backup");
        assert!(stale_temps(&path).is_empty(), "no temporary file left");
        // A new file (nothing to back up) cancelled is never created.
        let fresh = d.join("New.solidcraft");
        let e = solidcraft_kernel::watch::watching(watched(1), || write_atomic(&fresh, &newer, true)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::Interrupted);
        assert!(!fresh.exists() && stale_temps(&fresh).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// A network share (SMB on macOS) refuses the drive flush `sync_all` asks for with "Operation
    /// not supported" (os error 45). The file is then flushed the plain way and the save goes on;
    /// any other failure still fails the save.
    #[test]
    fn a_share_that_refuses_the_full_flush_gets_a_plain_one() {
        let d = dir("flush");
        let p = d.join("a.solidcraft");
        fs::write(&p, b"saved").unwrap();
        let f = fs::File::open(&p).unwrap();
        let refused = |_: &fs::File| Err(std::io::Error::from_raw_os_error(45));
        #[cfg(target_vendor = "apple")]
        flush_with(&f, refused).unwrap();
        #[cfg(not(target_vendor = "apple"))]
        assert!(flush_with(&f, refused).is_err(), "elsewhere sync_all is the plain flush already");
        let unsupported = |_: &fs::File| Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
        assert_eq!(flush_with(&f, unsupported).is_ok(), cfg!(target_vendor = "apple"));
        let broken = |_: &fs::File| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(flush_with(&f, broken).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        flush_with(&f, |_| Ok(())).unwrap();
        drop(f);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn replaces_and_keeps_a_backup() {
        let d = dir("bak");
        let p = d.join("a.solidcraft");
        write_atomic(&p, b"one", true).unwrap();
        assert!(!backup_path(&p).exists(), "nothing to back up the first time");
        write_atomic(&p, b"two", true).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read(backup_path(&p)).unwrap(), b"one");
        write_atomic(&p, b"three", false).unwrap();
        assert_eq!(fs::read(backup_path(&p)).unwrap(), b"one");
        assert!(stale_temps(&p).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// A save stopped after any step leaves the old file whole (or the new one).
    #[test]
    fn a_save_stopped_part_way_keeps_the_old_file() {
        let d = dir("stop");
        let p = d.join("a.solidcraft");
        write_atomic(&p, b"old", true).unwrap();
        for stop in [Step::TempWritten, Step::BackupWritten, Step::Replaced] {
            write_steps(&p, b"new content", true, Some(stop)).unwrap();
            let now = fs::read(&p).unwrap();
            assert!(now == b"old" || now == b"new content", "{stop:?}: {now:?}");
            if stop < Step::Replaced {
                assert_eq!(now, b"old");
                assert!(!stale_temps(&p).is_empty(), "the unfinished save's temporary file is left over");
                for t in stale_temps(&p) {
                    fs::remove_file(t).unwrap();
                }
            }
            write_atomic(&p, b"old", false).unwrap();
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_leaves_the_target() {
        let d = dir("fail");
        let p = d.join("missing-dir").join("a.solidcraft");
        assert!(write_atomic(&p, b"x", true).is_err());
        assert!(!p.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};

        fn mode(p: &Path) -> u32 {
            fs::metadata(p).unwrap().permissions().mode() & 0o777
        }

        /// saving through a link replaces the file it points to and leaves the link.
        #[test]
        fn a_save_through_a_link_updates_the_target() {
            let d = dir("link");
            fs::create_dir_all(d.join("cloud")).unwrap();
            fs::create_dir_all(d.join("work")).unwrap();
            let target = d.join("cloud").join("a.solidcraft");
            let link = d.join("work").join("a.solidcraft");
            write_atomic(&target, b"one", false).unwrap();
            symlink(&target, &link).unwrap();
            write_atomic(&link, b"two", true).unwrap();
            assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays a link");
            assert_eq!(fs::read(&target).unwrap(), b"two");
            assert_eq!(fs::read(&link).unwrap(), b"two");
            assert_eq!(fs::read(backup_path(&target)).unwrap(), b"one", "the backup sits beside the real file");
            assert!(!backup_path(&link).exists());
            let _ = fs::remove_dir_all(&d);
        }

        #[test]
        fn relative_and_chained_links_are_followed() {
            let d = dir("chain");
            fs::create_dir_all(d.join("real")).unwrap();
            fs::create_dir_all(d.join("mid")).unwrap();
            let target = d.join("real").join("a.solidcraft");
            write_atomic(&target, b"one", false).unwrap();
            symlink("../real/a.solidcraft", d.join("mid").join("b")).unwrap();
            symlink("mid/b", d.join("c")).unwrap();
            write_atomic(&d.join("c"), b"two", false).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"two");
            assert!(fs::symlink_metadata(d.join("c")).unwrap().file_type().is_symlink());
            assert!(fs::symlink_metadata(d.join("mid").join("b")).unwrap().file_type().is_symlink());
            let _ = fs::remove_dir_all(&d);
        }

        #[test]
        fn a_dangling_link_is_filled_in_and_a_loop_is_an_error() {
            let d = dir("dangling");
            let link = d.join("a.solidcraft");
            let target = d.join("later.solidcraft");
            symlink(&target, &link).unwrap();
            write_atomic(&link, b"new", true).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"new");
            assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());

            let (x, y) = (d.join("x"), d.join("y"));
            symlink(&y, &x).unwrap();
            symlink(&x, &y).unwrap();
            let e = write_atomic(&x, b"loop", true).unwrap_err();
            assert!(e.to_string().contains("symbolic links"), "{e}");
            let _ = fs::remove_dir_all(&d);
        }

        /// a private design stays private, a group's file keeps the group's access, and
        /// the backup carries the same permissions.
        #[test]
        fn a_save_keeps_the_permissions() {
            let d = dir("mode");
            for want in [0o600, 0o640, 0o664, 0o444] {
                let p = d.join(format!("a{want:o}.solidcraft"));
                write_atomic(&p, b"one", false).unwrap();
                fs::set_permissions(&p, fs::Permissions::from_mode(want)).unwrap();
                write_atomic(&p, b"two", true).unwrap();
                assert_eq!(fs::read(&p).unwrap(), b"two");
                assert_eq!(mode(&p), want, "{want:o}");
                assert_eq!(mode(&backup_path(&p)), want, "backup {want:o}");
                let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o644));
            }
            let _ = fs::remove_dir_all(&d);
        }

        #[test]
        fn a_save_through_a_link_keeps_the_targets_permissions() {
            let d = dir("linkmode");
            let target = d.join("a.solidcraft");
            write_atomic(&target, b"one", false).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            symlink(&target, d.join("l")).unwrap();
            write_atomic(&d.join("l"), b"two", true).unwrap();
            assert_eq!(mode(&target), 0o600);
            assert_eq!(mode(&backup_path(&target)), 0o600);
            let _ = fs::remove_dir_all(&d);
        }
    }
}

#[cfg(test)]
mod rename_retry_tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn only_windows_sharing_or_access_errors_retry_with_an_exact_finite_budget() {
        for code in [5, 32] {
            let calls = Cell::new(0);
            let mut waits = Vec::new();
            let result = rename_with_retry(
                || {
                    calls.set(calls.get() + 1);
                    if calls.get() < 3 { Err(std::io::Error::from_raw_os_error(code)) } else { Ok(()) }
                },
                true,
                |delay| waits.push(delay.as_millis()),
            );
            assert!(result.is_ok());
            assert_eq!(calls.get(), 3);
            assert_eq!(waits, [1, 2]);
            let calls = Cell::new(0);
            let mut waits = Vec::new();
            let error = rename_with_retry(
                || {
                    calls.set(calls.get() + 1);
                    Err(std::io::Error::from_raw_os_error(code))
                },
                true,
                |delay| waits.push(delay.as_millis()),
            )
            .unwrap_err();
            assert_eq!(error.raw_os_error(), Some(code));
            assert_eq!(calls.get(), 16);
            assert_eq!(waits, [1, 2, 4, 8, 16, 32, 64, 128, 250, 250, 250, 250, 250, 250, 250]);
            assert_eq!(waits.iter().sum::<u128>(), 2005);
        }
        for (windows, code) in [(false, 5), (false, 32), (true, 2), (true, 3), (true, 87), (true, 183)] {
            let calls = Cell::new(0);
            let error = rename_with_retry(
                || {
                    calls.set(calls.get() + 1);
                    Err(std::io::Error::from_raw_os_error(code))
                },
                windows,
                |_| panic!("unexpected retry"),
            )
            .unwrap_err();
            assert_eq!(calls.get(), 1);
            assert_eq!(error.raw_os_error(), Some(code));
        }
        let mut calls = 0;
        rename_with_retry(
            || {
                calls += 1;
                Ok(())
            },
            true,
            |_| panic!("success must not wait"),
        )
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[cfg(windows)]
    #[test]
    fn native_windows_rename_preserves_old_bytes_until_a_reader_releases_delete_sharing() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = std::env::temp_dir().join(format!("solidcraft-rename-reader-release-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let old = directory.join("image.png");
        let pending = directory.join("completed.tmp");
        fs::write(&old, b"old complete image").unwrap();
        write_synced(&pending, b"new complete image", false).unwrap();
        let mut reader = Some(OpenOptions::new().read(true).share_mode(1).open(&old).unwrap());
        let error = fs::rename(&pending, &old).unwrap_err();
        assert!(matches!(error.raw_os_error(), Some(5 | 32)), "{error}");
        let calls = Cell::new(0);
        rename_with_retry(
            || {
                calls.set(calls.get() + 1);
                fs::rename(&pending, &old)
            },
            true,
            |delay| {
                assert_eq!(fs::read(&old).unwrap(), b"old complete image");
                drop(reader.take());
                std::thread::sleep(delay);
            },
        )
        .unwrap();
        assert!(calls.get() >= 2 && calls.get() <= 16);
        assert_eq!(fs::read(&old).unwrap(), b"new complete image");
        assert!(!pending.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    /// #8104: a scanner (antivirus, indexer, thumbnailer) that opens the target without delete
    /// sharing and keeps it for a few hundred milliseconds under load delays the replace; it does
    /// not fail it, and the old file stays whole until the new one takes its place.
    #[cfg(windows)]
    #[test]
    fn a_reader_holding_the_target_for_a_while_delays_the_replace_without_failing_it() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = std::env::temp_dir().join(format!("solidcraft-rename-slow-reader-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let path = directory.join("library.json");
        fs::write(&path, b"old whole library").unwrap();
        let reader = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let scanner = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            drop(reader);
        });
        write_atomic(&path, b"new whole library", false).unwrap();
        scanner.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new whole library");
        assert!(stale_temps(&path).is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    /// A read-only target is refused for good: the export fails at once with that reason, rather
    /// than after the wait for a program holding the file, and the old file is kept.
    #[cfg(windows)]
    #[test]
    fn a_read_only_target_fails_at_once_and_says_so() {
        let directory = std::env::temp_dir().join(format!("solidcraft-rename-read-only-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let path = directory.join("render.png");
        fs::write(&path, b"old image").unwrap();
        let writable = fs::metadata(&path).unwrap().permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).unwrap();
        let started = std::time::Instant::now();
        let error = write_atomic(&path, b"new image", false).unwrap_err();
        assert!(started.elapsed() < std::time::Duration::from_millis(1000), "{:?}", started.elapsed());
        assert!(error.to_string().contains("the file is read-only"), "{error}");
        assert!(error.to_string().contains(&std::io::Error::from_raw_os_error(5).to_string()), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"old image");
        assert!(stale_temps(&path).is_empty());
        fs::set_permissions(&path, writable).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn persistent_native_lock_is_an_error_without_destroying_or_bypassing_the_target() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = std::env::temp_dir().join(format!("solidcraft-rename-persistent-reader-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("image.exr");
        fs::write(&path, b"old whole raw master").unwrap();
        let reader = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let error = write_atomic(&path, b"new whole raw master", false).unwrap_err();
        assert!(error.to_string().contains("another program may have it open, or it is read-only"), "{error}");
        assert_eq!(error.kind(), std::io::Error::from_raw_os_error(5).kind());
        assert_eq!(fs::read(&path).unwrap(), b"old whole raw master");
        assert!(stale_temps(&path).is_empty());
        drop(reader);
        write_atomic(&path, b"new whole raw master", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new whole raw master");
        fs::remove_dir_all(directory).unwrap();
    }
}
