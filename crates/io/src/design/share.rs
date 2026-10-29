//! Designs shared as a `.zip` (#4019). Issue trackers such as GitHub accept `.zip` attachments
//! but not `.solidcraft`. A design container is itself a ZIP, so it is shared as it is, under a
//! `.solidcraft.zip` name. A zip an archiver made of a design file opens too: its one design is
//! read in memory, and nothing is extracted to a path.

use std::io::{Read, Seek};

use crate::{IoError, MAX_DESIGN_BYTES, Result};

/// The extension of a shared design.
pub const SHARED_DESIGN_EXT: &str = "zip";
/// A shared zip is a design and perhaps a few files beside it, never an archive of a project.
const MAX_SHARED_ENTRIES: usize = 64;

fn invalid(message: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("shared design: {message}"))
}

/// Is this a shared design's file name (`.zip`, any case)?
pub fn is_shared_design_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(SHARED_DESIGN_EXT))
}

/// The design file name inside the zip at `zip_path`: `bracket.solidcraft.zip` and
/// `bracket.zip` hold `bracket.solidcraft`.
pub fn shared_design_name(zip_path: &str) -> String {
    let file = std::path::Path::new(zip_path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = if is_shared_design_path(&file) { file.get(..file.len().saturating_sub(4)).unwrap_or_default() } else { file.as_str() };
    let stem = crate::DESIGN_EXTS
        .iter()
        .find_map(|ext| {
            let cut = stem.len().checked_sub(ext.len() + 1)?;
            stem.get(cut..).filter(|tail| tail.eq_ignore_ascii_case(&format!(".{ext}"))).and(stem.get(..cut))
        })
        .unwrap_or(stem);
    let clean: String = stem.chars().filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':')).collect();
    let clean = clean.trim().trim_matches('.');
    format!("{}.{}", if clean.is_empty() { "design" } else { clean }, crate::DESIGN_EXT)
}

/// Entries an archiver adds next to the file it was given: folders and macOS resource forks.
fn beside(name: &str) -> bool {
    name.ends_with('/') || name.starts_with("__MACOSX/") || name.rsplit('/').next().is_some_and(|base| base.starts_with("._") || base == ".DS_Store")
}

/// The design inside a shared zip. `Ok(None)`: `source` is no shared zip (a design container
/// itself, or not a readable zip), to be read as a design. A zip with no design, more than one,
/// or an unsafe entry name is refused.
pub(super) fn inner<R: Read + Seek>(source: R) -> Result<Option<Vec<u8>>> {
    let config = zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(0) };
    let Ok(mut zip) = zip::ZipArchive::with_config(config, source) else { return Ok(None) };
    if zip.index_for_name("container.json").is_some() {
        return Ok(None);
    }
    if zip.len() > MAX_SHARED_ENTRIES {
        return Err(invalid(format!("the zip holds {} files; a shared design holds one design", zip.len())));
    }
    let mut designs = Vec::new();
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(invalid)?;
        let name = entry.name().map_err(invalid)?.into_owned();
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.contains(':')
            || name.trim_end_matches('/').split('/').any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(invalid(format!("unsafe entry name {name:?}")));
        }
        if !beside(&name) && crate::is_design_path(&name) {
            designs.push((index, entry.size()));
        }
    }
    let [(index, size)] = designs[..] else {
        return Err(invalid(format!(
            "a .zip opens as a design when it holds exactly one .{} design; this one holds {}",
            crate::DESIGN_EXT,
            designs.len()
        )));
    };
    if size > MAX_DESIGN_BYTES as u64 {
        return Err(invalid("the design in the zip is too large"));
    }
    let entry = zip.by_index(index).map_err(invalid)?;
    let mut bytes = Vec::with_capacity(size as usize);
    // Stop one byte past the declared size: a compressed entry may expand beyond it.
    entry.take(size + 1).read_to_end(&mut bytes).map_err(invalid)?;
    if bytes.len() as u64 != size {
        return Err(invalid("the design's size does not match the zip"));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::io::{Cursor, Write};

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_zip_an_archiver_made_of_a_design_opens_as_that_design() {
        let doc = solidcraft_doc::Document::new("Bracket");
        let design = crate::write_design_container(&doc).unwrap();
        assert_eq!(shared_design_name("/tmp/Bracket.solidcraft.ZIP"), format!("Bracket.{}", crate::DESIGN_EXT));
        assert_eq!(shared_design_name("a/b:c.zip"), format!("bc.{}", crate::DESIGN_EXT));
        // With the folder and the macOS resource forks an archiver adds; a JSON design too.
        let json = crate::write_design(&doc);
        let made = zip_of(&[("__MACOSX/", b""), ("__MACOSX/._Bracket.solidcraft", b"junk"), ("parts/Bracket.solidcraft", &design)]);
        let made_json = zip_of(&[("Bracket.json", &json)]);
        assert_eq!(crate::read_design(&made).unwrap().to_json(), doc.to_json());
        assert_eq!(crate::read_design(&made_json).unwrap().to_json(), doc.to_json());
        let dir = std::env::temp_dir().join(format!("solidcraft-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("made.zip");
        std::fs::write(&path, &made).unwrap();
        let (read, _) = crate::read_design_file(&path.to_string_lossy(), None::<fn(crate::DesignPreview)>).unwrap();
        assert_eq!(read.to_json(), doc.to_json());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zip_without_exactly_one_safe_design_is_refused() {
        let json = crate::write_design(&solidcraft_doc::Document::new("A"));
        for (zip, expected) in [
            (zip_of(&[("a.solidcraft", &json), ("b.json", &json)]), "holds 2"),
            (zip_of(&[("notes.txt", b"hello")]), "holds 0"),
            (zip_of(&[("../a.solidcraft", &json)]), "unsafe entry name"),
            (zip_of(&[("/etc/a.solidcraft", &json)]), "unsafe entry name"),
            (zip_of(&[("dir\\a.solidcraft", &json)]), "unsafe entry name"),
        ] {
            let error = crate::read_design(&zip).unwrap_err().to_string();
            assert!(error.contains(expected), "{expected}: {error}");
        }
        let many: Vec<(String, &[u8])> = (0..=MAX_SHARED_ENTRIES).map(|i| (format!("f{i}.txt"), &b""[..])).collect();
        let many: Vec<(&str, &[u8])> = many.iter().map(|(n, b)| (n.as_str(), *b)).collect();
        assert!(crate::read_design(&zip_of(&many)).unwrap_err().to_string().contains("holds 65 files"));
    }
}
