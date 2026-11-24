//! Internal components (experimental, #6892; `solidcraft_doc::internal`): parts and assemblies
//! stored inside a design's file. Where they are unpacked to while the design is open, and how
//! they get there and back.
//!
//! - **The work folder.** A design saved as `Job.sc` unpacks its internal components to
//!   `.~Job.sc.parts` beside it, named as its lock file is (a leading dot hides it on Linux and
//!   macOS). Each is an ordinary part file there, so everything a linked design does (edit in
//!   place, Update Links, the out-of-date report) works on it as it is.
//! - **A link names the component, never the work file.** Its path is `.~internal/<name>`
//!   ([`internal::LINK`]); [`super::design_link::resolve`] turns that into the work file of the
//!   design as it is saved now, so Save As and a moved folder need nothing rewritten.
//! - **Opening unpacks** the ones that have no work file ([`unpack`]). A work file that differs
//!   from what the design's file holds is left as it is and named: a **recovery component**, what
//!   a crash left behind. Until it is restored or discarded (`Session::recovery_components`), the
//!   design reads and saves what its file holds for it.
//! - **Saving packs** each linked one from its work file ([`pack`]), or keeps the bytes the file
//!   held when there is no work file (a read-only folder, the browser build).
//! - **Neither reads a file it need not.** The work folder records what each file held when last
//!   read ([`Stamps`]), so an open or a save of a design with thousands of parts reads only the
//!   ones that changed.
//!
//! The work file's name is made here from a name this module has checked
//! ([`internal::valid_name`]), never taken from a container entry as it stands. Names are told
//! apart without regard to letter case, so a design made on Linux unpacks on Windows and macOS.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use solidcraft_doc::blob::Blob;
use solidcraft_doc::{Document, internal};

use super::design_link::{absolute, read_linked_file};
use super::design_stamps::Stamps;

/// The folder the stored parts of the design saved at `design` are unpacked to.
pub(crate) fn work_folder(design: &str) -> Option<PathBuf> {
    let path = Path::new(design);
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(path.with_file_name(format!(".~{name}.parts")))
}

/// The file the stored part `name` of the design saved at `design` is unpacked to.
pub(crate) fn work_file(design: Option<&str>, name: &str) -> Option<PathBuf> {
    internal::valid_name(name).then(|| work_folder(design?)).flatten().map(|folder| folder.join(name))
}

/// The stored part `file` is the work file of, for the design saved at `design`.
pub(crate) fn member_at(design: Option<&str>, file: &Path) -> Option<String> {
    let folder = absolute(&work_folder(design?)?);
    let file = absolute(file);
    let name = file.file_name()?.to_str()?;
    (file.parent() == Some(folder.as_path()) && internal::valid_name(name)).then(|| name.to_string())
}

/// The stored parts `doc` links, by name.
fn linked(doc: &Document) -> BTreeSet<String> {
    doc.links.values().filter_map(|link| internal::linked(&link.path)).map(str::to_string).collect()
}

/// What [`unpack`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Unpacked {
    /// Work files written.
    pub written: Vec<String>,
    /// Work files that were there and differ from the stored part: left as they are.
    pub differ: Vec<String>,
    /// Stored parts that could not be unpacked, each with why. The design reads them from its file.
    pub failed: Vec<String>,
}

/// Unpack the stored parts of `doc`, the design saved at `design`, that have no work file yet.
/// Nothing is ever replaced. Never in the browser build, which has no folders.
pub(crate) fn unpack(doc: &Document, design: &str) -> Unpacked {
    let mut done = Unpacked::default();
    if cfg!(target_arch = "wasm32") {
        return done;
    }
    let Some(folder) = work_folder(design) else { return done };
    // What each work file held when last read ([`Stamps`]): a file as recorded is compared
    // without being read, which is what keeps an open from reading every part.
    let mut stamps = Stamps::of(&folder);
    let mut seen = BTreeSet::new();
    for (name, bytes) in &doc.internal {
        let Some(file) = work_file(Some(design), name) else { continue };
        // Two names that differ only in letter case are one file on Windows and macOS.
        if !seen.insert(name.to_lowercase()) {
            done.failed.push(format!("{name}: another internal component has this name in other letter case"));
            continue;
        }
        match stamps.holds(name, bytes.as_bytes(), || super::design_stamps::digest(bytes.as_bytes())) {
            Some(true) => {}
            // A work file a power cut left cut short while it was unpacked (the start of the
            // stored part and no more, or nothing but zeros) is no one's work: it is unpacked
            // again, not offered for recovery.
            Some(false) if std::fs::read(&file).is_ok_and(|found| cut_short(&found, bytes.as_bytes())) => {
                match write_unpacked(&file, bytes.as_bytes()) {
                    Ok(()) => {
                        stamps.record(name, &super::design_stamps::digest(bytes.as_bytes()));
                        done.written.push(name.clone());
                    }
                    Err(why) => done.failed.push(format!("{name}: {why}")),
                }
            }
            Some(false) => done.differ.push(name.clone()),
            None if !file.exists() => match write_unpacked(&file, bytes.as_bytes()) {
                Ok(()) => {
                    stamps.record(name, &super::design_stamps::digest(bytes.as_bytes()));
                    done.written.push(name.clone());
                }
                Err(why) => done.failed.push(format!("{name}: {why}")),
            },
            None => done.failed.push(format!("{name}: its unpacked file can't be read")),
        }
    }
    stamps.save();
    // A folder made by something that hides nothing (the command-line tool, an earlier build) is
    // hidden once the app has the design open.
    if let Some(hide) = HIDE.get().filter(|_| folder.is_dir()) {
        hide(&folder);
    }
    done
}

/// What hides a work folder where its name does not: set once by the app ([`set_work_folder_hider`]).
///
/// The folder is hidden on purpose, on every system, even though the files in it (`.scprt`,
/// `.scasm`, `.scdwg`) are there so that people from part / assembly / drawing CAD can see a
/// single file design as the kinds they know. At rest a design is one `.sc` and nothing else
/// should appear beside it; someone who wants to see what it holds turns hidden items on, which
/// the documentation says to do. Do not make the folder visible to "help" with that.
static HIDE: std::sync::OnceLock<fn(&Path)> = std::sync::OnceLock::new();

/// Have every work folder made from now on hidden by `hide`. A work folder's name starts with a
/// dot, which hides it on Linux and macOS; on Windows the app passes what sets the Hidden
/// attribute. The engine has no platform code, so without this the folder is only named.
pub fn set_work_folder_hider(hide: fn(&Path)) {
    let _ = HIDE.set(hide);
}

/// Whether `found`, a work file's bytes, is `stored` cut short: an unpacking that a power cut
/// stopped after the file was named and before its bytes reached the disk.
fn cut_short(found: &[u8], stored: &[u8]) -> bool {
    found.len() <= stored.len() && found != stored && (stored.starts_with(found) || found.iter().all(|byte| *byte == 0))
}

/// What ends the temporary name a work file is unpacked under ([`write_unpacked`]).
const UNPACKING: &str = ".unpacking";

/// Write the work file `file` while a design is unpacked: the whole file under a temporary name,
/// then renamed, so the name never holds part of a file while the app runs. Unlike [`write`]
/// nothing is flushed to the disk and the folder is not searched for old temporary files: with
/// thousands of parts those two steps, once for each file, were most of the time an open took.
/// The file is a copy of what the design's file holds, so one a power cut leaves cut short is
/// unpacked again by the next open ([`unpack`]).
fn write_unpacked(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let shown = file.to_string_lossy();
    solidcraft_io::vfs::check_new(&shown).map_err(|e| e.to_string())?;
    let (Some(folder), Some(name)) = (file.parent(), file.file_name()) else { return Err(format!("{shown}: not a file in a folder")) };
    if !folder.exists() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        if let Some(hide) = HIDE.get() {
            hide(folder);
        }
    }
    let mut temporary = std::ffi::OsString::from(".~");
    temporary.push(name);
    temporary.push(UNPACKING);
    let temporary = folder.join(temporary);
    std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, file)).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("{shown}: {e}")
    })
}

/// Write the work file `file`, making its folder. A file already there is replaced.
pub(crate) fn write(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let shown = file.to_string_lossy();
    solidcraft_io::vfs::check_new(&shown).map_err(|e| e.to_string())?;
    if let Some(folder) = file.parent() {
        let made = !folder.exists();
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        if let Some(hide) = HIDE.get().filter(|_| made) {
            hide(folder);
        }
    }
    solidcraft_io::write_atomic(file, bytes, false).map_err(|e| format!("{shown}: {e}"))
}

/// The bytes of the stored part `name` of `doc`, the design saved at `own`: its work file's when
/// there is one, else the ones the design's file holds: `held`, what this session last put
/// there, before what the file held when it was opened.
pub(crate) fn bytes(doc: &Document, held: &BTreeMap<String, Blob>, own: Option<&str>, name: &str) -> Option<Blob> {
    work_file(own, name).and_then(|file| read_linked_file(&file, own).ok()).map(Blob::from).or_else(|| stored(doc, held, name).cloned())
}

/// The stored part `name` as the design's file holds it: `held`, what this session last put
/// there, else what the file held when it was opened.
pub(crate) fn stored<'a>(doc: &'a Document, held: &'a BTreeMap<String, Blob>, name: &str) -> Option<&'a Blob> {
    held.get(name).or_else(|| doc.internal.get(name))
}

/// Pack the internal components into `design`, the copy a save of the design at `own` writes:
/// each one it links, from its work file, and none it no longer links. One in `recovery` (not yet
/// restored or discarded) keeps what the design's file held: a save takes no recovery file unasked.
pub(crate) fn pack(design: &mut Cow<'_, Document>, held: &BTreeMap<String, Blob>, own: Option<&str>, recovery: &BTreeSet<String>) {
    let names = linked(design);
    if names.is_empty() && design.internal.is_empty() {
        return;
    }
    let mut stamps = own.and_then(work_folder).map(|folder| Stamps::of(&folder));
    let packed: BTreeMap<String, Blob> = names
        .into_iter()
        .filter_map(|name| {
            let kept = recovery.contains(&name).then(|| stored(design, held, &name).cloned()).flatten();
            // A work file that still holds what the design holds is not read again ([`Stamps`]).
            let unchanged = || {
                let stored = stored(design, held, &name)?;
                let same = stamps.as_mut()?.holds(&name, stored.as_bytes(), || super::design_stamps::digest(stored.as_bytes()));
                (same == Some(true)).then(|| stored.clone())
            };
            kept.or_else(unchanged).or_else(|| bytes(design, held, own, &name)).map(|blob| (name, blob))
        })
        .collect();
    if let Some(stamps) = stamps.as_mut() {
        stamps.save();
    }
    if packed != design.internal {
        design.to_mut().internal = packed;
    }
}

/// Write the internal component `name`, holding `bytes`, back into the design file at `own`
/// without saving the design: the file is read, that one part replaced and the file written
/// again atomically. The size of the file written.
pub(crate) fn write_back(own: &str, name: &str, bytes: &[u8]) -> Result<usize, String> {
    let file = solidcraft_io::vfs::read_design_path(own, None, solidcraft_io::limits::max_design_bytes()).map_err(|e| format!("{own}: {e}"))?;
    let written = solidcraft_io::with_internal_component(&file, name, bytes).map_err(|e| format!("{own}: {e}"))?;
    solidcraft_io::vfs::check_new(own).map_err(|e| e.to_string())?;
    solidcraft_io::write_atomic(Path::new(own), &written, false).map_err(|e| format!("{own}: {e}"))?;
    Ok(written.len())
}

/// The design saved at `design` is closed and no one else has it open: the work files that hold
/// exactly what its file holds are removed, and the work folder with them when nothing is left.
/// A work file that differs stays (the next open offers it as a recovery component), and so does
/// anything else in the folder. How many were removed. A design that can't be read loses nothing.
///
/// A sub-assembly's work file has a work folder of its own beside it, tidied the same way before
/// the sub-assembly's file goes. The backup the last save left beside a removed work file
/// (`<file>.bak`) goes with it: what it backs up is held by the design.
pub fn tidy_work_folder(design: &str) -> usize {
    tidy(design, 0)
}

/// How deep sub-assemblies' work folders are followed.
const TIDY_DEPTH: usize = 16;

fn tidy(design: &str, depth: usize) -> usize {
    if cfg!(target_arch = "wasm32") || depth > TIDY_DEPTH {
        return 0;
    }
    let Some(folder) = work_folder(design) else { return 0 };
    let Ok(entries) = std::fs::read_dir(&folder) else { return 0 };
    let Ok(file) = solidcraft_io::vfs::read_design_path(design, None, solidcraft_io::limits::max_design_bytes()) else { return 0 };
    // Told apart without regard to letter case, as unpacking does. The design's drawings have
    // files here too ([`super::design_drawings`]), each held when it is the drawing as saved.
    let held: BTreeMap<String, Vec<u8>> = solidcraft_io::internal_components(&file)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, bytes)| (name.to_lowercase(), bytes))
        .chain(super::design_drawings::held(&file, design))
        .collect();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        // A temporary file an unpacking left behind when the app was stopped part way.
        if name.starts_with(".~") && name.ends_with(UNPACKING) {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        let Some(stored) = held.get(&name) else { continue };
        let file = entry.path();
        if !std::fs::read(&file).is_ok_and(|bytes| bytes == *stored) {
            continue;
        }
        // Its own work folder first, while its file is still there to say what that holds.
        removed += tidy(&file.to_string_lossy(), depth + 1);
        if std::fs::remove_file(&file).is_ok() {
            removed += 1;
            let mut backup = file.into_os_string();
            backup.push(".bak");
            let _ = std::fs::remove_file(backup);
        }
    }
    // With nothing left but the record of what the files held, that goes too.
    if std::fs::read_dir(&folder).is_ok_and(|mut left| left.all(|entry| entry.is_ok_and(|entry| entry.file_name() == super::design_stamps::FILE))) {
        Stamps::remove(&folder);
    }
    // Only an empty folder goes.
    let _ = std::fs::remove_dir(&folder);
    removed
}

/// The design that owns the file at `file`, which carries a tag ([`solidcraft_io::InternalOf`]):
/// the design whose work folder it is in, else the design the tag names, beside the file or one
/// folder up. It counts only when it holds a file with the tag's key, so a design that merely
/// has the name is not taken for the owner. `None` for a file with no tag, or no owner found.
pub fn internal_owner(file: &str) -> Option<String> {
    let tag = solidcraft_io::read_internal_of_file(file)?;
    let file = absolute(Path::new(file));
    let folder = file.parent()?;
    let mut candidates = Vec::new();
    let worked =
        folder.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix(".~")).and_then(|name| name.strip_suffix(".parts"));
    if let (Some(name), Some(beside)) = (worked, folder.parent()) {
        candidates.push(beside.join(name));
    }
    // The tag's name is used only as one plain file name, never as a path.
    let named = Path::new(&tag.design);
    if named.components().count() == 1 && matches!(named.components().next(), Some(std::path::Component::Normal(_))) {
        candidates.push(folder.join(named));
        if let Some(up) = folder.parent() {
            candidates.push(up.join(named));
        }
    }
    candidates.into_iter().find(|design| *design != file && holds(design, &tag.key)).map(|design| design.to_string_lossy().into_owned())
}

/// Whether the design file at `design` holds an internal component or a drawing with the tag key `key`.
fn holds(design: &Path, key: &str) -> bool {
    let Ok(bytes) = solidcraft_io::vfs::read_design_path(&design.to_string_lossy(), None, solidcraft_io::limits::max_design_bytes()) else {
        return false;
    };
    solidcraft_io::internal_components(&bytes)
        .is_ok_and(|held| held.values().any(|part| solidcraft_io::internal_of(part).is_some_and(|tag| tag.key == key)))
        || solidcraft_io::design_drawings(&bytes).is_ok_and(|drawings| drawings.iter().any(|drawing| super::design_drawings::key(drawing) == key))
}

/// Whether `file` is in the work folder of the design saved at `design`.
pub fn in_work_folder(design: &str, file: &str) -> bool {
    member_at(Some(design), Path::new(file)).is_some()
}

/// The design `doc`, saved at `own` until now (`None`: never), was written to `path`: its stored
/// parts get work files beside the new file where they have none, so its links still find them.
pub(crate) fn carry(doc: &Document, held: &BTreeMap<String, Blob>, own: Option<&str>, path: &str) -> Vec<String> {
    let mut failed = Vec::new();
    if cfg!(target_arch = "wasm32") || own == Some(path) {
        return failed;
    }
    for name in linked(doc) {
        let Some(target) = work_file(Some(path), &name) else { continue };
        if target.exists() {
            continue;
        }
        match bytes(doc, held, own, &name) {
            Some(blob) => {
                if let Err(why) = write(&target, blob.as_bytes()) {
                    failed.push(format!("{name}: {why}"));
                }
            }
            None => failed.push(format!("{name}: its file is gone, and this design holds no copy of it")),
        }
    }
    failed
}

/// A name for a new stored part of `doc`, the design saved at `own`, made from the file `source`:
/// that file's name where it is a member's name and free, else one made valid, numbered until no
/// stored part, link or work file has it.
pub(crate) fn free_name(doc: &Document, own: &str, source: &Path) -> String {
    let given = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, extension) = match given.rsplit_once('.') {
        Some((stem, extension)) if internal::valid_name(&format!("x.{extension}")) => (stem.to_string(), extension.to_ascii_lowercase()),
        _ => (given.clone(), "scprt".to_string()),
    };
    let cleaned: String =
        stem.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ').chars().take(80).collect::<String>();
    let stem = if internal::valid_name(&format!("{cleaned}.{extension}")) { cleaned } else { "Part".to_string() };
    // Without regard to letter case: `Top.scprt` and `top.scprt` are one file on Windows and macOS.
    let taken: BTreeSet<String> = linked(doc).iter().chain(doc.internal.keys()).map(|name| name.to_lowercase()).collect();
    let free = |name: &str| !taken.contains(&name.to_lowercase()) && work_file(Some(own), name).is_none_or(|file| !file.exists());
    let first = format!("{stem}.{extension}");
    if free(&first) {
        return first;
    }
    (2..10_000).map(|n| format!("{stem} ({n}).{extension}")).find(|name| free(name)).unwrap_or(first)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn folder(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-internal-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_work_folder_sits_beside_the_design_and_only_its_files_are_members() {
        let design = if cfg!(windows) { r"C:\jobs\Counter Line.sc" } else { "/jobs/Counter Line.sc" };
        let folder = work_folder(design).unwrap();
        assert_eq!(folder.file_name().unwrap(), ".~Counter Line.sc.parts");
        assert_eq!(folder.parent(), Path::new(design).parent());
        let top = work_file(Some(design), "Top.scprt").unwrap();
        assert_eq!(top, folder.join("Top.scprt"));
        assert_eq!(member_at(Some(design), &top).as_deref(), Some("Top.scprt"));
        assert_eq!(member_at(Some(design), &folder.join("sub").join("Top.scprt")), None);
        assert_eq!(member_at(Some(design), &Path::new(design).with_file_name("Top.scprt")), None);
        assert_eq!(work_file(Some(design), "../Top.scprt"), None);
        assert_eq!(work_file(None, "Top.scprt"), None);
    }

    #[test]
    fn unpacking_writes_what_is_missing_and_replaces_nothing() {
        let dir = folder("unpack");
        let design = dir.join("Job.sc").to_string_lossy().into_owned();
        let mut doc = Document::default();
        doc.internal.insert("Top.scprt".into(), b"top".to_vec().into());
        doc.internal.insert("Leg.scprt".into(), b"leg".to_vec().into());
        let first = unpack(&doc, &design);
        assert_eq!((first.written.len(), first.differ.len(), first.failed.len()), (2, 0, 0), "{first:?}");
        let top = work_file(Some(&design), "Top.scprt").unwrap();
        assert_eq!(std::fs::read(&top).unwrap(), b"top");
        // An edited work file is what a crash leaves: it stays, and is named.
        std::fs::write(&top, b"top, edited").unwrap();
        let again = unpack(&doc, &design);
        assert_eq!(again, Unpacked { written: vec![], differ: vec!["Top.scprt".into()], failed: vec![] });
        assert_eq!(std::fs::read(&top).unwrap(), b"top, edited");
        // A work file a power cut left cut short, or all zeros, is no one's work: unpacked again.
        for left in [&b"to"[..], &b""[..], &b"\0\0\0"[..]] {
            std::fs::write(&top, left).unwrap();
            let again = unpack(&doc, &design);
            assert_eq!(again, Unpacked { written: vec!["Top.scprt".into()], differ: vec![], failed: vec![] }, "{left:?}");
            assert_eq!(std::fs::read(&top).unwrap(), b"top");
        }
        // Unpacking leaves no temporary file behind.
        let names: Vec<String> =
            std::fs::read_dir(top.parent().unwrap()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|name| !name.ends_with(UNPACKING)), "{names:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Closing a design removes the work files that hold what its file holds, and the folder when
    /// it is then empty. A work file that differs (work a crash or an unsaved design left) stays.
    #[test]
    fn closing_tidies_the_work_files_the_design_already_holds() {
        let dir = folder("tidy");
        let design = dir.join("Job.sc").to_string_lossy().into_owned();
        let mut doc = Document::default();
        doc.internal.insert("Top.scprt".into(), b"top".to_vec().into());
        doc.internal.insert("Leg.scprt".into(), b"leg".to_vec().into());
        std::fs::write(&design, solidcraft_io::write_design_container(&doc).unwrap()).unwrap();
        unpack(&doc, &design);
        let work = work_folder(&design).unwrap();
        let top = work.join("Top.scprt");
        assert!(in_work_folder(&design, &top.to_string_lossy()));
        assert!(!in_work_folder(&design, &design));
        std::fs::write(&top, b"top, edited").unwrap();
        std::fs::write(work.join("notes.txt"), b"someone's own file").unwrap();
        assert_eq!(tidy_work_folder(&design), 1);
        assert!(!work.join("Leg.scprt").exists());
        assert_eq!(std::fs::read(&top).unwrap(), b"top, edited", "work the design does not hold is never removed");
        assert!(work.join("notes.txt").exists());
        // With only matching files, the folder goes too.
        std::fs::remove_file(work.join("notes.txt")).unwrap();
        std::fs::write(&top, b"top").unwrap();
        assert_eq!(tidy_work_folder(&design), 1);
        assert!(!work.exists());
        // No folder, or a design that is gone: nothing happens.
        assert_eq!(tidy_work_folder(&design), 0);
        unpack(&doc, &design);
        std::fs::remove_file(&design).unwrap();
        assert_eq!(tidy_work_folder(&design), 0);
        assert!(top.exists(), "a design that can't be read loses nothing");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_stored_part_gets_a_free_valid_name() {
        let dir = folder("names");
        let design = dir.join("Job.sc").to_string_lossy().into_owned();
        let mut doc = Document::default();
        assert_eq!(free_name(&doc, &design, Path::new("parts/Top.scprt")), "Top.scprt");
        doc.internal.insert("Top.scprt".into(), b"top".to_vec().into());
        assert_eq!(free_name(&doc, &design, Path::new("other/Top.scprt")), "Top (2).scprt");
        assert_eq!(free_name(&doc, &design, Path::new("other/TOP.scprt")), "TOP (2).scprt", "letter case does not make a name free");
        assert_eq!(free_name(&doc, &design, Path::new("a/plate.SC")), "plate.sc");
        assert_eq!(free_name(&doc, &design, Path::new("a/con.scprt")), "Part.scprt");
        assert_eq!(free_name(&doc, &design, Path::new("a/old.solidcraft")), "old.solidcraft.scprt");
        let _ = std::fs::remove_dir_all(dir);
    }
}
