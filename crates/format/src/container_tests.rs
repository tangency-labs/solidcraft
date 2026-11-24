use super::*;

const LIMITS: Limits = Limits {
    max_entries: 8,
    max_expanded: || 1 << 20,
    part_limit: |name| if name.ends_with(".big") { 1 << 20 } else { 1024 },
    level: |_, _| 1,
    check_size: |length| if length <= 1 << 20 { Ok(()) } else { Err("too large".into()) },
    cancelled: None,
};

/// Parts and blobs written are read back as they were; a blob is stored once, named by its hash,
/// and checked against it.
#[test]
fn parts_and_blobs_read_back() {
    let mut writer = Writer::new(LIMITS);
    writer.part("document.json", br#"{"a":1}"#, true).unwrap();
    let first = writer.blob(b"payload", "bin", false).unwrap();
    let again = writer.blob(b"payload", "bin", true).unwrap();
    assert_eq!(first, again, "the same payload is one blob");
    assert_eq!(first, Value::String(format!("blobs/{}.bin", digest(b"payload"))));
    let bytes = writer.finish().unwrap();
    assert!(is_container(&bytes));
    let mut reader = Reader::new(Cursor::new(bytes), LIMITS).unwrap();
    assert_eq!(reader.part_names().len(), 2);
    assert!(reader.has_part("document.json") && !reader.has_part("other.json"));
    assert_eq!(&*reader.document(64).unwrap(), br#"{"a":1}"#);
    let reference = serde_json::json!({"blob": first});
    assert_eq!(reader.blob(&reference, "blob", "bin").unwrap(), b"payload");
    assert!(reader.blob(&reference, "blob", "png").is_err(), "another extension");
    assert_eq!(reader.hydrated, 7);
    reader.hydrated = (LIMITS.max_expanded)();
    assert!(reader.blob(&reference, "blob", "bin").unwrap_err().to_string().contains("resolved payloads too large"));
}

/// A part past its limit, or more entries than the limit, are refused as they are written.
#[test]
fn budgets_are_kept_when_writing() {
    let mut writer = Writer::new(LIMITS);
    assert_eq!(writer.part("small.json", &[0; 1025], false), Err(FormatError::Invalid("part too large".into())));
    assert!(writer.part("large.big", &[0; 1025], true).is_ok());
    for i in 0..7 {
        writer.part(&format!("{i}.json"), b"x", false).unwrap();
    }
    assert_eq!(writer.part("one-more.json", b"x", false), Err(FormatError::Invalid("package too large".into())));
    assert!(!writer.room(1, 0, 1 << 20), "no room for an entry past the limit");
}

/// A thumbnail is kept only for the file it shows.
#[test]
fn a_thumbnail_is_keyed_to_its_file() {
    let picture = Thumbnail { format: "app-thumbnail/1", app: "1.0", size: [2, 1], max_bytes: 1024 };
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&2u32.to_be_bytes());
    png.extend_from_slice(&1u32.to_be_bytes());
    assert!(picture.valid(&png));
    let mut writer = Writer::new(LIMITS);
    writer.part("document.json", b"{}", false).unwrap();
    picture.write(&mut writer, &png, b"{}", 8 << 20).unwrap();
    let mut reader = Reader::new(Cursor::new(writer.finish().unwrap()), LIMITS).unwrap();
    assert_eq!(picture.read(&mut reader, 64).unwrap(), png);
    let mut writer = Writer::new(LIMITS);
    writer.part("document.json", b"{\"changed\":1}", false).unwrap();
    picture.write(&mut writer, &png, b"{}", 8 << 20).unwrap();
    let mut reader = Reader::new(Cursor::new(writer.finish().unwrap()), LIMITS).unwrap();
    assert!(picture.read(&mut reader, 64).is_err(), "stale");
}

/// A blob copied from another container is written exactly as that container stores it: the file
/// is byte for byte the one encoding the same content writes, so a save that reuses an unchanged
/// import cannot produce a different design file.
#[test]
fn a_blob_copied_from_a_container_is_written_as_it_is_stored() {
    let payload = vec![7u8; 512];
    let mut writer = Writer::new(LIMITS);
    writer.part("document.json", b"{}", true).unwrap();
    let name = writer.blob(&payload, "bin", true).unwrap();
    let source = writer.finish().unwrap();

    let mut reader = Reader::new(Cursor::new(source.clone()), LIMITS).unwrap();
    let mut copy = Writer::new(LIMITS);
    copy.part("document.json", b"{}", true).unwrap();
    assert_eq!(copy.blob_reusing(&payload, "bin", true, Some(&mut reader)).unwrap(), name);
    let copied = copy.finish().unwrap();

    let mut encoded = Writer::new(LIMITS);
    encoded.part("document.json", b"{}", true).unwrap();
    assert_eq!(encoded.blob(&payload, "bin", true).unwrap(), name);
    assert_eq!(copied, encoded.finish().unwrap(), "a copied blob writes the container encoding it writes");

    // The copy reads back to the same content.
    let mut reader = Reader::new(Cursor::new(copied), LIMITS).unwrap();
    let reference = serde_json::json!({"blob": name});
    assert_eq!(reader.blob(&reference, "blob", "bin").unwrap(), &payload);
}

/// A part is copied only when its entry holds the content being saved, stored as this writer
/// stores it: a part whose name does not match its content (a hand-made or changed file), or one
/// stored uncompressed (an older save) where a compressed one is written, is encoded instead, so a
/// copy can never put other content, or a larger file, into a save.
#[test]
fn a_blob_whose_part_does_not_hold_the_content_is_encoded_not_copied() {
    let payload = vec![7u8; 512];
    let mut writer = Writer::new(LIMITS);
    let name = writer.blob(&payload, "bin", true).unwrap();
    let Some(name) = name.as_str() else { panic!("a blob is named by a string") };
    let mut expected = Writer::new(LIMITS);
    expected.blob(&payload, "bin", true).unwrap();
    let expected = expected.finish().unwrap();

    // The payload's name, holding other bytes of the same length, and the payload stored.
    let forged = |content: &[u8], method: zip::CompressionMethod| {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
        zip.write_all(content).unwrap();
        zip.finish().unwrap().into_inner()
    };
    for (what, source) in [
        ("other content under the payload's name", forged(&[8u8; 512], zip::CompressionMethod::Deflated)),
        ("the payload stored, not compressed", forged(&payload, zip::CompressionMethod::Stored)),
    ] {
        let mut reader = Reader::new(Cursor::new(source), LIMITS).unwrap();
        let mut copy = Writer::new(LIMITS);
        copy.blob_reusing(&payload, "bin", true, Some(&mut reader)).unwrap();
        assert_eq!(copy.finish().unwrap(), expected, "{what}: encoded, not copied");
    }

    // A name the source lacks is encoded.
    let mut writer = Writer::new(LIMITS);
    writer.blob(b"other", "bin", true).unwrap();
    let mut reader = Reader::new(Cursor::new(writer.finish().unwrap()), LIMITS).unwrap();
    let mut copy = Writer::new(LIMITS);
    copy.blob_reusing(&payload, "bin", true, Some(&mut reader)).unwrap();
    assert_eq!(copy.finish().unwrap(), expected, "absent: encoded");
}
