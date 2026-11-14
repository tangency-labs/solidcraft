//! Designs shared as a `.zip` (#4019). Issue trackers such as GitHub accept `.zip` attachments
//! but not `.sc`. A design container is itself a ZIP, so it is shared as it is, under a
//! `.sc.zip` name. A zip an archiver made of a design file opens too: its one design is
//! read in memory, and nothing is extracted to a path.

use std::io::{Read, Seek, SeekFrom};

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

/// The design file name inside the zip at `zip_path`: `bracket.sc.zip` and
/// `bracket.zip` hold `bracket.sc`.
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

/// Read only the ZIP trailer before the archive parser allocates its entry index.
/// Keep this seekable so opening a file does not buffer the whole archive.
fn declared_entries<R: Read + Seek>(bytes: &mut R) -> Result<u64> {
    let length = bytes.seek(SeekFrom::End(0)).map_err(invalid)?;
    let tail_length = length.min(65_535 + 22) as usize;
    let tail_start = length - tail_length as u64;
    bytes.seek(SeekFrom::Start(tail_start)).map_err(invalid)?;
    let mut tail = vec![0; tail_length];
    bytes.read_exact(&mut tail).map_err(invalid)?;
    let eocd = tail.windows(4).rposition(|v| v == b"PK\x05\x06").ok_or_else(|| invalid("missing ZIP directory"))?;
    let end = tail.get(eocd..).filter(|v| v.len() >= 22).ok_or_else(|| invalid("truncated ZIP directory"))?;
    let read_u16 = |offset| -> Result<u16> {
        let value = end.get(offset..offset + 2).ok_or_else(|| invalid("truncated ZIP directory"))?;
        Ok(u16::from_le_bytes(value.try_into().map_err(invalid)?))
    };
    if end.len() != 22 + usize::from(read_u16(20)?) {
        return Err(invalid("invalid ZIP comment length"));
    }
    let count = read_u16(10)?;
    if count != u16::MAX {
        return Ok(u64::from(count));
    }
    let locator_offset = (tail_start + eocd as u64).checked_sub(20).ok_or_else(|| invalid("missing ZIP64 locator"))?;
    bytes.seek(SeekFrom::Start(locator_offset)).map_err(invalid)?;
    let mut locator = [0; 20];
    bytes.read_exact(&mut locator).map_err(invalid)?;
    if locator.get(..4) != Some(b"PK\x06\x07") {
        return Err(invalid("missing ZIP64 locator"));
    }
    let offset = locator.get(8..16).ok_or_else(|| invalid("truncated ZIP64 locator"))?;
    let offset = u64::from_le_bytes(offset.try_into().map_err(invalid)?);
    // A ZIP64 end record has at least 56 bytes and must precede its locator.
    if offset.checked_add(56).is_none_or(|end| end > locator_offset) {
        return Err(invalid("invalid ZIP64 directory offset"));
    }
    bytes.seek(SeekFrom::Start(offset)).map_err(invalid)?;
    let mut record = [0; 56];
    bytes.read_exact(&mut record).map_err(invalid)?;
    if record.get(..4) != Some(b"PK\x06\x06") {
        return Err(invalid("missing ZIP64 directory"));
    }
    let count = record.get(32..40).ok_or_else(|| invalid("truncated ZIP64 directory"))?;
    Ok(u64::from_le_bytes(count.try_into().map_err(invalid)?))
}

/// The design inside a shared zip. `Ok(None)`: `source` is a design container itself or an
/// unsupported zip, to be read as a design. A malformed trailer, too many entries, no design,
/// more than one design, or an unsafe entry name is refused.
pub(super) fn inner<R: Read + Seek>(mut source: R) -> Result<Option<Vec<u8>>> {
    let count = declared_entries(&mut source)?;
    if count > MAX_SHARED_ENTRIES as u64 {
        return Err(invalid(format!("the zip holds {count} files; a shared design holds one design")));
    }
    source.seek(SeekFrom::Start(0)).map_err(invalid)?;
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

    fn declared_zip(count: u64, zip64: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if zip64 {
            let mut record = [0; 56];
            record[..4].copy_from_slice(b"PK\x06\x06");
            record[4..12].copy_from_slice(&44u64.to_le_bytes());
            record[32..40].copy_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&record);
            let mut locator = [0; 20];
            locator[..4].copy_from_slice(b"PK\x06\x07");
            locator[16..20].copy_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&locator);
        }
        let mut end = [0; 22];
        end[..4].copy_from_slice(b"PK\x05\x06");
        end[10..12].copy_from_slice(&(if zip64 { u16::MAX } else { count as u16 }).to_le_bytes());
        bytes.extend_from_slice(&end);
        bytes
    }

    #[test]
    fn excessive_declared_entries_are_refused_before_parsing_the_directory() {
        // No central directory exists: only the preflight can report the entry limit.
        for zip64 in [false, true] {
            let bytes = declared_zip(MAX_SHARED_ENTRIES as u64 + 1, zip64);
            let error = inner(Cursor::new(bytes)).unwrap_err().to_string();
            assert!(error.contains("the zip holds 65 files; a shared design holds one design"), "{error}");
        }
    }

    #[test]
    fn zip64_declared_entries_use_the_full_count_and_locator_offset() {
        let count = u64::from(u32::MAX) + 1;
        let mut bytes = declared_zip(count, true);
        assert_eq!(declared_entries(&mut Cursor::new(&bytes)).unwrap(), count);
        // A locator points to an absolute archive offset, even outside the trailer window.
        bytes.splice(..0, vec![0; 70_000]);
        bytes[70_064..70_072].copy_from_slice(&70_000u64.to_le_bytes());
        let comment_length = bytes.len() - 2;
        bytes[comment_length..].copy_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend(vec![b'x'; usize::from(u16::MAX)]);
        assert_eq!(declared_entries(&mut Cursor::new(&bytes)).unwrap(), count);
        let mut comment = zip_of(&[("a.json", b"{}")]);
        let end = comment.len() - 22;
        comment[end + 20..end + 22].copy_from_slice(&u16::MAX.to_le_bytes());
        comment.extend(vec![b'x'; usize::from(u16::MAX)]);
        assert_eq!(declared_entries(&mut Cursor::new(comment)).unwrap(), 1);
    }

    #[test]
    fn malformed_zip_trailers_return_errors() {
        for zip64 in [false, true] {
            let bytes = declared_zip(1, zip64);
            for length in 0..bytes.len() {
                assert!(declared_entries(&mut Cursor::new(&bytes[..length])).is_err(), "zip64={zip64}, length={length}");
            }
        }
        assert!(declared_entries(&mut Cursor::new(b"garbage tail")).is_err());
        let mut bytes = declared_zip(1, true);
        bytes[64..72].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(declared_entries(&mut Cursor::new(bytes)).is_err());
        let mut bytes = declared_zip(1, true);
        bytes[0] = 0;
        assert!(declared_entries(&mut Cursor::new(bytes)).is_err());
        let mut bytes = declared_zip(1, false);
        bytes[20..22].copy_from_slice(&1u16.to_le_bytes());
        assert!(inner(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn a_zip_an_archiver_made_of_a_design_opens_as_that_design() {
        let doc = solidcraft_doc::Document::new("Bracket");
        let design = crate::write_design_container(&doc).unwrap();
        let current = format!("parts/Bracket.{}", crate::DESIGN_EXT);
        assert_eq!(crate::read_design(&zip_of(&[(&current, &design)])).unwrap().to_json(), doc.to_json());
        assert_eq!(shared_design_name(&format!("/tmp/Bracket.{}.ZIP", crate::DESIGN_EXT)), format!("Bracket.{}", crate::DESIGN_EXT));
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
    fn native_containers_keep_their_larger_entry_budget() {
        let doc = solidcraft_doc::Document::new("Many parts");
        let design = crate::write_design_container(&doc).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(design)).unwrap();
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let name = entry.name().unwrap().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            entries.push((name, bytes));
        }
        for index in 0..MAX_SHARED_ENTRIES {
            entries.push((format!("extra{index}.txt"), Vec::new()));
        }
        let entries: Vec<_> = entries.iter().map(|(name, bytes)| (name.as_str(), bytes.as_slice())).collect();
        let bytes = zip_of(&entries);
        assert_eq!(crate::read_design(&bytes).unwrap().to_json(), doc.to_json());
        let (restored, _) = super::super::read_source(Cursor::new(bytes), None::<fn(crate::DesignPreview)>, true).unwrap();
        assert_eq!(restored.to_json(), doc.to_json());
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
