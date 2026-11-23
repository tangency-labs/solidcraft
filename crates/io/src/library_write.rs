//! Writing library folders (#12982): a new library, and new, changed, copied and removed entries.
//! Each write is one file and crash-safe ([`crate::safe_write`]): a new file never replaces one,
//! a changed file replaces only the one a scan read, and a link at its name is never followed.
//! A change names the revision it was made from and is refused when the entry on disk has another.
//! A removed entry moves to the library's own bin, `.removed/<kind>/`, which no scan reads.
//! Read-only places, the built-in library and unavailable libraries are never written. Every path
//! follows the network rules a design's links follow, as a scan's reads do.
//!
//! After a write, the library written is read again with a scan's reads and caps, and the
//! [`Libraries`] returned hold it, so the next command sees the write.
use super::{Kind, LIMITS, Libraries, Library, Listed, MANIFEST_SUFFIX, Place, Reason, Scanned, Seen, Status, checked, list, read_library};
use crate::catalog::{Definition, Entry, Provenance, identifier, text};
use std::path::Path;
use std::sync::Arc;

/// The library's own bin for removed entries; a scan reads only the kind folders, never this.
pub const BIN: &str = ".removed";

/// What a write made or changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// The library's id.
    pub library: String,
    /// The entry's id, when the write was an entry's.
    pub entry: Option<String>,
    pub revision: Option<String>,
    /// The file written, or moved to the bin, from the library's folder, with `/` between names.
    pub file: String,
}

fn join(dir: &str, name: &str) -> String {
    Path::new(dir).join(name).to_string_lossy().into_owned()
}

/// `e`, a failed read or write at `path`, said with the path once: a folder check's refusal names
/// the path it refused already, so it is left as it is.
fn at(path: &str, e: impl std::fmt::Display) -> String {
    let e = e.to_string();
    if e.contains(path) { e } else { format!("{path}: {e}") }
}

/// The reads and writes a write makes, each checked first as a scan's reads are.
mod checked_write {
    use crate::vfs::{self, check_design_path, check_new_design_path};
    use std::io::Result;
    use std::path::Path;

    pub(super) fn create(path: &str, bytes: &[u8]) -> Result<()> {
        check_new_design_path(path, None)?;
        crate::safe_write::create_atomic(Path::new(path), bytes)
    }
    pub(super) fn replace(path: &str, bytes: &[u8]) -> Result<()> {
        check_design_path(path, None)?;
        crate::safe_write::write_atomic_no_follow(Path::new(path), bytes)
    }
    pub(super) fn move_to(from: &str, to: &str) -> Result<()> {
        check_design_path(from, None)?;
        check_new_design_path(to, None)?;
        crate::safe_write::move_no_replace(Path::new(from), Path::new(to))
    }
    pub(super) fn make_dirs(path: &str) -> Result<()> {
        check_new_design_path(path, None)?;
        vfs::create_new_dir_all(path)
    }
}

impl Libraries {
    /// The library `id` as this set uses it, and the scan it came from.
    fn find(&self, id: &str) -> Result<(&Arc<Scanned>, &Library), String> {
        let mut statuses = self.set.libraries().map(|(_, status)| status);
        for scan in self.set.places() {
            for library in &scan.libraries {
                let status = statuses.next();
                if library.id() != Some(id) {
                    continue;
                }
                return match status {
                    Some(Status::Available) => Ok((scan, library)),
                    Some(Status::Unavailable(why)) => Err(format!("the library {id} at {} {why}", library.shown_at())),
                    None => Err(format!("no library {id} is listed")),
                };
            }
        }
        Err(format!("no library {id} is listed"))
    }

    /// The library `id`, when it may be written: never the built-in one, nor one in a read-only
    /// place, nor one the scan found read-only.
    fn writable(&self, id: &str) -> Result<(&Arc<Scanned>, &Library, &str), String> {
        let (scan, library) = self.find(id)?;
        let Some(folder) = library.folder() else { return Err(Reason::BuiltIn.to_string()) };
        if self.my_library.iter().chain(&self.rows).any(|row| row.place == scan.place && row.read_only) {
            return Err(format!("the library {id} is in a read-only place; allow changes to the place first"));
        }
        if let Some(why) = library.read_only() {
            return Err(format!("the library {id} can't be changed: {why}"));
        }
        Ok((scan, library, folder))
    }

    /// The entry `id` with its library, and the file it was read from.
    fn listed_entry(&self, id: &str) -> Result<(&Library, &Listed, Entry), String> {
        let library_id = id.split_once('/').map(|(library, _)| library).ok_or_else(|| format!("{id}: an entry id is <library id>/<entry id>"))?;
        let (_, library) = self.find(library_id)?;
        let listed = library
            .entries
            .iter()
            .find(|l| l.entry().is_ok_and(|e| e.id == id))
            .ok_or_else(|| format!("no entry {id} is listed in {}", library.shown_at()))?;
        let entry = listed.entry().map_err(|why| why.to_string())?.clone();
        Ok((library, listed, entry))
    }

    /// These libraries with the one in `folder` (inside `scan`) read again, or with `scan`'s place
    /// scanned again when `folder` is `None` (a new library).
    fn read_again(&self, scan: &Arc<Scanned>, folder: Option<&str>) -> Libraries {
        let fresh = match folder {
            None => super::scan(&scan.place),
            Some(folder) => {
                let mut fresh = Scanned::clone(scan);
                let mut seen = Seen::default();
                let library = match list(folder, &LIMITS, &mut seen) {
                    Ok(listing) => read_library(folder, listing, &LIMITS, &mut seen),
                    Err(reason) => Library::unavailable(folder, reason),
                };
                if let Some(old) = fresh.libraries.iter_mut().find(|l| l.folder() == Some(folder)) {
                    *old = library;
                    // Its listings too, so the check after a write finds the place as it left it.
                    fresh.seen.replace(folder, seen);
                }
                fresh
            }
        };
        let fresh = Arc::new(fresh);
        let places = self.set.places().iter().map(|p| if Arc::ptr_eq(p, scan) { Arc::clone(&fresh) } else { Arc::clone(p) }).collect();
        Libraries { my_library: self.my_library.clone(), rows: self.rows.clone(), states: self.states.clone(), set: super::Set::shared(places) }
    }

    /// A new library called `name` in the listed place at `place`: in a folder place, a new
    /// subfolder named after it; in a library place, that folder itself, which must hold nothing
    /// yet. The manifest is written last, so a write stopped part way leaves no library.
    pub fn create_library(&self, place: &str, name: &str, provenance: Provenance) -> Result<(Libraries, Written), String> {
        if !text(name, 128) {
            return Err("a library needs a 1-128 byte name".into());
        }
        provenance.validate().map_err(|e| e.to_string())?;
        let row = self.listed(place).ok_or_else(|| format!("no place is listed at {place}"))?;
        if row.read_only {
            return Err(format!("{place} is read-only; allow changes to the place first"));
        }
        let scan = self.set.places().iter().find(|s| s.place == row.place).ok_or_else(|| format!("{place} hasn't been scanned yet"))?;
        // A missing place is made; any other reason it can't be read stops the write.
        if let Status::Unavailable(why) = &scan.status
            && *why != Reason::Missing
        {
            return Err(format!("{place} {why}"));
        }
        let folder = match &row.place {
            Place::Folder(dir) => {
                let taken = match checked::dirs_in(dir, LIMITS.libraries_per_folder) {
                    Ok(taken) => taken,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(e) => return Err(at(dir, e)),
                };
                if taken.len() >= LIMITS.libraries_per_folder {
                    return Err(format!("{dir} holds the most folders a place reads ({})", LIMITS.libraries_per_folder));
                }
                let taken: Vec<String> = taken.iter().map(|d| super::file_label(d).to_lowercase()).collect();
                let stem = unique(&stem(name), |candidate| taken.contains(&candidate.to_lowercase()))
                    .ok_or_else(|| format!("{dir}: every folder name for {name} is taken"))?;
                join(dir, &stem)
            }
            Place::Library(dir) => {
                if !scan.libraries.iter().all(|l| l.status == Status::Unavailable(Reason::NoManifest)) {
                    return Err(format!("{dir} holds a library already; a library place is one library"));
                }
                dir.clone()
            }
            Place::BuiltIn => return Err(Reason::BuiltIn.to_string()),
        };
        // In a library place the folder must be empty: never turn someone's folder into a library.
        if matches!(row.place, Place::Library(_)) && !empty_or_missing(&folder)? {
            return Err(format!("{folder} isn't empty; a new library needs an empty folder"));
        }
        let id = new_library_id(self, name);
        let header = super::Header { format: super::FORMAT.into(), id: id.clone(), name: name.into(), provenance };
        let bytes = serde_json::to_vec_pretty(&header).map_err(|e| e.to_string())?;
        let manifest = join(&folder, &format!("{}{MANIFEST_SUFFIX}", stem(name)));
        for kind in [Kind::Material, Kind::Appearance] {
            checked_write::make_dirs(&join(&folder, kind.folder())).map_err(|e| at(&folder, e))?;
        }
        checked_write::create(&manifest, &bytes).map_err(|e| at(&manifest, e))?;
        let written = Written { library: id, entry: None, revision: None, file: super::file_label(&manifest) };
        Ok((self.read_again(scan, None), written))
    }

    /// A new entry in the library `library`, under a new id made from its name.
    pub fn add_entry(&self, library: &str, definition: Definition, provenance: Provenance) -> Result<(Libraries, Written), String> {
        let (scan, target, folder) = self.writable(library)?;
        let local = new_entry_id(target, &definition);
        let entry = Entry::with_id(library, &local, definition, provenance).map_err(|e| e.to_string())?;
        let file = self.write_new(target, folder, &entry)?;
        let written = Written { library: library.into(), entry: Some(entry.id), revision: Some(entry.revision), file };
        Ok((self.read_again(scan, Some(folder)), written))
    }

    /// A copy of the entry `id` (from any library, the built-in one included) in the library `to`,
    /// under a new id there; it keeps its provenance.
    pub fn copy_entry(&self, id: &str, to: &str) -> Result<(Libraries, Written), String> {
        let (_, _, source) = self.listed_entry(id)?;
        self.add_entry(to, source.definition, source.provenance)
    }

    /// The entry `id` with a new definition (and provenance, when given), when the file still
    /// holds `revision`, the one the caller read.
    pub fn edit_entry(
        &self,
        id: &str,
        revision: &str,
        definition: Definition,
        provenance: Option<Provenance>,
    ) -> Result<(Libraries, Written), String> {
        let (library, listed, old) = self.listed_entry(id)?;
        let library_id = library.id().unwrap_or_default().to_string();
        let (scan, library, folder) = self.writable(&library_id)?;
        if Kind::of(&definition) != listed.kind() {
            return Err(format!("{id} is a {}; its kind can't change", old.definition.kind()));
        }
        let path = join(&join(folder, listed.kind().folder()), listed.label());
        let on_disk = unchanged(&path, listed.kind(), &library_id, revision)?;
        let local = id.split_once('/').map(|(_, local)| local).unwrap_or_default();
        let entry = Entry::with_id(&library_id, local, definition, provenance.unwrap_or(old.provenance)).map_err(|e| e.to_string())?;
        let bytes = encode(&entry)?;
        within_library_cap(library, on_disk, bytes.len() as u64)?;
        checked_write::replace(&path, &bytes).map_err(|e| at(&path, e))?;
        let file = listed.file();
        let written = Written { library: library_id, entry: Some(entry.id), revision: Some(entry.revision), file };
        Ok((self.read_again(scan, Some(folder)), written))
    }

    /// Move the entry `id`'s file to its library's bin, when the file still holds `revision`.
    pub fn remove_entry(&self, id: &str, revision: &str) -> Result<(Libraries, Written), String> {
        let (library, listed, _) = self.listed_entry(id)?;
        let library_id = library.id().unwrap_or_default().to_string();
        let (scan, _, folder) = self.writable(&library_id)?;
        let path = join(&join(folder, listed.kind().folder()), listed.label());
        unchanged(&path, listed.kind(), &library_id, revision)?;
        let to = to_bin(&path, folder, listed.kind(), listed.label())?;
        let written = Written { library: library_id, entry: Some(id.into()), revision: Some(revision.into()), file: to };
        Ok((self.read_again(scan, Some(folder)), written))
    }

    /// Write `entry` to a new file in its kind folder, named after it, never over another file.
    fn write_new(&self, library: &Library, folder: &str, entry: &Entry) -> Result<String, String> {
        let kind = Kind::of(&entry.definition);
        let dir = join(folder, kind.folder());
        let files = match checked::files_in(&dir, LIMITS.entries_per_kind) {
            Ok(files) => files,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Err(format!("{dir} holds the most entries a library reads ({})", LIMITS.entries_per_kind));
            }
            Err(e) => return Err(at(&dir, e)),
        };
        if files.len() >= LIMITS.entries_per_kind {
            return Err(format!("{dir} holds the most entries a library reads ({})", LIMITS.entries_per_kind));
        }
        let taken: Vec<String> = files.iter().map(|f| super::file_label(f).to_lowercase()).collect();
        let name = file_name(entry.definition.name(), |n| taken.iter().any(|t| t == n)).ok_or_else(|| format!("{dir}: every file name is taken"))?;
        let bytes = encode(entry)?;
        within_library_cap(library, 0, bytes.len() as u64)?;
        checked_write::make_dirs(&dir).map_err(|e| at(&dir, e))?;
        let path = join(&dir, &name);
        checked_write::create(&path, &bytes).map_err(|e| at(&path, e))?;
        Ok(super::entry_file(kind, &name))
    }
}

/// An entry file's bytes, within the entry cap a scan keeps.
fn encode(entry: &Entry) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec_pretty(entry).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMITS.entry_bytes {
        return Err(format!("the entry is larger than {} bytes", LIMITS.entry_bytes));
    }
    Ok(bytes)
}

/// A write that would take the library past its cap would make it unavailable at the next
/// scan: refused before it is written. `old` is the size of the file it replaces.
fn within_library_cap(library: &Library, old: u64, new: u64) -> Result<(), String> {
    if library.bytes.saturating_sub(old).saturating_add(new) > LIMITS.library_bytes {
        return Err(format!("the library would be larger than {} bytes", LIMITS.library_bytes));
    }
    Ok(())
}

/// The file at `path` still holds the entry at `revision` (read again, as a scan reads it):
/// its size on disk, or why not.
fn unchanged(path: &str, kind: Kind, library: &str, revision: &str) -> Result<u64, String> {
    let changed = || format!("{path}: the entry changed since it was read; list it again");
    let bytes = match super::read_capped(path, super::Cap::EntryBytes, LIMITS.entry_bytes) {
        Ok(bytes) => bytes,
        Err(Reason::Missing) => return Err(format!("{path}: removed since it was read")),
        Err(_) => return Err(changed()),
    };
    match super::read_entry(&bytes, kind, library) {
        Ok(entry) if entry.revision == revision => Ok(bytes.len() as u64),
        _ => Err(changed()),
    }
}

/// Move `path` into the bin of the library in `folder` under its own name, or `name (2).json` when the
/// bin holds that name: the file's name from the library's folder. The bin is never listed (it can grow
/// past any cap): each name is tried, and a name already there refuses the move, so nothing in the bin
/// is ever replaced. The one place a removal happens, so a different bin is a change here only.
fn to_bin(path: &str, folder: &str, kind: Kind, label: &str) -> Result<String, String> {
    let bin = join(&join(folder, BIN), kind.folder());
    checked_write::make_dirs(&bin).map_err(|e| at(&bin, e))?;
    let stem = label.get(..label.len().saturating_sub(5)).filter(|_| super::ends_with(label, ".json")).unwrap_or(label);
    for candidate in std::iter::once(stem.to_string()).chain((2..=LIMITS.entries_per_kind + 2).map(|n| format!("{stem} ({n})"))) {
        let name = format!("{candidate}.json");
        match checked_write::move_to(path, &join(&bin, &name)) {
            Ok(()) => return Ok(format!("{BIN}/{}", super::entry_file(kind, &name))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(at(path, e)),
        }
    }
    Err(format!("{bin}: every name for {label} is taken"))
}

fn empty_or_missing(folder: &str) -> Result<bool, String> {
    let dirs = match checked::dirs_in(folder, 1) {
        Ok(dirs) => dirs,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Ok(false),
        Err(e) => return Err(at(folder, e)),
    };
    let files = checked::files_in(folder, 1);
    Ok(dirs.is_empty() && files.is_ok_and(|f| f.is_empty()))
}

/// A new library id from `name`: `user.<slug>`, then `-2`, `-3`, against every library id the
/// set lists. Never `solidcraft.`, which is kept for SolidCraft's own libraries.
fn new_library_id(libraries: &Libraries, name: &str) -> String {
    let ids: Vec<&str> = libraries.set.libraries().filter_map(|(l, _)| l.id()).collect();
    unique_id("user.", &super::slug(name), "library", |id| ids.contains(&id))
}

/// A new entry id in `library` from the entry's kind and name: `material-<slug>`, then `-2`, `-3`,
/// against every id the library lists, refused entries' included, as the built-in ids are made.
fn new_entry_id(library: &Library, definition: &Definition) -> String {
    let taken = |local: &str| library.entries.iter().any(|l| l.entry().is_ok_and(|e| e.id.split_once('/').is_some_and(|(_, id)| id == local)));
    unique_id(&format!("{}-", definition.kind()), &super::slug(definition.name()), "entry", taken)
}

/// `prefix` + `slug` (or `fallback`) cut to an [`identifier`]'s 64 bytes, then `-2`, `-3`, … until
/// `taken` says no.
fn unique_id(prefix: &str, slug: &str, fallback: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = if slug.is_empty() { fallback } else { slug };
    let fit = |suffix: &str| {
        let room = 64usize.saturating_sub(prefix.len() + suffix.len());
        let mut body: String = base.chars().take(room).collect();
        while body.ends_with('-') {
            body.pop();
        }
        format!("{prefix}{body}{suffix}")
    };
    std::iter::once(String::new())
        .chain((2..).map(|n: u32| format!("-{n}")))
        .map(|suffix| fit(&suffix))
        .find(|id| identifier(id) && !taken(id))
        .unwrap_or_else(|| fit("-x"))
}

/// A file or folder name made from `name`: a label that works on every system SolidCraft runs
/// on, never an identity. Windows' name rules are [`crate::vfs`]'s.
fn stem(name: &str) -> String {
    const MAX_STEM: usize = 100;
    let mut stem: String =
        name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) || crate::vfs::invisible(c) { '_' } else { c }).collect();
    // Windows drops trailing dots and spaces; a leading dot hides the file elsewhere (and the bin's
    // name starts with one).
    stem = stem.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    while stem.len() > MAX_STEM {
        stem.pop();
    }
    if stem.is_empty() {
        stem = "entry".into();
    }
    // `NAME~1` reads as another file's short name on Windows.
    if crate::vfs::short_name(&stem) {
        stem = stem.replace('~', "-");
    }
    if crate::vfs::device_name(&stem) {
        stem.insert(0, '_');
    }
    stem
}

/// `stem`, then `stem (2)`, `stem (3)`, … until `taken` says no; `None` past the entry cap.
fn unique(stem: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    std::iter::once(stem.to_string()).chain((2..=LIMITS.entries_per_kind + 2).map(|n| format!("{stem} ({n})"))).find(|candidate| !taken(candidate))
}

/// The file name a new entry called `name` gets. `taken` says whether a lowercase file name is
/// already used, as names differ only by case on some drives. `None` when every candidate is taken.
pub fn file_name(name: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    unique(&stem(name), |candidate| taken(&format!("{candidate}.json").to_lowercase())).map(|stem| format!("{stem}.json"))
}
