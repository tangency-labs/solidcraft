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
    let mut seen = BTreeSet::new();
    for (name, bytes) in &doc.internal {
        let Some(file) = work_file(Some(design), name) else { continue };
        // Two names that differ only in letter case are one file on Windows and macOS.
        if !seen.insert(name.to_lowercase()) {
            done.failed.push(format!("{name}: another internal component has this name in other letter case"));
            continue;
        }
        match std::fs::read(&file) {
            Ok(existing) if existing == bytes.as_bytes() => {}
            Ok(_) => done.differ.push(name.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match write(&file, bytes.as_bytes()) {
                Ok(()) => done.written.push(name.clone()),
                Err(why) => done.failed.push(format!("{name}: {why}")),
            },
            Err(e) => done.failed.push(format!("{name}: {e}")),
        }
    }
    done
}

/// Write the work file `file`, making its folder. A file already there is replaced.
pub(crate) fn write(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let shown = file.to_string_lossy();
    solidcraft_io::vfs::check_new(&shown).map_err(|e| e.to_string())?;
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    solidcraft_io::write_atomic(file, bytes, false).map_err(|e| format!("{shown}: {e}"))
}

/// The bytes of the stored part `name` of `doc`, the design saved at `own`: its work file's when
/// there is one, else the ones the design's file held.
pub(crate) fn bytes(doc: &Document, own: Option<&str>, name: &str) -> Option<Blob> {
    work_file(own, name).and_then(|file| read_linked_file(&file, own).ok()).map(Blob::from).or_else(|| doc.internal.get(name).cloned())
}

/// Pack the internal components into `design`, the copy a save of the design at `own` writes:
/// each one it links, from its work file, and none it no longer links. One in `recovery` (not yet
/// restored or discarded) keeps what the design's file held: a save takes no recovery file unasked.
pub(crate) fn pack(design: &mut Cow<'_, Document>, own: Option<&str>, recovery: &BTreeSet<String>) {
    let names = linked(design);
    if names.is_empty() && design.internal.is_empty() {
        return;
    }
    let packed: BTreeMap<String, Blob> = names
        .into_iter()
        .filter_map(|name| {
            let held = recovery.contains(&name).then(|| design.internal.get(&name).cloned()).flatten();
            held.or_else(|| bytes(design, own, &name)).map(|blob| (name, blob))
        })
        .collect();
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

/// The design `doc`, saved at `own` until now (`None`: never), was written to `path`: its stored
/// parts get work files beside the new file where they have none, so its links still find them.
pub(crate) fn carry(doc: &Document, own: Option<&str>, path: &str) -> Vec<String> {
    let mut failed = Vec::new();
    if cfg!(target_arch = "wasm32") || own == Some(path) {
        return failed;
    }
    for name in linked(doc) {
        let Some(target) = work_file(Some(path), &name) else { continue };
        if target.exists() {
            continue;
        }
        match bytes(doc, own, &name) {
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
