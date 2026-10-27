use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use serde_json::{Value, json};
use solidcraft_doc::drawing::{MAX_DRAWINGS, Source};
use solidcraft_doc::format::DRAWING;
use solidcraft_doc::ids::IdSource;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};

use super::*;
use crate::{read_design, write_design, write_design_container};

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

/// What a container refuses in its drawings, each time the whole design: an entry under
/// `drawings/` that isn't a drawing's, one whose record has another id, a damaged drawing, one a
/// newer build saved, drawings inline in `document.json` beside the entries, and more entries than
/// a design may hold, counted before any is read.
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
    let why = refusal(&|p| {
        p.insert(entry(second), text(1).replace(&DRAWING.format_string(DRAWING.current()), &newer).into_bytes());
    });
    assert!(why.contains("this drawing was saved by a newer SolidCraft"), "{why}");
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
