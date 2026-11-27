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

#[test]
fn a_zip64_design_is_a_miss_whatever_record_ends_it() {
    let zip64 = crate::tests::as_zip64(&design(V2, 512, 384, 1000));
    assert_eq!(read(&zip64), None);
    let mut skipped = zip64;
    let position = skipped.len() as u32;
    skipped.extend_from_slice(&crate::tests::end_record(1, 0, position, 0));
    assert_eq!(read(&skipped), None, "a record the zip crate skips hides a ZIP64 design");
}

/// Each central-directory header in `bytes`: where it starts and its entry's name.
fn headers(bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = bytes[from..].windows(4).position(|w| w == b"PK\x01\x02") {
        let at = from + found;
        let len = u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]) as usize;
        out.push((at, bytes[at + 46..at + 46 + len].to_vec()));
        from = at + 4;
    }
    out
}

/// `bytes` after `change` has edited the central header (its start, first argument) and local
/// header (second) of the entry `name`.
fn patched(bytes: &[u8], name: &str, change: impl Fn(&mut Vec<u8>, usize, usize)) -> Vec<u8> {
    let (central, _) = headers(bytes).into_iter().find(|(_, n)| n == name.as_bytes()).unwrap();
    let local = u32::from_le_bytes(bytes[central + 42..central + 46].try_into().unwrap()) as usize;
    let mut out = bytes.to_vec();
    change(&mut out, central, local);
    out
}

/// A design saved with a format 2 picture, keyed to it, with `extra` empty entries besides.
fn design_with_entries(extra: usize) -> Vec<u8> {
    let picture = png(512, 384, 1000);
    let key = key(V2, DOCUMENT, &picture);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", key.as_slice()),
        ("cache/thumbnail.png", picture.as_slice()),
    ] {
        writer.start_file(name, SimpleFileOptions::default()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    for i in 0..extra {
        writer.start_file(format!("p/{i:05}"), SimpleFileOptions::default()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn the_key_has_its_byte_bound() {
    let picture = png(512, 384, 1000);
    let with_key_of = |len: usize| {
        let mut key = key(V2, DOCUMENT, &picture);
        key.resize(len, b' ');
        zip(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.json", &key), ("cache/thumbnail.png", &picture)])
    };
    assert_eq!(read(&with_key_of(4 << 10)), Some((512, 384)));
    assert_eq!(read(&with_key_of((4 << 10) + 1)), None);
}

#[test]
fn a_key_with_more_fields_is_read() {
    let picture = png(512, 384, 1000);
    let key = format!(r#"{{"format":"{V2}","app":"0.52.0","document":"{}","payload":"{}","camera":[1,2,3]}}"#, hex(DOCUMENT), hex(&picture));
    let bytes = zip(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", key.as_bytes()),
        ("cache/thumbnail.png", &picture),
    ]);
    assert_eq!(read(&bytes), Some((512, 384)));
}

#[test]
fn ten_thousand_entries_are_read_and_one_more_is_not() {
    assert_eq!(read(&design_with_entries(10_000 - 4)), Some((512, 384)));
    assert_eq!(read(&design_with_entries(10_001 - 4)), None);
}

/// `bytes` at the end of a source `len` bytes long whose bytes before them are zeros.
struct Padded {
    bytes: Vec<u8>,
    len: u64,
    at: u64,
}

impl Read for Padded {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let start = self.len - self.bytes.len() as u64;
        let n = if self.at < start {
            let n = buf.len().min(usize::try_from(start - self.at).unwrap_or(usize::MAX));
            buf[..n].fill(0);
            n
        } else {
            let rest = self.bytes.get(usize::try_from(self.at - start).unwrap()..).unwrap_or_default();
            let n = buf.len().min(rest.len());
            buf[..n].copy_from_slice(&rest[..n]);
            n
        };
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Padded {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.at = match from {
            SeekFrom::Start(n) => n,
            SeekFrom::End(n) => self.len.checked_add_signed(n).unwrap(),
            SeekFrom::Current(n) => self.at.checked_add_signed(n).unwrap(),
        };
        Ok(self.at)
    }
}

#[test]
fn a_source_of_exactly_the_file_bound_is_read() {
    let padded = |len: u64| Padded { bytes: design(V2, 512, 384, 1000), len, at: 0 };
    assert_eq!(keyed_picture(padded(crate::MAX_FILE)).map(|p| (p.width, p.height)), Some((512, 384)));
    assert_eq!(keyed_picture(padded(crate::MAX_FILE + 1)), None);
}

#[test]
fn a_part_whose_header_understates_it_stops_at_its_cap() {
    let picture = png(512, 384, 1000);
    let spaces = vec![b' '; 64 << 20];
    let set_size = |out: &mut Vec<u8>, central: usize, local: usize| {
        out[central + 24..central + 28].copy_from_slice(&100_u32.to_le_bytes());
        out[local + 22..local + 26].copy_from_slice(&100_u32.to_le_bytes());
    };
    let key_text = key(V2, DOCUMENT, &picture);
    let key_bomb =
        zip(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.json", &spaces), ("cache/thumbnail.png", &picture)]);
    let document_bomb =
        zip(&[("container.json", CONTAINER), ("document.json", &spaces), ("cache/thumbnail.json", &key_text), ("cache/thumbnail.png", &picture)]);
    let picture_bomb =
        zip(&[("container.json", CONTAINER), ("document.json", DOCUMENT), ("cache/thumbnail.json", &key_text), ("cache/thumbnail.png", &spaces)]);
    for (part, bytes) in [("cache/thumbnail.json", key_bomb), ("document.json", document_bomb), ("cache/thumbnail.png", picture_bomb)] {
        assert_eq!(read(&bytes), None, "{part}, honest header");
        assert_eq!(read(&patched(&bytes, part, set_size)), None, "{part}, header says 100 bytes");
    }
}

#[test]
fn entries_the_reader_cannot_trust_are_misses() {
    let bytes = design(V2, 512, 384, 1000);
    let png_entry = "cache/thumbnail.png";
    let encrypted = patched(&bytes, png_entry, |out, central, local| {
        out[central + 8] |= 1;
        out[local + 6] |= 1;
    });
    assert_eq!(read(&encrypted), None, "encrypted");
    let unknown_method = patched(&bytes, png_entry, |out, central, local| {
        out[central + 10..central + 12].copy_from_slice(&99_u16.to_le_bytes());
        out[local + 8..local + 10].copy_from_slice(&99_u16.to_le_bytes());
    });
    assert_eq!(read(&unknown_method), None, "unsupported compression method");
    let crc = patched(&bytes, png_entry, |out, central, local| {
        out[central + 16] ^= 0xFF;
        out[local + 14] ^= 0xFF;
    });
    assert_eq!(read(&crc), None, "CRC mismatch");
    let key_local = headers(&bytes).into_iter().find(|(_, n)| n == b"cache/thumbnail.json").map(|(c, _)| bytes[c + 42..c + 46].to_vec()).unwrap();
    let elsewhere = patched(&bytes, png_entry, |out, central, _| out[central + 42..central + 46].copy_from_slice(&key_local));
    assert_eq!(read(&elsewhere), None, "the picture's offset names the key's local header");
    let past_end = patched(&bytes, png_entry, |out, central, _| out[central + 42..central + 46].copy_from_slice(&0xFFFF_FF00_u32.to_le_bytes()));
    assert_eq!(read(&past_end), None, "the picture's offset is past the end");
    let too_long = patched(&bytes, png_entry, |out, central, local| {
        out[central + 20..central + 24].copy_from_slice(&0x7FFF_FFFF_u32.to_le_bytes());
        out[local + 18..local + 22].copy_from_slice(&0x7FFF_FFFF_u32.to_le_bytes());
    });
    // The Deflate stream ends on its own and the digests match: the saved picture, nothing else.
    assert_eq!(read(&too_long), Some((512, 384)), "the picture's compressed size is past the end");
}

#[test]
fn a_duplicate_entry_never_gives_a_picture_the_key_does_not_name() {
    let keyed = png(512, 384, 1000);
    let other = png(512, 384, 1001);
    let key_text = key(V2, DOCUMENT, &keyed);
    for (first, second) in [(&keyed, &other), (&other, &keyed)] {
        let bytes = zip(&[
            ("container.json", CONTAINER),
            ("document.json", DOCUMENT),
            ("cache/thumbnail.json", &key_text),
            ("cache/thumbnail.png", first),
            ("cache/thumbnail.pnX", second),
        ]);
        let renamed = patched(&bytes, "cache/thumbnail.pnX", |out, central, local| {
            out[central + 46 + 18] = b'g';
            out[local + 30 + 18] = b'g';
        });
        assert_eq!(headers(&renamed).iter().filter(|(_, n)| n == b"cache/thumbnail.png").count(), 2);
        let got = keyed_picture(Cursor::new(renamed)).map(|p| p.png);
        assert!(got.is_none() || got.as_ref() == Some(&keyed), "only the keyed picture, or none");
    }
}

#[test]
fn every_truncation_of_a_format_1_design_is_a_miss() {
    let bytes = design(V1, 240, 150, 1000);
    for n in 0..bytes.len() {
        assert!(matches!(std::panic::catch_unwind(|| read(&bytes[..n])), Ok(None)), "truncated to {n} bytes");
    }
}

/// A source whose reads fail from the `fail_from`-th on, counting the reads it is asked for.
struct Failing {
    inner: Cursor<Vec<u8>>,
    fail_from: usize,
    reads: usize,
}

impl Read for Failing {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        if self.reads > self.fail_from { Err(std::io::Error::other("the stream broke")) } else { self.inner.read(buf) }
    }
}

impl Seek for Failing {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(from)
    }
}

#[test]
fn a_source_that_fails_part_way_is_a_miss() {
    let bytes = design(V2, 512, 384, 1000);
    let mut whole = Failing { inner: Cursor::new(bytes.clone()), fail_from: usize::MAX, reads: 0 };
    assert!(keyed_picture(&mut whole).is_some());
    for fail_from in 0..whole.reads {
        let failing = Failing { inner: Cursor::new(bytes.clone()), fail_from, reads: 0 };
        assert_eq!(keyed_picture(failing).map(|p| p.width), None, "reads fail from read {fail_from} of {}", whole.reads);
    }
}
