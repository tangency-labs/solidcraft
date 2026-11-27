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
fn record(bytes: &[u8]) -> usize {
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
    let bytes = with_field(&with_field(&good(), 8, &0xFFFF_u16.to_le_bytes()), 10, &0xFFFF_u16.to_le_bytes());
    let path = std::env::temp_dir().join(format!("solidcraft-design-preview-directory-{}.sc", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(picture(&path), None);
    assert_eq!(model(&path), None);
    let _ = std::fs::remove_file(&path);
}
