use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use serde_json::{Value, json};
use solidcraft_doc::drawing::image::{ImageFile, SheetImage};
use solidcraft_doc::drawing::{MAX_DRAWINGS, Source};
use solidcraft_doc::format::DRAWING;
use solidcraft_doc::ids::IdSource;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};

use super::*;
use crate::{is_drawing_path, read_design, read_drawing, write_design, write_design_container, write_drawing_container};

/// A design with two drawings of it, each on one A3 sheet, in their order.
fn plate() -> Document {
    let mut ids = IdSource::seeded(3890);
    let mut doc = Document::new("Plate");
    for name in ["Assembly", "Details"] {
        let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
        let taken: BTreeSet<u64> = doc.drawings.iter().map(|d| d.id).collect();
        let mut d = Drawing::new(name, "en-GB", "Sheet 1", a3, Some(DrawingBorderProfile::iso()), &taken, &mut ids).unwrap();
        let id = d.allocate_id(&mut ids).unwrap();
        d.sources.push(Source { id, path: None });
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
    let json = write_design(&doc).unwrap();
    let read = read_design(&json).unwrap();
    assert_eq!(read.drawings, doc.drawings);
    assert_eq!(write_design(&read).unwrap(), json, "the plain form too");
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
    // The record names each image's part and keeps no bytes as text.
    let entries = parts(&bytes);
    let record: Value = serde_json::from_slice(&entries[&entry(doc.drawings[0].id)]).unwrap();
    let part = record["image_files"][0]["part"].as_str().unwrap().to_string();
    assert_eq!((record["image_files"][0]["data"].as_str(), entries[&part].as_slice()), (Some(""), header.as_slice()));
    // The design names the part too, for a release that can't read the drawing's record.
    let metadata: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
    assert_eq!(metadata["drawing_parts"], json!([part]));
    // A nightly build kept the records in `images`, naming their parts: it opens the same.
    let mut nightly = entries.clone();
    let mut layout = record.clone();
    let files = layout.as_object_mut().unwrap().remove("image_files").unwrap();
    let placed = layout.as_object_mut().unwrap().remove("placed_images").unwrap();
    (layout["images"], layout["sheet_images"]) = (files, placed);
    nightly.insert(entry(doc.drawings[0].id), layout.to_string().into_bytes());
    assert_eq!(read_design(&container(&nightly)).unwrap().drawings, doc.drawings);
    // An older design kept the bytes as base64 in the record: it opens the same.
    let mut older = entries.clone();
    let mut record = layout;
    record["images"][0].as_object_mut().unwrap().remove("part");
    record["images"][0]["data"] = json!(solidcraft_doc::canvas::base64_encode(&header));
    older.insert(entry(doc.drawings[0].id), record.to_string().into_bytes());
    older.remove(&part);
    assert_eq!(read_design(&container(&older)).unwrap().drawings, doc.drawings);
    // A plain JSON design can't hold the image: saving one is refused, naming it.
    assert!(write_design(&doc).unwrap_err().to_string().contains("drawing images"));

    let mut inside = parts(&bytes);
    let name = entry(doc.drawings[0].id);
    let mut record: Value = serde_json::from_slice(&inside[&name]).unwrap();
    record["image_files"][0]["pixels"] = json!([1, 1]);
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
    d.sources.push(Source { id, path: None });
    d
}

/// A container written part by part, in this order, through the container writer.
fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = PackageWriter::new(DesignCompression::Fast);
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
        (write_design(&doc).unwrap(), FileKind::Design),
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
    for bytes in [write_design_container(&doc).unwrap(), write_design(&doc).unwrap()] {
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

// W38a acceptance through the real design container, not only Drawing JSON.
fn design_with_sheet_sketches() -> Document {
    use solidcore_record::Vec2;
    use solidcraft_doc::drawing::{Owner, SheetSketch, Units};
    use solidcraft_doc::sketch::{ConstraintKind, PointExprs, Sketch};

    let mut doc = plate();
    let mut ids = IdSource::seeded(7082);
    let shared_id = doc.drawings[0].allocate_id(&mut ids).unwrap();
    for (i, drawing) in doc.drawings.iter_mut().enumerate() {
        let d = Arc::make_mut(drawing);
        let mut sketch = Sketch::new();
        let inset = 15.0 + i as f64;
        for (name, xy) in [("p0", (inset, 10.0)), ("p1", (inset + 40.0, 10.0)), ("p2", (inset + 40.0, 30.0)), ("p3", (inset, 30.0))] {
            sketch.add_point(Vec2::new(xy.0, xy.1), Some(name)).unwrap();
        }
        for j in 0..4 {
            sketch.add_line_pts(j + 1, (j + 1) % 4 + 1, Some(&format!("l{j}"))).unwrap();
        }
        sketch.add_constraint(ConstraintKind::Length { l: 0, value: 40.0 }, Some("Width".into())).unwrap();
        sketch.points[1].exprs = Some(Box::new(PointExprs { x: Some("Inset + 5 mm".into()), y: None }));
        d.units = Units::Fixed { unit: "in".into() };
        // Identical sketch ids in different drawings are legal: the namespace
        // is drawing-local, just as the curve/point ids are sketch-local.
        d.sketches.push(Arc::new(SheetSketch { id: shared_id, owner: Owner::Sheet(d.sheets[0].id), name: format!("Outline {i}"), sketch }));
        d.validate().unwrap();
    }
    doc
}

#[test]
fn sheet_sketches_round_trip_through_real_container_and_plain_design() {
    let source = design_with_sheet_sketches();
    let container_bytes = write_design_container(&source).unwrap();
    let plain_bytes = write_design(&source).unwrap();
    for bytes in [&container_bytes, &plain_bytes] {
        let reopened = read_design(bytes).unwrap();
        assert_eq!(reopened.drawings, source.drawings);
        for (i, d) in reopened.drawings.iter().enumerate() {
            let record = &d.sketches[0];
            assert_eq!(record.owner, solidcraft_doc::drawing::Owner::Sheet(d.sheets[0].id));
            assert_eq!(record.name, format!("Outline {i}"));
            assert_eq!(record.sketch.points[1].pos.x, 15.0 + i as f64);
            assert_eq!(record.sketch.points[2].pos.x - record.sketch.points[1].pos.x, 40.0);
            assert_eq!(record.sketch.points[3].pos.y - record.sketch.points[2].pos.y, 20.0);
            assert_eq!(record.sketch.points[1].exprs.as_ref().unwrap().x.as_deref(), Some("Inset + 5 mm"));
            assert_eq!(record.sketch.constraints[0].param.as_deref(), Some("Width"));
            assert_eq!(d.sheet_sketch(record.id).unwrap().name, format!("Outline {i}"));
        }
        assert_eq!(write_design_container(&reopened).unwrap(), container_bytes);
        assert_eq!(write_design(&reopened).unwrap(), plain_bytes);
    }
    let entries = parts(&container_bytes);
    for d in &source.drawings {
        assert_eq!(entries[&entry(d.id)], d.to_json().unwrap().as_bytes());
    }
}

#[test]
fn a_sheet_sketch_change_does_not_rewrite_model_or_another_drawings_entry() {
    let source = design_with_sheet_sketches();
    let before = parts(&write_design_container(&source).unwrap());
    let mut changed = source.clone();
    let drawing = Arc::make_mut(&mut changed.drawings[0]);
    let sketch = Arc::make_mut(&mut drawing.sketches[0]);
    sketch.name = "Revised outline".into();
    sketch.sketch.points[1].exprs.as_mut().unwrap().x = Some("Inset + 6 mm".into());
    drawing.validate().unwrap();
    let after = parts(&write_design_container(&changed).unwrap());
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    let own_entry = entry(source.drawings[0].id);
    for (name, bytes) in &before {
        if *name == own_entry {
            assert_ne!(bytes, &after[name]);
        } else {
            assert_eq!(bytes, &after[name], "a sketch edit rewrote {name}");
        }
    }
    assert_eq!(after["document.json"], before["document.json"]);
    assert_eq!(after["container.json"], before["container.json"]);
    let reopened = read_design(&write_design_container(&changed).unwrap()).unwrap();
    assert_eq!(reopened.drawings, changed.drawings);
    assert_eq!(source.drawings[0].sketches[0].name, "Outline 0", "copy-on-write changed the original");
}

#[test]
fn a_bad_sheet_sketch_refuses_the_whole_container_without_owner_fallback() {
    let source = design_with_sheet_sketches();
    let original = write_design_container(&source).unwrap();
    let good_parts = parts(&original);
    let own_entry = entry(source.drawings[0].id);
    let good: Value = serde_json::from_slice(&good_parts[&own_entry]).unwrap();
    let foreign_sheet = source.drawings[1].sheets[0].id;
    for (pointer, value) in [
        ("/sketches/0/owner/sheet", json!(ids::encode(foreign_sheet))),
        ("/sketches/0/id", json!(ids::encode(source.drawings[0].id))),
        ("/sketches/0/sketch/curves/0/a", json!(99_999)),
        ("/sketches/0/sketch/points/1/id", json!("origin")),
        ("/sketches/0/sketch/points/1/pos/0", json!(1e100)),
        ("/sketches/0/sketch/links", json!([null])),
    ] {
        let mut record = good.clone();
        if pointer == "/sketches/0/sketch/links" {
            // Empty links are omitted on write; a hostile file can add them.
            record["sketches"][0]["sketch"]["links"] = value;
        } else {
            *record.pointer_mut(pointer).unwrap() = value;
        }
        let mut corrupt = good_parts.clone();
        corrupt.insert(own_entry.clone(), serde_json::to_vec(&record).unwrap());
        let error = read_design(&container(&corrupt)).unwrap_err().to_string();
        assert!(error.contains(&own_entry), "missing drawing context for {pointer}: {error}");
        let mut inline: Value = serde_json::from_slice(&write_design(&source).unwrap()).unwrap();
        inline["drawings"][0] = record;
        assert!(read_design(&serde_json::to_vec(&inline).unwrap()).is_err(), "plain JSON accepted {pointer}");
    }
    assert_eq!(read_design(&original).unwrap().drawings, source.drawings);
    assert_eq!(parts(&original), good_parts, "hostile-input checks changed their source");
}

/// A design saved by main with every kind of binary part, opened and saved again by the last
/// release, 0.45.0 (`tests/fixtures/mixed-version`, which says what that release does): main reads
/// back what it saved, every part byte for byte, the drawing's images and logo too.
#[test]
fn what_the_last_release_saves_back_main_reads_whole() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-version");
    let ours = std::fs::read(fixtures.join("saved-by-main.sc")).unwrap();
    let theirs = std::fs::read(fixtures.join("saved-again-by-0.45.0.sc")).unwrap();
    let (before, after) = (parts(&ours), parts(&theirs));
    for (name, bytes) in before.iter().filter(|(name, _)| name.starts_with("blobs/") || name.starts_with("plugins/")) {
        assert_eq!(after.get(name), Some(bytes), "{name} kept byte for byte");
    }
    let (ours, theirs) = (read_design(&ours).unwrap(), read_design(&theirs).unwrap());
    assert!(theirs.newer_file.is_none(), "what it saved holds nothing this build can't read");
    assert_eq!(theirs.plugin_data, ours.plugin_data);
    assert_eq!((&theirs.canvases, &theirs.point_clouds, &theirs.render), (&ours.canvases, &ours.point_clouds, &ours.render));
    assert_eq!(theirs.drawings, ours.drawings);
    let d = &theirs.drawings[0];
    assert!(d.sheet_images.iter().next().is_some() && d.logos().next().is_some(), "the placed image and the logo");
}

/// What a release that can't read image parts (0.45.0) needs of a file to open it and keep it all:
/// each drawing's image records, placements and logos in fields it sets aside (no `images`, no
/// `sheet_images`, no logo in a title-block cell), and every part the design holds named in
/// `document.json`, the only place that release looks before it keeps a part.
#[test]
fn a_design_keeps_the_layout_the_last_release_reads() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-version/saved-by-main.sc");
    let doc = read_design(&std::fs::read(fixture).unwrap()).unwrap();
    let saved = parts(&write_design_container(&doc).unwrap());
    let metadata = String::from_utf8(saved["document.json"].clone()).unwrap();
    for name in saved.keys().filter(|name| name.starts_with("blobs/")) {
        assert!(metadata.contains(name.as_str()), "document.json names {name}");
    }
    let record: Value = serde_json::from_slice(&saved[&entry(doc.drawings[0].id)]).unwrap();
    assert!(record.get("images").is_none() && record.get("sheet_images").is_none(), "{record}");
    assert!(record["image_files"].is_array() && record["placed_images"].is_array() && record["logo_cells"].is_array(), "{record}");
    let cells = record.pointer("/sheets/0/title_block/cells").and_then(Value::as_array).unwrap();
    assert!(cells.iter().all(|cell| cell.pointer("/kind/text/image").is_none()), "a logo cell keeps its text only");
}

#[test]
fn a_readable_newer_drawing_keeps_edits_and_opaque_records_in_plain_json() {
    let doc = plate();
    let mut value: Value = serde_json::from_slice(&write_design(&doc).unwrap()).unwrap();
    let record = &mut value["drawings"][0];
    record["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
    let unknown = json!([{"id": ids::encode(23), "owner": {"sheet": record["sheets"][0]["id"]}, "geometry": "a later sketch"}]);
    record["sketches"] = unknown.clone();
    let mut opened = read_design(&serde_json::to_vec(&value).unwrap()).unwrap();
    let mut drawing = (*opened.drawings[0]).clone();
    let id = drawing.id;
    assert!(drawing.sketches.is_empty());
    drawing.name = "Renamed".into();
    opened.put_drawing(Arc::new(drawing));

    let written = write_design(&opened).unwrap();
    let saved: Value = serde_json::from_slice(&written).unwrap();
    let saved = saved["drawings"].as_array().unwrap().iter().find(|record| record["id"] == ids::encode(id)).unwrap();
    assert_eq!(saved["sketches"], unknown);
    assert_eq!(saved["name"], "Renamed");
    assert_eq!(read_design(&written).unwrap().drawing(id).unwrap().name, "Renamed");
}

#[test]
fn a_newer_drawing_keeps_opaque_record_text_in_every_file_form() {
    let doc = plate();
    let design = write_design(&doc).unwrap();
    let mut value: Value = serde_json::from_slice(&design).unwrap();
    let record = &mut value["drawings"][0];
    let id = doc.drawings[0].id;
    record["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
    record["sketches"] = json!(["opaque fixture"]);
    let opaque = r#"{ "text" : "\u0041", "wide" : 18446744073709551617,
 "precision": 0.123456789012345678901234567890, "pairs": [ 3,2 ] }"#;
    let drawing_text = record.to_string().replace(r#""opaque fixture""#, opaque);
    let rename = |mut d: Drawing| {
        assert!(d.sketches.is_empty());
        d.name = "Renamed".into();
        d
    };
    let drawing = rename(read_drawing(drawing_text.as_bytes()).unwrap());
    let saved = drawing.to_json().unwrap();
    assert!(saved.contains(opaque), "{saved}");
    assert_eq!(read_drawing(saved.as_bytes()).unwrap().name, "Renamed");
    let written = parts(&write_drawing_container(&drawing).unwrap());
    let text = std::str::from_utf8(&written["drawing.json"]).unwrap();
    assert!(text.contains(opaque), "{text}");
    let mut standalone = written;
    standalone.insert("drawing.json".into(), drawing_text.as_bytes().to_vec());
    let reopened = read_drawing(&container(&standalone)).unwrap();
    assert!(reopened.to_json().unwrap().contains(opaque));

    for newer_design in [false, true] {
        let mut design_record = value.clone();
        if newer_design {
            let design = solidcraft_doc::format::DESIGN;
            design_record["format"] = json!(design.format_string(design.current() + 1));
            design_record["foreign_design_data"] = json!({"example": [1, 2, 3]});
        }
        let design_text = design_record.to_string().replace(r#""opaque fixture""#, opaque);
        let mut opened = read_design(design_text.as_bytes()).unwrap();
        assert_eq!(opened.newer_file.is_some(), newer_design);
        opened.put_drawing(Arc::new(rename((*opened.drawings[0]).clone())));
        let saved = write_design(&opened).unwrap();
        let text = std::str::from_utf8(&saved).unwrap();
        assert!(text.contains(opaque), "{text}");
        assert_eq!(read_design(&saved).unwrap().drawing(id).unwrap().name, "Renamed");
        let saved: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(saved["foreign_design_data"], design_record["foreign_design_data"]);
    }

    let mut packed = parts(&write_design_container(&doc).unwrap());
    packed.insert(entry(id), drawing_text.into_bytes());
    let mut opened = read_design(&container(&packed)).unwrap();
    opened.put_drawing(Arc::new(rename((*opened.drawings[0]).clone())));
    let saved = write_design_container(&opened).unwrap();
    let packed = parts(&saved);
    assert!(std::str::from_utf8(&packed[&entry(id)]).unwrap().contains(opaque));
    assert_eq!(read_design(&saved).unwrap().drawing(id).unwrap().name, "Renamed");
}

#[test]
fn a_newer_drawing_keeps_unreadable_images_placements_and_container_parts() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-version/saved-by-main.sc");
    let original = parts(&std::fs::read(fixture).unwrap());
    let name = original.keys().find(|name| name.starts_with("drawings/")).unwrap();
    let record: Value = serde_json::from_slice(&original[name]).unwrap();
    let payload = b"later drawing payload";
    let blob = solidcraft_doc::blob::part_name(&solidcraft_doc::blob::Blob::from(&payload[..]), "future");
    let keyed_payload = b"later drawing payload named in a key";
    let keyed_blob = solidcraft_doc::blob::part_name(&solidcraft_doc::blob::Blob::from(&keyed_payload[..]), "future");

    for (key, kind) in [("image_files", "image"), ("placed_images", "placed image"), ("logo_cells", "title block logo")] {
        let mut entries = original.clone();
        let mut record = record.clone();
        record["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
        record[key][0]["future_channel"] = json!({"part": blob, (keyed_blob.as_str()): {"enabled": true}});
        entries.insert(name.clone(), serde_json::to_vec(&record).unwrap());
        entries.insert(blob.clone(), payload.to_vec());
        entries.insert(keyed_blob.clone(), keyed_payload.to_vec());

        let opened = read_design(&container(&entries)).unwrap();
        let drawing = &opened.drawings[0];
        assert!(drawing.newer_file.as_ref().unwrap().not_understood.iter().any(|what| what.starts_with(kind)), "{key}");
        assert_eq!(drawing.images.iter().count(), 1);
        assert_eq!(drawing.sheet_images.is_empty(), key == "placed_images");
        assert_eq!(drawing.logos().next().is_none(), key != "placed_images");
        assert!(write_design(&opened).is_err(), "plain JSON cannot carry the retained parts");
        let mut resaved = parts(&write_design_container(&opened).unwrap());
        let mut again: Value = serde_json::from_slice(&resaved[name]).unwrap();
        for field in ["image_files", "placed_images", "logo_cells"] {
            assert_eq!(again[field], record[field], "{key}: {field}");
        }
        for image in record["image_files"].as_array().unwrap() {
            let part = image["part"].as_str().unwrap();
            assert_eq!(resaved[part], entries[part]);
        }
        assert_eq!(resaved[&blob], payload);
        assert_eq!(resaved[&keyed_blob], keyed_payload);
        assert!(read_design(&container(&resaved)).unwrap().drawings[0].newer_file.is_some());

        // A reader that understands the extra field can read all the records after our save.
        again[key][0].as_object_mut().unwrap().remove("future_channel");
        again["format"] = json!(DRAWING.format_string(DRAWING.current()));
        resaved.insert(name.clone(), serde_json::to_vec(&again).unwrap());
        let reopened = read_design(&container(&resaved)).unwrap();
        assert_eq!(reopened.drawings[0].images.iter().count(), 2);
        assert!(reopened.drawings[0].logos().next().is_some());
    }
}

/// A drawing a newer build saved that this build can't read at all keeps the rest of the design
/// from nothing: the design opens without it, the notice names it, and saving writes its entry and
/// the parts it names back byte for byte. A drawing this build's own format calls damaged still
/// refuses the design, as damage does.
#[test]
fn a_newer_builds_unreadable_drawing_is_kept_and_the_rest_opens() {
    let doc = plate();
    let mut entries = parts(&write_design_container(&doc).unwrap());
    let name = entry(doc.drawings[1].id);
    let mut record: Value = serde_json::from_slice(&entries[&name]).unwrap();
    let payload = b"a future drawing's part".to_vec();
    let blob = format!("blobs/{}.future", "ab".repeat(32));
    let keyed_payload = b"a future drawing's part named in a key".to_vec();
    let keyed_blob = solidcraft_doc::blob::part_name(&solidcraft_doc::blob::Blob::from(keyed_payload.as_slice()), "future");
    record["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
    record["hologram"] = json!({"part": blob, (keyed_blob.as_str()): {"layers": 3}});
    record["sheets"] = json!("in a shape this build can't read");
    let future = serde_json::to_vec(&record).unwrap();
    entries.insert(name.clone(), future.clone());
    entries.insert(blob.clone(), payload.clone());
    entries.insert(keyed_blob.clone(), keyed_payload.clone());

    let opened = read_design(&container(&entries)).unwrap();
    assert_eq!(opened.drawings.len(), 1, "the drawing it can read");
    assert_eq!(opened.drawings[0].id, doc.drawings[0].id);
    let said = &opened.newer_file.as_ref().unwrap().not_understood;
    assert!(said.iter().any(|what| what.contains("drawing `Details`") && what.contains("newer SolidCraft")), "{said:?}");
    let resaved = parts(&write_design_container(&opened).unwrap());
    assert_eq!(resaved.get(&name), Some(&future), "the drawing's entry, as it came");
    assert_eq!(resaved.get(&blob), Some(&payload), "the part it names, as it came");
    assert_eq!(resaved.get(&keyed_blob), Some(&keyed_payload), "the part named by a key, as it came");
    // Reopened, it is kept again.
    assert_eq!(read_design(&container(&resaved)).unwrap().drawings.len(), 1);

    // Damage in this build's own format is not a newer drawing: the design is refused.
    let mut damaged = parts(&write_design_container(&doc).unwrap());
    let mut record: Value = serde_json::from_slice(&damaged[&name]).unwrap();
    record["sheets"] = json!("broken");
    damaged.insert(name, serde_json::to_vec(&record).unwrap());
    assert!(read_design(&container(&damaged)).is_err());
}

/// A drawing file (`.scdwg`) saved by main, its placed image and logo parts of it, reads back whole.
/// The last release can't open one at all: it refuses it, by name, and writes nothing
/// (`tests/fixtures/mixed-version/README.md`), so it never rewrites one without its parts.
#[test]
fn a_drawing_file_saved_by_main_reads_back_with_its_images() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-version/drawing-saved-by-main.scdwg");
    let bytes = std::fs::read(&fixture).unwrap();
    let d = read_drawing(&bytes).unwrap();
    assert!(d.sheet_images.iter().next().is_some() && d.logos().next().is_some(), "the placed image and the logo");
    assert!(d.images.iter().all(|i| !i.data.is_empty()), "every image has its bytes");
    let again = write_drawing_container(&d).unwrap();
    assert_eq!(read_drawing(&again).unwrap(), d);
}
