use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use solidcraft_doc::naming;
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
    let width = length(&mut s, view, [50.0, 0.0, 0.0]);
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
    // Coverage is its own list. The lost dimension named the chamfer, which is gone, so it covers
    // nothing; the other covers the plate.
    let plate = s.doc.features[0].id;
    assert_eq!(coverage(&reply), [(plate, "covered".to_string(), vec![named("dimension", &width)])], "{reply}");
    assert_eq!(reply["coverage_counts"], json!([{"state": "covered", "count": 1}]), "{reply}");
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

/// A deferred drawing is checked as its sheet shows the model: a bar added since it was drawn neither
/// pushes the held view off its sheet nor is drawn by it, until the drawing is updated.
#[test]
fn a_held_sheet_is_checked_as_it_shows_the_model() {
    let (mut s, _) = plate();
    let (_, view) = drawn(&mut s);
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    with_model_changed(&mut s, |s| {
        run(s, "solid.box", json!({"length": 900, "width": 10, "height": 10, "corner": [0, 80, 0], "body_name": "Bar", "operation": "new"}));
    });
    let bar = s.doc.features.last().unwrap().id;
    let view = json!(ids::encode(view));
    let held = checked(&mut s, json!({}));
    assert_eq!(of_kind(&held, "out_of_date"), [named("view", &view)], "{held}");
    assert!(of_kind(&held, "outside_sheet").is_empty(), "{held}");
    assert_eq!(coverage_of(&held, bar)["state"], json!("not_applicable"), "{held}");
    run(&mut s, "drawing.update", json!({}));
    let now = checked(&mut s, json!({}));
    assert_eq!(of_kind(&now, "outside_sheet"), [named("view", &view)], "{now}");
    assert_eq!(coverage_of(&now, bar)["state"], json!("not_covered"), "{now}");
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

/// A view of a configuration row the design no longer has shows nothing, and nor do the views that
/// follow it: each is listed, saying why (#13048).
#[test]
fn a_view_of_a_deleted_configuration_row_is_listed() {
    let (mut s, _, [front, top], _) = crate::drawing_scene::tests::configured_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    with_model_changed(&mut s, |s| {
        run(s, "config.row", json!({"name": "Long", "delete": true}));
    });
    let reply = checked(&mut s, json!({}));
    let mut views = vec![named("view", &json!(ids::encode(front))), named("view", &json!(ids::encode(top)))];
    views.sort();
    assert_eq!(of_kind(&reply, "nothing_shown"), views, "{reply}");
    let whys: Vec<&Value> = reply["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == "nothing_shown").map(|f| &f["why"]).collect();
    assert_eq!(whys, [&json!("the configuration it draws is no longer in the design"); 2], "{reply}");
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
    let failed = ViewState { view: 9, status: ViewStatus::Failed("too many lines".into()), stats: None, why: None };
    let finding = view_finding(7, &failed).unwrap();
    assert_eq!((finding.kind, finding.why.as_str(), finding.object), (FindingKind::Failed, "too many lines", object_of(CheckedKind::View, 9)));
    assert!(view_finding(7, &ViewState { view: 9, status: ViewStatus::Drawn, stats: None, why: None }).is_none());
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

/// Each feature's coverage as (feature, state, what points at it), in the reply's order.
fn coverage(reply: &Value) -> Vec<(u64, String, Vec<(String, String)>)> {
    let by = |c: &Value| c["by"].as_array().map(|by| by.iter().map(|o| named(o["kind"].as_str().unwrap(), &o["id"])).collect());
    let one = |c: &Value| (c["feature"].as_u64().unwrap(), c["state"].as_str().unwrap().to_string(), by(c).unwrap_or_default());
    reply["coverage"].as_array().unwrap().iter().map(one).collect()
}

/// The coverage entry of `feature`.
fn coverage_of(reply: &Value, feature: u64) -> Value {
    let entries = reply["coverage"].as_array().unwrap();
    entries.iter().find(|c| c["feature"] == json!(feature)).cloned().unwrap_or_else(|| panic!("{feature}: {reply}"))
}

/// The plate of [`plate`] with a hole of radius 2 through it from the front at x 30, z 5, called
/// Hole; its pattern of two, 40 mm apart along X, called Holes; and a sketch on XY that nothing is
/// made from. The session and each feature's id in timeline order: the plate, the chamfer, the
/// hole, the pattern, the sketch.
fn holed() -> (Session, [u64; 5]) {
    let (mut s, _) = plate();
    let hole = json!({"radius": 2, "height": 62, "base": [30, -1, 5], "axis": "Y", "operation": "cut", "targets": ["Plate"], "name": "Hole"});
    run(&mut s, "solid.cylinder", hole);
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Hole"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 40, "name": "Holes"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Unused"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let features: Vec<u64> = s.doc.features.iter().map(|f| f.id).collect();
    (s, features.try_into().unwrap())
}

/// A note on view `view`'s edge through `point`.
fn note_on(s: &mut Session, view: u64, point: [f64; 3]) -> Value {
    let edge = json!({"edge": {"view": ids::encode(view), "edge": point}});
    run(s, "drawing.note.add", json!({"anchor": edge, "at": [60, 40], "content": "Note"}))["note"].clone()
}

/// The plate's bottom front edge covers the plate and leaves the chamfer, which made a face the
/// view draws, not covered. A note on the hole's rim covers the hole, and a dimension on the
/// second copy's rim covers the pattern. The sketch makes no face: not applicable. Once the
/// bottom front edge is chamfered too, its dimension is lost and still covers the plate, and the
/// new chamfer is not covered.
#[test]
fn each_feature_is_covered_by_what_points_at_a_face_it_made() {
    let (mut s, [plate, chamfer, hole, holes, sketch]) = holed();
    let (_, view) = drawn(&mut s);
    let width = length(&mut s, view, [50.0, 0.0, 0.0]);
    let rim = note_on(&mut s, view, [32.0, 0.0, 5.0]);
    let radial = json!({"view": ids::encode(view), "kind": {"radial": {"edge": [72, 0, 5]}}});
    let copy = run(&mut s, "drawing.dimension.add", radial)["dimension"].clone();
    let reply = checked(&mut s, json!({}));
    let by = |what: &str, id: &Value| vec![named(what, id)];
    let mut expected = vec![
        (plate, "covered".to_string(), by("dimension", &width)),
        (chamfer, "not_covered".to_string(), vec![]),
        (hole, "covered".to_string(), by("note", &rim)),
        (holes, "covered".to_string(), by("dimension", &copy)),
        (sketch, "not_applicable".to_string(), vec![]),
    ];
    assert_eq!(coverage(&reply), expected, "{reply}");
    assert_eq!(coverage_of(&reply, sketch)["why"], json!("no view draws a face it made"), "{reply}");
    assert!(coverage_of(&reply, chamfer).get("why").is_none(), "{reply}");
    let counts = json!([{"state": "covered", "count": 3}, {"state": "not_covered", "count": 1}, {"state": "not_applicable", "count": 1}]);
    assert_eq!(reply["coverage_counts"], counts, "{reply}");
    assert!(reply.get("sheet").is_none(), "{reply}");
    with_model_changed(&mut s, |s| {
        run(s, "solid.chamfer", json!({"edges": [[50, 0, 0]], "distance": 1, "body": "Plate"}));
    });
    let reply = checked(&mut s, json!({}));
    assert_eq!(of_kind(&reply, "lost"), [named("dimension", &width)], "{reply}");
    let bottom = s.doc.features.last().unwrap().id;
    expected.push((bottom, "not_covered".to_string(), vec![]));
    assert_eq!(coverage(&reply), expected, "{reply}");
}

/// With `sheet`, coverage is of that sheet's views and annotations: the hole, which only the
/// other sheet's note points at, is covered there and on the whole drawing, and not covered on
/// the first sheet. A sheet with no view draws nothing. The reply echoes the sheet by id. A
/// dimension between two corners of the plate covers it once.
#[test]
fn coverage_is_of_the_sheet_checked() {
    let (mut s, [plate, _, hole, ..]) = holed();
    let (sheet, first_view) = drawn(&mut s);
    let across = json!({"view": ids::encode(first_view), "kind": {"linear": {"a": [0, 0, 0], "b": [100, 0, 0], "direction": "horizontal"}}});
    let across = run(&mut s, "drawing.dimension.add", across)["dimension"].clone();
    let other = run(&mut s, "drawing.sheet.add", json!({"name": "Other"}))["sheet"].clone();
    let view = id(&run(&mut s, "drawing.view.base", json!({"sheet": other, "scale": "1:1"}))["view"]);
    let rim = note_on(&mut s, view, [32.0, 0.0, 5.0]);
    let state = |reply: &Value, feature: u64| coverage_of(reply, feature)["state"].clone();
    let there = checked(&mut s, json!({"sheet": "Other"}));
    assert_eq!((state(&there, hole), there["sheet"].clone()), (json!("covered"), other), "{there}");
    assert_eq!(coverage_of(&there, hole)["by"], json!([{"kind": "note", "id": rim}]), "{there}");
    assert_eq!(state(&there, plate), json!("not_covered"), "{there}");
    let first = checked(&mut s, json!({"sheet": ids::encode(sheet)}));
    assert_eq!((state(&first, hole), first["sheet"].clone()), (json!("not_covered"), json!(ids::encode(sheet))), "{first}");
    assert_eq!(coverage_of(&first, plate)["by"], json!([{"kind": "dimension", "id": across}]), "{first}");
    assert_eq!(state(&checked(&mut s, json!({})), hole), json!("covered"));
    run(&mut s, "drawing.sheet.add", json!({"name": "Empty"}));
    let empty = checked(&mut s, json!({"sheet": "Empty"}));
    assert!(coverage(&empty).iter().all(|(_, state, _)| state == "not_applicable"), "{empty}");
}

/// The bolt's shank is not covered while the view draws it; hidden from the view it is not
/// applicable, and so it is when the bolt is a reference part, each with why.
#[test]
fn a_hidden_part_and_a_reference_part_are_not_applicable() {
    let (mut s, view, _, at) = assembly();
    let shank = s.doc.features.last().unwrap().id;
    // A plate at the root stays in the view: a view that would draw nothing can't be made.
    with_model_changed(&mut s, |s| {
        run(s, "solid.box", json!({"length": 30, "width": 30, "height": 5, "corner": [40, 0, 0], "body_name": "Base", "operation": "new"}));
    });
    let entry = |s: &mut Session| {
        let entry = coverage_of(&checked(s, json!({})), shank);
        (entry["state"].clone(), entry["why"].clone())
    };
    assert_eq!(entry(&mut s), (json!("not_covered"), Value::Null));
    run(&mut s, "drawing.view.edit", json!({"view": view, "set": {"hide": [[at]]}}));
    assert_eq!(entry(&mut s), (json!("not_applicable"), json!("no view draws a face it made")));
    run(&mut s, "drawing.view.edit", json!({"view": view, "set": {"hide": null}}));
    with_model_changed(&mut s, |s| {
        run(s, "component.metadata.set", json!({"component": "Bolt", "bom_type": "reference"}));
    });
    assert_eq!(entry(&mut s), (json!("not_applicable"), json!("only reference parts draw it")));
}

/// A suppressed feature and a rolled back one are not applicable, each with why.
#[test]
fn suppressed_and_rolled_back_features_are_not_applicable() {
    let (mut s, [_, _, _, holes, sketch]) = holed();
    drawn(&mut s);
    let why = |reply: &Value, feature: u64| {
        let entry = coverage_of(reply, feature);
        (entry["state"].clone(), entry["why"].clone())
    };
    with_model_changed(&mut s, |s| {
        run(s, "timeline.suppress", json!({"feature": holes, "suppressed": true}));
    });
    assert_eq!(why(&checked(&mut s, json!({})), holes), (json!("not_applicable"), json!("it is suppressed")));
    with_model_changed(&mut s, |s| {
        run(s, "timeline.suppress", json!({"feature": holes, "suppressed": false}));
        run(s, "timeline.rollback", json!({"position": 3}));
    });
    let reply = checked(&mut s, json!({}));
    for feature in [holes, sketch] {
        assert_eq!(why(&reply, feature), (json!("not_applicable"), json!("it is rolled back")), "{reply}");
    }
}

/// A block lifted by an operation that reports no history has no face named from exact history:
/// the block's coverage is unknown, with why, and no feature is not covered. A body imported from
/// STEP is the other way: the import names the faces it made from its own history
/// (`F<import>:face:<k>`), so they are exact, and an import nothing points at is not covered.
#[test]
fn faces_not_named_from_exact_history_give_unknown() {
    let mut source = Session::default();
    run(&mut source, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    let step = solidcraft_io::export(&source.model.state(), &[], solidcraft_io::Format::Step, "Plate").unwrap();
    let imported = solidcraft_io::step_import_feature(&step, "plate.step").unwrap();
    let mut doc = solidcraft_doc::Document::new("Imported plate");
    let import = doc.add_feature(imported.kind, None).unwrap();
    doc.feature_mut(import).unwrap().body_names = imported.body_names;
    let mut s = Session::new(doc);
    drawn(&mut s);
    let reply = checked(&mut s, json!({}));
    assert_eq!(coverage(&reply), [(import, "not_covered".to_string(), vec![])], "{reply}");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.align", json!({"bodies": ["Plate"], "from": [0, 0, 0], "to": [0, 0, 2], "name": "Lift"}));
    let block = s.doc.features[0].id;
    drawn(&mut s);
    let reply = checked(&mut s, json!({}));
    let entry = coverage_of(&reply, block);
    assert_eq!(entry["state"], json!("unknown"), "{reply}");
    assert!(!entry["why"].as_str().unwrap().is_empty(), "{reply}");
    assert!(coverage(&reply).iter().all(|(_, state, _)| state != "not_covered"), "{reply}");
}

/// A feature one body draws from exact history and another, moved since, from geometry: its exact
/// faces can be dimensioned, so it is not covered, not unknown. The plate is split across X and one
/// piece lifted.
#[test]
fn a_feature_drawn_both_exactly_and_from_geometry_is_not_covered() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.split_body", json!({"body": "Plate", "plane": {"origin": [50, 0, 0], "normal": [1, 0, 0]}}));
    run(&mut s, "solid.align", json!({"bodies": ["Plate (1)"], "from": [0, 0, 0], "to": [0, 0, 2], "name": "Lift"}));
    let block = s.doc.features[0].id;
    let state = s.model.state();
    let drawn_so = |exact: bool| {
        state.bodies.iter().any(|b| {
            let made_by_block = |name: &String| coverage::made_by(name) == Some(block);
            naming::face_names(b).iter().enumerate().any(|(i, name)| made_by_block(name) && naming::face_name_exact(b, i) == exact)
        })
    };
    assert!(drawn_so(true) && drawn_so(false), "the block is drawn both ways");
    drawn(&mut s);
    let reply = checked(&mut s, json!({}));
    assert_eq!(coverage_of(&reply, block)["state"], json!("not_covered"), "{reply}");
}

/// A saved dimension whose edge name is hostile covers nothing and never panics; the note on the
/// hole still covers it.
#[test]
fn hostile_saved_names_cover_nothing() {
    let (mut s, [plate, _, hole, ..]) = holed();
    let (_, view) = drawn(&mut s);
    length(&mut s, view, [50.0, 0.0, 0.0]);
    note_on(&mut s, view, [32.0, 0.0, 5.0]);
    let deep = "F1:inner:".repeat(64);
    let long = "x".repeat(2 << 20);
    let wide = vec!["F1:-y|F99:-z"; 1000].join("|");
    for name in ["", "|", "F1", "F99:top|F1:-y", "Plate:face:0|Plate:face:1", deep.as_str(), long.as_str(), wide.as_str()] {
        let mut d = (**s.drawing("test").unwrap()).clone();
        let dimension = d.dimensions.first_mut().map(Arc::make_mut).unwrap();
        let solidcraft_doc::drawing::dimension::DimensionKind::Length { edge, .. } = &mut dimension.kind else { panic!("a length") };
        edge.name = name.to_string();
        s.doc_mut().put_drawing(Arc::new(d));
        let reply = s.execute(CHECK, &json!({})).unwrap_or_else(|e| panic!("{}: {e}", name.len()));
        assert_eq!(coverage_of(&reply, hole)["state"], json!("covered"), "{}", name.len());
        assert_ne!(coverage_of(&reply, plate)["state"], json!("covered"), "{}", name.len());
    }
}

/// A note placed off the paper is outside the sheet, as a dimension is: a note is one of the sheet's
/// objects since notes came to the sheet (#13060). One on the paper isn't listed.
#[test]
fn a_note_off_the_paper_is_listed() {
    let (mut s, _) = plate();
    let (sheet, _) = drawn(&mut s);
    let note = |s: &mut Session, at: [f64; 2]| {
        run(s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": at, "content": "General"}))["note"].clone()
    };
    note(&mut s, [20.0, 20.0]);
    assert!(of_kind(&checked(&mut s, json!({})), "outside_sheet").is_empty());
    let off = note(&mut s, [1000.0, 20.0]);
    let reply = checked(&mut s, json!({}));
    assert_eq!(of_kind(&reply, "outside_sheet"), [named("note", &off)], "{reply}");
}

/// A note that prints nothing, its one field having no value, isn't on the paper or off it: it is never
/// outside the sheet.
#[test]
fn a_note_that_prints_nothing_is_not_outside_the_sheet() {
    let (mut s, _) = plate();
    let (sheet, _) = drawn(&mut s);
    let content = json!([{"field": {"source": "component", "key": "part_number"}}]);
    run(&mut s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": [20, 20], "content": content}));
    let reply = checked(&mut s, json!({}));
    assert!(of_kind(&reply, "outside_sheet").is_empty(), "{reply}");
}

/// [`plate`] with a component, Bolt, a 10 mm cube above it, on revolute joint Turn about Z, and an
/// exploded view, Apart, that moves the Bolt up 50 mm; [`drawn`]'s sheet and front view, with what
/// `annotate` adds on that view; then the view set by `apart`, as a file can hold it. The session, the
/// view, and the plate's feature.
fn drawn_apart(
    annotate: impl FnOnce(&mut Session, u64),
    apart: impl FnOnce(&Session, &mut solidcraft_doc::drawing_view::ViewOverrides),
) -> (Session, u64, u64) {
    let (mut s, _) = plate();
    let bolt = run(&mut s, "component.create", json!({"name": "Bolt"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 20], "body_name": "Cube"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    // An exploded view pulls components apart, so the design needs two.
    run(&mut s, "component.create", json!({"name": "Nut"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 20], "body_name": "Nut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": bolt, "translate": [0, 0, 50]}]}));
    let at = json!({"occurrence": 0, "point": [0, 0, 20], "z": [0, 0, 1]});
    run(&mut s, "joint.as_built", json!({"type": "revolute", "name": "Turn", "a": 0, "b": "Bolt", "at": at}));
    let plate = s.doc.features[0].id;
    let (_, view) = drawn(&mut s);
    annotate(&mut s, view);
    let mut d = (**s.drawing("test").unwrap()).clone();
    let record = d.views.iter_mut().find(|v| v.id == view).unwrap();
    apart(&s, &mut Arc::make_mut(record).settings);
    s.doc_mut().put_drawing(Arc::new(d));
    (s, view, plate)
}

/// [`drawn_apart`] with the view drawing Apart.
fn exploded(annotate: impl FnOnce(&mut Session, u64)) -> (Session, u64, u64) {
    drawn_apart(annotate, |s, set| set.exploded = Some(solidcraft_doc::drawing_view::Exploded::View(s.doc.assembly.exploded[0].id)))
}

/// [`drawn_apart`] with the view in a position of its own: Turn at half a radian.
fn posed(annotate: impl FnOnce(&mut Session, u64)) -> (Session, u64, u64) {
    use solidcraft_doc::drawing_view::{JointPose, Pose};
    drawn_apart(annotate, |s, set| {
        let turn = s.doc.assembly.joints.iter().find(|j| j.name == "Turn").unwrap().id;
        set.pose = Some(Pose::Joints(vec![JointPose { joint: turn, values: vec![0.5] }].into()));
    })
}

/// The why of each finding of `kind`, as (kind of record, id, why), sorted.
fn whys(reply: &Value, kind: &str) -> Vec<(String, String, String)> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let of = reply["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == kind);
    let mut all: Vec<_> = of.map(|f| (text(&f["object"]["kind"]), text(&f["object"]["id"]), text(&f["why"]))).collect();
    all.sort();
    all
}

/// A dimension on a view drawing an exploded view is checked as any view's (#13048): it is drawn, so
/// nothing lists it out of date.
#[test]
fn a_dimension_on_an_exploded_view_is_checked_as_any_views() {
    let (mut s, ..) = exploded(|s, view| {
        length(s, view, [50.0, 0.0, 0.0]);
    });
    let reply = checked(&mut s, json!({}));
    assert!(whys(&reply, "out_of_date").is_empty(), "{reply}");
}

/// A dimension on a view in a position of its own is checked as any view's, as an exploded view's is:
/// not out of date, and it covers the plate, whose bottom front edge it measures.
#[test]
fn a_dimension_on_a_posed_view_is_checked_as_any_views_and_covers_its_feature() {
    let mut width = Value::Null;
    let (mut s, _, plate) = posed(|s, view| width = length(s, view, [50.0, 0.0, 0.0]));
    let reply = checked(&mut s, json!({}));
    assert!(whys(&reply, "out_of_date").is_empty(), "{reply}");
    assert_eq!(coverage_of(&reply, plate)["by"], json!([{"kind": "dimension", "id": width}]), "{reply}");
}

/// A dimension on a view drawing an exploded view covers the plate, whose bottom front edge it
/// measures, and still does once the view is drawn assembled again.
#[test]
fn a_dimension_on_an_exploded_view_covers_its_feature_drawn_either_way() {
    let mut width = Value::Null;
    let (mut s, view, plate) = exploded(|s, view| width = length(s, view, [50.0, 0.0, 0.0]));
    let reply = checked(&mut s, json!({}));
    assert_eq!(coverage_of(&reply, plate)["by"], json!([{"kind": "dimension", "id": width}]), "{reply}");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"exploded": "assembled"}}));
    let reply = checked(&mut s, json!({}));
    assert_eq!(coverage_of(&reply, plate)["by"], json!([{"kind": "dimension", "id": width}]), "{reply}");
}

/// A note's branch that loses its edge is a finding of its own, naming the branch by its number; the
/// note's anchor, on the view itself, stands.
#[test]
fn a_notes_lost_branch_is_listed_by_its_number() {
    let (mut s, chamfer) = plate();
    let (_, view) = drawn(&mut s);
    let add =
        json!({"anchor": {"view": ids::encode(view)}, "at": [60, 40], "content": "Break", "branches": [{"edge": [50, 0, 0]}, {"edge": [50, 0, 8]}]});
    let note = run(&mut s, "drawing.note.add", add)["note"].clone();
    assert!(of_kind(&checked(&mut s, json!({})), "lost").is_empty());
    with_model_changed(&mut s, |s| {
        run(s, "timeline.delete", json!({"feature": chamfer}));
    });
    let reply = checked(&mut s, json!({}));
    let lost = whys(&reply, "lost");
    assert_eq!(lost.iter().map(|(kind, id, _)| (kind.as_str(), id.as_str())).collect::<Vec<_>>(), [("note", note.as_str().unwrap())], "{reply}");
    assert!(lost[0].2.starts_with("branch 2: "), "{reply}");
}

/// A note covers what each of its leaders points at: a branch on the chamfer's edge covers the
/// chamfer, though the note's anchor is the view itself.
#[test]
fn a_notes_branch_covers_the_feature_it_points_at() {
    let (mut s, chamfer) = plate();
    let (_, view) = drawn(&mut s);
    let add = json!({"anchor": {"view": ids::encode(view)}, "at": [60, 40], "content": "Break", "branches": [{"edge": [50, 0, 8]}]});
    let note = run(&mut s, "drawing.note.add", add)["note"].clone();
    let reply = checked(&mut s, json!({}));
    let entry = coverage_of(&reply, chamfer.as_u64().unwrap());
    assert_eq!((&entry["state"], &entry["by"]), (&json!("covered"), &json!([{"kind": "note", "id": note}])), "{reply}");
}

/// A note on the model of a view drawing an exploded view is checked as any view's note, as a
/// dimension there is: not out of date, and it covers the plate, whose bottom front edge it points at.
#[test]
fn a_note_on_an_exploded_view_is_checked_as_any_views_and_covers_its_feature() {
    let mut note = Value::Null;
    let (mut s, _, plate) = exploded(|s, view| note = note_on(s, view, [50.0, 0.0, 0.0]));
    let reply = checked(&mut s, json!({}));
    assert!(whys(&reply, "out_of_date").is_empty(), "{reply}");
    assert_eq!(coverage_of(&reply, plate)["by"], json!([{"kind": "note", "id": note}]), "{reply}");
}
