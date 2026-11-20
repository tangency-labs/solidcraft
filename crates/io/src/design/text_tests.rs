use solidcraft_doc::{Document, FeatureKind, MeshData};
use solidcraft_format::container::digest;

use super::super::{PackageReader, PackageWriter};
use super::*;
use crate::{DesignCompression, DesignPreview, write_design, write_design_container, write_design_container_with_preview};

/// A design with an imported STEP file, so its container holds a blob.
fn design() -> Document {
    let mut doc = Document::new("Text");
    let step = String::from("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n");
    doc.add_feature(
        FeatureKind::Import { file: "part.step".into(), step: step.into(), components: Vec::new(), skip: Vec::new(), records: None },
        None,
    )
    .unwrap();
    doc
}

fn mesh() -> MeshData {
    MeshData {
        name: "Preview".into(),
        positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].into(),
        triangles: vec![0, 1, 2].into(),
        color: Some([0.25, 0.5, 0.75]),
        corner_normals: Default::default(),
    }
}

/// A container made part by part, with exactly these parts.
fn container(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = PackageWriter::new();
    for (name, bytes) in parts {
        writer.part(name, bytes, true).unwrap();
    }
    writer.finish().unwrap()
}

/// A `.json` design prints as it was saved, with a final line feed. A copy checked out with
/// Windows line endings, and one in serde_json's older layout, print the same text.
#[test]
fn a_json_design_prints_as_saved() {
    let doc = design();
    let saved = write_design(&doc).unwrap();
    let text = design_text(&saved);
    assert_eq!(text, format!("{}\n", String::from_utf8(saved.clone()).unwrap()));
    let crlf = String::from_utf8(saved).unwrap().replace('\n', "\r\n");
    assert_eq!(design_text(crlf.as_bytes()), text, "Windows line endings");
    let older = serde_json::to_string_pretty(&doc).unwrap();
    assert_eq!(design_text(older.as_bytes()), text, "the layout of builds before #626");
}

/// A container prints its `container.json` on one line and its design laid out, then one line per
/// other part with the part's size and SHA-256, read back from the file here.
#[test]
fn a_container_prints_its_design_and_one_line_per_part() {
    let bytes = write_design_container(&design()).unwrap();
    let text = design_text(&bytes);
    assert!(text.starts_with("# SolidCraft container {\"format\":\"solidcraft-container/1\""), "{text}");
    let mut reader = PackageReader::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
    let document = reader.limited_part("document.json", part_limit("document.json")).unwrap();
    assert!(text.contains(&format!("# document.json\n{}", laid_out(std::str::from_utf8(&document).unwrap()))), "{text}");
    let mut blobs = 0;
    for name in reader.part_names() {
        if name.starts_with("blobs/") {
            let part = reader.limited_part(&name, part_limit(&name)).unwrap();
            assert!(text.contains(&format!("\n{name} {} {}\n", part.len(), digest(&part))), "{name} in {text}");
            blobs += 1;
        }
    }
    assert_eq!(blobs, 1, "the imported STEP file is one blob");
    assert!(text.contains("\n# parts (name, bytes, SHA-256)\n"), "{text}");
    assert!(!text.contains("damaged"), "{text}");
}

/// A preview changes on every save and says nothing new about the design, so a design saved with
/// one prints the same text as without.
#[test]
fn a_preview_does_not_change_the_text() {
    let doc = design();
    let plain = write_design_container(&doc).unwrap();
    let with = write_design_container_with_preview(&doc, DesignCompression::Fast, Some(&DesignPreview { meshes: vec![mesh()] })).unwrap();
    assert!(PackageReader::new(std::io::Cursor::new(with.as_slice())).unwrap().has_part("cache/preview.bin"), "the preview is saved");
    assert_eq!(design_text(&with), design_text(&plain));
}

/// The model cache, the preview, the signature log and a plugin's own cache are left out. A blob
/// whose bytes do not match the SHA-256 in its name says it is damaged. Text parts come first,
/// then the other parts by name.
#[test]
fn derived_parts_are_left_out_and_a_mismatched_blob_is_damaged() {
    let wrong = format!("blobs/{}.step", "0".repeat(64));
    let bytes = container(&[
        ("container.json", br#"{"format":"solidcraft-container/1"}"#),
        ("document.json", br#"{"format":"solidcraft/64","name":"x","pos":[1.0,2.0]}"#),
        ("drawings/abc.json", br#"{"id":"abc"}"#),
        (&wrong, b"data"),
        ("cache/model.json", b"{}"),
        ("cache/preview.bin", b"\x00\x01"),
        ("provenance/log.cbor", b"\x80"),
        ("plugins/acme/cache/tile.bin", b"\x02"),
        ("plugins/acme/data.json", b"{}"),
    ]);
    let expected = format!(
        "# SolidCraft container {{\"format\":\"solidcraft-container/1\"}}\n# document.json\n{{\n  \"format\": \"solidcraft/64\",\n  \"name\": \"x\",\n  \"pos\": [1.0, 2.0]\n}}\n# drawings/abc.json\n{{\n  \"id\": \"abc\"\n}}\n# parts (name, bytes, SHA-256)\n{wrong} 4 {} damaged (its bytes do not match its name)\nplugins/acme/data.json 2 {}\n",
        digest(b"data"),
        digest(b"{}"),
    );
    assert_eq!(design_text(&bytes), expected);
}

/// A drawing container prints its `drawing.json` laid out, as a design container prints its
/// `document.json`.
#[test]
fn a_drawing_container_prints_its_drawing() {
    let bytes = container(&[
        ("container.json", br#"{"content":"drawing","format":"solidcraft-container/1"}"#),
        ("drawing.json", br#"{"format":"solidcraft-drawing/1","sheets":[]}"#),
    ]);
    let text = design_text(&bytes);
    assert!(text.contains("# drawing.json\n{\n  \"format\": \"solidcraft-drawing/1\",\n  \"sheets\": []\n}\n"), "{text}");
}

/// Input that is not a design, or a container that does not open, gives one line with its size and
/// SHA-256, and never a panic.
#[test]
fn unreadable_input_gives_one_line() {
    let whole = write_design_container(&design()).unwrap();
    let half = whole.get(..whole.len() / 2).unwrap().to_vec();
    for bytes in [Vec::new(), b"\xff\xfe\x00binary".to_vec(), b"not json".to_vec(), b"PK\x03\x04garbage".to_vec(), half] {
        let text = design_text(&bytes);
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.starts_with("# ") && text.contains(&format!("({} bytes, SHA-256 {})", bytes.len(), digest(&bytes))), "{text}");
    }
}
