use std::collections::BTreeMap;
use std::io::Write;

use solidcraft_doc::Document;
use solidcraft_doc::drawing::Source;
use solidcraft_doc::ids::IdSource;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};

use super::*;
use crate::{is_drawing_path, read_design, write_design, write_design_container};

/// A drawing of two designs on one A3 sheet.
fn drawing() -> Drawing {
    let mut ids = IdSource::seeded(3890);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Plate assembly", "en-GB", "Assembly", a3, Some(DrawingBorderProfile::iso()), &mut ids).unwrap();
    for path in ["plate.solidcraft", "../parts/bolt.solidcraft"] {
        let id = d.allocate_id(&mut ids).unwrap();
        d.sources.push(Source { id, path: path.into() });
    }
    d
}

/// A container's parts by name.
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (entry.name().unwrap().into_owned(), bytes)
        })
        .collect()
}

/// A container written part by part, as a hostile or older tool might.
fn container(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = PackageWriter::new();
    for (name, bytes) in parts {
        writer.part(name, bytes, true).unwrap();
    }
    writer.finish().unwrap()
}

fn error(result: Result<impl std::fmt::Debug>) -> String {
    result.err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted"))
}

/// A drawing round-trips through both forms, each giving the same bytes every save; the
/// container holds `container.json` and `drawing.json`, and no `document.json`.
#[test]
fn a_drawing_round_trips_through_both_forms_byte_for_byte() {
    let d = drawing();
    let bytes = write_drawing_container(&d).unwrap();
    assert_eq!(read_drawing(&bytes).unwrap(), d);
    assert_eq!(write_drawing_container(&read_drawing(&bytes).unwrap()).unwrap(), bytes, "save, open, save: the same bytes");
    let inside = parts(&bytes);
    assert_eq!(inside.keys().map(String::as_str).collect::<Vec<_>>(), ["container.json", "drawing.json"]);
    assert_eq!(inside["container.json"], br#"{"content":"drawing","format":"solidcraft-container/1"}"#);
    let json = d.to_json().unwrap();
    assert_eq!(inside["drawing.json"], json.as_bytes(), "the container holds the plain form's text");
    let again = read_drawing(json.as_bytes()).unwrap();
    assert_eq!((again.to_json().unwrap(), &again), (json, &d));
}

/// Each file is the kind it says: a design container's `container.json` has no `content`, a
/// drawing's says `drawing`, and plain JSON is a drawing when its format is one.
#[test]
fn file_kind_tells_designs_and_drawings_apart() {
    let doc = Document::new("Plate");
    let design = write_design_container(&doc).unwrap();
    assert_eq!(parts(&design)["container.json"], br#"{"format":"solidcraft-container/1"}"#, "a design's container is as it was");
    let d = drawing();
    let cases = [
        (design, FileKind::Design),
        (write_design(&doc), FileKind::Design),
        (write_drawing_container(&d).unwrap(), FileKind::Drawing),
        (d.to_json().unwrap().into_bytes(), FileKind::Drawing),
        (b"{\"format\": \"solidcraft-drawing/1\"}".to_vec(), FileKind::Drawing),
        (b"{\"format\": \"solidcraft/1\"}".to_vec(), FileKind::Design),
        (b"not json".to_vec(), FileKind::Design),
    ];
    for (bytes, kind) in cases {
        assert_eq!(file_kind(&bytes).unwrap(), kind, "{}", String::from_utf8_lossy(&bytes[..bytes.len().min(40)]));
    }
}

/// Neither reader takes the other's file, and each says what the file is.
#[test]
fn a_design_is_never_read_as_a_drawing_nor_a_drawing_as_a_design() {
    let d = drawing();
    for bytes in [write_drawing_container(&d).unwrap(), d.to_json().unwrap().into_bytes()] {
        assert!(error(read_design(&bytes)).contains("is a SolidCraft drawing, not a design"));
    }
    let doc = Document::new("Plate");
    for bytes in [write_design_container(&doc).unwrap(), write_design(&doc)] {
        assert!(error(read_drawing(&bytes)).contains("is a SolidCraft design, not a drawing"));
    }
}

/// What a drawing container refuses: content this build doesn't know, a design's document beside
/// the drawing, a missing drawing, an unknown container version, and a damaged drawing.
#[test]
fn a_drawing_container_refuses_what_it_cant_take() {
    let text = drawing().to_json().unwrap();
    let drawing_json = |content: &str| format!(r#"{{"content":{content},"format":"solidcraft-container/1"}}"#);
    let cases: [(Vec<u8>, &str); 6] = [
        (container(&[("container.json", drawing_json("\"sketch\"").as_bytes()), ("drawing.json", text.as_bytes())]), "container content \"sketch\""),
        (container(&[("container.json", drawing_json("7").as_bytes()), ("drawing.json", text.as_bytes())]), "container content 7"),
        (
            container(&[
                ("container.json", drawing_json("\"drawing\"").as_bytes()),
                ("drawing.json", text.as_bytes()),
                ("document.json", &b"{}"[..]),
            ]),
            "also holds a design's document.json",
        ),
        (container(&[("container.json", drawing_json("\"drawing\"").as_bytes())]), "required part `drawing.json`"),
        (
            container(&[("container.json", &br#"{"content":"drawing","format":"solidcraft-container/9"}"#[..]), ("drawing.json", text.as_bytes())]),
            "version",
        ),
        (
            container(&[("container.json", drawing_json("\"drawing\"").as_bytes()), ("drawing.json", text.replace("en-GB", "e").as_bytes())]),
            "damaged drawing",
        ),
    ];
    for (bytes, says) in cases {
        let e = error(read_drawing(&bytes));
        assert!(e.contains(says), "{says}: {e}");
    }
    assert!(error(read_design(&container(&[("container.json", drawing_json("\"sketch\"").as_bytes())]))).contains("container content"));
}

/// Drawing files: the extension, and reading one from disk in either form.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn drawing_files_open_from_disk_in_either_form() {
    // The browsers' file pickers take only ASCII letters, digits, `+` and `.`, up to 16 long.
    assert!(crate::DRAWING_EXT.len() < 16 && crate::DRAWING_EXT.bytes().all(|b| b.is_ascii_alphanumeric()), "{}", crate::DRAWING_EXT);
    for (path, drawing) in [("a.scdwg", true), ("A.SCDWG", true), ("a.solidcraft", false), ("scdwg", false), ("a.dwg", false)] {
        assert_eq!(is_drawing_path(path), drawing, "{path}");
    }
    let d = drawing();
    let dir = std::env::temp_dir().join(format!("solidcraft-drawing-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in [("container.scdwg", write_drawing_container(&d).unwrap()), ("plain.scdwg", d.to_json().unwrap().into_bytes())] {
        let path = dir.join(name);
        std::fs::File::create(&path).unwrap().write_all(&bytes).unwrap();
        assert_eq!(read_drawing_file(path.to_str().unwrap()).unwrap(), d, "{name}");
    }
    assert!(error(read_drawing_file(dir.to_str().unwrap())).contains("not a regular file"));
    std::fs::remove_dir_all(&dir).unwrap();
}
