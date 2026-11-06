use serde_json::json;

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let schema = output_schema_for::<TableResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply) || id == DELETE, "{id}: {reply}");
    reply
}

/// `id` refuses `p`, leaving the drawing and the undo steps as they were: why.
fn refused(s: &mut Session, id: &str, p: Value) -> String {
    let (record, steps) = (s.drawing("test").ok().cloned(), s.undo.len());
    let message = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} accepted {p}"));
    assert_eq!((s.drawing("test").ok().cloned(), s.undo.len()), (record, steps), "{id} {p}");
    message
}

/// A plate and a bolt placed twice in the design, its Bolt inside a component, and a drawing, open.
fn assembly() -> Session {
    let mut s = Session::default();
    for (name, part) in [("Plate", "P-100"), ("Bolt", "B-200")] {
        s.execute("component.create", &json!({"name": name})).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": name})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        s.execute("component.metadata.set", &json!({"component": name, "part_number": part})).unwrap();
    }
    s.execute("occurrence.copy", &json!({"component": "Bolt", "translate": [0, 50, 0]})).unwrap();
    s.execute("drawing.create", &json!({})).unwrap();
    s
}

fn sheet_of(s: &Session) -> String {
    ids::encode(s.drawing("test").unwrap().sheets[0].id)
}

fn id_of(reply: &Value) -> u64 {
    ids::decode(reply["table"].as_str().unwrap()).unwrap()
}

/// The new parts list starts as the plan says: top right, headings on top, QTY, rows of 7 mm, listing
/// the design. A revision table starts at the bottom left. Each is one undo step, and reads back.
#[test]
fn a_new_table_has_its_defaults_and_saves() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let steps = s.undo.len();
    let list = run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}));
    assert_eq!(s.undo.len(), steps + 1);
    let record = &list["record"];
    assert_eq!(record["place"], json!({"anchor": {"corner": "top_right", "offset_mm": [0.0, 0.0]}}), "{record}");
    assert_eq!((&record["table"]["heading"], &record["table"]["row_height_mm"]), (&json!("top"), &json!(7.0)));
    assert_eq!(record["table"]["columns"][1]["heading"], json!("QTY"));
    assert_eq!((&record["table"]["columns"][6]["name"], &record["table"]["columns"][6]["printed"]), (&json!("ballooned"), &json!(false)));
    // Never on paper: until the sheet draws tables (P8) this guards that it never draws this one.
    let svg = s.execute("drawing.export", &json!({"sheet": sheet, "format": "svg"})).unwrap();
    assert!(!svg["svg"].as_str().unwrap().to_lowercase().contains("ballooned"), "{svg}");
    assert_eq!(record["kind"]["parts_list"]["of"]["component"], json!(0));
    assert_eq!(record["kind"]["parts_list"]["mass"], json!({"unit": "kg", "decimals": 2}));
    let revisions = run(&mut s, ADD, json!({"sheet": "Sheet 1", "kind": "revision"}));
    assert_eq!(revisions["record"]["place"]["anchor"]["corner"], json!("bottom_left"), "{revisions}");
    assert_eq!(revisions["record"]["kind"], json!("revision"));
    assert_eq!(revisions.get("warnings"), None);

    let d = s.drawing("test").unwrap().clone();
    assert_eq!(d.tables.len(), 2);
    assert_eq!(Drawing::from_json(&d.to_json().unwrap()).unwrap(), *d);
    let listed = s.execute("drawing.inspect", &json!({})).unwrap();
    assert_eq!(listed["sheets"][0]["tables"].as_array().map(Vec::len), Some(2), "{listed}");

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.drawing("test").unwrap().tables.len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.drawing("test").unwrap().clone(), *d);
}

/// A list's sheet documents the component it starts with.
#[test]
fn a_new_list_lists_what_its_sheet_documents() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let bolt = s.doc.find_component("Bolt").unwrap();
    let source = ids::encode(s.drawing("test").unwrap().sources[0].id);
    s.execute("drawing.sheet.set", &json!({"sheet": sheet, "documents": {"source": source, "component": bolt}})).unwrap();
    let list = run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}));
    assert_eq!(list["record"]["kind"]["parts_list"]["of"]["component"], json!(bolt), "{list}");
    let named = run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list", "set": {"list": {"component": "root"}}}));
    assert_eq!(named["record"]["kind"]["parts_list"]["of"]["component"], json!(0), "{named}");
}

/// Placing a list saves what its view shows, once: the same numbers, one numbering however many lists
/// show the view, and the step after it doesn't save them again. A list of another view numbers that
/// view. An undo puts back the drawing and keeps the numbers, as a drawing's step puts back only the
/// drawing; no model is evaluated for it.
#[test]
fn placing_a_list_keeps_the_numbers_its_view_shows() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    // A model read, so with the drawing closed for it.
    let shown = |s: &mut Session| {
        let drawing = ids::encode(s.open_drawing.unwrap());
        s.execute("drawing.close", &json!({})).unwrap();
        let rows = s.execute("assembly.bom", &json!({"levels": "flat"})).unwrap()["rows"].clone();
        s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
        rows
    };
    let before = shown(&mut s);
    assert!(s.doc.assembly.item_numbers.is_empty());
    let (revision, model) = (s.revision, s.model.state());
    run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}));
    assert!(Arc::ptr_eq(&model, &s.model.state()), "the model isn't evaluated");
    assert!(s.revision > revision);
    assert_eq!(s.doc.assembly.item_numbers.len(), 1);
    let rows = |v: &Value| v.as_array().unwrap().iter().map(|r| (r["item"].clone(), r["part_number"].clone())).collect::<Vec<_>>();
    assert_eq!(rows(&shown(&mut s)), rows(&before), "what it showed");

    let numbers = s.doc.assembly.item_numbers.clone();
    let second = run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}));
    assert_eq!(s.doc.assembly.item_numbers, numbers, "one numbering for one view");
    let (one, two) = (s.drawing("test").unwrap().tables[0].kind.clone(), s.drawing("test").unwrap().tables[1].kind.clone());
    assert_eq!(one, two, "{second}");

    run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list", "set": {"list": {"levels": "top"}}}));
    assert_eq!(s.doc.assembly.item_numbers.len(), 2);
    let edited = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    run(&mut s, EDIT, json!({"table": ids::encode(edited), "set": {"list": {"levels": "all"}}}));
    assert_eq!(s.doc.assembly.item_numbers.len(), 3, "an edit to another view numbers that one");

    let numbers = s.doc.assembly.item_numbers.clone();
    while !s.drawing("test").unwrap().tables.is_empty() {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(s.doc.assembly.item_numbers, numbers, "the design's, not the drawing's");
}

/// A revision table numbers nothing, and a list a renumbered view keeps its numbers.
#[test]
fn a_saved_numbering_stays_as_it_is() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    run(&mut s, ADD, json!({"sheet": sheet, "kind": "revision"}));
    assert!(s.doc.assembly.item_numbers.is_empty());
    s.execute("drawing.close", &json!({})).unwrap();
    s.execute("assembly.bom.renumber", &json!({"levels": "flat", "start": 10, "step": 10})).unwrap();
    let numbers = s.doc.assembly.item_numbers.clone();
    let drawing = ids::encode(s.doc.drawings[0].id);
    s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
    run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}));
    assert_eq!(s.doc.assembly.item_numbers, numbers);
}

/// An edit changes what it names and nothing else, one step, none when nothing changes. A table moves
/// between sheets; a change of paper moves nothing in the record. A sheet's delete counts its tables
/// and takes them, leaving the others'.
#[test]
fn a_table_edits_moves_and_goes_with_its_sheet() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let id = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    let table = ids::encode(id);
    let set = |s: &mut Session, set: Value| run(s, EDIT, json!({"table": table, "set": set}));

    let edited = set(&mut s, json!({"heading": "bottom", "row_height_mm": 9.0, "place": {"at": [20.0, 30.0]}}));
    assert_eq!((&edited["record"]["table"]["heading"], &edited["record"]["place"]), (&json!("bottom"), &json!({"at": [20.0, 30.0]})), "{edited}");
    assert_eq!(edited["record"]["table"]["columns"][1]["heading"], json!("QTY"), "the rest stays");
    let steps = s.undo.len();
    set(&mut s, json!({"heading": "bottom"}));
    assert_eq!(s.undo.len(), steps, "nothing changed, no step");

    let filter = json!({"only": "parts", "text": [{"column": "name", "contains": "bolt"}]});
    let overrides = json!([{"row": "a-key", "column": "name", "text": "Typed"}]);
    let edited = set(&mut s, json!({"filter": filter, "overrides": overrides}));
    assert_eq!((&edited["record"]["table"]["filter"], &edited["record"]["table"]["overrides"]), (&filter, &overrides), "{edited}");

    let before = s.drawing("test").unwrap().tables.by_id(id).unwrap().clone();
    s.execute("drawing.sheet.set", &json!({"sheet": sheet, "size": {"standard": "a1"}})).unwrap();
    assert_eq!(*s.drawing("test").unwrap().tables.by_id(id).unwrap(), before, "a change of paper moves nothing");

    let other = s.execute("drawing.sheet.add", &json!({"name": "Other"})).unwrap()["sheet"].as_str().unwrap().to_string();
    let moved = set(&mut s, json!({"sheet": "Other"}));
    assert_eq!(moved["sheet"], json!(other), "{moved}");
    let held = s.execute("drawing.sheet.delete", &json!({"sheet": other})).unwrap_err().to_string();
    assert!(held.contains("1 table"), "{held}");
    s.execute("drawing.sheet.delete", &json!({"sheet": other, "contents": "delete"})).unwrap();
    assert!(s.drawing("test").unwrap().tables.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    let id = s.drawing("test").unwrap().tables[0].id;
    let deleted = run(&mut s, DELETE, json!({"table": ids::encode(id)}));
    assert_eq!(deleted, json!({"table": ids::encode(id)}));
    assert!(s.drawing("test").unwrap().tables.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.drawing("test").unwrap().tables.len(), 1);
}

/// A table's id is drawn from the ids the drawing shares, and a damaged file's table is refused.
#[test]
fn hostile_tables_are_refused_with_nothing_changed() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let table = ids::encode(id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}))));
    let revisions = ids::encode(id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "revision"}))));
    for bad in [
        json!({"sheet": "no such", "kind": "revision"}),
        json!({"sheet": sheet, "kind": "chart"}),
        json!({"sheet": sheet}),
        json!({"sheet": sheet, "kind": "revision", "set": {"list": {"levels": "flat"}}}),
        json!({"sheet": sheet, "kind": "parts_list", "set": {"list": {"component": "no such"}}}),
    ] {
        refused(&mut s, ADD, bad);
    }
    let long = "x".repeat(70_000);
    for bad in [
        json!({"columns": []}),
        json!({"columns": "x"}),
        json!({"columns": [{"name": "A b", "heading": "A", "content": []}]}),
        json!({"columns": [{"name": "a", "heading": long, "content": []}]}),
        json!({"heading": "sideways"}),
        // Read as given, null reaches the table and is refused there: it changes nothing.
        json!({"heading": null}),
        json!({"row_height_mm": 0.0}),
        json!({"row_height_mm": -7.0}),
        json!({"row_height_mm": 1e300}),
        json!({"cap_mm": 8.0}),
        json!({"filter": {"text": [{"column": "nope", "contains": "x"}]}}),
        json!({"overrides": [{"row": "a", "column": "nope", "text": "x"}]}),
        json!({"place": {"at": [1e300, 0.0]}}),
        json!({"place": {"at": [0.0]}}),
        json!({"place": {"anchor": {"corner": "middle"}}}),
        json!({"place": {"anchor": {"corner": "top_left", "offset_mm": [-6000.0, 0.0]}}}),
        json!({"sheet": "no such"}),
        json!({"list": {"levels": "some"}}),
        json!({"list": {"group_by": "colour"}}),
        json!({"list": {"group_children": "yes"}}),
        json!({"list": {"mass": {"unit": "stone", "decimals": 2}}}),
        json!({"list": {"mass": {"unit": "kg", "decimals": 7}}}),
        json!({"list": {"component": [1]}}),
    ] {
        refused(&mut s, EDIT, json!({"table": table, "set": bad}));
    }
    assert!(refused(&mut s, EDIT, json!({"table": revisions, "set": {"list": {}}})).contains("parts list"));
    refused(&mut s, EDIT, json!({"table": "0000000000000", "set": {}}));
    refused(&mut s, DELETE, json!({"table": "0000000000000"}));
    s.drawing("test").unwrap().validate().unwrap();
}

/// Too large a design to list still places the list, says so, and saves no numbering.
#[test]
fn a_view_that_cant_be_numbered_still_places_its_list() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let mut doc = (*s.doc).clone();
    let Some(first) = doc.occurrences.first().cloned() else { panic!("an occurrence") };
    doc.occurrences =
        (0..=solidcraft_doc::MAX_PLACEMENTS as u64).map(|i| solidcraft_doc::Occurrence { id: 1_000_000 + i, ..first.clone() }).collect();
    s.doc = Arc::new(doc);
    let reply = run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"}));
    assert!(reply["warnings"][0].as_str().is_some_and(|w| w.contains("can still move")), "{reply}");
    assert!(s.doc.assembly.item_numbers.is_empty());
    assert_eq!(s.drawing("test").unwrap().tables.len(), 1);
}

/// Recipes 3, 5 and 6 of #11773 on the W23 assembly sheet, from its command script. An assembly's
/// drawing starts on A2. The first-level parts list, placed at (330, 60), keeps items 1, 2 and 3 for
/// BP-100 1, HL-300 2 and SC-M6 4; with the second holder deleted only HL-300 drops to 1, and undo
/// brings it back. Revision rows 1, 2 and 3 with row 2 deleted leave 1 and 3, next 4, and a tag on 3
/// at (200, 120) shows 3. Moving the second holder changes none of it, nor the drawing's record. The
/// sheet exports the same SVG after save and reopen. Every model edit is made with the drawing closed.
#[test]
fn the_assembly_sheet_keeps_its_parts_list_revisions_and_tag_through_the_model() {
    let script: Value = serde_json::from_str(include_str!("../../../../tests/fixtures/w23-assembly-sheet/assembly.json")).unwrap();
    let mut s = Session::default();
    let made = s.run_script(&script).unwrap();
    let step = script["commands"].as_array().unwrap().iter().position(|c| c["as"] == "second_holder").unwrap();
    let second = made[step]["occurrence"].as_u64().unwrap();
    let exec = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));

    exec(&mut s, "assembly.bom.renumber", json!({"levels": "top"}));
    let created = exec(&mut s, "drawing.create", json!({}));
    assert_eq!(created["sheets"][0]["size_mm"], json!([594.0, 420.0]), "an assembly's drawing starts on A2: {created}");
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let sheet = sheet_of(&s);
    exec(&mut s, "drawing.view.base", json!({"sheet": "Sheet 1", "view": "top", "scale": "1:2", "position_mm": [150, 150]}));
    let list =
        run(&mut s, ADD, json!({"sheet": "Sheet 1", "kind": "parts_list", "set": {"list": {"levels": "top"}, "place": {"at": [330.0, 60.0]}}}));
    let list = list["table"].clone();
    run(&mut s, ADD, json!({"sheet": "Sheet 1", "kind": "revision"}));
    let mut rows = Vec::new();
    for (description, date) in [("First issue", "2026-11-03"), ("Withdrawn", "2026-11-04"), ("Holder moved", "2026-11-05")] {
        rows.push(exec(&mut s, "drawing.revision.add", json!({"set": {"description": description, "designer": "BS", "date": date}}))["row"].clone());
    }
    let three = exec(&mut s, "drawing.inspect", json!({}))["revisions"].clone();
    let numbers = |r: &Value| r["rows"].as_array().unwrap().iter().map(|r| r["revision"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!((numbers(&three), three["next"].as_str()), (vec!["1".to_string(), "2".into(), "3".into()], Some("4")), "{three}");
    exec(&mut s, "drawing.revision.delete", json!({"row": rows[1]}));
    let tag = exec(&mut s, "drawing.revision.tag.add", json!({"row": rows[2], "sheet": sheet, "at": [200.0, 120.0]}))["tag"].clone();

    let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    // The list's items as the drawing has them, its revisions, and the drawing's record.
    let printed = |s: &mut Session| {
        let inspect = exec(s, "drawing.inspect", json!({}));
        let on_list = inspect["parts_lists"].as_array().unwrap().iter().find(|l| l["table"] == list).cloned().unwrap_or_else(|| panic!("{inspect}"));
        let items: Vec<String> = on_list["ballooned"].as_array().unwrap_or_else(|| panic!("{on_list}")).iter().map(|r| text(&r["item"])).collect();
        (items, inspect["revisions"].clone(), s.drawing("test").unwrap().to_json().unwrap())
    };
    // The first level as the model counts it: item, part number, count. A model read, so with the drawing closed.
    let counted = |s: &mut Session| {
        exec(s, "drawing.close", json!({}));
        let top = exec(s, "assembly.bom", json!({"levels": "top"}));
        exec(s, "drawing.open", json!({"drawing": drawing}));
        let rows = top["rows"].as_array().unwrap().iter();
        rows.map(|r| (text(&r["item"]), r["part_number"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap())).collect::<Vec<_>>()
    };
    let on_the_model = |s: &mut Session, id: &str, p: Value| {
        exec(s, "drawing.close", json!({}));
        exec(s, id, p);
        exec(s, "drawing.open", json!({"drawing": drawing}));
    };
    let first_level =
        |holders: u64| vec![("1".to_string(), "BP-100".to_string(), 1), ("2".into(), "HL-300".into(), holders), ("3".into(), "SC-M6".into(), 4)];

    let (items, revisions, record) = printed(&mut s);
    assert_eq!(items, ["1", "2", "3"], "the list's items on the sheet");
    assert_eq!(counted(&mut s), first_level(2), "the hand counts");
    assert_eq!((numbers(&revisions), revisions["next"].as_str()), (vec!["1".to_string(), "3".into()], Some("4")), "{revisions}");
    let tags = &revisions["tags"];
    assert_eq!(
        (&tags[0]["tag"], &tags[0]["shows"], &tags[0]["lost"], &tags[0]["at"]),
        (&tag, &json!("3"), &json!(false), &json!([200.0, 120.0])),
        "{tags}"
    );

    // The second holder deleted: only HL-300 drops to 1, every item stays; undone, back to 2.
    on_the_model(&mut s, "selection.delete", json!({"occurrences": [second]}));
    assert_eq!(counted(&mut s), first_level(1), "the second holder deleted");
    assert_eq!(printed(&mut s), (items.clone(), revisions.clone(), record.clone()), "the drawing as it was");
    on_the_model(&mut s, "edit.undo", json!({}));
    assert_eq!(counted(&mut s), first_level(2), "undone");

    // The second holder moved 30 mm along Y: the list, the revisions, the tag and the record stay.
    on_the_model(&mut s, "occurrence.move", json!({"occurrence": second, "space": "world", "translate": [0, 30, 0]}));
    assert_eq!(counted(&mut s), first_level(2));
    assert_eq!(printed(&mut s), (items.clone(), revisions.clone(), record.clone()), "a model edit leaves the drawing alone");

    // Saved and opened again: the same SVG, list, revisions, tag and record.
    let svg = |s: &mut Session| exec(s, "drawing.export", json!({"sheet": "Sheet 1"}))["svg"].as_str().unwrap().to_string();
    let before = svg(&mut s);
    let folder = crate::tests::Folder::new("assembly-sheet-tables");
    let path = folder.path("assembly.solidcraft");
    exec(&mut s, "file.save", json!({"path": path}));
    let mut opened = Session::default();
    exec(&mut opened, "doc.open", json!({"path": path}));
    exec(&mut opened, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(svg(&mut opened), before, "the same SVG after save and reopen");
    assert_eq!(printed(&mut opened), (items, revisions, record), "opened again");
    assert_eq!(counted(&mut opened), first_level(2));
}
