//! A design's drawings as files of their own kind while the design is open (experimental,
//! #6892). A drawing lives inside its design's file (`drawings/<id>.json`, #3890), and stays
//! there. With the Parts and assemblies experiment on, each one is also written to the design's
//! work folder ([`super::design_internal::work_folder`]) as a drawing file, `<name>.scdwg`
//! ([`solidcraft_io::write_drawing_container`]), beside the design's internal components, so the
//! folder lists everything the design holds by name and kind.
//!
//! - **Opening** writes the ones that have no file there ([`unpack`]) and replaces nothing. A file
//!   that differs from the design's drawing is left as it is and named.
//! - **Saving** brings every file up to the drawing the design now holds ([`refresh`]), and takes
//!   away the file a renamed drawing had under its old name.
//! - **Closing** removes the files that hold what the design's file holds
//!   ([`super::design_internal::tidy_work_folder`]).
//!
//! - **Only the top container.** A single design (`.sc`) opened as it is. A part or assembly kind's
//!   file keeps its drawings inside it, and so does every internal component (a part or a
//!   sub-assembly stored inside another design): opening or saving its work file writes no
//!   drawing files, so there is one set, the top design's.
//! - **Each file is tagged** with the design it belongs to ([`solidcraft_io::InternalOf`]), so one
//!   opened by itself says what it is.
//!
//! The files are copies: the design's file is what a save writes and what an open reads. A file's
//! name is made here from the drawing's name ([`file_names`]), never used as a path as it stands.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use solidcraft_doc::Document;
use solidcraft_doc::drawing::Drawing;
use solidcraft_io::DesignKind;

use super::design_internal::{work_folder, write};

/// The longest stem a drawing's file name takes from the drawing's name, in bytes.
const MAX_STEM_BYTES: usize = 150;

/// What [`unpack`] or [`refresh`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Written {
    /// Files written.
    pub written: Vec<String>,
    /// Files that were there and differ from the design's drawing: left as they are ([`unpack`]).
    pub differ: Vec<String>,
    /// Old-named files of renamed drawings that were removed ([`refresh`]).
    pub removed: Vec<String>,
    /// Drawings that got no file, each with why.
    pub failed: Vec<String>,
}

/// A file name's stem made from a drawing's name: one name any system can write. Folder
/// separators, control characters and what Windows refuses become `_`; no leading dot, no
/// trailing dot or space, not a Windows device name, at most [`MAX_STEM_BYTES`] long.
fn stem(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    let mut cleaned = cleaned.trim_matches(|c: char| c == ' ' || c == '.').to_string();
    while cleaned.len() > MAX_STEM_BYTES {
        cleaned.pop();
    }
    let cleaned = cleaned.trim_end_matches([' ', '.']).to_string();
    let first = cleaned.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let device = matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (first.len() == 4 && (first.starts_with("COM") || first.starts_with("LPT")) && first.ends_with(|c: char| c.is_ascii_digit()));
    match (cleaned.is_empty(), device) {
        (true, _) => "Drawing".to_string(),
        (false, true) => format!("_{cleaned}"),
        (false, false) => cleaned,
    }
}

/// The file name of each of `drawings`, in their order: its name made writable, then `.scdwg`.
/// Two drawings whose names give the same file (told apart without regard to letter case, as
/// Windows and macOS tell files apart) are numbered: `Sheet.scdwg`, `Sheet (2).scdwg`.
pub(crate) fn file_names(drawings: &[Arc<Drawing>]) -> Vec<String> {
    names_for(drawings.iter().map(|drawing| drawing.name.as_str()))
}

/// [`file_names`] of drawings named `names`.
fn names_for<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut taken = BTreeSet::new();
    names
        .map(|drawing| {
            let stem = stem(drawing);
            let mut name = format!("{stem}.{}", solidcraft_io::DRAWING_EXT);
            let mut n = 1;
            while !taken.insert(name.to_lowercase()) {
                n += 1;
                name = format!("{stem} ({n}).{}", solidcraft_io::DRAWING_EXT);
            }
            name
        })
        .collect()
}

/// The key of a drawing's tag ([`solidcraft_io::InternalOf`]): its id, as the design's file names it.
pub(crate) fn key(drawing: &Drawing) -> String {
    solidcraft_doc::ids::encode(drawing.id)
}

/// The bytes of `drawing`'s file in the work folder of the design saved at `design`: a drawing
/// file whose header says which design it belongs to, so it tells what it is opened anywhere.
fn file_bytes(drawing: &Drawing, design: &str) -> Result<Vec<u8>, String> {
    let bytes = solidcraft_io::write_drawing_container(drawing).map_err(|e| e.to_string())?;
    let tag = solidcraft_io::InternalOf {
        design: Path::new(design).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        key: key(drawing),
    };
    solidcraft_io::with_internal_of(&bytes, Some(&tag)).map_err(|e| e.to_string())
}

/// Each of `drawings` with its file name and the bytes of its file beside `design`, or why it has none.
fn files(drawings: &[Arc<Drawing>], design: &str) -> Vec<(String, Result<Vec<u8>, String>)> {
    file_names(drawings).into_iter().zip(drawings).map(|(name, drawing)| (name, file_bytes(drawing, design))).collect()
}

/// Whether the drawings of the design of `kind` saved at `design` get files: the experiment is on,
/// this is not the browser build (which has no folders), and the design is the top container: a
/// single design (`.sc`) opened as it is. Never a part or an assembly kind's file, and never an
/// internal component's work file: those keep their drawings inside their own file and only
/// there, so a design's work folder holds one set of drawing files, however deep its
/// sub-assemblies go.
fn on(design: &str, kind: DesignKind) -> bool {
    !cfg!(target_arch = "wasm32") && super::experimental_file_kinds() && kind == DesignKind::Single && !in_a_work_folder(design)
}

/// Whether `design` is a file in some design's work folder (`.~<file>.parts`).
fn in_a_work_folder(design: &str) -> bool {
    Path::new(design)
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy())
        .is_some_and(|name| name.starts_with(".~") && name.ends_with(".parts"))
}

/// Write the drawing files of `doc`, the design saved at `design`, that are not there yet.
/// Nothing is replaced: a file that differs from the design's drawing is named in `differ`.
pub(crate) fn unpack(doc: &Document, design: &str, kind: DesignKind) -> Written {
    let mut done = Written::default();
    if !on(design, kind) || doc.drawings.is_empty() {
        return done;
    }
    let Some(folder) = work_folder(design) else { return done };
    for (name, bytes) in files(&doc.drawings, design) {
        let file = folder.join(&name);
        match (bytes, std::fs::read(&file)) {
            (Err(why), _) => done.failed.push(format!("{name}: {why}")),
            (Ok(bytes), Ok(existing)) if existing == bytes => {}
            (Ok(_), Ok(_)) => done.differ.push(name),
            (Ok(bytes), Err(e)) if e.kind() == std::io::ErrorKind::NotFound => match write(&file, &bytes) {
                Ok(()) => done.written.push(name),
                Err(why) => done.failed.push(format!("{name}: {why}")),
            },
            (Ok(_), Err(e)) => done.failed.push(format!("{name}: {e}")),
        }
    }
    done
}

/// The design `doc` was saved at `design`: every drawing file there now holds the drawing the
/// design holds, and a drawing file left under the old name of a drawing that was renamed is
/// removed. A drawing file of a drawing the design no longer has stays: it is the only copy.
pub(crate) fn refresh(doc: &Document, design: &str, kind: DesignKind) -> Written {
    let mut done = Written::default();
    if !on(design, kind) {
        return done;
    }
    let Some(folder) = work_folder(design) else { return done };
    let mut current = BTreeSet::new();
    for (name, bytes) in files(&doc.drawings, design) {
        current.insert(name.to_lowercase());
        let file = folder.join(&name);
        match bytes {
            Err(why) => done.failed.push(format!("{name}: {why}")),
            Ok(bytes) if std::fs::read(&file).is_ok_and(|existing| existing == bytes) => {}
            Ok(bytes) => match write(&file, &bytes) {
                Ok(()) => done.written.push(name),
                Err(why) => done.failed.push(format!("{name}: {why}")),
            },
        }
    }
    let ids: BTreeSet<u64> = doc.drawings.iter().map(|d| d.id).collect();
    for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if current.contains(&name.to_lowercase()) || !solidcraft_io::is_drawing_path(&name) {
            continue;
        }
        if renamed(&entry.path(), &ids) && std::fs::remove_file(entry.path()).is_ok() {
            done.removed.push(name);
        }
    }
    done
}

/// Whether `file` is a drawing file of one of the drawings `ids`: the file a drawing had before
/// it was renamed.
fn renamed(file: &Path, ids: &BTreeSet<u64>) -> bool {
    solidcraft_io::read_drawing_file(&file.to_string_lossy()).is_ok_and(|drawing| ids.contains(&drawing.id))
}

/// The drawing files that hold exactly what `container`, the file of the design saved at `design`,
/// holds, by lowercase name: what closing the design may remove.
pub(crate) fn held(container: &[u8], design: &str) -> Vec<(String, Vec<u8>)> {
    let drawings = solidcraft_io::design_drawings(container).unwrap_or_default();
    files(&drawings, design).into_iter().filter_map(|(name, bytes)| Some((name.to_lowercase(), bytes.ok()?))).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_drawings_file_name_is_its_name_made_writable_and_told_apart() {
        let names = names_for(["Bracket", "bracket", "A/B: sheet?", "", "..", "con", "Flat pattern.", "Bracket"].into_iter());
        assert_eq!(
            names,
            [
                "Bracket.scdwg",
                "bracket (2).scdwg",
                "A_B_ sheet_.scdwg",
                "Drawing.scdwg",
                "Drawing (2).scdwg",
                "_con.scdwg",
                "Flat pattern.scdwg",
                "Bracket (3).scdwg"
            ]
        );
        let long = names_for([&*"é".repeat(200)].into_iter());
        assert!(long[0].len() <= MAX_STEM_BYTES + 6 && long[0].ends_with(".scdwg"), "{}", long[0].len());
    }
}
