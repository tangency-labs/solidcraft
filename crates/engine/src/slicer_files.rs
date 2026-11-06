//! The files sent to a slicer: a private per-user folder, a name that stays the same for the same
//! design and bodies (so sending again replaces the file and the slicer's "Reload from disk"
//! picks it up), writes that never leave half a file behind, and a sweep of old files.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long a sent file is kept.
pub const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);
/// Most numbered copies (`name-2.3mf`, …) tried when the file is locked.
const MAX_COPIES: u32 = 9;

/// The per-user folder: `%LOCALAPPDATA%\SolidCraft\slicer`, `~/Library/Caches/SolidCraft/slicer`
/// or `$XDG_CACHE_HOME/solidcraft/slicer` (`~/.cache/solidcraft/slicer`).
pub fn dir() -> Option<PathBuf> {
    // Tests never touch the user's real folder.
    if cfg!(test) {
        return Some(solidcraft_io::vfs::temp_dir().join(format!("solidcraft-test-slicer-{}", std::process::id())));
    }
    let env = |k: &str| std::env::var_os(k).filter(|d| !d.is_empty()).map(PathBuf::from);
    let (root, name) = if cfg!(windows) {
        (env("LOCALAPPDATA")?, "SolidCraft")
    } else if cfg!(target_os = "macos") {
        (env("HOME")?.join("Library/Caches"), "SolidCraft")
    } else {
        (env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))?, "solidcraft")
    };
    Some(root.join(name).join("slicer"))
}

/// Make `dir`, readable by its owner only (on Unix).
fn make_private(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A file name (no extension) for `bodies` of a design: the design's name, then a hash of which
/// document it is and of the bodies, so two designs with the same name, or two selections of one
/// design, never share a file. A saved design is told apart by its path; one that was never saved
/// by this process and its `uid` (`Session::uid`), so two unsaved "Untitled" designs differ too.
/// At the first Save As the name changes to the path-based one: the slicer gets a new file once,
/// and the unsaved one is left to the seven-day sweep.
pub fn stem(design: &str, path: Option<&str>, uid: u64, bodies: &[String]) -> String {
    let mut names: Vec<&str> = bodies.iter().map(String::as_str).collect();
    names.sort_unstable();
    // FNV-1a: the same on every release, unlike `DefaultHasher`.
    let mut h: u64 = 0xcbf29ce484222325;
    let unsaved = format!("unsaved:{}:{uid}", std::process::id());
    for part in std::iter::once(path.unwrap_or(&unsaved)).chain(names) {
        for b in part.bytes().chain([0]) {
            h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
    }
    let name: String = design.chars().take(60).map(|c| if c.is_alphanumeric() || "-_ ".contains(c) { c } else { '_' }).collect();
    let name = name.trim();
    format!("{}-{:016x}", if name.is_empty() { "design" } else { name }, h)
}

/// Write `bytes` as `<stem>.<ext>` in `dir`: to a temporary name first, then renamed over the
/// file, so the slicer never reads half a file. When the name can't be replaced (Windows keeps a
/// file the slicer has open locked) the next free `<stem>-2.<ext>`, … is used; the flag says so.
pub fn write(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<(PathBuf, bool)> {
    make_private(dir)?;
    let tmp = dir.join(format!(".{stem}.{ext}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    let mut failure = None;
    for n in 1..=MAX_COPIES {
        let target = dir.join(if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") });
        match std::fs::rename(&tmp, &target) {
            Ok(()) => return Ok((target, n > 1)),
            Err(e) => failure = Some(e),
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(failure.unwrap_or_else(|| io::Error::other("no free file name")))
}

/// Delete the files in `dir` last changed before `now - keep`; how many went.
pub fn sweep(dir: &Path, now: SystemTime, keep: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut gone = 0;
    for e in entries.flatten().take(5_000) {
        let old = e
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| m.modified().ok())
            .is_some_and(|t| now.duration_since(t).is_ok_and(|age| age > keep));
        if old && std::fs::remove_file(e.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// Delete the sent files older than [`KEEP`]. Run at startup, never right after a send.
pub fn clean_old() {
    if let Some(dir) = dir() {
        sweep(&dir, SystemTime::now(), KEEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-slicer-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_name_is_stable_per_design_and_bodies() {
        let a = |names: &[&str]| stem("Bracket", Some("/d/bracket.solidcraft"), 1, &names.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(a(&["Arm", "Base"]), a(&["Base", "Arm"]));
        assert_ne!(a(&["Arm", "Base"]), a(&["Arm"]));
        assert_ne!(a(&["Arm"]), stem("Bracket", Some("/other/bracket.solidcraft"), 1, &["Arm".to_string()]));
        assert!(a(&["Arm"]).starts_with("Bracket-"));
        assert_eq!(stem("a/b:c", None, 1, &[]).split('-').next(), Some("a_b_c"));
        assert!(stem("", None, 1, &[]).starts_with("design-"));
    }

    #[test]
    fn unsaved_designs_with_the_same_name_get_different_files() {
        let bodies = ["Arm".to_string()];
        let (a, b) = (stem("Untitled", None, 1, &bodies), stem("Untitled", None, 2, &bodies));
        assert_ne!(a, b);
        assert_eq!(a, stem("Untitled", None, 1, &bodies), "one document keeps one file");
        assert_eq!(a.len(), "Untitled-".len() + 16);
    }

    #[test]
    fn a_second_send_replaces_the_file_and_leaves_no_temporary_one() {
        let dir = scratch("replace");
        let (first, numbered) = write(&dir, "part", "3mf", b"one").unwrap();
        let (second, numbered_again) = write(&dir, "part", "3mf", b"two").unwrap();
        assert_eq!((first.clone(), numbered, numbered_again), (second, false, false));
        assert_eq!(std::fs::read(&first).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_name_that_can_not_be_replaced_gets_a_numbered_copy() {
        let dir = scratch("locked");
        // A folder in the way stands in for a file the slicer holds open.
        std::fs::create_dir_all(dir.join("part.3mf/inside")).unwrap();
        let (path, numbered) = write(&dir, "part", "3mf", b"data").unwrap();
        assert!(numbered);
        assert_eq!(path.file_name().unwrap(), "part-2.3mf");
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_old_files_are_swept() {
        let dir = scratch("sweep");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.3mf"), b"x").unwrap();
        let now = SystemTime::now();
        assert_eq!(sweep(&dir, now, KEEP), 0);
        assert_eq!(sweep(&dir, now + KEEP + Duration::from_secs(60), KEEP), 1);
        assert!(!dir.join("a.3mf").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
