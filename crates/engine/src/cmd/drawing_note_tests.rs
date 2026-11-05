use serde_json::json;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let schema = output_schema_for::<NoteResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply) || !matches!(id, ADD | INSPECT), "{id}: {reply}");
    reply
}

fn refused(s: &mut Session, cmd: &str, p: Value) -> String {
    s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

fn saved(s: &Session) -> String {
    s.drawing("test").unwrap().to_json().unwrap()
}

/// A box 80 x 40 x 30 mm along X, Y and Z from the origin, placed twice as a part (the second copy
/// 100 along X), and a drawing of it, open, with a front view at 1:1 at (200, 150). The session, the
/// sheet, the view, and the two occurrences.
fn placed() -> (Session, u64, u64, [u64; 2]) {
    let mut s = Session::default();
    let first = run(&mut s, "component.create", json!({"name": "Bracket"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut ids::IdSource::seeded(4896)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_shown_percent: 0.0 })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, sheet, id, [first, second])
}

/// A note on the Box's bottom front edge, through `occurrence`.
fn on_edge(s: &mut Session, view: u64, occurrence: u64) -> Value {
    run(
        s,
        ADD,
        json!({"anchor": {"edge": {"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [occurrence]}}}, "at": [60, 40], "content": "Deburr"}),
    )
}

fn ends_at(reply: &Value, p: [f64; 3]) {
    let point: Vec<f64> = reply["point"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(point.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-9), "{reply}");
}

/// Every anchor: a sheet by name, a view, an edge and a corner, each one undo step; a note that points
/// at nothing reads no model, and one on an edge has its leader end on the edge's middle.
#[test]
fn a_note_goes_on_a_sheet_a_view_an_edge_or_a_corner() {
    let (mut s, sheet, view, [bracket, _]) = placed();
    let evaluations = s.evaluations;
    let on_sheet = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": "General note"}));
    assert_eq!(on_sheet["record"]["anchor"], json!({"sheet": ids::encode(sheet)}));
    assert_eq!((&on_sheet["status"], &on_sheet["exact"], on_sheet.get("point")), (&json!("ok"), &json!(true), None), "{on_sheet}");
    assert_eq!(on_sheet["record"]["content"], json!([{"text": "General note"}]));
    let on_view = run(
        &mut s,
        ADD,
        json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": [{"text": "Sheet "}, {"field": {"source": "sheet", "key": "number"}}], "cap_mm": 5}),
    );
    assert_eq!((&on_view["record"]["anchor"], &on_view["record"]["cap_mm"]), (&json!({"view": ids::encode(view)}), &json!(5.0)), "{on_view}");
    let edge = on_edge(&mut s, view, bracket);
    assert_eq!((&edge["status"], &edge["exact"]), (&json!("ok"), &json!(true)), "{edge}");
    assert_eq!(edge["record"]["anchor"]["edge"]["edge"]["path"], json!([bracket]));
    ends_at(&edge, [40.0, 0.0, 0.0]);
    let corner = run(
        &mut s,
        ADD,
        json!({"anchor": {"corner": {"view": ids::encode(view), "corner": {"point": [80, 0, 30], "path": [bracket]}}}, "at": [90, 40], "content": "Here"}),
    );
    ends_at(&corner, [80.0, 0.0, 30.0]);
    assert_eq!(s.evaluations, evaluations);
    let before = saved(&s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().notes.len(), 3, "one undo step each");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(saved(&s), before);
}

/// The same edge through each copy names its own: the two placements' leaders end 100 mm apart.
#[test]
fn two_copies_are_told_apart_by_path() {
    let (mut s, _, view, [first, second]) = placed();
    ends_at(&on_edge(&mut s, view, first), [40.0, 0.0, 0.0]);
    let other = run(
        &mut s,
        ADD,
        json!({"anchor": {"edge": {"view": ids::encode(view), "edge": {"point": [140, 0, 0], "path": [second]}}}, "at": [60, 40], "content": "x"}),
    );
    ends_at(&other, [140.0, 0.0, 0.0]);
}

/// A note on an edge follows a model change: the box 20 longer moves its leader's end 10. Filleting
/// the edge loses it: the note and its text stay where they are, with no leader; undone, it is back.
#[test]
fn a_note_on_an_edge_follows_the_model_and_keeps_its_place_when_lost() {
    let (mut s, _, view, [bracket, _]) = placed();
    let id = on_edge(&mut s, view, bracket)["note"].clone();
    let inspect = |s: &mut Session| run(s, INSPECT, json!({"note": id}));
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 100}}));
    run(&mut s, "drawing.open", drawing.clone());
    let grown = inspect(&mut s);
    assert_eq!(grown["status"], "ok", "{grown}");
    ends_at(&grown, [50.0, 0.0, 0.0]);
    let place = grown["record"]["at"].clone();

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": "Bracket"}));
    run(&mut s, "solid.fillet", json!({"edges": [[50, 0, 0]], "radius": 2, "body": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing);
    let lost = inspect(&mut s);
    assert_ne!(lost["status"], "ok", "{lost}");
    assert_eq!((lost.get("point"), &lost["record"]["at"], &lost["record"]["content"]), (None, &place, &json!([{"text": "Deburr"}])), "{lost}");
}

/// An edit changes what it is told, once, and an edit with nothing new makes no step. What it belongs
/// to stays. A view's note goes with its view's copy and delete; a sheet's stays, and is selected and
/// deleted by its id.
#[test]
fn an_edit_is_one_step_and_a_view_takes_its_notes_along() {
    let (mut s, _, view, [bracket, _]) = placed();
    let id = on_edge(&mut s, view, bracket)["note"].clone();
    let free = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": "Stays"}))["note"].clone();
    let reply = run(&mut s, EDIT, json!({"note": id, "set": {"at": [1, 2], "content": "Other", "cap_mm": 7}}));
    assert_eq!(reply["changed"], json!(["at", "content", "cap_mm"]));
    let reply = run(&mut s, EDIT, json!({"note": id, "set": {"at": [1, 2]}}));
    assert_eq!(reply["changed"], json!([]));
    let reply = run(&mut s, EDIT, json!({"note": id, "set": {"cap_mm": null}}));
    assert_eq!(reply["changed"], json!(["cap_mm"]));
    assert!(s.execute(EDIT, &json!({"note": id, "set": {"anchor": {"view": ids::encode(view)}}})).is_err());

    let copy = run(
        &mut s,
        "drawing.view.copy_to_sheet",
        json!({"view": ids::encode(view), "sheet": "Sheet 1", "position_mm": [100, 100], "with_linked": false}),
    );
    let copied = solidcraft_doc::ids::decode(copy["view"].as_str().unwrap()).unwrap();
    let d = s.drawing("test").unwrap().clone();
    assert_eq!(d.notes.len(), 3, "the view's note is copied, the sheet's isn't");
    assert_eq!(d.notes.iter().filter(|n| n.anchor.view() == Some(copied)).count(), 1);

    run(&mut s, "selection.delete", json!({"items": [{"type": "drawing", "drawing": ids::encode(d.id), "object": ids::encode(view)}]}));
    let left: Vec<Value> = s.drawing("test").unwrap().notes.iter().map(|n| json!(ids::encode(n.id))).collect();
    assert!(left.contains(&free) && !left.contains(&id), "{left:?}");
    // A note is selected by its id, as a balloon is, and its delete takes it.
    let picked = json!({"type": "drawing", "drawing": ids::encode(d.id), "object": free});
    assert_eq!(run(&mut s, "select.set", json!({"items": [picked.clone()]}))["selected"], json!(1));
    run(&mut s, "selection.delete", json!({"items": [picked]}));
    assert_eq!(s.drawing("test").unwrap().notes.len(), 1);
}

#[test]
fn hostile_notes_are_refused() {
    let (mut s, _, view, [bracket, _]) = placed();
    let before = saved(&s);
    let good = |at: Value, content: Value| json!({"anchor": {"view": ids::encode(view)}, "at": at, "content": content});
    let cases = [
        (json!({"anchor": {"sheet": "no such"}, "at": [0, 0], "content": "x"}), "no such"),
        (json!({"anchor": {"view": ids::encode(99)}, "at": [0, 0], "content": "x"}), "no view"),
        (json!({"anchor": {"edge": {"view": ids::encode(view), "edge": [1e9, 0, 0]}}, "at": [0, 0], "content": "x"}), "edge"),
        (json!({"anchor": {"corner": {"view": ids::encode(view), "corner": [1e9, 0, 0]}}, "at": [0, 0], "content": "x"}), "corner"),
        (json!({"anchor": {"page": 1}, "at": [0, 0], "content": "x"}), "anchor"),
        (good(json!([5000.1, 0]), json!("x")), "5000"),
        (good(json!([0]), json!("x")), "at"),
        (good(json!([0, 0]), json!([])), "some text"),
        (good(json!([0, 0]), json!("x".repeat(4097))), "4096"),
        (good(json!([0, 0]), json!([{"field": {"source": "prompt", "key": "x"}}])), "fields"),
        (good(json!([0, 0]), json!(7)), "content"),
        (json!({"anchor": {"view": ids::encode(view)}, "at": [0, 0], "content": "x", "cap_mm": 0.1}), "text height"),
        (json!({"anchor": {"view": ids::encode(view)}, "at": [0, 0], "content": "x", "extra": 1}), "extra"),
    ];
    for (p, says) in cases {
        let e = refused(&mut s, ADD, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(saved(&s), before, "{p}");
    }
    let e = refused(&mut s, INSPECT, json!({"note": ids::encode(99)}));
    assert!(e.contains("no note"), "{e}");
    let e = refused(&mut s, EDIT, json!({"note": ids::encode(99), "set": {}}));
    assert!(e.contains("no note"), "{e}");
    let id = on_edge(&mut s, view, bracket)["note"].clone();
    let e = refused(&mut s, EDIT, json!({"note": id, "set": {"content": []}}));
    assert!(e.contains("some text"), "{e}");
}
