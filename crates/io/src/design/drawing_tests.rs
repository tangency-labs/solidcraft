use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use serde_json::{Value, json};
use solidcraft_doc::drawing::image::{ImageFile, SheetImage};
use solidcraft_doc::drawing::{MAX_DRAWINGS, Source};
use solidcraft_doc::format::DRAWING;
use solidcraft_doc::ids::IdSource;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};

use super::*;
use crate::{is_drawing_path, read_design, write_design, write_design_container};

/// A design with two drawings of it, each on one A3 sheet, in their order.
fn plate() -> Document {
    let mut ids = IdSource::seeded(3890);
    let mut doc = Document::new("Plate");
    for name in ["Assembly", "Details"] {
        let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
        let taken: BTreeSet<u64> = doc.drawings.iter().map(|d| d.id).collect();
        let mut d = Drawing::new(name, "en-GB", "Sheet 1", a3, Some(DrawingBorderProfile::iso()), &taken, &mut ids).unwrap();
        let id = d.allocate_id(&mut ids).unwrap();
        d.sources.push(Source { id });
        doc.drawings.push(Arc::new(d));
    }
    doc
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

/// A container of these parts, as a hostile or older tool might write it.
fn container(parts: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The entry of the drawing with this id.
fn entry(id: u64) -> String {
    format!("drawings/{}.json", ids::encode(id))
}

/// A design's drawings round-trip through both forms byte for byte. The container keeps each in its
/// own entry, holding the drawing's own text, and its `document.json` and `container.json` are the
/// same bytes with drawings or without: a drawing edit never makes the model cache, keyed to
/// `document.json`, stale.
#[test]
fn a_designs_drawings_round_trip_through_both_forms_byte_for_byte() {
    let doc = plate();
    let bytes = write_design_container(&doc).unwrap();
    let read = read_design(&bytes).unwrap();
    assert_eq!(read.drawings, doc.drawings);
    assert_eq!(write_design_container(&read).unwrap(), bytes, "save, open, save: the same bytes");
    let inside = parts(&bytes);
    let entries: Vec<_> = inside.keys().filter(|n| n.starts_with("drawings/")).cloned().collect();
    assert_eq!(entries, doc.drawings.iter().map(|d| entry(d.id)).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    for d in &doc.drawings {
        assert_eq!(inside[&entry(d.id)], d.to_json().unwrap().as_bytes());
    }
    let without = parts(&write_design_container(&Document { drawings: Vec::new(), ..doc.clone() }).unwrap());
    for part in ["document.json", "container.json"] {
        assert_eq!(inside[part], without[part], "{part} is the same with drawings or without");
    }
    let json = write_design(&doc);
    let read = read_design(&json).unwrap();
    assert_eq!(read.drawings, doc.drawings);
    assert_eq!(write_design(&read), json, "the plain form too");
}

/// An image on a sheet travels in its drawing's record through both forms, and an entry whose file
/// isn't what its record says refuses the whole design.
#[test]
fn an_image_on_a_sheet_round_trips_and_a_lying_one_is_refused() {
    let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    header.extend(640u32.to_be_bytes());
    header.extend(480u32.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    let mut doc = plate();
    let mut d = (*doc.drawings[0]).clone();
    let mut ids = IdSource::seeded(5426);
    let (file, placed) = (d.allocate_id(&mut ids).unwrap(), d.allocate_id(&mut ids).unwrap());
    d.images.add([Arc::new(ImageFile::from_file(file, &header).unwrap())]);
    d.sheet_images.add([Arc::new(SheetImage { id: placed, sheet: d.sheets[0].id, rect_mm: [10.0, 20.0, 60.0, 45.0], image: file })]);
    doc.drawings[0] = Arc::new(d);
    let bytes = write_design_container(&doc).unwrap();
    let read = read_design(&bytes).unwrap();
    assert_eq!(read.drawings, doc.drawings);
    assert_eq!(write_design_container(&read).unwrap(), bytes, "save, open, save: the same bytes");
    let json = write_design(&doc);
    assert_eq!(read_design(&json).unwrap().drawings, doc.drawings, "the plain form too");

    let mut inside = parts(&bytes);
    let name = entry(doc.drawings[0].id);
    let mut record: Value = serde_json::from_slice(&inside[&name]).unwrap();
    record["images"][0]["pixels"] = json!([1, 1]);
    inside.insert(name, record.to_string().into_bytes());
    assert!(read_design(&container(&inside)).is_err());
}

/// What a container refuses in its drawings, each time the whole design: an entry under
/// `drawings/` that isn't a drawing's, one whose record has another id, a damaged drawing,
/// drawings inline in `document.json` beside the entries, and more entries than a design may hold,
/// counted before any is read. One a newer build saved is no refusal: it opens.
#[test]
fn a_container_refuses_drawing_entries_it_cant_take() {
    let doc = plate();
    let good = parts(&write_design_container(&doc).unwrap());
    let [first, second] = [0, 1].map(|i| doc.drawings[i].id);
    let text = |i: usize| String::from_utf8(good[&entry(doc.drawings[i].id)].clone()).unwrap();
    let refusal = |edit: &dyn Fn(&mut BTreeMap<String, Vec<u8>>)| {
        let mut parts = good.clone();
        edit(&mut parts);
        read_design(&container(&parts)).map_or_else(|e| e.to_string(), |d| format!("read {}", d.drawings.len()))
    };
    assert_eq!(refusal(&|_| {}), "read 2");
    for name in ["drawings/0000000000000.json", "drawings/ABCDEFGHJKMNP.json", "drawings/x/0000000000001.json", "drawings/0000000000001.svg"] {
        let why = refusal(&|p| {
            p.insert(name.into(), b"{}".to_vec());
        });
        assert_eq!(why, format!("design container: `{name}` is not a drawing's entry"));
    }
    let moved = refusal(&|p| {
        let bytes = p.remove(&entry(first)).unwrap();
        p.insert(entry(7), bytes);
    });
    assert_eq!(moved, format!("design container: `{}` holds the drawing {}", entry(7), ids::encode(first)));
    let damaged = refusal(&|p| {
        p.insert(entry(second), text(1).replace("en-GB", "e").into_bytes());
    });
    assert!(damaged.starts_with(&format!("design container: `{}`: damaged drawing: the language", entry(second))), "{damaged}");
    let newer = DRAWING.format_string(DRAWING.current() + 1);
    // A drawing a newer build saved opens with its notice, as a newer design does (#12166).
    let mut parts = good.clone();
    parts.insert(entry(second), text(1).replace(&DRAWING.format_string(DRAWING.current()), &newer).into_bytes());
    let opened = read_design(&container(&parts)).unwrap();
    assert_eq!(opened.drawings.len(), 2);
    assert!(opened.drawings.iter().any(|d| d.id == second && d.newer_file.is_some()));
    let inline = refusal(&|p| {
        let mut metadata: Value = serde_json::from_slice(&p["document.json"]).unwrap();
        metadata["drawings"] = json!([serde_json::from_str::<Value>(&text(0)).unwrap()]);
        p.insert("document.json".into(), serde_json::to_vec(&metadata).unwrap());
    });
    assert_eq!(inline, "design container: ambiguous drawings in document.json and in their own entries");
    let over = refusal(&|p| {
        for id in 1..=MAX_DRAWINGS as u64 {
            p.insert(entry(id), Vec::new());
        }
    });
    assert_eq!(over, format!("design container: a design holds at most {MAX_DRAWINGS} drawings"));
}

/// A design whose drawings break the rules isn't saved: two with one id would share one entry, and
/// one of them would be lost.
#[test]
fn a_design_with_two_drawings_of_one_id_is_not_saved() {
    let mut doc = plate();
    let first = doc.drawings[0].clone();
    doc.drawings[1] = Arc::new(Drawing { name: "Details".into(), ..(*first).clone() });
    let why = write_design_container(&doc).unwrap_err().to_string();
    assert_eq!(why, format!("two drawings have the id {}", ids::encode(first.id)));
}

/// A drawing that can't be saved names itself in the design's error: which of the design's
/// drawings, and why. Here one too large to open again (2,000,000 numbers indent past the limit).
#[test]
fn a_drawing_not_saved_is_named_in_the_designs_error() {
    let items = vec!["0"; 2_000_000].join(",");
    let format = DRAWING.format_string(1);
    let big = Drawing::from_json(&format!(
        r#"{{"format":"{format}","id":"0000000000002","name":"Huge","sources":[],"language":"en","sheets":[{{"id":"0000000000003","name":"S","sheet":{{"size":{{"standard":"a4"}}}}}}],"properties":{{"com.example":{{"items":[{items}]}}}}}}"#
    ))
    .unwrap();
    let mut doc = plate();
    doc.drawings.push(Arc::new(big));
    let why = write_design_container(&doc).unwrap_err().to_string();
    assert_eq!(why, r#"design container: drawing "Huge" (0000000000002) not saved: drawing too large to save (at most 16 MiB)"#);
}

/// A drawing on its own, as a drawing file holds it: one A3 sheet of the design `plate.sc`.
fn drawing() -> Drawing {
    let mut ids = IdSource::seeded(3890);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Plate assembly", "en-GB", "Assembly", a3, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut ids).unwrap();
    let id = d.allocate_id(&mut ids).unwrap();
    d.sources.push(Source { id });
    d
}

/// A container written part by part, in this order, through the container writer.
fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = PackageWriter::new();
    for (name, bytes) in parts {
        writer.part(name, bytes, true).unwrap();
    }
    writer.finish().unwrap()
}

fn error(result: Result<impl std::fmt::Debug>) -> String {
    result.err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted"))
}

/// A drawing file round-trips through both forms, each giving the same bytes every save; the
/// container holds `container.json` and `drawing.json`, and no `document.json`.
#[test]
fn a_drawing_file_round_trips_through_both_forms_byte_for_byte() {
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
        (package(&[("container.json", drawing_json("\"sketch\"").as_bytes()), ("drawing.json", text.as_bytes())]), "it holds \"sketch\""),
        (package(&[("container.json", drawing_json("7").as_bytes()), ("drawing.json", text.as_bytes())]), "it holds 7"),
        (
            package(&[("container.json", drawing_json("\"drawing\"").as_bytes()), ("drawing.json", text.as_bytes()), ("document.json", &b"{}"[..])]),
            "also holds a design's document.json",
        ),
        (package(&[("container.json", drawing_json("\"drawing\"").as_bytes())]), "required part `drawing.json`"),
        (package(&[("container.json", &br#"{"content":"drawing","format":"another-tool/9"}"#[..]), ("drawing.json", text.as_bytes())]), "version"),
        (
            package(&[("container.json", drawing_json("\"drawing\"").as_bytes()), ("drawing.json", text.replace("en-GB", "e").as_bytes())]),
            "damaged drawing",
        ),
    ];
    for (bytes, says) in cases {
        let e = error(read_drawing(&bytes));
        assert!(e.contains(says), "{says}: {e}");
    }
    assert!(error(read_design(&package(&[("container.json", drawing_json("\"sketch\"").as_bytes())]))).contains("this version doesn't know"));
    // A later container of ours still holds a drawing: it opens.
    let later = package(&[("container.json", &br#"{"content":"drawing","format":"solidcraft-container/9"}"#[..]), ("drawing.json", text.as_bytes())]);
    assert!(read_drawing(&later).is_ok());
}

/// Drawing files: the extension, and reading one from disk in either form.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn drawing_files_open_from_disk_in_either_form() {
    // The browsers' file pickers take only ASCII letters, digits, `+` and `.`, up to 16 long.
    assert!(crate::DRAWING_EXT.len() < 16 && crate::DRAWING_EXT.bytes().all(|b| b.is_ascii_alphanumeric()), "{}", crate::DRAWING_EXT);
    for (path, drawing) in [("a.scdwg", true), ("A.SCDWG", true), ("a.sc", false), ("scdwg", false), ("a.dwg", false)] {
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
