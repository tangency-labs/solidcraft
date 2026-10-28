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

// W38a acceptance through the real design container, not only Drawing JSON.
fn design_with_sheet_sketches() -> Document {
    use solidcraft_doc::drawing::{SheetSketch, Units};
    use solidcraft_doc::sketch::{ConstraintKind, PointExprs, Sketch};
    use solidcraft_geom::Vec2;

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
        d.sketches.push(Arc::new(SheetSketch { id: shared_id, sheet: d.sheets[0].id, name: format!("Outline {i}"), sketch }));
        d.validate().unwrap();
    }
    doc
}

#[test]
fn sheet_sketches_round_trip_through_real_container_and_plain_design() {
    let source = design_with_sheet_sketches();
    let container_bytes = write_design_container(&source).unwrap();
    let plain_bytes = write_design(&source);
    for bytes in [&container_bytes, &plain_bytes] {
        let reopened = read_design(bytes).unwrap();
        assert_eq!(reopened.drawings, source.drawings);
        for (i, d) in reopened.drawings.iter().enumerate() {
            let record = &d.sketches[0];
            assert_eq!(record.sheet, d.sheets[0].id);
            assert_eq!(record.name, format!("Outline {i}"));
            assert_eq!(record.sketch.points[1].pos.x, 15.0 + i as f64);
            assert_eq!(record.sketch.points[2].pos.x - record.sketch.points[1].pos.x, 40.0);
            assert_eq!(record.sketch.points[3].pos.y - record.sketch.points[2].pos.y, 20.0);
            assert_eq!(record.sketch.points[1].exprs.as_ref().unwrap().x.as_deref(), Some("Inset + 5 mm"));
            assert_eq!(record.sketch.constraints[0].param.as_deref(), Some("Width"));
            assert_eq!(d.sheet_sketch(record.id).unwrap().name, format!("Outline {i}"));
        }
        assert_eq!(write_design_container(&reopened).unwrap(), container_bytes);
        assert_eq!(write_design(&reopened), plain_bytes);
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
        ("/sketches/0/sheet", json!(ids::encode(foreign_sheet))),
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
        let mut inline: Value = serde_json::from_slice(&write_design(&source)).unwrap();
        inline["drawings"][0] = record;
        assert!(read_design(&serde_json::to_vec(&inline).unwrap()).is_err(), "plain JSON accepted {pointer}");
    }
    assert_eq!(read_design(&original).unwrap().drawings, source.drawings);
    assert_eq!(parts(&original), good_parts, "hostile-input checks changed their source");
}
