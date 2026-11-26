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
/// Most temporary names tried when others are taken.
const MAX_TEMP_NAMES: u32 = 32;
/// Most version numbers tried when other sends took them first.
const MAX_VERSION_TRIES: u32 = 64;

/// The per-user folder: `%LOCALAPPDATA%\SolidCraft\slicer`, `~/Library/Caches/SolidCraft/slicer`
/// or `$XDG_CACHE_HOME/solidcraft/slicer` (`~/.cache/solidcraft/slicer`).
pub fn dir() -> Option<PathBuf> {
    // Tests never touch the user's real folder.
    if cfg!(test) {
        return Some(solidcraft_io::vfs::temp_dir().join(format!("solidcraft-test-slicer-{}", std::process::id())));
    }
    Some(crate::recovery::cache_dir()?.join("slicer"))
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
    format!("{}-{:016x}", clean_name(design), h)
}

/// The design's name as a file name: letters, digits, `-`, `_` and spaces, at most 60 characters,
/// `design` when nothing is left.
fn clean_name(design: &str) -> String {
    let name: String = design.chars().take(60).map(|c| if c.is_alphanumeric() || "-_ ".contains(c) { c } else { '_' }).collect();
    let name = name.trim();
    if name.is_empty() { "design" } else { name }.to_string()
}

/// The name (no extension) of a copy saved in a folder the user chose (#18311): the design's name
/// alone, with no hash, so the copies sit with the user's other files under a name they know.
pub fn copy_stem(design: &str) -> String {
    clean_name(design)
}

/// The number of the next `<stem>-v<N>.<ext>` in `dir`: one more than the highest there, 1 when
/// there is none or the folder does not exist yet.
pub fn next_version(dir: &Path, stem: &str, ext: &str) -> u32 {
    let (head, tail) = (format!("{stem}-v"), format!(".{ext}"));
    let Ok(entries) = std::fs::read_dir(dir) else { return 1 };
    let highest = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|name| name.strip_prefix(&head)?.strip_suffix(&tail)?.parse::<u32>().ok())
        .max();
    highest.map_or(1, |n| n.saturating_add(1))
}

/// Write `bytes` as `<stem>.<ext>` in `dir`: to a temporary name first, then renamed over the
/// file, so the slicer never reads half a file. When the name can't be replaced (Windows keeps a
/// file the slicer has open locked) the next free `<stem>-2.<ext>`, … is used; the flag says so.
pub fn write(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<(PathBuf, bool)> {
    write_with(dir, stem, ext, bytes, true)
}

/// [`write`] into a folder the user chose: made if it is missing, and left with the permissions it
/// has or is given by the system, not readable by its owner only.
pub fn write_copy(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<(PathBuf, bool)> {
    write_with(dir, stem, ext, bytes, false)
}

fn write_with(dir: &Path, stem: &str, ext: &str, bytes: &[u8], private: bool) -> io::Result<(PathBuf, bool)> {
    if private {
        make_private(dir)?;
    } else {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = write_tmp(dir, stem, ext, bytes)?;
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

/// Write `bytes` to a temporary file in `dir` that did not exist: made exclusively, so a name
/// another user left there (a link to a file of ours, in a folder others can write to) is never
/// followed or truncated; a taken name moves on to the next.
fn write_tmp(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    use std::io::Write as _;
    for attempt in 0..MAX_TEMP_NAMES {
        let tmp = dir.join(format!(".{stem}.{ext}.{}.{attempt}.tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(e);
                }
                return Ok(tmp);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free temporary file name"))
}

/// Write `bytes` as the next `<stem>-v<N>.<ext>` in a folder the user chose, made if it is
/// missing, and say which `N`. A name that exists is never replaced: the file is made exclusively,
/// and when another send took the number first the next one is tried.
pub fn write_version(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<(PathBuf, u32)> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir)?;
    let mut version = next_version(dir, stem, ext);
    for _ in 0..MAX_VERSION_TRIES {
        let target = dir.join(format!("{stem}-v{version}.{ext}"));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes) {
                    let _ = std::fs::remove_file(&target);
                    return Err(e);
                }
                return Ok((target, version));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => version = version.saturating_add(1),
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free version number"))
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
    fn a_copy_is_named_after_the_design_and_numbered_past_the_highest_version() {
        assert_eq!(copy_stem("Mount plate"), "Mount plate");
        assert_eq!(copy_stem("a/b:c"), "a_b_c");
        assert_eq!(copy_stem(""), "design");
        let dir = scratch("versions");
        assert_eq!(next_version(&dir, "plate", "3mf"), 1, "no folder yet");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["plate-v1.3mf", "plate-v7.3mf", "plate-v2.stl", "plate-vx.3mf", "other-v9.3mf", "plate.3mf"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(next_version(&dir, "plate", "3mf"), 8);
        assert_eq!(next_version(&dir, "plate", "stl"), 3);
        assert_eq!(next_version(&dir, "other", "stl"), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_copy_makes_its_folder_and_does_not_make_it_private() {
        let root = scratch("copy");
        let dir = root.join("deeper");
        let (path, numbered) = write_copy(&dir, "plate", "3mf", b"data").unwrap();
        assert!(!numbered);
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // A folder made the ordinary way, whatever the process umask: the copy's is no stricter.
            let control = root.join("control");
            std::fs::create_dir_all(&control).unwrap();
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), mode(&control));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_version_is_never_replaced_and_a_taken_number_moves_on() {
        let dir = scratch("version");
        let (one, n1) = write_version(&dir, "plate", "stl", b"first").unwrap();
        let (two, n2) = write_version(&dir, "plate", "stl", b"second").unwrap();
        assert_eq!((n1, n2), (1, 2));
        assert_eq!((std::fs::read(&one).unwrap(), std::fs::read(&two).unwrap()), (b"first".to_vec(), b"second".to_vec()));
        // A send that read the folder before the others wrote would pick 1 again: make that happen by
        // taking the number it would find, and see it go to the next free one.
        std::fs::write(dir.join("plate-v3.stl"), b"taken").unwrap();
        let (_, n) = write_version(&dir, "plate", "stl", b"third").unwrap();
        assert_eq!(n, 4);
        assert_eq!(std::fs::read(dir.join("plate-v3.stl")).unwrap(), b"taken");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_link_in_the_copy_folder_is_not_followed() {
        let dir = scratch("link");
        std::fs::create_dir_all(&dir).unwrap();
        let precious = dir.join("precious");
        std::fs::write(&precious, b"keep me").unwrap();
        // The temporary name a replacing send would try first, planted as a link to a file of ours.
        std::os::unix::fs::symlink(&precious, dir.join(format!(".plate.stl.{}.0.tmp", std::process::id()))).unwrap();
        let (path, _) = write_copy(&dir, "plate", "stl", b"mesh").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"mesh");
        assert_eq!(std::fs::read(&precious).unwrap(), b"keep me");
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
