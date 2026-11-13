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
    d.add_views([Arc::new(View {
        id,
        sheet,
        kind,
        place: ViewPlace::At([200.0, 150.0]),
        settings,
        reference_shown_percent: 0.0,
        centre_marks: None,
    })]);
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

/// A note's leader attaches only to a part its view draws. On the copy its view hides, an edge or
/// a corner is refused and the drawing left as it was; on the other copy it is added; a note on the
/// view or the sheet points at no part. One added before the hide stays, and inspect warns until
/// the edit is undone. So for each branch: one added on that copy is refused, by add and by edit;
/// one there from before warns, and can be taken out.
#[test]
fn a_notes_leader_is_refused_on_a_part_its_view_leaves_out() {
    let (mut s, _, view, [first, second]) = placed();
    let v = ids::encode(view);
    let kept = on_edge(&mut s, view, first)["note"].clone();
    let on_first = json!([{"corner": {"point": [80, 0, 30], "path": [first]}}]);
    let branched = run(&mut s, ADD, json!({"anchor": {"view": v}, "at": [0, 0], "content": "B", "branches": on_first}))["note"].clone();
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[first]]}}));
    let words = format!("its hide leaves out occurrence path [{first}]");
    let warned = run(&mut s, INSPECT, json!({"note": kept}));
    assert_eq!(warned["warnings"], json!([format!("view {v} doesn't draw this part now: {words}")]), "{warned}");
    let warned = run(&mut s, INSPECT, json!({"note": branched}));
    assert_eq!(warned["warnings"], json!([format!("view {v} doesn't draw this part now: {words}")]), "{warned}");
    let before = saved(&s);
    let edge = json!({"anchor": {"edge": {"view": v, "edge": {"point": [40, 0, 0], "path": [first]}}}, "at": [0, 0], "content": "A"});
    let corner = json!({"anchor": {"corner": {"view": v, "corner": {"point": [80, 0, 30], "path": [first]}}}, "at": [0, 0], "content": "A"});
    let branch = json!({"anchor": {"view": v}, "at": [0, 0], "content": "A", "branches": [{"edge": {"point": [40, 0, 0], "path": [first]}}]});
    for p in [edge, corner, branch] {
        let e = refused(&mut s, ADD, p.clone());
        assert!(e.contains(&format!("view {v} doesn't draw that part: {words}")), "{p}: {e}");
    }
    let added = json!({"note": kept, "set": {"branches": {"add": [{"edge": {"point": [40, 0, 30], "path": [first]}}]}}});
    let e = refused(&mut s, EDIT, added);
    assert!(e.contains(&format!("view {v} doesn't draw that part: {words}")), "{e}");
    assert_eq!(saved(&s), before);
    let taken_out = run(&mut s, EDIT, json!({"note": branched, "set": {"branches": {"remove": [0]}}}));
    assert_eq!(taken_out["changed"], json!(["branches"]));
    run(&mut s, "edit.undo", json!({}));
    let on_second = json!({"anchor": {"edge": {"view": v, "edge": {"point": [140, 0, 0], "path": [second]}}}, "at": [0, 0], "content": "A"});
    assert_eq!(run(&mut s, ADD, on_second)["status"], "ok");
    run(&mut s, ADD, json!({"anchor": {"view": v}, "at": [0, 0], "content": "On the view"}));
    run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [0, 0], "content": "On the sheet"}));
    for _ in 0..4 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert!(run(&mut s, INSPECT, json!({"note": kept}))["warnings"].is_null());
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

/// `reply`'s branches: each one's status and, when found, where it ends.
fn branch_ends(reply: &Value) -> Vec<(String, Option<[f64; 3]>)> {
    let ends = reply["branches"].as_array().map(Vec::as_slice).unwrap_or_default();
    let point = |b: &Value| b.get("point").map(|p| [0, 1, 2].map(|i| p[i].as_f64().unwrap()));
    ends.iter().map(|b| (b["status"].as_str().unwrap().to_string(), point(b))).collect()
}

fn ends_near(found: &[(String, Option<[f64; 3]>)], want: &[(&str, Option<[f64; 3]>)]) {
    let near = |a: &Option<[f64; 3]>, b: &Option<[f64; 3]>| match (a, b) {
        (Some(a), Some(b)) => a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9),
        (None, None) => true,
        _ => false,
    };
    let same = found.len() == want.len() && found.iter().zip(want).all(|((s, a), (t, b))| s == t && near(a, b));
    assert!(same, "{found:?} != {want:?}");
}

/// A note with three leaders, its anchor's and two branches, follows a model change on each: the
/// box 20 longer moves them. Filleting one branch's edge loses only that branch; the note and its
/// other leaders stay.
#[test]
fn a_note_with_three_leaders_follows_the_model_and_loses_only_its_own() {
    let (mut s, _, view, [bracket, _]) = placed();
    let anchor = json!({"edge": {"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [bracket]}}});
    let branches = json!([{"corner": {"point": [80, 40, 0], "path": [bracket]}}, {"edge": {"point": [40, 0, 30], "path": [bracket]}}]);
    let added = run(&mut s, ADD, json!({"anchor": anchor, "at": [60, 40], "content": "Deburr", "branches": branches}));
    ends_at(&added, [40.0, 0.0, 0.0]);
    ends_near(&branch_ends(&added), &[("ok", Some([80.0, 40.0, 0.0])), ("ok", Some([40.0, 0.0, 30.0]))]);
    assert_eq!(added["record"]["branches"].as_array().map(Vec::len), Some(2), "{added}");
    let id = added["note"].clone();
    let inspect = |s: &mut Session| run(s, INSPECT, json!({"note": id}));
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 100}}));
    run(&mut s, "drawing.open", drawing.clone());
    let grown = inspect(&mut s);
    ends_at(&grown, [50.0, 0.0, 0.0]);
    ends_near(&branch_ends(&grown), &[("ok", Some([100.0, 40.0, 0.0])), ("ok", Some([50.0, 0.0, 30.0]))]);

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": "Bracket"}));
    run(&mut s, "solid.fillet", json!({"edges": [[50, 0, 30]], "radius": 2, "body": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing);
    let filleted = inspect(&mut s);
    assert_eq!(filleted["status"], "ok", "{filleted}");
    ends_at(&filleted, [50.0, 0.0, 0.0]);
    let ends = branch_ends(&filleted);
    ends_near(&ends[..1], &[("ok", Some([100.0, 40.0, 0.0]))]);
    assert!(ends.len() == 2 && ends[1].0 != "ok" && ends[1].1.is_none(), "{ends:?}");
}

/// The branches edit takes out by number as the list is now, then adds after those that stay, in one
/// undo step, keeping the note's id; one that changes nothing makes no step. A wrong number, a
/// repeat or a 17th leader is refused before anything is looked up, and the note is left as it was.
#[test]
fn a_notes_branches_edit_keeps_its_id_in_one_step() {
    let (mut s, _, view, [bracket, _]) = placed();
    let id = on_edge(&mut s, view, bracket)["note"].clone();
    let corner = |p: [i32; 3]| json!({"corner": {"point": p, "path": [bracket]}});
    let edit = |s: &mut Session, branches: Value| run(s, EDIT, json!({"note": id, "set": {"branches": branches}}));
    let ends = |s: &mut Session| branch_ends(&run(s, INSPECT, json!({"note": id})));
    let steps = s.undo.len();
    assert_eq!(edit(&mut s, json!({"add": [corner([80, 0, 30]), corner([0, 0, 30])]}))["changed"], json!(["branches"]));
    ends_near(&ends(&mut s), &[("ok", Some([80.0, 0.0, 30.0])), ("ok", Some([0.0, 0.0, 30.0]))]);
    let reply = edit(&mut s, json!({"remove": [0], "add": [corner([80, 40, 30])]}));
    assert_eq!((&reply["note"], &reply["changed"]), (&id, &json!(["branches"])));
    ends_near(&ends(&mut s), &[("ok", Some([0.0, 0.0, 30.0])), ("ok", Some([80.0, 40.0, 30.0]))]);
    assert_eq!(s.undo.len(), steps + 2, "one step each");
    assert_eq!(edit(&mut s, json!({}))["changed"], json!([]));
    assert_eq!(s.undo.len(), steps + 2, "no step when nothing changes");
    let before = saved(&s);
    // The far corners are far from the part but inside the parameters' limit (under 1e9), so only the count or
    // the lookup refuses them.
    let refusals = [
        (json!({"remove": [2]}), "`branches.remove` numbers this note's branches, 0 to 1"),
        (json!({"remove": [1, 1]}), "names each branch once"),
        (json!({"add": vec![json!({"corner": {"point": [1e8, 0, 0], "path": [bracket]}}); 14]}), "at most 16 leaders, its anchor's included"),
        (json!({"add": [{"corner": {"point": [1e8, 0, 0], "path": [bracket]}}]}), "corner"),
        (json!({"add": [{"face": [0, 0, 0]}]}), "face"),
    ];
    for (branches, says) in refusals {
        let e = refused(&mut s, EDIT, json!({"note": id, "set": {"branches": branches}}));
        assert!(e.contains(says), "{branches}: {e}");
        assert_eq!(saved(&s), before, "{branches}");
    }
    // 13 more make 16 leaders, the anchor's included.
    edit(&mut s, json!({"add": vec![corner([0, 0, 0]); 13]}));
    assert_eq!(ends(&mut s).len(), 15);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(ends(&mut s).len(), 2, "undone in one step");
}

/// A note on a view points at nothing itself: its branches are its leaders, its first branch its
/// first, 16 in all. A note on a sheet has none.
#[test]
fn a_note_on_a_view_has_its_branches_as_its_leaders() {
    let (mut s, _, view, [bracket, _]) = placed();
    let corners = |n: usize| json!(vec![json!({"corner": {"point": [80, 0, 30], "path": [bracket]}}); n]);
    let on_view = |n: usize| json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": "Chamfer", "branches": corners(n)});
    let sixteen = run(&mut s, ADD, on_view(16));
    assert_eq!((&sixteen["status"], sixteen.get("point")), (&json!("ok"), None), "{sixteen}");
    assert_eq!(branch_ends(&sixteen).len(), 16);
    let before = saved(&s);
    for (p, says) in [
        (on_view(17), "a note has at most 16 leaders, its anchor's included"),
        (json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": "x", "branches": corners(1)}), "a note on a sheet has no branches"),
    ] {
        let e = refused(&mut s, ADD, p);
        assert!(e.contains(says), "{e}");
        assert_eq!(saved(&s), before);
    }
    let e = refused(&mut s, EDIT, json!({"note": sixteen["note"], "set": {"branches": {"add": corners(1)}}}));
    assert!(e.contains("at most 16 leaders"), "{e}");
    let sheet = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": "x"}))["note"].clone();
    let e = refused(&mut s, EDIT, json!({"note": sheet, "set": {"branches": {"add": corners(1)}}}));
    assert!(e.contains("a note on a sheet has no branches"), "{e}");
    let e = refused(&mut s, EDIT, json!({"note": sheet, "set": {"branches": {"remove": [0]}}}));
    assert!(e.contains("this note has no branches"), "{e}");
}

/// A note's fields read its sheet: `Material: {component.material}` on a sheet documenting a steel
/// block says `Material: Steel`, and the new material after `material.assign`. Inspect tells each
/// field shown, empty or lost: once the documented component is deleted its fields are lost, and
/// say nothing, as an empty one does.
#[test]
fn a_notes_fields_read_its_sheet_and_inspect_tells_a_lost_one() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name": "Block"}))["component"].clone();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Steel"}));
    run(&mut s, "drawing.create", json!({}));
    let source = ids::encode(s.drawing("test").unwrap().sources[0].id);
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": source, "component": component}}));
    let field = |source: &str, key: &str| json!({"field": {"source": source, "key": key}});
    let content = json!([{"text": "Material: "}, field("component", "material"), {"text": " ("}, field("sheet", "name"), {"text": ")"}, field("drawing", "organization")]);
    let note = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": content}))["note"].clone();
    let shown = |state: &str, value: Option<&str>, source: &str, key: &str| {
        let mut one = json!({"source": source, "key": key, "state": state});
        if let Some(value) = value {
            one["value"] = json!(value);
        }
        one
    };
    let inspect = |s: &mut Session| {
        let reply = run(s, INSPECT, json!({"note": note}));
        (reply["text"].clone(), reply["fields"].clone())
    };
    let on_model = |s: &mut Session, id: &str, p: Value| {
        let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
        run(s, "drawing.close", json!({}));
        run(s, id, p);
        run(s, "drawing.open", drawing);
    };
    let sheet = shown("shown", Some("Sheet 1"), "sheet", "name");
    let organization = shown("empty", None, "drawing", "organization");
    let steel = json!([shown("shown", Some("Steel"), "component", "material"), sheet.clone(), organization.clone()]);
    assert_eq!(inspect(&mut s), (json!("Material: Steel (Sheet 1)"), steel));
    on_model(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Aluminum"}));
    assert_eq!(inspect(&mut s).0, json!("Material: Aluminum (Sheet 1)"));
    on_model(&mut s, "component.delete", json!({"component": component}));
    let lost = json!([shown("lost", None, "component", "material"), sheet, organization]);
    assert_eq!(inspect(&mut s), (json!("Material:  (Sheet 1)"), lost));
    // A note on plain text reads no field.
    let plain = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 40], "content": "Deburr"}));
    assert_eq!((&plain["text"], plain.get("fields")), (&json!("Deburr"), None), "{plain}");
}

/// A field its source doesn't have is refused when a note is added or edited, and the note is left
/// as it was; a drawing saved with one still opens, and inspect says the field is unknown and why.
#[test]
fn a_field_its_source_lacks_is_refused_and_one_saved_reads_unknown() {
    let (mut s, _, view, _) = placed();
    let weight = json!([{"text": "Mass: "}, {"field": {"source": "component", "key": "weight"}}]);
    let why = "the component has no field `weight`";
    let e = refused(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": weight}));
    assert!(e.contains(why), "{e}");
    let note = run(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": "Mass"}))["note"].clone();
    let before = saved(&s);
    let e = refused(&mut s, EDIT, json!({"note": note, "set": {"content": weight}}));
    assert!(e.contains(why), "{e}");
    assert_eq!(saved(&s), before, "the refused edit changed nothing");
    // As a file saved with one opens.
    let mut v: Value = serde_json::from_str(&before).unwrap();
    v["notes"][0]["content"] = weight.clone();
    let opened = solidcraft_doc::drawing::Drawing::from_json(&v.to_string()).unwrap();
    s.doc_mut().put_drawing(Arc::new(opened));
    let reply = run(&mut s, INSPECT, json!({"note": note}));
    assert_eq!(reply["text"], json!("Mass: "));
    assert_eq!(reply["fields"], json!([{"source": "component", "key": "weight", "state": "unknown", "why": why}]));
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
        (good(json!([0, 0]), json!([{"field": {"source": "sheet", "key": "page"}}])), "the sheet has no field `page`"),
        (good(json!([0, 0]), json!(7)), "content"),
        (json!({"anchor": {"view": ids::encode(view)}, "at": [0, 0], "content": "x", "cap_mm": 0.1}), "text height"),
        (json!({"anchor": {"view": ids::encode(view)}, "at": [0, 0], "content": "x", "extra": 1}), "extra"),
        (json!({"anchor": {"sheet": "Sheet 1"}, "at": [0, 0], "content": "x", "branches": [{"corner": [0, 0, 0]}]}), "no branches"),
        (json!({"anchor": {"view": ids::encode(view)}, "at": [0, 0], "content": "x", "branches": [{"edge": [1e8, 0, 0]}]}), "edge"),
        (
            json!({"anchor": {"view": ids::encode(view)}, "at": [0, 0], "content": "x", "branches": vec![json!({"corner": [1e8, 0, 0]}); 17]}),
            "16 leaders",
        ),
        (
            // Branches far from the part but inside the parameters' limit (under 1e9): only the count refuses them.
            json!({"anchor": {"edge": {"view": ids::encode(view), "edge": [1e8, 0, 0]}}, "at": [0, 0], "content": "x", "branches": vec![json!({"corner": [1e8, 0, 0]}); 16]}),
            "16 leaders",
        ),
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
