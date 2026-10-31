//! `file.pack` (#3890 K12): the saved design and the designs its links name, in one `.zip` to hand
//! on. The design goes at the top level with each link pointing at `designs/<name>`, and each
//! linked design under `designs/`, byte for byte. Packs go one level deep: a linked design's own
//! links are listed in the reply, not followed. Only design files are copied.
//!
//! A pack holds the files as saved, so a design with unsaved changes or no file is refused; the
//! design, its file and its links are left as they were. Every linked file is checked before any
//! is read, and the sizes are summed before any is read, within [`solidcraft_io::MAX_PACK_BYTES`].

use std::path::{Path, PathBuf};

use serde_json::Value;
use solidcraft_io::{MAX_PACK_DESIGNS, PACK_DESIGNS, PackWriter};

use super::CommandSpec;
use super::design_link::{absolute, resolve};
use crate::params::bad;
use crate::schema::file_pack::{DeeperLink, PackParams, PackResult, PackedDesign};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "file.pack";
const CONTRACT: TypedContract<PackParams> = TypedContract::new();
/// The longest path taken, in bytes, as `drawing.export`'s.
const MAX_PATH_BYTES: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Pack Design", crate::checked!("file.pack", pack))
    .any_part()
    .icon("export")
    .noundo()
    .doc_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<PackResult>)
    .params("path: .zip file → {path, bytes, design, designs: [{file, entry}], deeper: [{design, path}]}. The saved design at the top level, its links pointing at designs/<name>, and each design its links name under designs/, byte for byte, once each; a linked design's own links are listed in deeper, not followed. Refused when the design has no file or unsaved changes, or a linked file can't be read or isn't a design; at most 1 GiB in all.")];

fn pack(s: &mut Session, p: &Value) -> Result<Value> {
    let request: PackParams = crate::schema::read(CMD, p)?;
    let path = request.path;
    if path.len() > MAX_PATH_BYTES || !solidcraft_io::is_pack_path(&path) {
        return Err(bad(CMD, "path must be a .zip file"));
    }
    let io = |e: std::io::Error| EngineError::Other(e.to_string());
    solidcraft_io::vfs::check(&path).map_err(io)?;
    let own = s.path.clone().ok_or_else(|| bad(CMD, "the design has no file yet: save it first"))?;
    if s.is_dirty() {
        return Err(bad(CMD, "the design has unsaved changes: save it first"));
    }
    let files = linked_files(s, &own)?;
    // Every file checked from its text, then measured, before any is read.
    for (file, link) in &files {
        solidcraft_io::vfs::check_design_path(&file.to_string_lossy(), Some(&own)).map_err(|e| bad(CMD, format!("{link}: {e}")))?;
    }
    let mut total = 0u64;
    for (file, link) in &files {
        total = total.saturating_add(solidcraft_io::vfs::len(&file.to_string_lossy()).map_err(|e| bad(CMD, format!("{link}: {e}")))?);
    }
    solidcraft_io::fits_pack(total).map_err(|e| bad(CMD, e.to_string()))?;

    let mut pack = PackWriter::default();
    let (mut designs, mut deeper) = (Vec::new(), Vec::new());
    let mut doc = s.design_at(None).into_owned();
    for (file, link) in &files {
        let shown = file.to_string_lossy();
        let bytes = solidcraft_io::vfs::read_design_path(&shown, Some(&own), solidcraft_io::limits::max_design_bytes())
            .map_err(|e| bad(CMD, format!("{link}: {}", solidcraft_io::limits::explain(solidcraft_io::limits::Kind::Design, e))))?;
        // Only a design is copied: anything else is refused by its link alone, never quoted.
        let linked = solidcraft_io::read_design(&bytes).map_err(|_| bad(CMD, format!("{link}: not a design file")))?;
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let entry = pack.add(Some(PACK_DESIGNS), &name, &bytes).map_err(|e| bad(CMD, e.to_string()))?;
        for l in doc.links.values_mut().filter(|l| absolute(&resolve(Some(&own), &l.path)) == *file) {
            l.path.clone_from(&entry);
        }
        deeper.extend(linked.links.values().map(|l| DeeperLink { design: entry.clone(), path: l.path.clone() }));
        designs.push(PackedDesign { file: shown.into_owned(), entry });
    }
    let top = match super::file::default_design_format(&own) {
        "json" => solidcraft_io::write_design(&doc),
        _ => solidcraft_io::write_design_container(&doc).map_err(|e| bad(CMD, e.to_string()))?,
    };
    let name = Path::new(&own).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let design = pack.add(None, &name, &top).map_err(|e| bad(CMD, e.to_string()))?;
    let bytes = pack.finish().map_err(|e| bad(CMD, e.to_string()))?;
    // Checked again just before writing, as drawing.export does.
    solidcraft_io::vfs::check(&path).map_err(io)?;
    solidcraft_io::write_atomic(Path::new(&path), &bytes, false).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let reply = PackResult { path, bytes: bytes.len(), design, designs, deeper, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Each file the design's links name, once, in link order, with the link that names it first
/// (as the design keeps it, for messages). The design itself is never one of them.
fn linked_files(s: &Session, own: &str) -> Result<Vec<(PathBuf, String)>> {
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    for link in s.doc.links.values() {
        let file = absolute(&resolve(Some(own), &link.path));
        if file == absolute(Path::new(own)) {
            return Err(bad(CMD, format!("{}: a design can't be packed as its own link", link.path)));
        }
        if !files.iter().any(|(f, _)| *f == file) {
            files.push((file, link.path.clone()));
        }
    }
    if files.len() > MAX_PACK_DESIGNS {
        return Err(bad(CMD, format!("the design links {} files; a pack holds at most {MAX_PACK_DESIGNS}", files.len())));
    }
    Ok(files)
}

#[cfg(test)]
#[path = "file_pack_tests.rs"]
mod tests;
