//! The kind of design a file holds (experimental, #6892): single design `.sc`, part `.scprt`, sheet
//! metal part `.scsht`, assembly `.scasm` ([`solidcraft_io::DesignKind`]). The kind is fixed in the
//! file's header; renaming the file never changes it.
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
        other => DesignKind::from_content(other)
            .ok_or_else(|| bad("file.new", format!("`kind` must be single, part, sheet_metal or assembly, not {other:?}")))?,
    };
    if kind != DesignKind::Single && !on {
        return Err(bad(
            "file.new",
            "Parts and assemblies is off: turn it on in Preferences › General › Environment to make a new part, sheet metal part or assembly",
        ));
    }
    Ok(kind)
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
