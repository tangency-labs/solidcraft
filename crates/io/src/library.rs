//! Material and appearance libraries read from their places (#12982): the built-in "SolidCraft"
//! library, one library folder, or a folder of library folders. This module reads; [`write`]
//! makes libraries and changes their entries.
//!
//! A library folder holds `<Library name>.sclibrary.json` (the library's id, name and provenance)
//! and one JSON file per entry in `materials/` and `appearances/`, each in the catalog's entry
//! shape ([`Entry`]). A file's name is only a label: an entry is known by the id inside it.
//! `textures/` is not read yet.
//!
//! Nothing is dropped quietly. A place that can't be read stays listed with its reason, a library
//! that can't be used stays listed as unavailable with its reason, and an entry that can't be used
//! stays in its library's list as refused with its reason.
use crate::catalog::{Definition, Entry, LibraryLook, LibraryMaterial, Provenance, identifier, text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::sync::Arc;

/// The end of a library folder's manifest file name.
pub const MANIFEST_SUFFIX: &str = ".sclibrary.json";
/// The manifest's format; a later version is read as made by a newer SolidCraft.
pub const FORMAT: &str = "solidcraft-library-folder/1";
const FORMAT_FAMILY: &str = "solidcraft-library-folder/";
/// The id of the built-in library.
pub const BUILT_IN_ID: &str = "solidcraft.builtin";

/// How much one scan reads, at most. A limit reached is a [`Reason::OverLimit`], never a
/// shorter list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub manifest_bytes: u64,
    pub entry_bytes: u64,
    /// Entry files in `materials/`, and again in `appearances/`.
    pub entries_per_kind: usize,
    /// The manifest and every entry file together.
    pub library_bytes: u64,
    /// Files directly in a library folder, its manifest included, and again its subfolders.
    pub files_beside_manifest: usize,
    /// Subfolders of a folder place, library folders or not.
    pub libraries_per_folder: usize,
}
/// The limits every scan uses.
pub const LIMITS: Limits = Limits {
    manifest_bytes: 16 * 1024,
    entry_bytes: 256 * 1024,
    entries_per_kind: crate::catalog::MAX_ENTRIES,
    library_bytes: crate::catalog::MAX_BYTES as u64,
    files_beside_manifest: 64,
    libraries_per_folder: 64,
};

/// Where libraries are read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum Place {
    /// The libraries that come with SolidCraft (read-only).
    BuiltIn,
    /// One library folder.
    Library(String),
    /// A folder whose subfolders are library folders.
    Folder(String),
}

/// The most places a user's list holds, My Library apart.
pub const MAX_PLACES: usize = 64;

/// Why `path` can't be a place's folder, from its text alone: a place is a whole folder path, on
/// this computer or a network share, never empty, relative (read from wherever SolidCraft started)
/// or a device. The commands, the preferences and every scan keep this one rule.
pub fn place_refusal(path: &str) -> Option<&'static str> {
    use crate::vfs::Remote;
    match crate::vfs::remote(path) {
        Some(Remote::Device) => Some("a library place is a folder, not a device"),
        Some(Remote::Network(_)) => None,
        None if std::path::Path::new(path).is_absolute() => None,
        None => Some("a library place needs an absolute path"),
    }
}

impl Place {
    /// The folder a place reads; `None` for the built-in library.
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::BuiltIn => None,
            Self::Library(path) | Self::Folder(path) => Some(path),
        }
    }
}

/// One of the user's places: where, and whether SolidCraft may change the libraries there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceRow {
    pub place: Place,
    pub read_only: bool,
}

/// A change to the user's list of places. A place is named by its folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceEdit {
    /// Adds a place, at the end or at `at`.
    Add {
        row: PlaceRow,
        at: Option<usize>,
    },
    Remove {
        path: String,
    },
    /// Changes a place: whether it is read-only, and where it is in the list.
    Set {
        path: String,
        read_only: Option<bool>,
        at: Option<usize>,
    },
}

/// What a folder is compared by, so one folder is one place however it is spelled: on Windows,
/// ASCII case and the two separators don't matter; elsewhere only a trailing `/`. Text only: a
/// network folder is never asked.
fn place_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_ascii_lowercase().replace('/', "\\").trim_end_matches('\\').to_string()
    } else {
        path.trim_end_matches('/').to_string()
    }
}

/// A library folder's manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub format: String,
    pub id: String,
    pub name: String,
    pub provenance: Provenance,
}

/// One of the [`Limits`], named so a window can say which in any language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    ManifestBytes,
    EntryBytes,
    EntriesPerKind(Kind),
    LibraryBytes,
    FilesBesideManifest,
    LibrariesPerFolder,
}

/// Why a library or an entry can't be used, or why a library can't be changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    Missing,
    Unreadable(String),
    NoManifest,
    TwoManifests,
    Invalid(String),
    /// Made by a newer SolidCraft: a later format or a field this version doesn't know.
    NewerFormat,
    OverLimit {
        cap: Cap,
        limit: u64,
    },
    /// The built-in library is never changed; a copy can be.
    BuiltIn,
    /// Another library, or another entry, has the same id. `other` names it: the folder of the library
    DuplicateId {
        other: String,
    },
    /// The entry's id belongs to another library.
    ForeignId,
    /// A material in `appearances/`, or an appearance in `materials/`.
    WrongKind,
    Unsupported(String),
}
impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("not found"),
            Self::Unreadable(why) => write!(f, "can't be read: {why}"),
            Self::NoManifest => write!(f, "not a library folder: no *{MANIFEST_SUFFIX} file"),
            Self::TwoManifests => write!(f, "more than one *{MANIFEST_SUFFIX} file"),
            Self::Invalid(why) => write!(f, "damaged: {why}"),
            Self::NewerFormat => f.write_str("made by a newer SolidCraft"),
            Self::OverLimit { cap, limit } => match cap {
                Cap::ManifestBytes => write!(f, "the manifest is larger than {limit} bytes"),
                Cap::EntryBytes => write!(f, "the entry file is larger than {limit} bytes"),
                Cap::EntriesPerKind(kind) => write!(f, "more than {limit} files in {}/", kind.folder()),
                Cap::LibraryBytes => write!(f, "the library is larger than {limit} bytes"),
                Cap::FilesBesideManifest => write!(f, "more than {limit} files or folders beside the manifest"),
                Cap::LibrariesPerFolder => write!(f, "more than {limit} folders"),
            },
            Self::BuiltIn => f.write_str("the built-in library can't be changed; save a copy to edit"),
            Self::DuplicateId { other } => write!(f, "the same id as {other}"),
            Self::ForeignId => f.write_str("its id belongs to another library"),
            Self::WrongKind => f.write_str("in the wrong folder for its kind"),
            Self::Unsupported(why) => write!(f, "not supported yet: {why}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Available,
    Unavailable(Reason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Material,
    Appearance,
}
impl Kind {
    /// The kind as entries and commands name it: `material` or `appearance`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Material => "material",
            Self::Appearance => "appearance",
        }
    }
    fn folder(self) -> &'static str {
        match self {
            Self::Material => "materials",
            Self::Appearance => "appearances",
        }
    }
    fn of(definition: &Definition) -> Self {
        match definition {
            Definition::Material(_) => Self::Material,
            Definition::Appearance(_) => Self::Appearance,
        }
    }
}

/// One entry as listed: its label (file name, or the built-in name) and the entry or why not.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    label: String,
    kind: Kind,
    state: Result<Entry, Reason>,
}
impl Listed {
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    /// The entry, or why it can't be used.
    pub fn entry(&self) -> Result<&Entry, &Reason> {
        self.state.as_ref()
    }
    /// Its file from the library's folder: `materials/Steel.json`.
    pub fn file(&self) -> String {
        entry_file(self.kind, &self.label)
    }
}

/// An entry file's name from its library's folder, with `/` between names on every system.
fn entry_file(kind: Kind, label: &str) -> String {
    format!("{}/{label}", kind.folder())
}

/// One library as a scan found it. Only a scan makes one, so what it says is what was read.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    folder: Option<String>,
    header: Option<Header>,
    status: Status,
    read_only: Option<Reason>,
    entries: Vec<Listed>,
    /// The bytes read: the manifest and every entry file (0 for the built-in library).
    bytes: u64,
}
impl Library {
    fn unavailable(folder: &str, reason: Reason) -> Self {
        Self { folder: Some(folder.to_string()), header: None, status: Status::Unavailable(reason), read_only: None, entries: Vec::new(), bytes: 0 }
    }
    /// The library's own folder; `None` for the built-in library.
    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }
    /// The manifest, when it could be read.
    pub fn header(&self) -> Option<&Header> {
        self.header.as_ref()
    }
    pub fn id(&self) -> Option<&str> {
        self.header.as_ref().map(|h| h.id.as_str())
    }
    pub fn name(&self) -> Option<&str> {
        self.header.as_ref().map(|h| h.name.as_str())
    }
    /// What the scan found; [`Set`] can add its own verdict on top.
    pub fn status(&self) -> &Status {
        &self.status
    }
    /// Why the library can't be changed, when it can't.
    pub fn read_only(&self) -> Option<&Reason> {
        self.read_only.as_ref()
    }
    pub fn entries(&self) -> &[Listed] {
        &self.entries
    }
    /// Where the library is, for messages: its folder, or "the built-in library".
    pub fn shown_at(&self) -> &str {
        self.folder.as_deref().unwrap_or("the built-in library")
    }
}

/// One place as a scan found it: whether it could be read, and the libraries in it. A folder
/// place may hold no library at all; it is still listed.
#[derive(Clone, Debug, PartialEq)]
pub struct Scanned {
    place: Place,
    status: Status,
    libraries: Vec<Library>,
    /// Every listing the scan made, for [`rescan_if_changed`].
    seen: Seen,
}
impl Scanned {
    pub fn place(&self) -> &Place {
        &self.place
    }
    pub fn status(&self) -> &Status {
        &self.status
    }
    pub fn libraries(&self) -> &[Library] {
        &self.libraries
    }
}

/// What a place holds, with its status, each library's status and each entry's, within
/// [`LIMITS`]. Reads only the place, through [`crate::vfs`], so the file scope of the thread that
/// calls it applies: a scan on another thread runs inside the caller's [`crate::vfs::scoped`].
pub fn scan(place: &Place) -> Scanned {
    scan_with(place, &LIMITS)
}

/// [`scan`] with other limits, for tests at the limits' boundaries.
pub(crate) fn scan_with(place: &Place, limits: &Limits) -> Scanned {
    let mut seen = Seen::default();
    let found = match (place, place.path().and_then(place_refusal)) {
        (_, Some(why)) => Err(Reason::Unreadable(why.into())),
        (Place::BuiltIn, None) => Ok(vec![built_in()]),
        (Place::Library(dir), None) => list(dir, limits, &mut seen).map(|listing| vec![read_library(dir, listing, limits, &mut seen)]),
        (Place::Folder(dir), None) => seen.folders(dir, Cap::LibrariesPerFolder, limits.libraries_per_folder).map(|dirs| {
            dirs.iter()
                .filter_map(|d| match list(d, limits, &mut seen) {
                    // Listed, not skipped: a subfolder that can't be listed may be a library.
                    Err(reason) => Some(Library::unavailable(d, reason)),
                    // A subfolder with no manifest isn't a library; one with two is listed as broken.
                    Ok(listing) if listing.manifests.is_empty() => None,
                    Ok(listing) => Some(read_library(d, listing, limits, &mut seen)),
                })
                .collect()
        }),
    };
    let (status, libraries) = match found {
        Ok(libraries) => (Status::Available, libraries),
        Err(reason) => (Status::Unavailable(reason), Vec::new()),
    };
    Scanned { place: place.clone(), status, libraries, seen }
}

/// `scan` made again when its place's files may have changed since; `None` keeps it (and the
/// same `Arc`, so nothing that holds it reads again). Every listing the scan made is made again,
/// through the same path checks and within the same caps ([`Seen`]), and compared with what the
/// scan saw: a file's length and time, a file or folder added or gone, a folder that went missing,
/// turned unreadable, went over a cap or came back. Nothing is opened.
///
/// What it can't see, on purpose: an edit that keeps a file's length and its time. A drive that
/// keeps time in steps (FAT: 2 s) can hide an edit made inside one step; the Refresh button scans
/// in full for that. A file with no time (browser storage) counts as changed.
pub fn rescan_if_changed(scan: &Scanned) -> Option<Scanned> {
    rescan_with(scan, &LIMITS)
}

/// [`rescan_if_changed`] whose new scan uses other limits (tests); the check uses the scan's own.
pub(crate) fn rescan_with(scan: &Scanned, limits: &Limits) -> Option<Scanned> {
    scan.seen.changed().then(|| scan_with(&scan.place, limits))
}

/// Every listing a scan made, in its order: what the scan saw of its place's files. A listing
/// holds no more than its cap allows (past it, only the reason), so the record is at most the
/// names and stamps the caps let one scan list.
#[derive(Clone, Debug, Default, PartialEq)]
struct Seen(Vec<Look>);

#[derive(Clone, Debug, PartialEq)]
struct Look {
    dir: String,
    list: List,
    cap: Cap,
    limit: usize,
    found: Result<Found, Reason>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum List {
    Folders,
    Files,
}

#[derive(Clone, Debug, PartialEq)]
enum Found {
    Folders(Vec<String>),
    Files(Vec<crate::vfs::Stamped>),
}

/// The two ways a scan, or the check after it, lists a folder: through the scan's path check
/// ([`checked`]), within `limit`, a failure named for `cap`.
fn folders_in(dir: &str, cap: Cap, limit: usize) -> Result<Vec<String>, Reason> {
    checked::dirs_in(dir, limit).map_err(|e| listing_reason(&e, cap, limit))
}
fn files_in(dir: &str, cap: Cap, limit: usize) -> Result<Vec<crate::vfs::Stamped>, Reason> {
    checked::stamped_files_in(dir, limit).map_err(|e| listing_reason(&e, cap, limit))
}

impl Look {
    /// The same listing made now.
    fn again(&self) -> Result<Found, Reason> {
        match self.list {
            List::Folders => folders_in(&self.dir, self.cap, self.limit).map(Found::Folders),
            List::Files => files_in(&self.dir, self.cap, self.limit).map(Found::Files),
        }
    }
    /// A file with no time: a change to it may keep its length, so it is never taken as unchanged.
    fn timeless(&self) -> bool {
        matches!(&self.found, Ok(Found::Files(files)) if files.iter().any(|f| f.modified.is_none()))
    }
}

impl Seen {
    fn folders(&mut self, dir: &str, cap: Cap, limit: usize) -> Result<Vec<String>, Reason> {
        let found = folders_in(dir, cap, limit);
        self.0.push(Look { dir: dir.to_string(), list: List::Folders, cap, limit, found: found.clone().map(Found::Folders) });
        found
    }
    /// The files' paths; their stamps stay in the record.
    fn files(&mut self, dir: &str, cap: Cap, limit: usize) -> Result<Vec<String>, Reason> {
        let found = files_in(dir, cap, limit);
        self.0.push(Look { dir: dir.to_string(), list: List::Files, cap, limit, found: found.clone().map(Found::Files) });
        found.map(|files| files.into_iter().map(|f| f.path).collect())
    }
    fn changed(&self) -> bool {
        self.0.iter().any(|look| look.timeless() || look.again() != look.found)
    }
    /// The listings of the library in `folder` (its own and its kind folders') swapped for `new`,
    /// where the old ones were: the order a fresh scan of the place would have.
    fn replace(&mut self, folder: &str, new: Seen) {
        let folder = std::path::Path::new(folder);
        let inside = |look: &Look| {
            let dir = std::path::Path::new(&look.dir);
            dir == folder || dir.parent() == Some(folder)
        };
        let at = self.0.iter().position(inside).unwrap_or(self.0.len());
        self.0.retain(|look| !inside(look));
        self.0.splice(at..at, new.0);
    }
}

/// The reads a scan makes. A library place follows the network rules a design's links follow:
/// each path is checked before anything on the disk or the network is asked.
mod checked {
    use crate::vfs::{self, check_design_path};
    use std::io::Result;

    pub(super) fn dirs_in(dir: &str, max: usize) -> Result<Vec<String>> {
        check_design_path(dir, None)?;
        vfs::dirs_in(dir, max)
    }
    pub(super) fn files_in(dir: &str, max: usize) -> Result<Vec<String>> {
        check_design_path(dir, None)?;
        vfs::files_in(dir, max)
    }
    pub(super) fn stamped_files_in(dir: &str, max: usize) -> Result<Vec<vfs::Stamped>> {
        check_design_path(dir, None)?;
        vfs::stamped_files_in(dir, max)
    }
    pub(super) fn len(path: &str) -> Result<u64> {
        check_design_path(path, None)?;
        vfs::len(path)
    }
    pub(super) fn read(path: &str, max: usize) -> Result<Vec<u8>> {
        check_design_path(path, None)?;
        vfs::read(path, max)
    }
}

/// What is directly in a library folder: its manifest files and its subfolders, each listed once.
struct Listing {
    manifests: Vec<String>,
    folders: Vec<String>,
}

fn list(dir: &str, limits: &Limits, seen: &mut Seen) -> Result<Listing, Reason> {
    // Folders first: in the browser only the folder listing can tell a missing folder from an empty one.
    let (cap, limit) = (Cap::FilesBesideManifest, limits.files_beside_manifest);
    let folders = seen.folders(dir, cap, limit)?;
    let files = seen.files(dir, cap, limit)?;
    Ok(Listing { manifests: files.into_iter().filter(|p| ends_with(p, MANIFEST_SUFFIX)).collect(), folders })
}

fn listing_reason(e: &std::io::Error, cap: Cap, limit: usize) -> Reason {
    match e.kind() {
        ErrorKind::NotFound => Reason::Missing,
        ErrorKind::InvalidData => Reason::OverLimit { cap, limit: limit as u64 },
        _ => Reason::Unreadable(e.to_string()),
    }
}

fn file_label(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

fn ends_with(path: &str, suffix: &str) -> bool {
    file_label(path).to_ascii_lowercase().ends_with(suffix)
}

/// A file made by a newer SolidCraft has a field or a kind this version doesn't know, at any
/// depth. serde says so in its message, also when a nested reader (a material card) adds its
/// own prefix; anything else is damage.
fn newer(e: &serde_json::Error) -> bool {
    let message = e.to_string();
    message.contains("unknown field") || message.contains("unknown variant")
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Reason> {
    serde_json::from_slice(bytes).map_err(|e| if newer(&e) { Reason::NewerFormat } else { Reason::Invalid(e.to_string()) })
}

fn read_header(bytes: &[u8]) -> Result<Header, Reason> {
    let header: Header = decode(bytes)?;
    if header.format != FORMAT {
        return Err(if header.format.starts_with(FORMAT_FAMILY) {
            Reason::NewerFormat
        } else {
            Reason::Invalid("not a library folder manifest".into())
        });
    }
    if !identifier(&header.id) || !text(&header.name, 128) {
        return Err(Reason::Invalid("a library needs an id (lowercase letters, digits, . _ -) and a 1-128 byte name".into()));
    }
    // A folder can't stand in for SolidCraft's own libraries, wherever it sits in the list.
    if header.id.starts_with("solidcraft.") {
        return Err(Reason::Invalid("library ids starting with `solidcraft.` are kept for SolidCraft's own libraries".into()));
    }
    header.provenance.validate().map_err(|e| Reason::Invalid(e.to_string()))?;
    Ok(header)
}

fn read_entry(bytes: &[u8], kind: Kind, library_id: &str) -> Result<Entry, Reason> {
    let entry: Entry = decode(bytes)?;
    if Kind::of(&entry.definition) != kind {
        return Err(Reason::WrongKind);
    }
    if entry.id.split_once('/').map(|(library, _)| library) != Some(library_id) {
        return Err(Reason::ForeignId);
    }
    entry.validate(library_id).map_err(|e| Reason::Invalid(e.to_string()))?;
    Ok(entry)
}

fn read_library(dir: &str, listing: Listing, limits: &Limits, seen: &mut Seen) -> Library {
    let unavailable = |reason| Library::unavailable(dir, reason);
    let manifest = match listing.manifests.as_slice() {
        [] => return unavailable(Reason::NoManifest),
        [one] => one,
        _ => return unavailable(Reason::TwoManifests),
    };
    // The library's size is what was read: the manifest and every entry file within its own cap.
    let mut used = 0_u64;
    let header = match read_capped(manifest, Cap::ManifestBytes, limits.manifest_bytes).and_then(|bytes| {
        used = bytes.len() as u64;
        read_header(&bytes)
    }) {
        Ok(header) => header,
        Err(reason) => return unavailable(reason),
    };
    let mut entries = Vec::new();
    for kind in [Kind::Material, Kind::Appearance] {
        // Only a real folder: a link named `materials` is not one, so a library can't reach outside.
        let Some(sub) = listing.folders.iter().find(|f| file_label(f) == kind.folder()) else { continue };
        let files = match seen.files(sub, Cap::EntriesPerKind(kind), limits.entries_per_kind) {
            Ok(files) => files,
            Err(reason) => return unavailable(reason),
        };
        for path in files.iter().filter(|p| ends_with(p, ".json")) {
            let state = read_capped(path, Cap::EntryBytes, limits.entry_bytes).and_then(|bytes| {
                used = used.saturating_add(bytes.len() as u64);
                read_entry(&bytes, kind, &header.id)
            });
            if used > limits.library_bytes {
                return unavailable(Reason::OverLimit { cap: Cap::LibraryBytes, limit: limits.library_bytes });
            }
            entries.push(Listed { label: file_label(path), kind, state });
        }
    }
    refuse_duplicate_entries(&mut entries);
    Library { folder: Some(dir.to_string()), header: Some(header), status: Status::Available, read_only: None, entries, bytes: used }
}

fn read_capped(path: &str, cap: Cap, limit: u64) -> Result<Vec<u8>, Reason> {
    let reason = |e: std::io::Error| match e.kind() {
        ErrorKind::InvalidData => Reason::OverLimit { cap, limit },
        ErrorKind::NotFound => Reason::Missing,
        _ => Reason::Unreadable(e.to_string()),
    };
    // The size first, as `Catalog::read` does: in the browser a read copies the stored bytes before it checks them.
    if checked::len(path).map_err(reason)? > limit {
        return Err(Reason::OverLimit { cap, limit });
    }
    checked::read(path, usize::try_from(limit).unwrap_or(usize::MAX)).map_err(reason)
}

/// Two files with one entry id: neither is used, and each names the others by kind folder and file name
/// (`materials/Common.json` and `appearances/Common.json` share a file name).
fn refuse_duplicate_entries(entries: &mut [Listed]) {
    let mut seen: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, listed) in entries.iter().enumerate() {
        if let Ok(entry) = &listed.state {
            seen.entry(entry.id.clone()).or_default().push(i);
        }
    }
    for indices in seen.values().filter(|v| v.len() > 1) {
        let names: Vec<(usize, String)> = indices.iter().filter_map(|&i| entries.get(i).map(|l| (i, l.file()))).collect();
        for &(i, _) in &names {
            let others = names.iter().filter(|(j, _)| *j != i).map(|(_, name)| name.as_str()).collect::<Vec<_>>().join(", ");
            if let Some(listed) = entries.get_mut(i) {
                listed.state = Err(Reason::DuplicateId { other: others });
            }
        }
    }
}

/// Every place's libraries, in the places' order. A library whose id an earlier one already has
/// is never used in its place: the set marks it unavailable, naming the first. The set keeps that
/// verdict apart from what the scan found, so the same scans can be merged again in another order.
/// The scans are shared, so merging again after one place's rescan copies pointers, not libraries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Set {
    places: Vec<Arc<Scanned>>,
    /// One status per library, in place order: the scan's, or the set's verdict on top of it.
    statuses: Vec<Status>,
}
impl Set {
    pub fn new(places: Vec<Scanned>) -> Self {
        Self::shared(places.into_iter().map(Arc::new).collect())
    }
    /// [`Set::new`] over scans other sets may hold too.
    pub fn shared(places: Vec<Arc<Scanned>>) -> Self {
        let mut first: BTreeMap<&str, &str> = BTreeMap::new();
        let mut statuses = Vec::new();
        for library in places.iter().flat_map(|p| &p.libraries) {
            let status = match (library.status(), library.id()) {
                (Status::Available, Some(id)) => match first.get(id) {
                    Some(at) => Status::Unavailable(Reason::DuplicateId { other: at.to_string() }),
                    None => {
                        first.insert(id, library.shown_at());
                        Status::Available
                    }
                },
                (status, _) => status.clone(),
            };
            statuses.push(status);
        }
        Self { places, statuses }
    }
    pub fn places(&self) -> &[Arc<Scanned>] {
        &self.places
    }
    /// Every library, in order, with its status in this set.
    pub fn libraries(&self) -> impl Iterator<Item = (&Library, &Status)> {
        self.places.iter().flat_map(|p| &p.libraries).zip(&self.statuses)
    }
    /// Every listed entry of every library this set uses, refused ones included, with its library.
    pub fn entries(&self) -> impl Iterator<Item = (&Library, &Listed)> {
        self.libraries().filter(|(_, s)| **s == Status::Available).flat_map(|(l, _)| l.entries.iter().map(move |e| (l, e)))
    }
    /// A usable entry by its id (`<library id>/<entry id>`).
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries().find_map(|(_, listed)| listed.state.as_ref().ok().filter(|e| e.id == id))
    }
}

/// What the libraries of a session come from: the built-in library, My Library and the user's
/// places in their order, how far each place's scan has got, and the set merged from the scans.
/// Only real scans enter the set: a place still scanning is listed, not made up.
#[derive(Clone, Debug, PartialEq)]
pub struct Libraries {
    my_library: Option<PlaceRow>,
    rows: Vec<PlaceRow>,
    /// One per place, My Library first.
    states: Vec<RowState>,
    set: Set,
}

/// How far a place's scan has got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    /// Its scan is in the set.
    Scanned,
    Scanning,
    /// No scan could start, and why.
    NotScanned(String),
}

impl Libraries {
    /// The libraries of `my_library` and `rows`, from the scans found so far: `scans` holds each
    /// place's latest scan and `failed` each place whose scan couldn't start. The built-in scan
    /// is always first.
    pub fn new(
        built_in: Arc<Scanned>,
        my_library: Option<PlaceRow>,
        rows: Vec<PlaceRow>,
        scans: &[Arc<Scanned>],
        failed: &[(Place, String)],
    ) -> Self {
        let mut found = vec![built_in];
        let mut states = Vec::new();
        for row in my_library.iter().chain(&rows) {
            if let Some(scan) = scans.iter().find(|s| s.place == row.place) {
                found.push(Arc::clone(scan));
                states.push(RowState::Scanned);
            } else if let Some((_, why)) = failed.iter().find(|(place, _)| *place == row.place) {
                states.push(RowState::NotScanned(why.clone()));
            } else {
                states.push(RowState::Scanning);
            }
        }
        Self { my_library, rows, states, set: Set::shared(found) }
    }
    pub fn my_library(&self) -> Option<&PlaceRow> {
        self.my_library.as_ref()
    }
    /// The user's places, in order, My Library apart.
    pub fn rows(&self) -> &[PlaceRow] {
        &self.rows
    }
    /// Every place but the built-in library, My Library first, with how far its scan has got.
    pub fn places(&self) -> impl Iterator<Item = (&PlaceRow, &RowState)> {
        self.my_library.iter().chain(&self.rows).zip(&self.states)
    }
    pub fn set(&self) -> &Set {
        &self.set
    }
    /// The place listed at `path`, My Library included, however the folder is spelled.
    pub fn listed(&self, path: &str) -> Option<&PlaceRow> {
        self.my_library.iter().chain(&self.rows).find(|r| r.place.path().is_some_and(|p| place_key(p) == place_key(path)))
    }
    /// The user's places after `edit`, or why not. My Library can't be added again, and the list
    /// holds at most [`MAX_PLACES`]. The engine checks a change with this and the app applies it
    /// with this, so the two never disagree.
    pub fn edit(&self, edit: &PlaceEdit) -> Result<Vec<PlaceRow>, String> {
        let mut rows = self.rows.clone();
        let at_path = |rows: &[PlaceRow], path: &str| rows.iter().position(|r| r.place.path().is_some_and(|p| place_key(p) == place_key(path)));
        let fits = |at: Option<usize>, len: usize| match at {
            Some(at) if at > len => Err(format!("`at` is {at}, past the end of the {len} places")),
            _ => Ok(at.unwrap_or(len)),
        };
        match edit {
            PlaceEdit::Add { row, at } => {
                let Some(path) = row.place.path() else { return Err("the built-in library is always listed".into()) };
                if self.listed(path).is_some() {
                    return Err(format!("{path} is listed already"));
                }
                if rows.len() >= MAX_PLACES {
                    return Err(format!("a list holds at most {MAX_PLACES} places"));
                }
                let at = fits(*at, rows.len())?;
                rows.insert(at, row.clone());
            }
            PlaceEdit::Remove { path } => {
                let i = at_path(&rows, path).ok_or_else(|| format!("no place is listed at {path}"))?;
                rows.remove(i);
            }
            PlaceEdit::Set { path, read_only, at } => {
                let i = at_path(&rows, path).ok_or_else(|| format!("no place is listed at {path}"))?;
                let mut row = rows.remove(i);
                if let Some(read_only) = read_only {
                    row.read_only = *read_only;
                }
                let at = fits(at.or(Some(i)), rows.len())?;
                rows.insert(at, row);
            }
        }
        Ok(rows)
    }
}

/// The built-in library: today's materials, appearances, finishes and starter presets, under ids
/// made once from their names (the starter presets keep their own ids). A test pins every id.
/// Each entry's provenance names the table it comes from, as ATTRIBUTION.md lists it.
fn built_in() -> Library {
    use solidcraft_doc::{MATERIALS, appearance, appearance_pack::STARTER_PACK};
    const LICENSE: &str = "MIT OR Apache-2.0";
    const AUTHOR: &str = "SolidCraft contributors";
    // A path in SolidCraft's source, as the starter pack's own provenance gives it.
    let from = |source: &str| Provenance { license: LICENSE.into(), author: Some(AUTHOR.into()), source_url: Some(source.into()) };
    let listed = |label: &str, kind: Kind, local: String, definition: crate::Result<Definition>, provenance: Provenance| Listed {
        label: label.to_string(),
        kind,
        state: definition.and_then(|d| Entry::with_id(BUILT_IN_ID, &local, d, provenance)).map_err(|e| Reason::Invalid(e.to_string())),
    };
    let mut entries: Vec<Listed> = MATERIALS
        .iter()
        .map(|&(name, density, _)| {
            let definition = appearance::material_look(name).as_ref().map(LibraryLook::from_look).transpose().map(|appearance| {
                Definition::Material(LibraryMaterial { name: name.to_string(), density_g_cm3: density, appearance, physical_card: None })
            });
            listed(name, Kind::Material, format!("material-{}", slug(name)), definition, from("assets/materials/library.rs"))
        })
        .collect();
    let looks = appearance::LIBRARY
        .iter()
        .map(|(name, ..)| (*name, "assets/appearances/library.rs"))
        .chain(appearance::OPAQUE_FINISHES.iter().map(|(name, ..)| (*name, "assets/appearances/opaque_finishes.rs")));
    entries.extend(looks.map(|(name, source)| {
        let definition = appearance::library(name)
            .ok_or_else(|| crate::IoError::Invalid(format!("no built-in appearance {name}")))
            .and_then(|look| LibraryLook::from_look(&look))
            .map(Definition::Appearance);
        listed(name, Kind::Appearance, format!("appearance-{}", slug(name)), definition, from(source))
    }));
    entries.extend(STARTER_PACK.iter().map(|preset| match preset.opaque_look() {
        Ok(look) => {
            let p = preset.provenance;
            let provenance = Provenance { license: p.license.into(), author: Some(p.author.into()), source_url: Some(p.source.into()) };
            listed(preset.name, Kind::Appearance, preset.id.to_string(), LibraryLook::from_look(&look).map(Definition::Appearance), provenance)
        }
        Err(e) => Listed { label: preset.name.to_string(), kind: Kind::Appearance, state: Err(Reason::Unsupported(e.to_string())) },
    }));
    refuse_duplicate_entries(&mut entries);
    let provenance = Provenance { license: LICENSE.into(), author: Some(AUTHOR.into()), source_url: None };
    Library {
        folder: None,
        header: Some(Header { format: FORMAT.into(), id: BUILT_IN_ID.into(), name: "SolidCraft".into(), provenance }),
        status: Status::Available,
        read_only: Some(Reason::BuiltIn),
        entries,
        bytes: 0,
    }
}

/// An id part from a name: lowercase letters and digits, anything else one `-`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[path = "library_write.rs"]
pub mod write;

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
