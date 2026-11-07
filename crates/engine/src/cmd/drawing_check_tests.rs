use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use solidcraft_geom::cell::{Content, Format, Part, Source};
use solidcraft_geom::title_block::{CellKind, default_block};

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// `drawing.check`'s reply, held to its schema, its findings in sheet order, then by the id of the
/// record each names.
fn checked(s: &mut Session, p: Value) -> Value {
    let reply = run(s, CHECK, p);
    let schema = output_schema_for::<CheckResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let sheets: Vec<String> = s.drawing("test").unwrap().sheets.iter().map(|sheet| ids::encode(sheet.id)).collect();
    let key = |f: &Value| (sheets.iter().position(|id| f["sheet"] == json!(id)).unwrap(), id(&f["object"]["id"]));
    let keys: Vec<_> = reply["findings"].as_array().unwrap().iter().map(key).collect();
    assert!(keys.is_sorted(), "{reply}");
    reply
}

/// Each finding as (kind, the kind of what it names, its id), sorted.
fn found(reply: &Value) -> Vec<(String, String, String)> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let one = |f: &Value| (text(&f["kind"]), text(&f["object"]["kind"]), text(&f["object"]["id"]));
    let mut all: Vec<_> = reply["findings"].as_array().unwrap().iter().map(one).collect();
    all.sort();
    all
}

/// What the findings of `kind` name, as (kind of record, id), sorted.
fn of_kind(reply: &Value, kind: &str) -> Vec<(String, String)> {
    found(reply).into_iter().filter(|(k, ..)| k == kind).map(|(_, what, id)| (what, id)).collect()
}

fn named(what: &str, id: &Value) -> (String, String) {
    (what.to_string(), id.as_str().unwrap().to_string())
}

fn id(v: &Value) -> u64 {
    ids::decode(v.as_str().unwrap()).unwrap()
}

/// A folder of this test's own.
fn folder(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-drawing-check-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A plate 100 x 60 x 10 mm from the origin, called Plate, with a 2 mm chamfer on its top front
/// edge: the session and the chamfer's feature.
fn plate() -> (Session, Value) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    let chamfer = run(&mut s, "solid.chamfer", json!({"edges": [[50, 0, 10]], "distance": 2, "body": "Plate"}))["feature"].clone();
    (s, chamfer)
}

/// A drawing of `s`'s design, open, its first sheet documenting the design, with a title block of
/// two fields, the component's name and its part number (which the design doesn't have), and text
/// in every other cell, with nothing typed over any (a new sheet's template types its tolerances
/// note over one); and a front view at 1:1. The sheet and the view.
fn drawn(s: &mut Session) -> (u64, u64) {
    let sheet = id(&run(s, "drawing.create", json!({}))["sheets"][0]["id"]);
    let mut block = default_block();
    let mut fields = [(Source::Component, "name"), (Source::Component, "part_number")].into_iter();
    for cell in &mut block.cells {
        if let CellKind::Text(text) = &mut cell.kind {
            text.content = Content(vec![match fields.next() {
                Some((source, key)) => Part::Field { source, key: key.into(), format: Format::default() },
                None => Part::Text("Filled".into()),
            }]);
        }
    }
    let design = ids::encode(s.drawing("test").unwrap().sources[0].id);
    let set = json!({"sheet": ids::encode(sheet), "title_block": block, "title_block_overrides": null, "documents": {"source": design}});
    run(s, "drawing.sheet.set", set);
    let view = id(&run(s, "drawing.view.base", json!({"sheet": ids::encode(sheet), "scale": "1:1"}))["view"]);
    (sheet, view)
}

/// The names of the block's first two text cells: the name field's and the part number field's.
fn field_cells(s: &Session) -> [String; 2] {
    let block = s.drawing("test").unwrap().sheets[0].title_block.clone().unwrap();
    let mut texts = block.cells.iter().filter(|c| matches!(c.kind, CellKind::Text(_))).map(|c| c.name.clone());
    [texts.next().unwrap(), texts.next().unwrap()]
}

/// A length dimension on `view` of the edge through `point`.
fn length(s: &mut Session, view: u64, point: [f64; 3]) -> Value {
    run(s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": {"length": {"edge": point}}}))["dimension"].clone()
}

/// `change` run on the model with the drawing closed, then the drawing opened again.
fn with_model_changed(s: &mut Session, change: impl FnOnce(&mut Session)) {
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(s, "drawing.close", json!({}));
    change(s);
    run(s, "drawing.open", drawing);
}

/// The issue's case: a drawing with one lost dimension and an empty part number field lists both,
/// and nothing else. Before the chamfer is deleted only the field is listed: a part drawing with
/// no parts list lists no row without a balloon.
#[test]
fn a_lost_dimension_and_an_empty_part_number_are_listed_and_nothing_else() {
    let (mut s, chamfer) = plate();
    let (sheet, view) = drawn(&mut s);
    length(&mut s, view, [50.0, 0.0, 0.0]);
    let on_chamfer = length(&mut s, view, [50.0, 0.0, 8.0]);
    let reply = checked(&mut s, json!({}));
    let block = ("empty_field".to_string(), "title_block".to_string(), ids::encode(sheet));
    assert_eq!(found(&reply), std::slice::from_ref(&block), "{reply}");
    let field = &reply["findings"][0];
    let [_, part_number] = field_cells(&s);
    let expected = (json!(part_number), json!("component.part_number"), json!(ids::encode(sheet)));
    assert_eq!((field["cell"].clone(), field["field"].clone(), field["sheet"].clone()), expected, "{reply}");
    with_model_changed(&mut s, |s| {
        run(s, "timeline.delete", json!({"feature": chamfer}));
    });
    let reply = checked(&mut s, json!({}));
    let lost = ("lost".to_string(), "dimension".to_string(), on_chamfer.as_str().unwrap().to_string());
    let mut both = vec![block, lost];
    both.sort();
    assert_eq!(found(&reply), both, "{reply}");
    assert_eq!(reply["counts"], json!([{"kind": "lost", "count": 1}, {"kind": "empty_field", "count": 1}]), "{reply}");
    assert!(reply["findings"].as_array().unwrap().iter().all(|f| !f["why"].as_str().unwrap().is_empty()), "{reply}");
}

/// Typed text over a value hides it; typed into an empty field, it fills it and is no finding. A
/// dimension whose text is typed over hides its value too, and one on a suppressed feature is
/// listed so.
#[test]
fn overrides_and_suppressed_references_are_listed() {
    let (mut s, chamfer) = plate();
    let (sheet, view) = drawn(&mut s);
    let width = length(&mut s, view, [50.0, 0.0, 0.0]);
    let on_chamfer = length(&mut s, view, [50.0, 0.0, 8.0]);
    let [name, part_number] = field_cells(&s);
    let typed = json!({name.clone(): "Typed name", part_number: "PN-7"});
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block_overrides": typed}));
    run(&mut s, "drawing.dimension.edit", json!({"dimension": width, "set": {"override_text": "SEE NOTE"}}));
    with_model_changed(&mut s, |s| {
        run(s, "timeline.suppress", json!({"feature": chamfer, "suppressed": true}));
    });
    let reply = checked(&mut s, json!({}));
    assert_eq!(of_kind(&reply, "overridden"), [named("dimension", &width), named("title_block", &json!(ids::encode(sheet)))], "{reply}");
    assert_eq!(of_kind(&reply, "suppressed"), [named("dimension", &on_chamfer)], "{reply}");
    assert_eq!(found(&reply).len(), 3, "nothing else: {reply}");
    let cell = reply["findings"].as_array().unwrap().iter().find(|f| f["object"]["kind"] == "title_block").unwrap()["cell"].clone();
    assert_eq!(cell, json!(name));
}

/// A deferred drawing whose model moved on lists its view and dimension as out of date.
#[test]
fn a_held_view_and_its_dimension_are_out_of_date() {
    let (mut s, _) = plate();
    let (_, view) = drawn(&mut s);
    let width = length(&mut s, view, [50.0, 0.0, 0.0]);
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    assert!(of_kind(&checked(&mut s, json!({})), "out_of_date").is_empty());
    with_model_changed(&mut s, |s| {
        run(s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    });
    let reply = checked(&mut s, json!({}));
    let mut expected = vec![named("dimension", &width), named("view", &json!(ids::encode(view)))];
    expected.sort();
    assert_eq!(of_kind(&reply, "out_of_date"), expected, "{reply}");
}

/// A view of a design with nothing to draw shows nothing: its one box, drawn, then suppressed (a
/// drawing of an empty design isn't made at all).
#[test]
fn a_view_with_nothing_to_measure_is_listed() {
    let mut s = Session::default();
    let block = run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}))["feature"].clone();
    let sheet = run(&mut s, "drawing.create", json!({}))["sheets"][0]["id"].clone();
    let view = run(&mut s, "drawing.view.base", json!({"sheet": sheet, "scale": "1:1"}))["view"].clone();
    with_model_changed(&mut s, |s| {
        run(s, "timeline.suppress", json!({"feature": block, "suppressed": true}));
    });
    let reply = checked(&mut s, json!({}));
    assert_eq!(of_kind(&reply, "nothing_shown"), [named("view", &view)], "{reply}");
}

fn object_of(kind: CheckedKind, id: u64) -> CheckedObject {
    CheckedObject { kind, id: ids::encode(id) }
}

/// Every status a reference can have gives its own kind, or none when it is found and measured,
/// and a view that can't be drawn is listed with why.
#[test]
fn each_status_gives_its_kind() {
    let cases = [
        (Status::Ok, None),
        (Status::Overridden, Some(FindingKind::Overridden)),
        (Status::Ambiguous(Vec::new()), Some(FindingKind::Ambiguous)),
        (Status::Suppressed, Some(FindingKind::Suppressed)),
        (Status::Failed("no".into()), Some(FindingKind::Failed)),
        (Status::Lost("gone".into(), Vec::new()), Some(FindingKind::Lost)),
        (Status::Unmeasurable("flat".into()), Some(FindingKind::Unmeasurable)),
    ];
    let dimension = object_of(CheckedKind::Dimension, 5);
    for (status, kind) in cases {
        let finding = status_finding(7, dimension.clone(), status.clone());
        assert_eq!(finding.as_ref().map(|f| f.kind), kind, "{status:?}");
        assert!(finding.is_none_or(|f| !f.why.is_empty() && f.sheet == ids::encode(7) && f.object == dimension), "{status:?}");
    }
    let failed = ViewState { view: 9, status: ViewStatus::Failed("too many lines".into()), stats: None };
    let finding = view_finding(7, &failed).unwrap();
    assert_eq!((finding.kind, finding.why.as_str(), finding.object), (FindingKind::Failed, "too many lines", object_of(CheckedKind::View, 9)));
    assert!(view_finding(7, &ViewState { view: 9, status: ViewStatus::Drawn, stats: None }).is_none());
}

/// A note on the chamfer's edge is lost with it; a note on the sheet reads no model.
#[test]
fn a_lost_note_is_listed() {
    let (mut s, chamfer) = plate();
    let (sheet, view) = drawn(&mut s);
    let edge = json!({"edge": {"view": ids::encode(view), "edge": [50, 0, 8]}});
    let note = run(&mut s, "drawing.note.add", json!({"anchor": edge, "at": [60, 40], "content": "Break"}))["note"].clone();
    run(&mut s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": [20, 20], "content": "General"}));
    assert!(of_kind(&checked(&mut s, json!({})), "lost").is_empty());
    with_model_changed(&mut s, |s| {
        run(s, "timeline.delete", json!({"feature": chamfer}));
    });
    assert_eq!(of_kind(&checked(&mut s, json!({})), "lost"), [named("note", &note)]);
}

/// A component, Bolt, of a 10 x 10 x 40 mm box called Shank, placed once; a drawing of the design,
/// open, with a front view at 1:1 and a parts list on its sheet. The view, the list and the Bolt's
/// occurrence.
fn assembly() -> (Session, Value, Value, u64) {
    let mut s = Session::default();
    let at = run(&mut s, "component.create", json!({"name": "Bolt"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 40, "body_name": "Shank"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let sheet = run(&mut s, "drawing.create", json!({}))["sheets"][0]["id"].clone();
    let view = run(&mut s, "drawing.view.base", json!({"sheet": sheet, "scale": "1:1"}))["view"].clone();
    let table = run(&mut s, "drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"}))["table"].clone();
    (s, view, table, at)
}

/// A parts list row no balloon points at is listed, by its item, and only with its sheet; a balloon
/// on it takes it off, and once the balloon's part is gone the balloon is lost and the row listed
/// again.
#[test]
fn a_parts_list_row_without_a_balloon_is_listed() {
    let (mut s, view, table, at) = assembly();
    let rows = |reply: &Value| -> Vec<Value> {
        let rows = reply["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == "not_ballooned");
        rows.map(|f| json!([f["object"], f["item"]])).collect()
    };
    let row = json!([{"kind": "table", "id": table}, "1"]);
    let reply = checked(&mut s, json!({}));
    assert_eq!(rows(&reply), std::slice::from_ref(&row), "{reply}");
    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    assert!(rows(&checked(&mut s, json!({"sheet": "Other"}))).is_empty());
    let edge = json!({"point": [5, 0, 0], "path": [at]});
    let balloon = run(&mut s, "drawing.balloon.add", json!({"view": view, "edge": edge, "at": [80, 20]}))["balloon"].clone();
    let reply = checked(&mut s, json!({}));
    assert_eq!((rows(&reply), of_kind(&reply, "lost")), (vec![], vec![]), "{reply}");
    let mut d = (**s.drawing("test").unwrap()).clone();
    d.balloons.first_mut().map(Arc::make_mut).unwrap().edge.path = vec![999];
    s.doc_mut().put_drawing(Arc::new(d));
    let reply = checked(&mut s, json!({}));
    assert_eq!((rows(&reply), of_kind(&reply, "lost")), (vec![row], vec![named("balloon", &balloon)]), "{reply}");
}

/// A view moved off its sheet's drawing space, a note on it with it, and an image off the paper, are outside the sheet;
/// `sheet` narrows the check to one sheet, by id or name, a lost note in a view included.
#[test]
fn what_leaves_the_sheet_is_listed_one_sheet_at_a_time() {
    let (mut s, chamfer) = plate();
    let (sheet, view) = drawn(&mut s);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"place": {"at": [-500, 100]}}}));
    let edge = json!({"edge": {"view": ids::encode(view), "edge": [50, 0, 8]}});
    let note = run(&mut s, "drawing.note.add", json!({"anchor": edge, "at": [60, 40], "content": "Break"}))["note"].clone();
    let other = run(&mut s, "drawing.sheet.add", json!({"name": "Other"}))["sheet"].clone();
    let file = folder("outside").join("logo.png");
    std::fs::write(&file, crate::drawing_pictures::tests::png(4, 2)).unwrap();
    let image = run(&mut s, "drawing.image.add", json!({"sheet": other, "file": file, "rect_mm": [-60, -40, -20, -10]}))["image"].clone();
    with_model_changed(&mut s, |s| {
        run(s, "timeline.delete", json!({"feature": chamfer}));
    });
    let all = checked(&mut s, json!({}));
    let view = json!(ids::encode(view));
    let mut expected = vec![named("image", &image), named("note", &note), named("view", &view)];
    expected.sort();
    assert_eq!((of_kind(&all, "outside_sheet"), of_kind(&all, "lost")), (expected, vec![named("note", &note)]), "{all}");
    let others = checked(&mut s, json!({"sheet": "Other"}));
    assert_eq!((of_kind(&others, "outside_sheet"), of_kind(&others, "lost")), (vec![named("image", &image)], vec![]), "{others}");
    let first = checked(&mut s, json!({"sheet": ids::encode(sheet)}));
    assert!(first["findings"].as_array().unwrap().iter().all(|f| f["sheet"] == json!(ids::encode(sheet))), "{first}");
    assert_eq!(
        (of_kind(&first, "outside_sheet"), of_kind(&first, "lost")),
        (vec![named("note", &note), named("view", &view)], vec![named("note", &note)])
    );
    assert!(s.execute(CHECK, &json!({"sheet": "Nowhere"})).is_err());
}

/// A dimension whose line is moved off the paper is outside the sheet, as a table would be: the
/// check reads them from the sheet's objects, where images aren't.
#[test]
fn a_dimension_off_the_paper_is_listed() {
    let (mut s, _) = plate();
    let (_, view) = drawn(&mut s);
    let dimension = length(&mut s, view, [50.0, 0.0, 8.0]);
    assert!(of_kind(&checked(&mut s, json!({})), "outside_sheet").is_empty());
    run(&mut s, "drawing.dimension.edit", json!({"dimension": dimension, "set": {"offset_mm": 1000}}));
    let reply = checked(&mut s, json!({}));
    assert_eq!(of_kind(&reply, "outside_sheet"), [named("dimension", &dimension)], "{reply}");
}

/// The check changes nothing: the design and the undo steps are as they were. Its answer is the
/// same twice, the first time with a font search that nothing else asked (the typed letters are
/// Tibetan, in no other test), and the same from a fresh session with nothing cached.
#[test]
fn the_check_changes_nothing_and_answers_the_same() {
    let (mut s, chamfer) = plate();
    let (sheet, view) = drawn(&mut s);
    length(&mut s, view, [50.0, 0.0, 8.0]);
    let [name, _] = field_cells(&s);
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block_overrides": {name: "\u{0f40}\u{0f41}"}}));
    with_model_changed(&mut s, |s| {
        run(s, "timeline.delete", json!({"feature": chamfer}));
    });
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    let first = checked(&mut s, json!({}));
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.undo.len() == undo, "nothing changed");
    assert_eq!(found(&first).len(), 3, "lost, overridden, empty: {first}");
    assert_eq!(checked(&mut s, json!({})), first);
    // Saved under the design's own name: Save As names an untitled design after its file.
    let path = folder("cold").join(format!("{}.solidcraft", s.doc.name));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": path}));
    run(&mut cold, "drawing.open", json!({"drawing": ids::encode(s.open_drawing.unwrap())}));
    assert_eq!(checked(&mut cold, json!({})), first, "a cold session answers the same");
}

/// Every hostile value in the sheet's place is answered without a panic.
#[test]
fn hostile_check_params_never_panic() {
    let (mut s, _) = plate();
    drawn(&mut s);
    let mut values = crate::tests::hostile_values();
    values.extend([json!(u64::MAX), Value::Array(vec![json!(1); 1000]), json!("x".repeat(600))]);
    let mut internal = Vec::new();
    for value in values {
        let params = json!({"sheet": value});
        if let Err(crate::EngineError::Internal(_, msg)) = s.execute(CHECK, &params) {
            internal.push(format!("{params}: {msg}"));
        }
    }
    assert!(internal.is_empty(), "drawing.check panicked:\n{}", internal.join("\n"));
    assert!(s.execute(CHECK, &json!({"other": 1})).is_err());
}
