use serde_json::json;
use solidcore_record::Vec2;

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

fn inspected(s: &mut Session, id: u64) -> Value {
    let reply = s.execute(INSPECT, &json!({"table": ids::encode(id)})).unwrap();
    let schema = output_schema_for::<InspectResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// The open drawing's first sheet drawn: what it says of table `id`, and the box round its lines.
fn on_sheet(s: &Session, id: u64) -> (crate::drawing_scene::TableState, Option<Aabb2>) {
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(s, &d, d.sheets[0].id).unwrap();
    let state = built.tables.iter().find(|t| t.table == id).cloned().unwrap();
    (state, built.objects.iter().find(|o| o.object == id).map(|o| o.bounds_mm))
}

/// Where the open drawing's first sheet anchors its tables.
fn space(s: &Session) -> Aabb2 {
    crate::drawing_table_paper::drawing_space(&s.drawing("test").unwrap().sheets[0])
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

/// Table `id` of `s`'s drawing with `change` made to its record, put in as a design file could hold it.
fn put_table(s: &mut Session, id: u64, change: impl FnOnce(&mut SheetTable)) {
    let mut d = (**s.drawing("test").unwrap()).clone();
    let at = d.tables.index_of(id).unwrap();
    change(Arc::make_mut(d.tables.get_mut(at).unwrap()));
    s.doc_mut().put_drawing(Arc::new(d));
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
    // Never on paper: the sheet draws the list, but never this column.
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

/// A parts list draws at the top right of the drawing space, its heading and two rows 7 mm each by
/// the defaults; a revision table with no rows yet at the bottom left. Each is its own kind of object
/// in the scene, and the scene and an export both say each one is drawn.
#[test]
fn a_table_draws_where_its_record_says() {
    use solidcraft_geom::paper::ObjectKind;

    let mut s = assembly();
    let sheet = sheet_of(&s);
    let list = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    let revisions = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "revision"})));
    let drawing = space(&s);

    let (state, bounds) = on_sheet(&s, list);
    assert_eq!((state.drawn, state.failed, &state.why), (true, false, &None), "{state:?}");
    let bounds = bounds.unwrap();
    assert!(near(bounds.max, drawing.max), "{bounds:?} {drawing:?}");
    assert!((bounds.size().y - 3.0 * 7.0).abs() < 1e-9 && bounds.size().x > 0.0, "a heading and two rows: {bounds:?}");
    let (state, bounds) = on_sheet(&s, revisions);
    assert!(state.drawn, "{state:?}");
    assert!(near(bounds.unwrap().min, drawing.min), "{bounds:?} {drawing:?}");
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(&s, &d, d.sheets[0].id).unwrap();
    // A sheet's tables paint in id order, and ids don't follow the order the tables were added.
    let kinds: Vec<_> = built.objects.iter().filter(|o| o.object == list || o.object == revisions).map(|o| (o.object, o.kind)).collect();
    let mut by_id = vec![(list, ObjectKind::PartsList), (revisions, ObjectKind::RevisionTable)];
    by_id.sort_by_key(|(id, _)| *id);
    assert_eq!(kinds, by_id);

    let schema = output_schema_for::<crate::schema::drawing_scene::SheetSceneResult>();
    let scene = s.execute("drawing.scene", &json!({"sheet": sheet})).unwrap();
    assert!(conforms(schema.as_value(), schema.as_value(), &scene), "{scene}");
    let words = |reply: &Value| reply["tables"].as_array().unwrap().iter().map(|t| (t["status"].clone(), t["drawn"].clone())).collect::<Vec<_>>();
    assert_eq!(words(&scene), vec![(json!("ok"), json!(true)); 2], "{scene}");
    let exported = s.execute("drawing.export", &json!({"sheet": sheet, "format": "svg"})).unwrap();
    assert_eq!(words(&exported), words(&scene), "{exported}");
    assert_eq!(exported["svg"], s.execute("drawing.export", &json!({"sheet": sheet, "format": "svg"})).unwrap()["svg"], "the same bytes");
}

/// An offset moves an anchored table; a heading at the bottom keeps the same corner of the outline;
/// a free place names the outline's top left. A change of paper takes an anchored table along and
/// leaves a free one where it was.
#[test]
fn a_tables_place_follows_its_record_and_the_paper() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let table = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    let free = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list", "set": {"place": {"at": [30, 200]}}})));
    let at = on_sheet(&s, table).1.unwrap();
    let was_free = on_sheet(&s, free).1.unwrap();
    assert!(near(Vec2::new(was_free.min.x, was_free.max.y), Vec2::new(30.0, 200.0)), "{was_free:?}");

    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"place": {"anchor": {"corner": "top_right", "offset_mm": [-5, -7]}}}}));
    let moved = on_sheet(&s, table).1.unwrap();
    assert!(near(moved.max, at.max + Vec2::new(-5.0, -7.0)), "{moved:?} {at:?}");
    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"heading": "bottom"}}));
    let bottom = on_sheet(&s, table).1.unwrap();
    assert!(near(bottom.max, moved.max) && near(bottom.min, moved.min), "the same outline: {bottom:?} {moved:?}");

    // An assembly's drawing starts on A2: the paper grows to A1, set as a user sets it.
    let a2 = space(&s);
    s.execute("drawing.sheet.set", &json!({"sheet": sheet, "size": {"standard": "a1"}})).unwrap();
    let grown = space(&s).max - a2.max;
    assert!(grown.x > 0.0, "{grown:?}");
    assert!(near(on_sheet(&s, table).1.unwrap().max, bottom.max + grown), "it follows the paper");
    assert_eq!(on_sheet(&s, free).1, Some(was_free), "a free one stays");
}

/// Tables are anchored inside a sheet's drawing border, and on its paper when it has none.
#[test]
fn a_tables_space_is_inside_the_drawing_border() {
    let s = assembly();
    let mut sheet = (*s.drawing("test").unwrap().sheets[0]).clone();
    let paper = sheet.sheet.size_mm().unwrap();
    let border = sheet.border.as_ref().unwrap().layout(&sheet.sheet).unwrap().drawing_border;
    let space = crate::drawing_table_paper::drawing_space(&sheet);
    assert_eq!((space.min, space.max), (border.min, border.max));
    assert!(space.min.x > 0.0 && space.max.x < paper.x, "{space:?}");
    sheet.border = None;
    let bare = crate::drawing_table_paper::drawing_space(&sheet);
    assert_eq!((bare.min, bare.max), (Vec2::ZERO, paper));
}

/// A list of a component the design no longer has can't be read: it says why and draws nothing, the
/// sheet's other table still draws, and inspect says the same.
#[test]
fn a_table_that_cant_be_read_says_why_and_the_sheet_still_draws() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let good = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    let bad = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    put_table(&mut s, bad, |table| {
        if let Kind::PartsList(list) = &mut table.kind {
            list.of.component = 999_999;
        }
    });
    let (state, bounds) = on_sheet(&s, bad);
    assert_eq!((state.drawn, state.failed, bounds), (false, true, None), "{state:?}");
    assert!(state.why.as_deref().is_some_and(|why| why.contains("999999")), "{state:?}");
    assert!(on_sheet(&s, good).0.drawn);
    let reply = inspected(&mut s, bad);
    assert_eq!((reply["status"].as_str(), reply.get("outline_mm")), (Some("failed"), None), "{reply}");
    assert_eq!(reply["why"].as_str(), state.why.as_deref());
    let scene = s.execute("drawing.scene", &json!({"sheet": sheet})).unwrap();
    let said = scene["tables"].as_array().unwrap().iter().find(|t| t["table"] == json!(ids::encode(bad))).cloned().unwrap();
    assert_eq!((said["status"].as_str(), said["drawn"].as_bool()), (Some("failed"), Some(false)), "{said}");
}

/// Inspect gives what a placed table shows: its outline (the scene's box round it), each row with its
/// key and rectangle, a cell an override changed with the value it replaced, named by its column even
/// past a column that isn't printed, a filter that leaves nothing, and a word wider than its column.
/// It changes nothing.
#[test]
fn inspect_says_where_a_table_is_and_what_it_shows() {
    let mut s = assembly();
    let sheet = sheet_of(&s);
    let table = id_of(&run(&mut s, ADD, json!({"sheet": sheet, "kind": "parts_list"})));
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let steps = s.undo.len();
    let reply = inspected(&mut s, table);
    assert_eq!((reply["status"].as_str(), reply["listed"].as_u64()), (Some("ok"), Some(2)), "{reply}");
    assert_eq!((s.drawing("test").unwrap().to_json().unwrap(), s.undo.len()), (before, steps));
    let bounds = on_sheet(&s, table).1.unwrap();
    let outline: Vec<f64> = reply["outline_mm"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(outline, [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]);
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let top = |row: &Value| row["rect_mm"][3].as_f64().unwrap();
    assert!((top(&rows[0]) - (outline[3] - 7.0)).abs() < 1e-9 && (top(&rows[1]) - (outline[3] - 14.0)).abs() < 1e-9, "{reply}");

    // DESCRIPTION, before MATERIAL, off the paper: the override still names MATERIAL.
    let mut columns = s.drawing("test").unwrap().tables.by_id(table).unwrap().table.columns.clone();
    columns.iter_mut().filter(|c| c.name == "description").for_each(|c| c.printed = false);
    let key = rows[0]["key"].as_str().unwrap().to_string();
    let overrides = json!([{"row": key, "column": "material", "text": "By hand"}]);
    let set = json!({"columns": serde_json::to_value(&columns).unwrap(), "overrides": overrides});
    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": set}));
    let reply = inspected(&mut s, table);
    let changed = &reply["overrides"][0];
    assert_eq!((&changed["row"], &changed["column"], &changed["text"]), (&json!(key), &json!("material"), &json!("By hand")), "{reply}");

    let filter = json!({"text": [{"column": "name", "contains": "no such part"}]});
    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"filter": filter, "overrides": []}}));
    let reply = inspected(&mut s, table);
    assert_eq!((reply["listed"].as_u64(), reply.get("rows")), (Some(2), None), "the filter leaves nothing: {reply}");

    columns.iter_mut().filter(|c| c.name == "name").for_each(|c| c.width_mm = Some(3.0));
    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"filter": {}, "columns": serde_json::to_value(&columns).unwrap()}}));
    let reply = inspected(&mut s, table);
    assert!(reply["warnings"].as_array().unwrap().iter().any(|w| w["column"] == "name" && w["warning"] == "width"), "{reply}");
}

/// The window's sheet keeps its tables until what they read changes: drawn as a build draws them,
/// read again for a table's edit and not for nothing, and showing a part's new name once it opens again.
#[test]
fn the_window_keeps_a_table_until_what_it_reads_changes() {
    let mut s = assembly();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let table = id_of(&run(&mut s, ADD, json!({"sheet": ids::encode(sheet), "kind": "parts_list"})));
    let settled = |s: &mut Session| {
        crate::drawing_scene::extract(s, sheet).unwrap();
        while crate::drawing_scene::step(s) {}
        let view = crate::drawing_scene::extract(s, sheet).unwrap();
        let d = s.drawing("test").unwrap().clone();
        assert_eq!(*view.scene, crate::drawing_scene::build(s, &d, sheet).unwrap().scene, "as a build draws it");
        view
    };
    let first = settled(&mut s);
    assert!(first.tables.iter().any(|t| t.table == table && t.drawn), "{:?}", first.tables);
    assert_eq!(settled(&mut s).generation, first.generation, "nothing read again");

    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"row_height_mm": 9.0}}));
    let edited = settled(&mut s);
    assert_ne!(edited.generation, first.generation);
    // The model changes only with the drawing closed. Opened again, the list reads the part's new name.
    let drawing = ids::encode(s.open_drawing.unwrap());
    s.execute("drawing.close", &json!({})).unwrap();
    s.execute("component.rename", &json!({"component": "Bolt", "name": "Long bolt"})).unwrap();
    s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
    settled(&mut s);
    let d = s.drawing("test").unwrap().clone();
    let lettering = PaperFonts::new(&d.font, &d.language, true);
    let placed = place(&s, &d, d.tables.by_id(table).unwrap(), drawing_space(&d.sheets[0]), &lettering).ok().unwrap();
    let names: Vec<_> = placed.layout.rows.iter().flat_map(|row| &row.cells).flat_map(|cell| &cell.lines).map(|line| line.text.as_str()).collect();
    assert!(names.contains(&"Long bolt"), "the list shows the part's new name: {names:?}");
}

/// A table's words stand in its cells: each line on its baseline, its left, middle or right at the
/// anchor its layout gives as its column aligns. The scene carries their fonts, the export draws
/// each one, and a redraw gives the same scene.
#[test]
fn a_tables_words_stand_where_their_cells_say() {
    use solidcraft_geom::cell::HAlign;
    use solidcraft_geom::paper::Item;

    let mut s = assembly();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let table = id_of(&run(&mut s, ADD, json!({"sheet": ids::encode(sheet), "kind": "parts_list"})));
    let mut words = 0;
    for (align, along) in [(HAlign::Left, 0.0), (HAlign::Centre, 0.5), (HAlign::Right, 1.0)] {
        put_table(&mut s, table, |record| record.table.columns.iter_mut().for_each(|column| column.align = align));
        let d = s.drawing("test").unwrap().clone();
        let scene = crate::drawing_scene::build(&s, &d, sheet).unwrap().scene;
        let lettering = PaperFonts::new(&d.font, &d.language, true);
        let record = d.tables.by_id(table).unwrap();
        let placed = place(&s, &d, record, drawing_space(&d.sheets[0]), &lettering).ok().unwrap();
        let group = scene.groups.iter().find(|g| g.object == table).unwrap();
        let mut runs: Vec<_> = group.items.iter().filter_map(|item| if let Item::Text(run) = item { Some(run) } else { None }).collect();
        let lines: Vec<_> =
            std::iter::once(&placed.layout.heading).chain(&placed.layout.rows).flat_map(|row| &row.cells).flat_map(|cell| &cell.lines).collect();
        assert!(lines.iter().any(|line| line.text == "Bolt") && lines.iter().any(|line| line.text == "QTY"), "{lines:?}");
        assert_eq!(lines.len(), runs.len(), "{align:?}");
        words = lines.len();
        for line in lines {
            let at = runs.iter().position(|run| run.text == line.text && (run.origin.y - line.at.y).abs() < 1e-9).unwrap();
            let run = runs.swap_remove(at);
            assert!((run.origin.x + run.reach() * along - line.at.x).abs() < 1e-9, "{align:?} {line:?} {:?}", run.origin);
            assert_eq!(run.angle, 0.0);
        }
        assert!(!scene.fonts.is_empty());
        assert_eq!(scene, crate::drawing_scene::build(&s, &d, sheet).unwrap().scene, "the same scene on a redraw");
    }
    let exported = s.execute("drawing.export", &json!({"sheet": ids::encode(sheet), "format": "svg"})).unwrap();
    let svg = exported["svg"].as_str().unwrap();
    let group = svg.split(&format!("data-object=\"{table}\"")).nth(1).unwrap().split("</g>").next().unwrap();
    assert_eq!(group.matches("<path fill=\"#000\" stroke=\"none\" d=\"").count(), words, "each word is in the file: {group}");
}

/// A word no font can set is left off and its table says why; the table and the rest of its words
/// still draw. Here an override puts an unassigned code point in one cell.
#[test]
fn a_word_no_font_can_set_is_left_off_and_says_why() {
    use solidcraft_geom::paper::Item;

    let mut s = assembly();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let table = id_of(&run(&mut s, ADD, json!({"sheet": ids::encode(sheet), "kind": "parts_list"})));
    let texts = |s: &Session| {
        let d = s.drawing("test").unwrap().clone();
        let scene = crate::drawing_scene::build(s, &d, sheet).unwrap().scene;
        let group = scene.groups.iter().find(|g| g.object == table).cloned().unwrap();
        group.items.iter().filter(|item| matches!(item, Item::Text(_))).count()
    };
    let plain = texts(&s);
    assert_eq!(on_sheet(&s, table).0.why, None);
    let key = inspected(&mut s, table)["rows"][0]["key"].as_str().unwrap().to_string();
    let overrides = json!([{"row": key, "column": "name", "text": "\u{378}"}]);
    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"overrides": overrides}}));
    let (state, bounds) = on_sheet(&s, table);
    assert!(state.drawn && !state.failed && bounds.is_some(), "{state:?}");
    assert!(state.why.as_deref().is_some_and(|why| why.contains('\u{378}') && why.contains("can't set")), "{state:?}");
    assert_eq!(texts(&s) + 1, plain, "only that word goes");
    let scene = s.execute("drawing.scene", &json!({"sheet": ids::encode(sheet)})).unwrap();
    assert_eq!((&scene["tables"][0]["status"], &scene["tables"][0]["drawn"]), (&json!("ok"), &json!(true)), "{scene}");
    assert!(scene["tables"][0]["why"].as_str().is_some(), "{scene}");
}

/// The window's sheet never waits for the search for a font to fall back to: a table's word in a
/// script the drawing's font lacks leaves the table off until the search answers, and the settled
/// sheet is the one `build` gives (which waits). The character is this test's alone, so no other
/// test has searched for it first.
#[test]
fn a_tables_word_waits_for_its_font_off_the_windows_thread() {
    let mut s = assembly();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let table = id_of(&run(&mut s, ADD, json!({"sheet": ids::encode(sheet), "kind": "parts_list"})));
    crate::drawing_scene::extract(&mut s, sheet).unwrap();
    let key = inspected(&mut s, table)["rows"][0]["key"].as_str().unwrap().to_string();
    let overrides = json!([{"row": key, "column": "name", "text": "Bolt \u{30f9}"}]);
    run(&mut s, EDIT, json!({"table": ids::encode(table), "set": {"overrides": overrides}}));
    let started = std::time::Instant::now();
    let mut answer = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert!(answer.waiting_for_fonts, "the answer says its text is waiting");
    assert!(answer.tables.iter().all(|t| t.table != table), "off the sheet while it waits: {:?}", answer.tables);
    while crate::drawing_scene::step(&mut s) || answer.waiting_for_fonts {
        assert!(started.elapsed().as_secs() < 120, "the sheet never settled");
        std::thread::yield_now();
        answer = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    }
    let d = s.drawing("test").unwrap().clone();
    assert_eq!(*answer.scene, crate::drawing_scene::build(&s, &d, sheet).unwrap().scene);
    assert!(answer.tables.iter().any(|t| t.table == table && t.drawn), "{:?}", answer.tables);
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
