use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::drawing_view::NamedView;
use solidcraft_geom::prelude::*;

use super::*;
use crate::drawing_scene::tests::in_the_model;
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
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_shown_percent: 0.0, label: None })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, sheet, id, [first, second])
}

/// A centred note on the Box's bottom front edge, through `occurrence`.
fn on_edge(s: &mut Session, view: u64, occurrence: u64) -> Value {
    run(
        s,
        ADD,
        json!({"anchor": {"edge": {"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [occurrence]}}}, "at": [60, 40], "content": "Deburr", "align": "centre"}),
    )
}

fn ends_at(reply: &Value, p: [f64; 3]) {
    let point: Vec<f64> = reply["point"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(point.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-9), "{reply}");
}

/// A centred note's text is centred on its place, so one placed near the paper's edge can run past it: its
/// reply warns, naming where its text spans, and one that fits says nothing. A note on a view is held
/// to the paper where its view puts it.
#[test]
fn a_note_whose_text_runs_off_the_paper_is_warned_about() {
    let (mut s, _, view, _) = placed();
    let off_paper =
        |reply: &Value| reply["warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().unwrap_or("").contains("runs off the paper")));
    let long = "ALL HOLES THROUGH. MATERIAL: AL 6061.";
    let edge = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [10, 40], "content": long, "align": "centre"}));
    assert!(off_paper(&edge), "{edge}");
    let note = edge["note"].as_str().unwrap().to_string();
    assert!(off_paper(&run(&mut s, INSPECT, json!({"note": note}))), "inspect warns too");
    let inside = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [200, 40], "content": long, "align": "centre"}));
    assert!(!off_paper(&inside), "{inside}");
    // 200 mm left of a view at x = 200 is the paper's left edge.
    let on_view = run(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [-195, 0], "content": long, "align": "centre"}));
    assert!(off_paper(&on_view), "{on_view}");
    let centred = run(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [0, -60], "content": long, "align": "centre"}));
    assert!(!off_paper(&centred), "{centred}");
}

/// A note starting at its place runs off the paper's right edge from near it, and one ending there
/// does not; the same place, and the text is the same width: the warning follows the alignment.
#[test]
fn a_note_is_held_to_the_paper_by_its_alignment() {
    let (mut s, _, _, _) = placed();
    let off_paper =
        |reply: &Value| reply["warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().unwrap_or("").contains("runs off the paper")));
    let width = s.drawing("test").unwrap().sheets[0].sheet.size_mm().unwrap().x;
    let long = "ALL HOLES THROUGH. MATERIAL: AL 6061.";
    let at = json!([width - 10.0, 40]);
    let sheet = json!({"sheet": "Sheet 1"});
    let left = run(&mut s, ADD, json!({"anchor": sheet, "at": at, "content": long}));
    assert!(off_paper(&left), "left, the default: {left}");
    assert!(left["warnings"][0].as_str().unwrap().contains("starting at its place"), "{left}");
    let right = run(&mut s, ADD, json!({"anchor": sheet, "at": at, "content": long, "align": "right"}));
    assert!(!off_paper(&right), "{right}");
    let near_left = json!([10, 40]);
    let right = run(&mut s, ADD, json!({"anchor": sheet, "at": near_left, "content": long, "align": "right"}));
    assert!(off_paper(&right) && right["warnings"][0].as_str().unwrap().contains("ending at its place"), "{right}");
    let left = run(&mut s, ADD, json!({"anchor": sheet, "at": near_left, "content": long, "align": "left"}));
    assert!(!off_paper(&left), "{left}");
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
    let sheet = shown("shown", Some("Sheet 1"), "sheet", "name");
    let organization = shown("empty", None, "drawing", "organization");
    let steel = json!([shown("shown", Some("Steel"), "component", "material"), sheet.clone(), organization.clone()]);
    assert_eq!(inspect(&mut s), (json!("Material: Steel (Sheet 1)"), steel));
    in_the_model(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Aluminum"}));
    assert_eq!(inspect(&mut s).0, json!("Material: Aluminum (Sheet 1)"));
    in_the_model(&mut s, "component.delete", json!({"component": component}));
    let lost = json!([shown("lost", None, "component", "material"), sheet, organization]);
    assert_eq!(inspect(&mut s), (json!("Material:  (Sheet 1)"), lost));
    // A note on plain text reads no field.
    let plain = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 40], "content": "Deburr"}));
    assert_eq!((&plain["text"], plain.get("fields")), (&json!("Deburr"), None), "{plain}");
}

/// A note on a drawing of a sheet metal part reads the part's sheet metal fields, and follows the
/// rule's thickness and K-factor when they change. The part is oracle 71's: rule `Custom`, 1.5 mm
/// (the parameter `Thick`), K-factor 0.33, radius 3 mm, an 80 x 50 base flanged 20 mm on both long
/// edges. Its flat is 80 wide and 50 − 2·(1.5 + 3) + 2·(π/2)(3 + 0.33·1.5) + 2·15.5 = 82.980 long;
/// at 2 mm, 50 − 10 + π(3 + 0.66) + 30 = 81.498; and with K 0.5, 70 + π(3 + 1) = 82.566.
#[test]
fn a_notes_sheet_metal_fields_follow_the_rule() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "Thick", "expression": "1.5 mm"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "Custom", "thickness": "Thick", "k_factor": 0.33, "bend_radius": "3 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [80, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    run(&mut s, "sheet.flange", json!({"edges": [[40, 50, 1.5], [40, 0, 1.5]], "height": 20}));
    run(&mut s, "drawing.create", json!({}));
    let source = ids::encode(s.drawing("test").unwrap().sources[0].id);
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": source}}));
    let field = |key: &str| json!({"field": {"source": "component", "key": key}});
    let content = json!([
        {"text": "T "}, field("thickness"), {"text": " "}, field("sheet_rule"), {"text": " K "}, field("k_factor"),
        {"text": " R "}, field("bend_radius"), {"text": " FLAT "}, field("flat_length"), {"text": " x "}, field("flat_width"),
    ]);
    let note = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": content}))["note"].clone();
    let text = |s: &mut Session| run(s, INSPECT, json!({"note": note}))["text"].clone();
    assert_eq!(text(&mut s), json!("T 1.50 mm Custom K 0.33 R 3.00 mm FLAT 82.98 mm x 80.00 mm"));
    in_the_model(&mut s, "parameters.change", json!({"name": "Thick", "expression": "2 mm"}));
    assert_eq!(text(&mut s), json!("T 2.00 mm Custom K 0.33 R 3.00 mm FLAT 81.50 mm x 80.00 mm"));
    in_the_model(&mut s, "sheet.manage_rules", json!({"name": "Custom", "k_factor": 0.5}));
    assert_eq!(text(&mut s), json!("T 2.00 mm Custom K 0.5 R 3.00 mm FLAT 82.57 mm x 80.00 mm"));
    let fields = run(&mut s, INSPECT, json!({"note": note}))["fields"].clone();
    assert_eq!(fields[0], json!({"source": "component", "key": "thickness", "state": "shown", "value": "2.00 mm"}), "{fields}");
}

/// A note's sheet metal field on a part that isn't sheet metal reads empty, never a number: the
/// box's drawing shows nothing for its thickness, and inspect says the field is empty.
#[test]
fn a_sheet_metal_field_on_a_part_without_a_sheet_reads_empty() {
    let (mut s, _, view, _) = placed();
    let content = json!([{"text": "T "}, {"field": {"source": "component", "key": "thickness"}}]);
    let source = ids::encode(s.drawing("test").unwrap().sources[0].id);
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": source}}));
    let note = run(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": content}))["note"].clone();
    let reply = run(&mut s, INSPECT, json!({"note": note}));
    assert_eq!(reply["text"], json!("T "), "{reply}");
    assert_eq!(reply["fields"], json!([{"source": "component", "key": "thickness", "state": "empty"}]), "{reply}");
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

/// That `reply` reads ok with its leader ending on `p`, to within what the joints' solve places parts to.
fn leader_ends_near(reply: &Value, p: [f64; 3]) {
    let point: Vec<f64> = reply["point"].as_array().unwrap_or_else(|| panic!("no point: {reply}")).iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(reply["status"] == "ok" && point.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-6), "{reply}");
}

/// A note with a leader on the model goes on a view in a position of its own, and its leader ends
/// where that view draws the edge (#13048): on [`hinge_pair`](crate::drawing_scene::tests::hinge_pair)'s
/// view with the leaf turned up 90° about the hinge line (y 20, z 0), the leaf's free bottom edge,
/// (0..40, 40, 0) as designed, has its middle at (20, 20, 20). A branch can be added there too.
#[test]
fn a_note_on_a_posed_view_ends_where_the_view_draws_its_edge() {
    let (mut s, view, _) = crate::drawing_scene::tests::hinge_pair();
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    let v = ids::encode(view);
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"pose": [{"joint": "Hinge", "values": [90]}]}}));
    let edge = json!({"point": [20, 40, 0], "path": [leaf]});
    let added = run(&mut s, ADD, json!({"anchor": {"edge": {"view": v, "edge": edge}}, "at": [60, 40], "content": "Deburr"}));
    leader_ends_near(&added, [20.0, 20.0, 20.0]);
    leader_ends_near(&run(&mut s, INSPECT, json!({"note": added["note"]})), [20.0, 20.0, 20.0]);
    let plain = run(&mut s, ADD, json!({"anchor": {"view": v}, "at": [0, -30], "content": "Turned up"}))["note"].clone();
    let branched = run(&mut s, EDIT, json!({"note": plain, "set": {"branches": {"add": [{"edge": edge}]}}}));
    assert_eq!(branched["changed"], json!(["branches"]), "{branched}");
}

/// A note with a leader on the model goes on a view that draws an exploded view, and its leader ends
/// where that view draws the edge: with the second copy moved 50 mm up, its Box's bottom front edge,
/// (100..180, 0, 0) assembled, has its middle at (140, 0, 0 + 50).
#[test]
fn a_note_on_an_exploded_view_ends_where_the_view_draws_its_edge() {
    let (mut s, _, view, [_, second]) = placed();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": second, "translate": [0, 0, 50]}]}));
    run(&mut s, "drawing.open", drawing);
    let v = ids::encode(view);
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"exploded": "Apart"}}));
    let edge = json!({"point": [140, 0, 0], "path": [second]});
    let added = run(&mut s, ADD, json!({"anchor": {"edge": {"view": v, "edge": edge}}, "at": [60, 40], "content": "Deburr"}));
    leader_ends_near(&added, [140.0, 0.0, 50.0]);
    leader_ends_near(&run(&mut s, INSPECT, json!({"note": added["note"]})), [140.0, 0.0, 50.0]);
}

/// A new note starts at its place: left unless it says otherwise, kept in its record, and an edit sets
/// it in one undo step, none when it is as it was. Centre is spelt either way and saved as `centre`.
#[test]
fn a_new_note_is_left_aligned_and_an_edit_sets_it() {
    let (mut s, _, _, _) = placed();
    let anchor = json!({"sheet": "Sheet 1"});
    let plain = run(&mut s, ADD, json!({"anchor": anchor, "at": [15, 60], "content": "2 x 8.5mm Holes - Smooth Bore"}));
    assert_eq!((&plain["align"], &plain["record"]["align"]), (&json!("left"), &json!("left")), "{plain}");
    let right = run(&mut s, ADD, json!({"anchor": anchor, "at": [15, 60], "content": "x", "align": "right"}));
    assert_eq!(right["align"], json!("right"));
    let centred = run(&mut s, ADD, json!({"anchor": anchor, "at": [15, 60], "content": "x", "align": "center"}));
    assert_eq!((&centred["align"], &centred["record"]["align"]), (&json!("centre"), &json!("centre")), "{centred}");
    let id = plain["note"].clone();
    let steps = |s: &Session| s.drawing("test").unwrap().notes.len();
    assert_eq!(steps(&s), 3);
    let reply = run(&mut s, EDIT, json!({"note": id, "set": {"align": "right"}}));
    assert_eq!(reply["changed"], json!(["align"]));
    assert_eq!(run(&mut s, INSPECT, json!({"note": id}))["align"], json!("right"));
    let before = saved(&s);
    assert_eq!(run(&mut s, EDIT, json!({"note": id, "set": {"align": "right"}}))["changed"], json!([]));
    assert_eq!(saved(&s), before, "no change, no step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, INSPECT, json!({"note": id}))["align"], json!("left"));
    for bad in [json!("middle"), json!(1), json!(["left"])] {
        refused(&mut s, EDIT, json!({"note": id, "set": {"align": bad}}));
        refused(&mut s, ADD, json!({"anchor": anchor, "at": [0, 0], "content": "x", "align": bad}));
    }
}

/// A point of a reply, `[x, y]`.
fn xy(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

/// `reply`'s layout: its leaders point by point, its arrows (each tip and the unit direction it points
/// in) and its text box's corners.
fn laid_out(reply: &Value) -> (Vec<Vec<Vec2>>, Vec<(Vec2, Vec2)>, Option<Vec<Vec2>>) {
    let layout = reply.get("layout").unwrap_or_else(|| panic!("no layout: {reply}"));
    let points = |line: &Value| line.as_array().unwrap().iter().map(xy).collect::<Vec<_>>();
    let leaders = layout["leaders"].as_array().unwrap().iter().map(points).collect();
    let arrows = layout["arrows"].as_array().unwrap().iter().map(|arrow| (xy(&arrow["tip"]), xy(&arrow["points"]))).collect();
    (leaders, arrows, layout.get("text_box").map(points))
}

/// The middle of a box's corners.
fn middle(corners: &[Vec2]) -> Vec2 {
    corners.iter().fold(Vec2::ZERO, |sum, p| sum + *p) * 0.25
}

/// Sheet 1 of the drawing open, drawn at once, as the sheet page and an export draw it.
fn sheet_now(s: &Session) -> crate::drawing_scene::SheetScene {
    let d = s.drawing("test").unwrap().clone();
    crate::drawing_scene::build(s, &d, d.sheets[0].id).unwrap()
}

/// Sheet 1 of the drawing open as a command reads it: for a deferred drawing, what its last update drew.
fn shown_now(s: &mut Session) -> crate::drawing_scene::SheetScene {
    let d = s.drawing("test").unwrap().clone();
    crate::drawing_scene::as_shown(s, &d, d.sheets[0].id).unwrap()
}

/// What the note `added` answered for draws on `sheet`: its strokes point by point, its arrowheads'
/// tips and its text runs' origins.
fn drawn_parts(sheet: &crate::drawing_scene::SheetScene, added: &Value) -> (Vec<Vec<Vec2>>, Vec<Vec2>, Vec<Vec2>) {
    use solidcraft_geom::paper::Item;
    let id = ids::decode(added["note"].as_str().unwrap()).unwrap();
    let group = sheet.scene.groups.iter().find(|g| g.object == id).unwrap_or_else(|| panic!("{} isn't drawn", added["note"]));
    let (mut strokes, mut tips, mut origins) = (Vec::new(), Vec::new(), Vec::new());
    for item in &group.items {
        match item {
            Item::Stroke { path, .. } => strokes.push(std::iter::once(path[0].start()).chain(path.iter().map(|seg| seg.end())).collect()),
            Item::Fill { region } => tips.push(region.outer.segs[0].start()),
            Item::Text(text) => origins.push(text.origin),
            Item::Picture(_) => {}
        }
    }
    (strokes, tips, origins)
}

/// Where the note `added` answered for stands on `sheet`.
fn state_of<'a>(sheet: &'a crate::drawing_scene::SheetScene, added: &Value) -> &'a crate::drawing_scene::NoteState {
    let id = ids::decode(added["note"].as_str().unwrap()).unwrap();
    sheet.notes.iter().find(|n| n.note == id).unwrap_or_else(|| panic!("{} isn't listed", added["note"]))
}

/// A note's layout is where the sheet draws it, in paper mm from the sheet's lower-left corner. The
/// front view at 1:1 has the middle of the two copies' outline (x 90, z 15) on its centre (200, 150),
/// so the Box's bottom front edge's middle (40, 0, 0) is at (150, 135): the leader's tip, its arrow
/// pointing there from its elbow. The note sits 60 right of and 40 above the view's centre, at
/// (260, 190): its leader comes from the left, to the landing's start, then along the landing to the
/// text's gap, level with the text's middle. After its tip, each point is the drawn leader's (whose
/// line starts at its arrowhead's base); the tip is the arrowhead's; the text box's first corner is the
/// text run's origin; all exactly, as one function lays both out and both are moved by the view's
/// centre. With the second copy moved 30 up and not captured, the sheet draws the design's places, and
/// so does the layout.
#[test]
fn a_notes_layout_is_where_the_sheet_draws_it() {
    let (mut s, _, view, [bracket, second]) = placed();
    let added = on_edge(&mut s, view, bracket);
    assert!(added.get("layout_why").is_none(), "{added}");
    let (leaders, arrows, text_box) = laid_out(&added);
    assert!(leaders.len() == 1 && leaders[0].len() == 3 && arrows.len() == 1, "{added}");
    let (tip, elbow, end) = (leaders[0][0], leaders[0][1], leaders[0][2]);
    assert!(tip.dist(Vec2::new(150.0, 135.0)) < 1e-9 && arrows[0].0 == tip, "{added}");
    assert!(arrows[0].1.dist((tip - elbow) * (1.0 / tip.dist(elbow))) < 1e-12, "{added}");
    assert!(elbow.y == 190.0 && end.y == 190.0 && elbow.x < end.x && end.x < 260.0, "{added}");
    let corners = text_box.unwrap_or_else(|| panic!("no text box: {added}"));
    assert!(middle(&corners).dist(Vec2::new(260.0, 190.0)) < 1e-9, "{added}");
    let (strokes, tips, origins) = drawn_parts(&sheet_now(&s), &added);
    assert!(strokes.len() == 1 && strokes[0][1..] == leaders[0][1..], "{strokes:?} {added}");
    assert_eq!((tips, origins), (vec![tip], vec![corners[0]]));
    assert_eq!(run(&mut s, INSPECT, json!({"note": added["note"]})), added, "inspect answers what add did");

    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [0, 0, 30], "capture": false, "move_grounded": true}));
    run(&mut s, "drawing.open", drawing);
    assert_eq!(s.pending_moves.len(), 1);
    assert_eq!(run(&mut s, INSPECT, json!({"note": added["note"]}))["layout"], added["layout"], "where the design places its parts");
}

/// A note with no leader has its text box alone: on the sheet, centred on its place from the sheet's
/// lower-left corner, unmoved; on a view, on its place from the view's centre (200, 150). Its box's
/// first corner is where the sheet starts its text.
#[test]
fn a_note_with_no_leader_has_only_its_text_box() {
    let (mut s, _, view, _) = placed();
    let on_sheet = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": "General note", "align": "centre"}));
    let on_view = run(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": "Chamfer", "align": "centre"}));
    let sheet = sheet_now(&s);
    for (reply, centre) in [(&on_sheet, Vec2::new(20.0, 20.0)), (&on_view, Vec2::new(200.0, 120.0))] {
        let (leaders, arrows, text_box) = laid_out(reply);
        assert!(leaders.is_empty() && arrows.is_empty() && reply.get("layout_why").is_none(), "{reply}");
        let corners = text_box.unwrap_or_else(|| panic!("no text box: {reply}"));
        assert!(middle(&corners).dist(centre) < 1e-9, "{reply}");
        assert_eq!(drawn_parts(&sheet, reply).2, vec![corners[0]], "{reply}");
    }
}

/// A note with three leaders, two from its left and one from its right, has three leaders and three
/// arrows, in its leaders' order, each where the sheet draws it. The first from each side ends in that
/// side's landing, its last two points level with the text's middle (y 190); the second from the left
/// ends where that landing starts. The tips: the Box's bottom front edge's middle (40, 0, 0) at
/// (150, 135), its corner (0, 0, 0) at (110, 135), and the second copy's corner (180, 0, 30) at
/// (290, 165).
#[test]
fn a_note_with_three_leaders_lands_once_per_side() {
    let (mut s, _, view, [bracket, second]) = placed();
    let anchor = json!({"edge": {"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [bracket]}}});
    let branches = json!([{"corner": {"point": [0, 0, 0], "path": [bracket]}}, {"corner": {"point": [180, 0, 30], "path": [second]}}]);
    let added = run(&mut s, ADD, json!({"anchor": anchor, "at": [60, 40], "content": "Deburr", "align": "centre", "branches": branches}));
    let (leaders, arrows, _) = laid_out(&added);
    let tips = [Vec2::new(150.0, 135.0), Vec2::new(110.0, 135.0), Vec2::new(290.0, 165.0)];
    assert!(leaders.len() == 3 && arrows.len() == 3, "{added}");
    for ((leader, (tip, _)), want) in leaders.iter().zip(&arrows).zip(tips) {
        assert!(leader[0] == *tip && tip.dist(want) < 1e-9, "{added}");
    }
    let (left, next, right) = (&leaders[0], &leaders[1], &leaders[2]);
    assert!(left.len() == 3 && next.len() == 2 && right.len() == 3, "{added}");
    assert_eq!(next[1], left[1], "the second from the left meets the landing where it starts");
    assert!(left[2].x < 260.0 && right[2].x > 260.0, "{added}");
    assert!([left[1], left[2], right[1], right[2]].iter().all(|p| p.y == 190.0), "{added}");
    let (strokes, drawn, _) = drawn_parts(&sheet_now(&s), &added);
    assert_eq!(drawn, arrows.iter().map(|(tip, _)| *tip).collect::<Vec<_>>());
    assert!(strokes.len() == 3 && strokes.iter().zip(&leaders).all(|(line, leader)| line[1..] == leader[1..]), "{strokes:?} {added}");
}

/// A note that isn't drawn has no layout, and says why where its status doesn't, in the sheet's words.
/// Its view drawing nothing (both copies hidden, as a file can hold it: a placeholder) or one the sheet
/// can't draw yet (shaded, in a position of its own): "its view isn't drawn", as the sheet says. Lost
/// (its edge filleted away): its text box alone and nothing more than its status, as the sheet keeps
/// its text without a leader. On an exploded view it is drawn where that view draws the moved copy
/// (#13048): its arrow's tip is the sheet's.
#[test]
fn a_note_not_drawn_has_no_layout_and_says_why() {
    let not_drawn = "its view isn't drawn";
    let (mut s, _, view, [first, second]) = placed();
    let added = on_edge(&mut s, view, first);
    let mut d = (**s.drawing("test").unwrap()).clone();
    let record = d.views.iter_mut().find(|v| v.id == view).unwrap();
    Arc::make_mut(record).settings.hide = Some(vec![vec![first], vec![second]].into());
    s.doc_mut().put_drawing(Arc::new(d));
    let hidden = run(&mut s, INSPECT, json!({"note": added["note"]}));
    assert!(hidden.get("layout").is_none() && hidden["layout_why"] == not_drawn, "{hidden}");
    assert_eq!(state_of(&sheet_now(&s), &added).why.as_deref(), Some(not_drawn));

    let (mut s, _, view, [first, _]) = placed();
    let added = on_edge(&mut s, view, first);
    crate::drawing_scene::tests::not_drawable_yet(&mut s, view);
    let shaded = run(&mut s, INSPECT, json!({"note": added["note"]}));
    assert!(shaded.get("layout").is_none() && shaded["layout_why"] == not_drawn, "{shaded}");
    assert_eq!(state_of(&sheet_now(&s), &added).why.as_deref(), Some(not_drawn));

    let (mut s, _, view, [first, _]) = placed();
    let added = on_edge(&mut s, view, first);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": "Bracket"}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 0]], "radius": 2, "body": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing);
    let lost = run(&mut s, INSPECT, json!({"note": added["note"]}));
    assert!(lost["status"] != "ok" && lost.get("layout_why").is_none(), "{lost}");
    let (leaders, arrows, text_box) = laid_out(&lost);
    assert!(leaders.is_empty() && arrows.is_empty(), "{lost}");
    let corners = text_box.unwrap_or_else(|| panic!("no text box: {lost}"));
    assert_eq!(drawn_parts(&sheet_now(&s), &lost).2, vec![corners[0]]);

    let (mut s, _, view, [_, second]) = placed();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": second, "translate": [0, 0, 50]}]}));
    run(&mut s, "drawing.open", drawing);
    let v = ids::encode(view);
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"exploded": "Apart"}}));
    let edge = json!({"point": [140, 0, 0], "path": [second]});
    let apart = run(&mut s, ADD, json!({"anchor": {"edge": {"view": v, "edge": edge}}, "at": [60, 40], "content": "Deburr"}));
    assert!(apart.get("layout_why").is_none(), "{apart}");
    let tips: Vec<Vec2> = laid_out(&apart).1.into_iter().map(|(tip, _)| tip).collect();
    assert_eq!(drawn_parts(&sheet_now(&s), &apart).1, tips, "where the sheet draws the copy, moved apart");
}

/// A note whose text needs a font to fall back to never waits for the search, as add and its dry run
/// run on the window thread: the reply has no layout and says its text waits. Once the sheet has drawn
/// it, inspect has the layout. The character is this test's alone.
#[test]
fn a_notes_layout_never_waits_for_a_font() {
    let (mut s, _, view, _) = placed();
    let added = run(&mut s, ADD, json!({"anchor": {"view": ids::encode(view)}, "at": [0, -30], "content": "\u{30fd} 1"}));
    let waits = "its text waits for a font to fall back to; drawing.scene draws it when one is found";
    assert!(added.get("layout").is_none() && added["layout_why"] == waits, "{added}");
    sheet_now(&s);
    // Found, or known to be in no font: either way the answer is in, and the layout with it.
    let inspected = run(&mut s, INSPECT, json!({"note": added["note"]}));
    assert!(inspected.get("layout").is_some() && inspected["layout_why"] != waits, "{inspected}");
}

/// In a deferred drawing a note's layout reads the model as it is now, as its point does, while the
/// sheet shows what its last update drew. The Box 20 longer, with the drawing closed, moves its corner
/// (80, 0, 30) to (100, 0, 30) and the two copies' middle from x 90 to 100: the tip from
/// x 200 + 80 - 90 = 190 to 200 + 100 - 100 = 200, at y 150 + 30 - 15 = 165, while the sheet still
/// draws its update's. Updated, the sheet's arrowhead is where the layout's tip is.
#[test]
fn a_notes_layout_reads_the_model_now_in_a_deferred_drawing() {
    let (mut s, _, view, [bracket, _]) = placed();
    let corner = json!({"corner": {"view": ids::encode(view), "corner": {"point": [80, 0, 30], "path": [bracket]}}});
    let added = run(&mut s, ADD, json!({"anchor": corner, "at": [60, 40], "content": "Here"}));
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let tip = |reply: &Value| laid_out(reply).1.first().map(|(tip, _)| *tip).unwrap_or_else(|| panic!("no arrow: {reply}"));
    let was = tip(&added);
    assert!(was.dist(Vec2::new(190.0, 165.0)) < 1e-9, "{added}");
    // The sheet draws it at its update's place, as the sheet page does before the drawing is closed.
    let before = drawn_parts(&shown_now(&mut s), &added).1;
    assert!(before.len() == 1 && before[0].dist(was) < 1e-9, "{before:?}");
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 100}}));
    run(&mut s, "drawing.open", drawing);
    let now = tip(&run(&mut s, INSPECT, json!({"note": added["note"]})));
    assert!(now.dist(Vec2::new(200.0, 165.0)) < 1e-9, "{now:?}");
    let shown = drawn_parts(&shown_now(&mut s), &added).1;
    assert!(shown.len() == 1 && shown[0].dist(was) < 1e-9, "the sheet holds its update's: {shown:?}");
    run(&mut s, "drawing.update", json!({}));
    let updated = drawn_parts(&shown_now(&mut s), &added).1;
    assert!(updated.len() == 1 && updated[0].dist(now) < 1e-9, "{updated:?}");
}

/// A note the paper scene can't hold has no layout, and says why in the sheet's own words: a line of
/// capitals 100 mm high, from the sheet's (5000, 5000), running past the paper's 1e5 mm coordinates.
#[test]
fn a_note_past_the_papers_coordinates_has_no_layout_and_says_why() {
    let (mut s, ..) = placed();
    let style = solidcraft_sketch::TextStyle::default();
    let w = solidcraft_sketch::TextMetrics::new(&solidcraft_sketch::font_data(None).unwrap(), &style).unwrap().line_width("W", 100.0).unwrap();
    let count = (1.25e5 / w).ceil() as usize;
    assert!(count <= 1000, "a note's text is at most 1000 characters: {count}");
    let content = "W".repeat(count);
    let added = run(&mut s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [5000, 5000], "content": content, "cap_mm": 100}));
    let sheet = sheet_now(&s);
    let state = state_of(&sheet, &added);
    assert!(!state.drawn && state.why.is_some(), "the sheet leaves it out: {:?}", state.why);
    assert!(added.get("layout").is_none() && added["layout_why"] == json!(state.why), "{added}");
}

/// A note on a sheet lays out where the Note tool's preview draws it, which is how the sheet will draw
/// it once placed (#19543): its text box's first corner is the preview's text run's origin, exactly.
#[test]
fn a_sheet_notes_layout_is_where_its_preview_draws_it() {
    use solidcraft_geom::paper::Item;
    let (mut s, ..) = placed();
    let params = json!({"anchor": {"sheet": "Sheet 1"}, "at": [20, 20], "content": "General note"});
    let preview = crate::drawing_scene::note_preview_scene(&s, &params).unwrap();
    let added = run(&mut s, ADD, params);
    let corners = laid_out(&added).2.unwrap_or_else(|| panic!("no text box: {added}"));
    let [group] = preview.groups.as_slice() else { panic!("one group: {}", preview.groups.len()) };
    let origins: Vec<Vec2> = group.items.iter().filter_map(|item| if let Item::Text(text) = item { Some(text.origin) } else { None }).collect();
    assert_eq!(origins, vec![corners[0]], "{added}");
}

/// A note on a view of another configuration row lays out as that row draws it, where the sheet draws
/// it. The front view at 1:2 centres the block's outline: its far top corner (L, 0, 20) is 25 mm right
/// of the view's centre at L = 100 mm, and 30 mm at Long's 120, so the arrow's tip moves 5 mm right,
/// and is the sheet's arrowhead exactly.
#[test]
fn a_note_on_a_view_of_another_row_lays_out_as_the_row_draws_it() {
    let (mut s, _, [front, _], _) = crate::drawing_scene::tests::configured_front_and_top();
    let corner = json!({"corner": {"view": ids::encode(front), "corner": {"point": [100, 0, 20], "path": []}}});
    let added = run(&mut s, ADD, json!({"anchor": corner, "at": [40, 30], "content": "Here"}));
    let tip = |reply: &Value| laid_out(reply).1.first().map(|(tip, _)| *tip).unwrap_or_else(|| panic!("no arrow: {reply}"));
    let designed = tip(&added);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    let inspected = run(&mut s, INSPECT, json!({"note": added["note"]}));
    let long = tip(&inspected);
    assert!((long - designed).dist(Vec2::new(5.0, 0.0)) < 1e-9, "{designed:?} {long:?}");
    assert_eq!(drawn_parts(&sheet_now(&s), &added).1, vec![long], "{inspected}");
}

/// A note's layout follows its alignment, as the sheet draws it (#20122). The same text at (100, 60)
/// on the sheet has its box start there when left, centred on it when centre and end there when
/// right, its middle level with it each time, and its first corner where the sheet starts the text.
/// Edited to right, the left one's box ends there. A left note on a view lands before its text: its
/// box starts at its place, (260, 190), and its leader's last point is left of the box, level with
/// its middle.
#[test]
fn a_notes_layout_follows_its_alignment() {
    let (mut s, _, view, [bracket, _]) = placed();
    let at = Vec2::new(100.0, 60.0);
    let add = |s: &mut Session, align: &str| {
        run(s, ADD, json!({"anchor": {"sheet": "Sheet 1"}, "at": [at.x, at.y], "content": "General note", "align": align}))
    };
    let added = [add(&mut s, "left"), add(&mut s, "centre"), add(&mut s, "right")];
    let sheet = sheet_now(&s);
    // How far along the box's foot its place is: its start, its middle, its end.
    for (reply, along) in added.iter().zip([0.0, 0.5, 1.0]) {
        let corners = laid_out(reply).2.unwrap_or_else(|| panic!("no text box: {reply}"));
        let x = corners[0].x + (corners[1].x - corners[0].x) * along;
        assert!((x - at.x).abs() < 1e-9 && (middle(&corners).y - at.y).abs() < 1e-9, "{reply}");
        assert_eq!(drawn_parts(&sheet, reply).2, vec![corners[0]], "{reply}");
    }
    run(&mut s, EDIT, json!({"note": added[0]["note"], "set": {"align": "right"}}));
    let edited = run(&mut s, INSPECT, json!({"note": added[0]["note"]}));
    let corners = laid_out(&edited).2.unwrap_or_else(|| panic!("no text box: {edited}"));
    assert!((corners[1].x - at.x).abs() < 1e-9, "{edited}");

    let anchor = json!({"edge": {"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [bracket]}}});
    let left = run(&mut s, ADD, json!({"anchor": anchor, "at": [60, 40], "content": "Deburr", "align": "left"}));
    let (leaders, _, text_box) = laid_out(&left);
    let corners = text_box.unwrap_or_else(|| panic!("no text box: {left}"));
    assert!((corners[0].x - 260.0).abs() < 1e-9, "{left}");
    let end = leaders.first().and_then(|leader| leader.last().copied()).unwrap_or_else(|| panic!("no leader: {left}"));
    assert!(end.x < corners[0].x && (end.y - middle(&corners).y).abs() < 1e-9, "{left}");
}
