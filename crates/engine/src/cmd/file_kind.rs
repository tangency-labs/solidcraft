//! The kind of design a file holds (experimental, #6892): single design `.sc`, part `.scprt` or
//! assembly `.scasm` ([`solidcraft_io::DesignKind`]). The kind is fixed in the file's header;
//! renaming the file never changes it. A part that holds sheet metal is marked so
//! ([`sheet_metal_mark`]): a mark the model gives it, not a kind.
//!
//! The session keeps the kind it opened ([`Session::design_kind`]) and every save writes it back,
//! whether the experiment is on or not, so nothing a file says is lost. Only with the experiment
//! on ([`set_experimental_file_kinds`], Preferences › General) does an extension mean anything:
//! the first save of a new design takes the kind its extension names, a save whose extension
//! names another kind is refused, and opening a file whose name and header disagree says so (the
//! header wins). Converting a design to another kind comes later.

use std::sync::atomic::{AtomicBool, Ordering};

use solidcraft_io::DesignKind;

use crate::params::bad;
use crate::{Result, Session};

/// Whether extensions pick and check a design's kind (off by default).
static EXPERIMENTAL: AtomicBool = AtomicBool::new(false);

/// Turn the experimental file kinds on or off for every session from now on. The app sets it
/// from Preferences › General.
pub fn set_experimental_file_kinds(on: bool) {
    EXPERIMENTAL.store(on, Ordering::Relaxed);
}

/// Whether the experimental file kinds are on: [`set_experimental_file_kinds`] turned them on, or
/// `SOLIDCRAFT_FILE_KINDS` is set (for command-line runs, which read no preferences).
pub fn experimental_file_kinds() -> bool {
    EXPERIMENTAL.load(Ordering::Relaxed) || ON_FOR_TESTS.load(Ordering::Relaxed) || solidcraft_geom::env::flag("FILE_KINDS")
}

/// On for the rest of the process, whatever [`set_experimental_file_kinds`] is told after.
static ON_FOR_TESTS: AtomicBool = AtomicBool::new(false);

/// For tests that need the experiment on while others in the same process apply preferences
/// that turn it off: on for the rest of the process. A single design saves and opens the same
/// either way, so the other tests are unaffected. Under nextest each test is its own process.
#[doc(hidden)]
pub fn file_kinds_on_for_tests() {
    ON_FOR_TESTS.store(true, Ordering::Relaxed);
}

/// The kind a save of `s` to `path` in `format` writes: the session's own, or with the experiment
/// on the one a new design's extension names. A save that would put a design of one kind in
/// another kind's file is refused, and so is a JSON save of anything but a single design, whose
/// kind has no header to live in.
pub(super) fn for_save(s: &Session, cmd: &str, path: &str, format: &str) -> Result<DesignKind> {
    checked(s, cmd, path, format, experimental_file_kinds())
}

/// [`for_save`] with the experiment `on` or off.
pub(super) fn checked(s: &Session, cmd: &str, path: &str, format: &str, on: bool) -> Result<DesignKind> {
    if !on {
        return Ok(s.design_kind);
    }
    let kind = s.design_kind;
    if format == "json" {
        return match kind {
            DesignKind::Single => Ok(kind),
            _ => Err(bad(cmd, format!("{path}: {} keeps its kind in the design file's header; save it as .{}", a(kind), kind.extension()))),
        };
    }
    let Some(named) = DesignKind::for_path(path) else { return Ok(kind) };
    if named == kind {
        return Ok(kind);
    }
    // A design never saved takes the kind its first file's extension names.
    if kind == DesignKind::Single && s.path.is_none() {
        return Ok(named);
    }
    Err(bad(
        cmd,
        format!(
            "{path}: this design is {}, and a .{} file holds {}. Save it as .{}, or create a new {} or use features to convert",
            a(kind),
            named.extension(),
            a(named),
            kind.extension(),
            named.name()
        ),
    ))
}

/// The kind the file at `path` holds, by its header, and with the experiment on a note when its
/// extension names another kind: the header wins.
pub(super) fn opened(path: &str) -> (DesignKind, Option<String>) {
    opened_with(path, experimental_file_kinds())
}

/// [`opened`] with the experiment `on` or off.
pub(super) fn opened_with(path: &str, on: bool) -> (DesignKind, Option<String>) {
    let kind = solidcraft_io::read_design_kind_file(path);
    // A sheet metal part of the experiment's earlier builds is a part now (step 16).
    if on && solidcraft_io::is_legacy_sheet_metal_file(path) {
        return (
            kind,
            Some(format!("{path} was saved as a sheet metal part by an earlier experimental build: it opens as a part, which may hold sheet metal")),
        );
    }
    let note = on.then(|| DesignKind::for_path(path)).flatten().filter(|named| *named != kind).map(|named| {
        format!(
            "{path} is named as {} (.{}) but holds {}: it opened as {}, since the kind saved in the file wins over its name",
            a(named),
            named.extension(),
            a(kind),
            a(kind)
        )
    });
    (kind, note)
}

/// The kind `file.new {kind}` asks for: none or `single` is a single design; another kind needs
/// the experiment on (the Parts and assemblies environment).
pub(super) fn for_new(kind: Option<&serde_json::Value>) -> Result<DesignKind> {
    for_new_with(kind, experimental_file_kinds())
}

/// [`for_new`] with the experiment `on` or off.
pub(super) fn for_new_with(kind: Option<&serde_json::Value>, on: bool) -> Result<DesignKind> {
    let asked = match kind.filter(|k| !k.is_null()) {
        None => return Ok(DesignKind::Single),
        Some(k) => k.as_str().unwrap_or_default(),
    };
    let kind = match asked {
        "single" => DesignKind::Single,
        // Sheet metal is a part that holds a sheet metal body, not a kind (step 16).
        "sheet_metal" => return Err(bad("file.new", "a sheet metal part is a part: make a `part`, then add a base flange or convert it")),
        other => DesignKind::from_content(other).ok_or_else(|| bad("file.new", format!("`kind` must be single, part or assembly, not {other:?}")))?,
    };
    if kind != DesignKind::Single && !on {
        return Err(bad("file.new", "Parts and assemblies is off: turn it on in Preferences › General › Environment to make a new part or assembly"));
    }
    Ok(kind)
}

/// The sheet metal mark of a part (#6892, step 16): `Some(true)` when its model holds a sheet metal
/// body (`ModelState::surface_sheets`, the list Flat Pattern and Export DXF use). Derived each time,
/// never saved; none for anything else, so replies stay as they were.
pub(super) fn sheet_metal_mark(s: &Session) -> Option<bool> {
    (s.design_kind == DesignKind::Part && !s.model.state().surface_sheets.is_empty()).then_some(true)
}

/// A kind's name for a reply: none for a single design, so replies stay as they were.
pub(super) fn reported(kind: DesignKind) -> Option<String> {
    (kind != DesignKind::Single).then(|| kind.name().to_string())
}

/// A kind's name with its article: "a part", "an assembly".
fn a(kind: DesignKind) -> String {
    let name = kind.name();
    let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{article} {name}")
}

/// The sheet metal commands that model: an assembly doesn't hold their features (a part does, step
/// 16). The ones that only read (export, measure, flat pattern, the rules) are allowed everywhere.
const SHEET_METAL_MODELLING: [&str; 9] = [
    "sheet.flange",
    "sheet.convert",
    "sheet.fold",
    "sheet.hem",
    "sheet.path_flange",
    "sheet.perforate",
    "sheet.unfold",
    "sheet.refold",
    "sheet.recognize",
];

/// The commands that make a new body from nothing: an assembly holds parts from their own files.
/// A feature that can also cut or add to what is there (an extrude, a hole) is not here: whether
/// an assembly may hold such features is still open (step 13).
const NEW_BODY_FROM_NOTHING: [&str; 10] = [
    "solid.box",
    "solid.cylinder",
    "solid.sphere",
    "solid.cone",
    "solid.torus",
    "solid.coil",
    "solid.pipe",
    "solid.bounding_solid",
    "solid.cell_grid",
    "solid.boundary_fill",
];

/// Why the command `id` doesn't fit a file of `kind`, under Parts and assemblies (the experiment
/// on); none when it fits. A single design is never restricted (step 13 has the rules and what
/// is still open).
pub fn refusal(kind: DesignKind, id: &str) -> Option<String> {
    refusal_with(kind, id, experimental_file_kinds())
}

/// [`refusal`] with the experiment `on` or off.
pub(crate) fn refusal_with(kind: DesignKind, id: &str, on: bool) -> Option<String> {
    if !on {
        return None;
    }
    match kind {
        DesignKind::Assembly if NEW_BODY_FROM_NOTHING.contains(&id) || SHEET_METAL_MODELLING.contains(&id) => {
            Some("an assembly holds parts from their own files and their joints: create a new part, then insert it".into())
        }
        _ => None,
    }
}

/// Whether what is modelled in `s` now goes into a part being made in it: a component of the
/// design that is active and not linked to a file. New Part makes one inside an assembly, and it
/// becomes a part of its own when it is stored (`component.make_internal`), so the assembly still
/// ends up holding parts from their own files.
pub(crate) fn in_new_part(s: &Session) -> bool {
    s.active_component != 0 && !s.doc.links.contains_key(&s.active_component)
}

/// The linked parts of `s` found neither at their link's path nor in a library folder, under
/// Parts and assemblies (#6892, slice 4b): what `doc.open` names so the app can ask where they
/// are. Their components keep their last bodies; `component.relink` points one at its file.
pub(super) fn missing_links(s: &Session) -> Vec<crate::schema::file::MissingLink> {
    if !experimental_file_kinds() {
        return Vec::new();
    }
    s.doc
        .links
        .iter()
        .filter_map(|(component, link)| {
            // A part stored inside the design is in its file, unpacked or not.
            if solidcraft_doc::internal::linked(&link.path).is_some_and(|member| s.doc.internal.contains_key(member)) {
                return None;
            }
            let file = super::super::design_link::resolve(s.path.as_deref(), &link.path);
            let problem = super::super::design_link::link_presence(&file, s.path.as_deref())?;
            let missing =
                problem.state == super::super::design_link::LinkState::Missing && super::super::design_link::found_in_library(&file).is_none();
            let name = s.doc.components.iter().find(|c| c.id == *component).map(|c| c.name.clone())?;
            let internal = solidcraft_doc::internal::linked(&link.path).is_some();
            missing.then(|| crate::schema::file::MissingLink { component: *component, name, path: file.display().to_string(), internal })
        })
        .collect()
}
