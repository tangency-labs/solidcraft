use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use super::*;

const DOCUMENT: &[u8] = br#"{"format":"solidcraft/73","name":"Keyed","units":"mm","features":[]}"#;
const CONTAINER: &[u8] = br#"{"format":"solidcraft-container/1"}"#;
const V1: &str = "solidcraft-thumbnail/1";
const V2: &str = "solidcraft-thumbnail/2";

/// A PNG signature and an IHDR of `width` x `height`, padded with zeros to `len` bytes: enough for
/// the reader, which never decodes.
fn png(width: u32, height: u32, len: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let at_least = bytes.len();
    bytes.resize(len.max(at_least), 0);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn key(tag: &str, document: &[u8], picture: &[u8]) -> Vec<u8> {
    format!(r#"{{"format":"{tag}","app":"0.51.1","document":"{}","payload":"{}"}}"#, hex(document), hex(picture)).into_bytes()
}

fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        writer.start_file(*name, SimpleFileOptions::default()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// A design saved with a picture of format `tag`, `width` x `height` and `len` bytes, keyed to it.
fn design(tag: &str, width: u32, height: u32, len: usize) -> Vec<u8> {
    let picture = png(width, height, len);
    zip(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", &key(tag, DOCUMENT, &picture)),
        ("cache/thumbnail.png", &picture),
    ])
}

fn read(bytes: &[u8]) -> Option<(u32, u32)> {
    keyed_picture(Cursor::new(bytes)).map(|p| (p.width, p.height))
}

#[test]
fn a_picture_of_either_format_is_read_when_its_key_matches() {
    assert_eq!(read(&design(V2, 512, 384, 1000)), Some((512, 384)));
    assert_eq!(read(&design(V1, 240, 150, 1000)), Some((240, 150)));
    let picture = keyed_picture(Cursor::new(design(V2, 512, 384, 1000))).unwrap();
    assert_eq!(picture.png, png(512, 384, 1000));
}

#[test]
fn a_picture_keyed_to_another_document_or_picture_is_a_miss() {
    let picture = png(512, 384, 1000);
    let other_document = zip(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", &key(V2, b"{}", &picture)),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&other_document), None);
    let other_picture = png(512, 384, 999);
    let other_bytes = zip(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", &key(V2, DOCUMENT, &other_picture)),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&other_bytes), None);
}

#[test]
fn digests_are_compared_exactly() {
    let picture = png(512, 384, 1000);
    let upper = format!(r#"{{"format":"{V2}","app":"0.51.1","document":"{}","payload":"{}"}}"#, hex(DOCUMENT).to_uppercase(), hex(&picture));
    let bytes = zip(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", upper.as_bytes()),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&bytes), None);
}

#[test]
fn an_unknown_format_or_a_picture_of_the_wrong_size_is_a_miss() {
    assert_eq!(read(&design("solidcraft-thumbnail/3", 512, 384, 1000)), None);
    assert_eq!(read(&design(V1, 512, 384, 1000)), None);
    assert_eq!(read(&design(V2, 240, 150, 1000)), None);
    assert_eq!(read(&design(V2, 512, 383, 1000)), None);
}

#[test]
fn each_format_has_its_byte_bound() {
    assert!(read(&design(V2, 512, 384, 512 << 10)).is_some());
    assert_eq!(read(&design(V2, 512, 384, (512 << 10) + 1)), None);
    assert!(read(&design(V1, 240, 150, 256 << 10)).is_some());
    assert_eq!(read(&design(V1, 240, 150, (256 << 10) + 1)), None);
}

#[test]
fn a_missing_or_malformed_key_is_a_miss() {
    let picture = png(512, 384, 1000);
    let with_key = |key: &[u8]| {
        zip(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.json", key), ("cache/thumbnail.png", &picture)])
    };
    let without_key = zip(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.png", &picture)]);
    assert_eq!(read(&without_key), None);
    assert_eq!(read(&with_key(b"not json".as_slice())), None);
    let no_payload = format!(r#"{{"format":"{V2}","app":"0.51.1","document":"{}"}}"#, hex(DOCUMENT));
    assert_eq!(read(&with_key(no_payload.as_bytes())), None);
    let numeric_document = format!(r#"{{"format":"{V2}","document":5,"payload":"{}"}}"#, hex(&picture));
    assert_eq!(read(&with_key(numeric_document.as_bytes())), None);
    let long_app = format!(r#"{{"format":"{V2}","app":"{}","document":"{}","payload":"{}"}}"#, "x".repeat(5_000), hex(DOCUMENT), hex(&picture));
    assert_eq!(read(&with_key(long_app.as_bytes())), None);
}

#[test]
fn a_missing_or_oversized_document_is_a_miss() {
    let picture = png(512, 384, 1000);
    let without_document =
        zip(&[("container.json", CONTAINER), ("cache/thumbnail.json", &key(V2, DOCUMENT, &picture)), ("cache/thumbnail.png", &picture)]);
    assert_eq!(read(&without_document), None);
    let largest = vec![b' '; 16 << 20];
    let at_bound = zip(&[
        ("container.json", CONTAINER),
        ("document.json", &largest),
        ("cache/thumbnail.json", &key(V2, &largest, &picture)),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&at_bound), Some((512, 384)));
    let over = vec![b' '; (16 << 20) + 1];
    let over_bytes = zip(&[
        ("container.json", CONTAINER),
        ("document.json", &over),
        ("cache/thumbnail.json", &key(V2, &over, &picture)),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&over_bytes), None);
}

#[test]
fn other_containers_are_taken_as_the_path_reader_takes_them() {
    let picture = png(512, 384, 1000);
    let key_text = key(V2, DOCUMENT, &picture);
    let with_container = |container: &[u8]| {
        zip(&[("container.json", container), ("document.json", DOCUMENT), ("cache/thumbnail.json", &key_text), ("cache/thumbnail.png", &picture)])
    };
    assert!(read(&with_container(br#"{"format":"solvecraft-container/1"}"#.as_slice())).is_some());
    assert_eq!(read(&with_container(br#"{"format":"something-else/9"}"#.as_slice())), None);
    let no_container = zip(&[("document.json", DOCUMENT), ("cache/thumbnail.json", &key_text), ("cache/thumbnail.png", &picture)]);
    assert_eq!(read(&no_container), None);
    let drawing = zip(&[
        ("container.json", br#"{"content":"drawing","format":"solidcraft-container/1"}"#.as_slice()),
        ("drawing.json", DOCUMENT),
        ("cache/thumbnail.json", &key_text),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&drawing), None);
}

#[test]
fn anything_that_is_not_a_container_is_a_miss() {
    assert_eq!(read(DOCUMENT), None);
    assert_eq!(read(b""), None);
    let noise: Vec<u8> = (0..4096_u32).map(|i| (i * 31 % 251) as u8).collect();
    assert_eq!(read(&noise), None);
}

#[test]
fn every_truncation_is_a_miss_and_never_a_panic() {
    let bytes = design(V2, 512, 384, 1000);
    for n in 0..bytes.len() {
        assert!(matches!(std::panic::catch_unwind(|| read(&bytes[..n])), Ok(None)), "truncated to {n} bytes");
    }
}

#[test]
fn a_source_is_read_from_its_start() {
    let mut cursor = Cursor::new(design(V2, 512, 384, 1000));
    cursor.seek(SeekFrom::End(0)).unwrap();
    assert!(keyed_picture(cursor).is_some());
}

/// A source over the file bound: it reports its length as past the bound and counts any read.
struct Huge {
    reads: usize,
}

impl Read for Huge {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        Ok(0)
    }
}

impl Seek for Huge {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        Ok(match from {
            SeekFrom::End(_) => crate::MAX_FILE + 1,
            _ => 0,
        })
    }
}

#[test]
fn a_source_longer_than_the_file_bound_is_refused_unread() {
    let mut huge = Huge { reads: 0 };
    assert_eq!(keyed_picture(&mut huge), None);
    assert_eq!(huge.reads, 0);
}
