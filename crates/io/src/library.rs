//! Material and appearance libraries read from their places (#12982): the built-in "SolidCraft"
//! library, one library folder, or a folder of library folders. This module only reads.
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
}

/// One library as a scan found it. Only a scan makes one, so what it says is what was read.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    folder: Option<String>,
    header: Option<Header>,
    status: Status,
    read_only: Option<Reason>,
    entries: Vec<Listed>,
}
impl Library {
    fn unavailable(folder: &str, reason: Reason) -> Self {
        Self { folder: Some(folder.to_string()), header: None, status: Status::Unavailable(reason), read_only: None, entries: Vec::new() }
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
    let found = |libraries| Scanned { place: place.clone(), status: Status::Available, libraries };
    let failed = |reason| Scanned { place: place.clone(), status: Status::Unavailable(reason), libraries: Vec::new() };
    match place {
        Place::BuiltIn => found(vec![built_in()]),
        Place::Library(dir) => match list(dir, limits) {
            Err(reason) => failed(reason),
            Ok(listing) => found(vec![read_library(dir, listing, limits)]),
        },
        Place::Folder(dir) => match crate::vfs::dirs_in(dir, limits.libraries_per_folder) {
            Err(e) => failed(listing_reason(&e, Cap::LibrariesPerFolder, limits.libraries_per_folder)),
            Ok(dirs) => found(
                dirs.iter()
                    .filter_map(|d| match list(d, limits) {
                        // Listed, not skipped: a subfolder that can't be listed may be a library.
                        Err(reason) => Some(Library::unavailable(d, reason)),
                        // A subfolder with no manifest isn't a library; one with two is listed as broken.
                        Ok(listing) if listing.manifests.is_empty() => None,
                        Ok(listing) => Some(read_library(d, listing, limits)),
                    })
                    .collect(),
            ),
        },
    }
}

/// What is directly in a library folder: its manifest files and its subfolders, each listed once.
struct Listing {
    manifests: Vec<String>,
    folders: Vec<String>,
}

fn list(dir: &str, limits: &Limits) -> Result<Listing, Reason> {
    // Folders first: in the browser only the folder listing can tell a missing folder from an empty one.
    let (cap, limit) = (Cap::FilesBesideManifest, limits.files_beside_manifest);
    let folders = crate::vfs::dirs_in(dir, limit).map_err(|e| listing_reason(&e, cap, limit))?;
    let files = crate::vfs::files_in(dir, limit).map_err(|e| listing_reason(&e, cap, limit))?;
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

fn read_library(dir: &str, listing: Listing, limits: &Limits) -> Library {
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
        let files = match crate::vfs::files_in(sub, limits.entries_per_kind) {
            Ok(files) => files,
            Err(e) => return unavailable(listing_reason(&e, Cap::EntriesPerKind(kind), limits.entries_per_kind)),
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
    Library { folder: Some(dir.to_string()), header: Some(header), status: Status::Available, read_only: None, entries }
}

fn read_capped(path: &str, cap: Cap, limit: u64) -> Result<Vec<u8>, Reason> {
    let reason = |e: std::io::Error| match e.kind() {
        ErrorKind::InvalidData => Reason::OverLimit { cap, limit },
        ErrorKind::NotFound => Reason::Missing,
        _ => Reason::Unreadable(e.to_string()),
    };
    // The size first, as `Catalog::read` does: in the browser a read copies the stored bytes before it checks them.
    if crate::vfs::len(path).map_err(reason)? > limit {
        return Err(Reason::OverLimit { cap, limit });
    }
    crate::vfs::read(path, usize::try_from(limit).unwrap_or(usize::MAX)).map_err(reason)
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
        let names: Vec<(usize, String)> =
            indices.iter().filter_map(|&i| entries.get(i).map(|l| (i, format!("{}/{}", l.kind.folder(), l.label)))).collect();
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Set {
    places: Vec<Scanned>,
    /// One status per library, in place order: the scan's, or the set's verdict on top of it.
    statuses: Vec<Status>,
}
impl Set {
    pub fn new(places: Vec<Scanned>) -> Self {
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
    pub fn places(&self) -> &[Scanned] {
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
        .map(|&(name, density)| {
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

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
