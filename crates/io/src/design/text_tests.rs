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
    let document = std::str::from_utf8(&document).unwrap();
    assert!(text.contains(&format!("# document.json\n{}", laid_out(document, budget(document.len())).0)), "{text}");
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

/// The model cache, the preview and a plugin's own cache are left out. The signature log is shown,
/// because only a signature changes it. A blob whose bytes do not match the SHA-256 in its name
/// says it is damaged. Text parts come first, then the other parts by name.
#[test]
fn caches_are_left_out_and_a_mismatched_blob_is_damaged() {
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
        "# SolidCraft container {{\"format\":\"solidcraft-container/1\"}}\n# document.json\n{{\n  \"format\": \"solidcraft/64\",\n  \"name\": \"x\",\n  \"pos\": [1.0, 2.0]\n}}\n# drawings/abc.json\n{{\n  \"id\": \"abc\"\n}}\n# parts (name, bytes, SHA-256)\n{wrong} 4 {} damaged (its bytes do not match its name)\nplugins/acme/data.json 2 {}\nprovenance/log.cbor 1 {}\n",
        digest(b"data"),
        digest(b"{}"),
        digest(b"\x80"),
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

/// JSON nested deeper than serde_json reads a design is summed up in one line, never laid out.
/// Laying out text nested `n` deep takes about `n * n` bytes, so a small file could take gigabytes.
#[test]
fn deeply_nested_json_is_summed_up() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let text = design_text(deep.as_bytes());
    assert_eq!(text, format!("# JSON nested more than {MAX_DEPTH} levels deep ({} bytes, SHA-256 {})\n", deep.len(), digest(deep.as_bytes())));
    let bytes = container(&[("container.json", br#"{"format":"solidcraft-container/1"}"#), ("document.json", deep.as_bytes())]);
    let text = design_text(&bytes);
    assert!(text.contains("# document.json\n# JSON nested more than") && text.len() < 1000, "{}", text.get(..200).unwrap_or_default());
    let fits = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(!design_text(fits.as_bytes()).starts_with("# "), "{MAX_DEPTH} levels are laid out");
    let past = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert!(design_text(past.as_bytes()).starts_with("# JSON nested more than"), "one level more is summed up");
}

/// JSON within the depth limit that would still lay out to more than its budget is summed up too.
/// Every line is indented two spaces a level, so 400,000 numbers 100 levels deep would take about
/// 80 MB, where the file is under 1 MB.
#[test]
fn json_that_lays_out_too_large_is_summed_up() {
    let wide = format!("{}{}1{}", "[".repeat(100), "1,".repeat(400_000), "]".repeat(100));
    let text = design_text(wide.as_bytes());
    assert_eq!(text, summary(&format!("JSON that would lay out to more than {} bytes", budget(wide.len())), wide.as_bytes()));
}

/// The laid-out parts of a container share one budget, so many parts that each fit cannot add up to
/// more than the budget allows together. Here each of three drawings would lay out to about 31 MB,
/// and the third is summed up.
#[test]
fn a_containers_parts_share_one_budget() {
    let part = format!("{}{}1{}", "[".repeat(50), "1,".repeat(300_000), "]".repeat(50));
    let bytes = container(&[
        ("container.json", br#"{"format":"solidcraft-container/1"}"#),
        ("drawings/a.json", part.as_bytes()),
        ("drawings/b.json", part.as_bytes()),
        ("drawings/c.json", part.as_bytes()),
    ]);
    let text = design_text(&bytes);
    assert_eq!(text.matches("# JSON that would lay out to more than").count(), 1, "only one part is summed up");
    assert!(text.contains("# drawings/c.json\n# JSON that would lay out to more than"), "the third part is the one summed up");
    assert!(text.len() < FLOOR + 3 * GROWTH * part.len(), "{} bytes", text.len());
}

/// A layout that gives up has written its whole budget, so the parts after it share what is left
/// and the work stays within the budget, not only the text printed. Here the first drawing would
/// lay out to about 80 MB and is summed up. The second lays out to about 220 KB, more than its own
/// share of four times its 2 KB, so it is summed up too, where on its own it is laid out.
#[test]
fn a_layout_that_gives_up_spends_the_budget() {
    let large = format!("{}{}1{}", "[".repeat(100), "1,".repeat(400_000), "]".repeat(100));
    let small = format!("{}{}1{}", "[".repeat(100), "1,".repeat(1_000), "]".repeat(100));
    let alone = container(&[("drawings/b.json", small.as_bytes())]);
    assert!(!design_text(&alone).contains("# JSON that would lay out"), "on its own, the small part is laid out");
    let both = container(&[("drawings/a.json", large.as_bytes()), ("drawings/b.json", small.as_bytes())]);
    let text = design_text(&both);
    assert_eq!(text.matches("# JSON that would lay out to more than").count(), 2, "{}", text.get(..400).unwrap_or_default());
}

/// A part name holding a line feed would otherwise print as the lines of other parts, so two
/// different containers would print the same text. Such a name is printed as a JSON string, and
/// so is a name starting with a quote, which no quoted name can then match.
#[test]
fn a_part_name_that_could_forge_lines_is_quoted() {
    let two = container(&[("a", b"1"), ("b", b"2")]);
    let forged_name = format!("a 1 {}\nb", digest(b"1"));
    let forged = container(&[(&forged_name, b"2")]);
    let text = design_text(&forged);
    assert_ne!(design_text(&two), text);
    assert!(text.contains(&format!("\n{} 1 {}\n", serde_json::to_string(&forged_name).unwrap(), digest(b"2"))), "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
    let quoted = design_text(&container(&[("\"a\"", b"1")]));
    assert!(quoted.contains(&format!("\n\"\\\"a\\\"\" 1 {}\n", digest(b"1"))), "{quoted}");
}

/// A part that does not read (here its bytes no longer match the CRC-32 the ZIP keeps) is named
/// as damaged, and the text ends with the SHA-256 of the whole file, so a change inside that part
/// still shows in a diff.
#[test]
fn a_part_that_does_not_read_adds_the_hash_of_the_whole_file() {
    let payload = b"payload that the test damages, kept uncompressed";
    let mut writer = PackageWriter::new();
    writer.part("container.json", br#"{"format":"solidcraft-container/1"}"#, true).unwrap();
    writer.part("notes/readme.txt", payload, false).unwrap();
    let mut bytes = writer.finish().unwrap();
    let at = bytes.windows(payload.len()).position(|w| w == payload).unwrap();
    bytes[at] ^= 0x20;
    let text = design_text(&bytes);
    assert!(text.contains("\nnotes/readme.txt damaged ("), "{text}");
    assert!(text.ends_with(&format!("({} bytes, SHA-256 {})\n", bytes.len(), digest(&bytes))), "{text}");
}

/// A file too large to read whole is summed up from its bytes read a piece at a time, as one line
/// with the same size and SHA-256 that reading it whole gives. Two files of one size that differ
/// give different lines.
#[test]
fn a_large_file_is_summed_up_with_its_hash() {
    let first: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let mut second = first.clone();
    second[2_999_999] ^= 1;
    let line = large_file_text("too large", first.as_slice()).unwrap();
    assert_eq!(line, summary("too large", &first));
    assert_ne!(line, large_file_text("too large", second.as_slice()).unwrap());
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
