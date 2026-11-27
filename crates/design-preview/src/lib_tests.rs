use std::io::Write;
use std::path::PathBuf;

use zip::write::SimpleFileOptions;

use super::*;

/// A 240 x 150 PNG header and a few bytes after it: enough for the reader, which never decodes.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

const DOCUMENT: &[u8] = br#"{"format":"solidcraft/51","name":"Filter cap","units":"mm","features":[]}"#;

/// A design container in a file of its own, removed when the test ends.
struct Design(PathBuf);

impl Design {
    fn new(test: &str, parts: &[(&str, &[u8])]) -> Self {
        let path = std::env::temp_dir().join(format!("solidcraft-design-preview-{test}-{}.sc", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, bytes) in parts {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        Design(path)
    }
}

impl Drop for Design {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

const CONTAINER: &[u8] = br#"{"format":"solidcraft-container/1"}"#;

#[test]
fn a_saved_design_gives_its_picture_and_model() {
    let picture_bytes = png(240, 150);
    let mesh = [b"SCMESH01".as_slice(), &9_u64.to_le_bytes(), &3_u64.to_le_bytes()].concat();
    let coordinates: Vec<u8> = [0.0_f64, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let indices: Vec<u8> = [0_u32, 1, 2].iter().flat_map(|i| i.to_le_bytes()).collect();
    let mesh = [mesh, coordinates, indices].concat();
    let manifest = format!(r#"{{"format":"solidcraft-preview/1","build":"0.0.1","meshes":[{{"name":"Body1","offset":0,"length":{}}}]}}"#, mesh.len());
    let design = Design::new(
        "saved",
        &[
            ("container.json", CONTAINER),
            ("document.json", DOCUMENT),
            ("cache/thumbnail.png", &picture_bytes),
            ("cache/preview.json", manifest.as_bytes()),
            ("cache/preview.bin", &mesh),
        ],
    );
    let picture = picture(&design.0).unwrap();
    assert_eq!((picture.width, picture.height), (240, 150));
    assert_eq!(picture.png, picture_bytes);
    let model = model(&design.0).unwrap();
    assert_eq!(model.vertices.len(), 3);
    assert_eq!(model.vertices[0].position, [-1.0, -1.0, 0.0]);
}

#[test]
fn a_design_without_caches_has_neither() {
    let design = Design::new("no-caches", &[("container.json", CONTAINER), ("document.json", DOCUMENT)]);
    assert_eq!(picture(&design.0), None);
    assert_eq!(model(&design.0), None);
}

#[test]
fn the_pre_rename_container_is_read_too() {
    let picture_bytes = png(240, 150);
    let design = Design::new("solvecraft", &[("container.json", br#"{"format":"solvecraft-container/1"}"#), ("cache/thumbnail.png", &picture_bytes)]);
    assert!(picture(&design.0).is_some());
}

#[test]
fn other_files_and_containers_are_misses() {
    let picture_bytes = png(240, 150);
    let unknown =
        Design::new("unknown-container", &[("container.json", br#"{"format":"something-else/9"}"#), ("cache/thumbnail.png", &picture_bytes)]);
    assert_eq!(picture(&unknown.0), None);
    let no_container = Design::new("no-container", &[("cache/thumbnail.png", &picture_bytes)]);
    assert_eq!(picture(&no_container.0), None);
    // A plain JSON design (or any other file) is not a container.
    let path = std::env::temp_dir().join(format!("solidcraft-design-preview-json-{}.sc", std::process::id()));
    std::fs::write(&path, DOCUMENT).unwrap();
    assert_eq!(picture(&path), None);
    assert_eq!(model(&path), None);
    let _ = std::fs::remove_file(&path);
    assert_eq!(picture(Path::new("/nonexistent/solidcraft/design.sc")), None);
}

#[test]
fn hostile_pictures_are_misses() {
    assert_eq!(png_size(&png(240, 150)), Some((240, 150)));
    assert_eq!(png_size(&png(0, 150)), None);
    assert_eq!(png_size(&png(240, MAX_PICTURE_SIDE + 1)), None);
    assert_eq!(png_size(b"\x89PNG\r\n\x1a\n"), None);
    assert_eq!(png_size(b"GIF89a"), None);
    let not_png = Design::new("not-png", &[("container.json", CONTAINER), ("cache/thumbnail.png", b"<svg/>")]);
    assert_eq!(picture(&not_png.0), None);
    let huge = vec![0_u8; (MAX_PICTURE + 1) as usize];
    let oversized = Design::new("oversized", &[("container.json", CONTAINER), ("cache/thumbnail.png", &huge)]);
    assert_eq!(picture(&oversized.0), None);
}

#[test]
fn a_picture_fits_its_box_with_its_proportions() {
    let picture = Picture { png: Vec::new(), width: 240, height: 150 };
    assert_eq!(picture.fit(512.0, 512.0), Some((512.0, 320.0)));
    assert_eq!(picture.fit(64.0, 20.0), Some((32.0, 20.0)));
    assert_eq!(picture.fit(0.0, 512.0), None);
    assert_eq!(picture.fit(f64::NAN, 512.0), None);
    assert_eq!(picture.fit(-5.0, 512.0), None);
}

/// A container's bytes, with an archive comment when `comment` is not empty.
fn container_bytes(parts: &[(&str, &[u8])], comment: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        writer.start_file(*name, SimpleFileOptions::default()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    if !comment.is_empty() {
        writer.set_comment(comment).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Where the end-of-central-directory record starts.
pub(crate) fn record(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap()
}

fn fits(bytes: &[u8]) -> bool {
    directory_fits(&mut std::io::Cursor::new(bytes), bytes.len() as u64).is_some()
}

fn good() -> Vec<u8> {
    container_bytes(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.png", &png(240, 150))], "")
}

/// `bytes` with the record's little-endian field at `offset` (from the record's start) set.
fn with_field(bytes: &[u8], offset: usize, value: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let at = record(&out) + offset;
    out[at..at + value.len()].copy_from_slice(value);
    out
}

#[test]
fn written_containers_pass_the_directory_checks() {
    assert!(fits(&good()));
    let commented =
        container_bytes(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.png", &png(240, 150))], "a comment");
    assert!(fits(&commented));
}

#[test]
fn a_directory_claiming_too_many_entries_is_refused() {
    let bytes = good();
    assert!(!fits(&with_field(&with_field(&bytes, 8, &10_001_u16.to_le_bytes()), 10, &10_001_u16.to_le_bytes())));
    assert!(!fits(&with_field(&with_field(&bytes, 8, &0xFFFF_u16.to_le_bytes()), 10, &0xFFFF_u16.to_le_bytes())));
    assert!(!fits(&with_field(&bytes, 10, &4_u16.to_le_bytes())));
}

#[test]
fn a_directory_elsewhere_or_too_large_is_refused() {
    let bytes = good();
    assert!(!fits(&with_field(&bytes, 4, &1_u16.to_le_bytes())));
    assert!(!fits(&with_field(&bytes, 6, &1_u16.to_le_bytes())));
    assert!(!fits(&with_field(&bytes, 12, &((8_u32 << 20) + 1).to_le_bytes())));
    assert!(!fits(&with_field(&bytes, 16, &0xFFFF_FFFF_u32.to_le_bytes())));
    let own_position = record(&bytes) as u32;
    assert!(!fits(&with_field(&bytes, 16, &own_position.to_le_bytes())));
}

#[test]
fn a_zip64_locator_is_refused() {
    let bytes = good();
    let at = record(&bytes);
    let mut with_locator = bytes[..at].to_vec();
    with_locator.extend_from_slice(b"PK\x06\x07");
    with_locator.extend_from_slice(&[0; 16]);
    with_locator.extend_from_slice(&bytes[at..]);
    assert!(!fits(&with_locator));
}

#[test]
fn a_file_without_a_whole_record_is_refused() {
    assert!(!fits(&good()[..21]));
    assert!(!fits(&with_field(&good(), 20, &1_u16.to_le_bytes())));
}

#[test]
fn the_path_reader_shares_the_directory_checks() {
    let path = |what: &str| std::env::temp_dir().join(format!("solidcraft-design-preview-directory-{what}-{}.sc", std::process::id()));
    let many = with_field(&with_field(&good(), 8, &0xFFFF_u16.to_le_bytes()), 10, &0xFFFF_u16.to_le_bytes());
    // Archives the zip crate opens on its own, which only the directory checks refuse.
    let zip64 = as_zip64(&good());
    assert!(zip_crate_opens(&zip64));
    let large = large_directory(130);
    assert!(zip::ZipArchive::new(std::io::Cursor::new(large.clone())).is_ok());
    for (what, bytes) in [("many", many), ("zip64", zip64), ("large", large)] {
        let file = path(what);
        std::fs::write(&file, &bytes).unwrap();
        let (picture, model) = (picture(&file), model(&file));
        let _ = std::fs::remove_file(&file);
        assert_eq!(picture, None, "{what}");
        assert_eq!(model, None, "{what}");
    }
}

/// `bytes`, a container written without a comment, as a ZIP64 archive the `zip` crate opens: the
/// same entries and directory, then a ZIP64 end record, its locator, and an end record whose
/// counts, size and offset say "in the ZIP64 record".
pub(crate) fn as_zip64(bytes: &[u8]) -> Vec<u8> {
    let at = record(bytes);
    let u16_at = |o: usize| u64::from(u16::from_le_bytes([bytes[at + o], bytes[at + o + 1]]));
    let u32_at = |o: usize| u64::from(u32::from_le_bytes(bytes[at + o..at + o + 4].try_into().unwrap()));
    let (entries, size, offset) = (u16_at(10), u32_at(12), u32_at(16));
    let mut out = bytes[..at].to_vec();
    let zip64_record = out.len() as u64;
    out.extend_from_slice(b"PK\x06\x06");
    out.extend_from_slice(&44_u64.to_le_bytes());
    out.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    for field in [entries, entries, size, offset] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(b"PK\x06\x07\0\0\0\0");
    out.extend_from_slice(&zip64_record.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(b"PK\x05\x06\0\0\0\0\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\0\0");
    out
}

/// A plain end record: one disk, `entries` entries, a directory of `size` bytes at `offset`, and a
/// comment of `comment` bytes.
pub(crate) fn end_record(entries: u16, size: u32, offset: u32, comment: u16) -> Vec<u8> {
    let mut out = b"PK\x05\x06\0\0\0\0".to_vec();
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&comment.to_le_bytes());
    out
}

/// The zip crate opens `bytes` and finds the container's three entries in it.
fn zip_crate_opens(bytes: &[u8]) -> bool {
    zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).map(|a| a.len()).ok() == Some(3)
}

#[test]
fn a_zip64_archive_behind_a_record_the_zip_crate_skips_is_refused() {
    let zip64 = as_zip64(&good());
    assert!(zip_crate_opens(&zip64));
    assert!(open_archive(std::io::Cursor::new(zip64.clone())).is_none());
    // After the ZIP64 records, a plain record the zip crate skips: its directory would start
    // where the record does, so the crate goes back to the ZIP64 record before it.
    let mut skipped = zip64.clone();
    let position = skipped.len() as u32;
    skipped.extend_from_slice(&end_record(1, 0, position, 0));
    assert!(zip_crate_opens(&skipped));
    assert!(open_archive(std::io::Cursor::new(skipped)).is_none(), "a record the zip crate skips hides a ZIP64 archive");
}

#[test]
fn a_zip64_archive_behind_a_decoy_record_is_refused() {
    let zip64 = as_zip64(&good());
    // A plain record before the ZIP64 records whose comment runs to the end of the file, and a
    // byte after the last record: the zip crate takes the last record, this check the first.
    let at = record(&good());
    let directory_end = zip64.len() - 22 - 20 - 56;
    let mut decoy = zip64[..directory_end].to_vec();
    decoy.extend_from_slice(&end_record(0, 0, 0, 56 + 20 + 22 + 1));
    decoy.extend_from_slice(&zip64[directory_end..]);
    decoy.push(0);
    let locator = decoy.len() - 1 - 22 - 20;
    let moved = (directory_end + 22) as u64;
    decoy[locator + 8..locator + 16].copy_from_slice(&moved.to_le_bytes());
    assert_eq!(at, directory_end, "the directory ends where the plain container's record began");
    assert!(zip_crate_opens(&decoy));
    assert!(open_archive(std::io::Cursor::new(decoy)).is_none(), "a decoy record hides a ZIP64 archive");
}

/// A well-formed container with its three parts and `extra` empty entries named with 65,000 bytes
/// each: 129 or more make its directory larger than [`MAX_DIRECTORY`].
pub(crate) fn large_directory(extra: usize) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let picture = png(240, 150);
    for (name, bytes) in [("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.png", picture.as_slice())] {
        writer.start_file(name, SimpleFileOptions::default()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    let long = "x".repeat(65_000);
    for i in 0..extra {
        writer.start_file(format!("pad/{i:03}/{long}"), SimpleFileOptions::default()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn a_directory_larger_than_its_record_says_is_refused() {
    let bytes = large_directory(130);
    let at = record(&bytes);
    let size = u32::from_le_bytes(bytes[at + 12..at + 16].try_into().unwrap());
    assert!(u64::from(size) > MAX_DIRECTORY);
    assert!(!fits(&bytes), "an honest record is refused by its size");
    // The record says the directory is one header long; the zip crate reads all 133 entries anyway.
    let understated = with_field(&bytes, 12, &46_u32.to_le_bytes());
    assert_eq!(zip::ZipArchive::new(std::io::Cursor::new(understated.clone())).map(|a| a.len()).ok(), Some(133));
    assert!(open_archive(std::io::Cursor::new(understated)).is_none(), "the zip crate parsed more than 8 MiB of directory");
    // A directory just within the bound still opens, read in full.
    let largest = large_directory(128);
    let at = record(&largest);
    assert!(u64::from(u32::from_le_bytes(largest[at + 12..at + 16].try_into().unwrap())) > MAX_DIRECTORY - (64 << 10));
    assert_eq!(open_archive(std::io::Cursor::new(largest)).map(|a| a.len()), Some(131));
}

#[test]
fn the_directory_bounds_hold_exactly_at_their_values() {
    let bytes = good();
    let entries = |n: u16| with_field(&with_field(&bytes, 8, &n.to_le_bytes()), 10, &n.to_le_bytes());
    assert!(fits(&entries(10_000)), "10,000 entries is the most a record may claim");
    assert!(!fits(&entries(10_001)));
    // A file over 8 MiB, so that a directory of 8 MiB from its start lies before the record.
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("container.json", stored).unwrap();
    writer.write_all(CONTAINER).unwrap();
    writer.start_file("filler", stored).unwrap();
    writer.write_all(&vec![0_u8; 9 << 20]).unwrap();
    let big = writer.finish().unwrap().into_inner();
    let directory = |size: u32| with_field(&with_field(&big, 16, &0_u32.to_le_bytes()), 12, &size.to_le_bytes());
    assert!(fits(&directory(8 << 20)), "a directory of exactly 8 MiB");
    assert!(!fits(&directory((8 << 20) + 1)), "one byte more, still inside the file");
}

#[test]
fn a_second_record_counts_unless_it_claims_several_disks() {
    let parts: &[(&str, &[u8])] = &[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.png", &png(240, 150))];
    // A record inside the archive comment (four bytes before its end, so it is not the one that
    // ends the file), claiming 0x7F7F entries in a directory of 0x7F7F7F7F bytes: refused when it
    // gives one disk, ignored when it gives two (no reader takes those).
    let one_disk = "PK\u{5}\u{6}\0\0\0\0\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\0\0tail";
    assert!(!fits(&container_bytes(parts, one_disk)));
    let two_disks = "PK\u{5}\u{6}\u{1}\0\0\0\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\0\0tail";
    assert!(fits(&container_bytes(parts, two_disks)));
}
