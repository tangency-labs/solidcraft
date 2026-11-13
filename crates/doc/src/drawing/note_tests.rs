use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::drawing::Source as DrawingSource;
use crate::drawing::view::{View, ViewKind, ViewPlace};
use crate::drawing_view::{ViewDirection, ViewOverrides};
use crate::format::DRAWING;
use crate::ids::IdSource;

const SEED: u64 = 4896;

fn text(words: &str) -> Content {
    Content(vec![Part::Text(words.into())])
}

/// A drawing on one A3 sheet with a front view, and three notes: on the sheet, on the view, and on
/// an edge in the view. Their ids, in that order.
fn noted() -> (Drawing, [u64; 3]) {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Noted", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(DrawingSource { id: source });
    let (view, sheet) = (d.allocate_id(&mut id_source).unwrap(), d.sheets[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    d.add_views([Arc::new(View {
        id: view,
        sheet,
        kind,
        place: ViewPlace::At([150.0, 150.0]),
        settings: ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() },
        reference_shown_percent: 0.0,
        centre_marks: None,
    })]);
    let edge = ModelEdge { body: "Bolt".into(), origin_feature: 4, name: "F1:side|F2:head".into(), path: vec![3] };
    let anchors = [Anchor::Sheet(sheet), Anchor::View(view), Anchor::Edge { view, edge }];
    let mut added = [0; 3];
    for (anchor, id) in anchors.into_iter().zip(&mut added) {
        *id = d.allocate_id(&mut id_source).unwrap();
        d.notes.add([Arc::new(Note { id: *id, anchor, at: [40.0, 25.0], content: text("Break all edges"), cap_mm: None, branches: Vec::new() })]);
    }
    d.validate().unwrap();
    (d, added)
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

fn note<'a>(v: &'a mut Value, id: &str) -> &'a mut Value {
    v["notes"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == id).unwrap()
}

fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    Drawing::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |_| "read".into())
}

/// A note saves its anchor, place, content and height, and nothing worked out, and reads back as
/// saved, the same bytes again, in any listed order. An older drawing reads with none, and a drawing
/// with none writes none.
#[test]
fn notes_save_what_they_say_and_read_back_as_saved() {
    let (d, [on_sheet, on_view, on_edge]) = noted();
    let mut v = file(&d);
    let sheet = ids::encode(d.sheets[0].id);
    let view = ids::encode(d.views[0].id);
    assert_eq!(
        *note(&mut v, &ids::encode(on_sheet)),
        json!({"id": ids::encode(on_sheet), "anchor": {"sheet": sheet}, "at": [40.0, 25.0], "content": [{"text": "Break all edges"}]})
    );
    assert_eq!(note(&mut v, &ids::encode(on_view))["anchor"], json!({"view": view}));
    assert_eq!(
        note(&mut v, &ids::encode(on_edge))["anchor"],
        json!({"edge": {"view": view, "edge": {"body": "Bolt", "origin_feature": 4, "name": "F1:side|F2:head", "path": [3]}}})
    );
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
    let mut other_order = v.clone();
    other_order["notes"].as_array_mut().unwrap().reverse();
    assert_eq!(Drawing::from_json(&other_order.to_string()).unwrap(), d);
    let mut unsorted = d.clone();
    unsorted.notes.reverse();
    assert_eq!(unsorted.validate().unwrap_err().to_string(), "a drawing lists its notes by id");
    let mut none = d.clone();
    none.notes.clear();
    assert_eq!(file(&none).get("notes"), None);
    let older = json!({
        "format": DRAWING.format_string(1), "id": "0000000000002", "name": "Old", "sources": [], "language": "en",
        "sheets": [{"id": "0000000000001", "name": "S", "sheet": {"size": {"standard": "a4"}}}]
    });
    assert!(Drawing::from_json(&older.to_string()).unwrap().notes.is_empty());
}

/// An edge branch as a file saves it.
fn edge_branch() -> Value {
    json!({"edge": {"body": "Bolt", "origin_feature": 4, "name": "F1:side|F2:head", "path": [3]}})
}

/// A note on a view saves its branches by name, in order, after its anchor, and reads back as saved;
/// a note without them saves none, so an older note reads as it was.
#[test]
fn a_notes_branches_save_by_name_and_read_back() {
    let (mut d, [_, on_view, on_edge]) = noted();
    let edge = ModelEdge { body: "Bolt".into(), origin_feature: 4, name: "F1:side|F2:head".into(), path: vec![3] };
    let point = ModelPoint { body: "Bolt".into(), origin_feature: 4, edges: vec!["a".into(), "b".into()], centre: false, path: vec![] };
    let at = d.notes.index_of(on_edge).unwrap();
    Arc::make_mut(d.notes.get_mut(at).unwrap()).branches = vec![Branch::Corner(point), Branch::Edge(edge)];
    d.validate().unwrap();
    let mut v = file(&d);
    let corner = json!({"corner": {"body": "Bolt", "origin_feature": 4, "edges": ["a", "b"]}});
    assert_eq!(note(&mut v, &ids::encode(on_edge))["branches"], json!([corner, edge_branch()]));
    assert_eq!(note(&mut v, &ids::encode(on_view)).get("branches"), None);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    let noted = reopened.notes.by_id(on_edge).unwrap();
    let paths: Vec<Vec<u64>> = noted.paths().map(<[u64]>::to_vec).collect();
    assert_eq!(paths, [vec![3], vec![], vec![3]], "the anchor's, then each branch's");
}

/// What reading refuses, on both sides of every cap.
#[test]
fn the_notes_rules_are_kept_on_reading() {
    let (d, [on_sheet, on_view, on_edge]) = noted();
    let [on_sheet, on_view, on_edge] = [on_sheet, on_view, on_edge].map(ids::encode);
    let rule = |id: &str, rule: &str| format!("damaged drawing: note {id}: {rule}");
    let nothing = ids::encode(99);
    let gone = format!("it is on sheet {nothing}, which the drawing doesn't have");
    assert_eq!(refused(&d, |v| note(v, &on_sheet)["anchor"] = json!({"sheet": nothing})), rule(&on_sheet, &gone));
    let gone = format!("it is on view {nothing}, which the drawing doesn't have");
    assert_eq!(refused(&d, |v| note(v, &on_view)["anchor"] = json!({"view": nothing})), rule(&on_view, &gone));
    assert_eq!(refused(&d, |v| note(v, &on_edge)["anchor"]["edge"]["view"] = json!(nothing)), rule(&on_edge, &gone));
    let names = "a name in a reference is 1 to 512 bytes, without control characters";
    assert_eq!(refused(&d, |v| note(v, &on_edge)["anchor"]["edge"]["edge"]["body"] = json!("")), rule(&on_edge, names));
    // Its text: some, 4096 bytes and no more, and only fields it can read.
    let content = |c: Value| refused(&d, |v| note(v, &on_view)["content"] = c);
    let words = |n: usize| json!([{"text": "x".repeat(n)}]);
    assert_eq!(content(words(4096)), "read");
    assert!(content(words(4097)).starts_with(&format!("damaged drawing: note {on_view}: ")));
    assert_eq!(content(json!([])), rule(&on_view, "it has some text"));
    let reads = "its fields read the drawing, its sheet, its component or its newest revision";
    for source in ["prompt", "row"] {
        assert_eq!(content(json!([{"field": {"source": source, "key": "x"}}])), rule(&on_view, reads), "{source}");
    }
    assert_eq!(content(json!([{"text": "Sheet "}, {"field": {"source": "sheet", "key": "number"}}])), "read");
    // A field its source doesn't have is refused when given, never on opening: the note opens.
    assert_eq!(content(json!([{"field": {"source": "component", "key": "weight"}}])), "read");
    // Branches: only on a view, 16 leaders in all, the anchor's included, each keeping a reference's rules.
    let branches = |id: &str, n: usize| refused(&d, |v| note(v, id)["branches"] = json!(vec![edge_branch(); n]));
    let most = "a note has at most 16 leaders, its anchor's included";
    assert_eq!(branches(&on_sheet, 1), rule(&on_sheet, "a note on a sheet has no branches: it points at no model"));
    assert_eq!([branches(&on_edge, 15), branches(&on_edge, 16)], ["read".to_string(), rule(&on_edge, most)]);
    assert_eq!([branches(&on_view, 16), branches(&on_view, 17)], ["read".to_string(), rule(&on_view, most)]);
    let unnamed = json!([{"edge": {"body": "", "origin_feature": 4, "name": "e", "path": []}}]);
    assert_eq!(refused(&d, |v| note(v, &on_view)["branches"] = unnamed), rule(&on_view, names));
    assert!(refused(&d, |v| note(v, &on_view)["branches"] = json!([{"face": {}}])).starts_with("damaged drawing: "));
    // Its place and height are on paper.
    let paper = rule(&on_sheet, "its place is at most 5000 mm on paper");
    let at = |at: [f64; 2]| refused(&d, |v| note(v, &on_sheet)["at"] = json!(at));
    assert_eq!([at([5000.0, -5000.0]), at([5000.001, 0.0])], ["read".to_string(), paper]);
    let cap = |mm: f64| refused(&d, |v| note(v, &on_sheet)["cap_mm"] = json!(mm));
    let height = rule(&on_sheet, "its text height is 0.5 to 100 mm");
    assert_eq!([cap(0.5), cap(100.0), cap(0.49), cap(100.1)], ["read".to_string(), "read".into(), height.clone(), height]);
    // Its id is the drawing's, and fields it doesn't know aren't read.
    let view_id = ids::encode(d.views[0].id);
    assert_eq!(refused(&d, |v| note(v, &on_sheet)["id"] = json!(view_id)), format!("damaged drawing: two objects have the id {view_id}"));
    assert!(refused(&d, |v| note(v, &on_sheet)["leader"] = json!([0, 0])).starts_with("damaged drawing: "));
    assert!(refused(&d, |v| note(v, &on_sheet)["anchor"] = json!({"page": 1})).starts_with("damaged drawing: "));
    // At most MAX_NOTES.
    let many = |n: usize| {
        let mut v = file(&d);
        let one = note(&mut v, &on_sheet).clone();
        let copies = (1..=n as u64).map(|i| {
            let mut copy = one.clone();
            copy["id"] = json!(ids::encode(i));
            copy
        });
        v["notes"] = Value::Array(copies.collect());
        Drawing::from_json(&v.to_string()).map(|d| d.notes.len()).map_err(|e| e.to_string())
    };
    assert_eq!(many(MAX_NOTES), Ok(MAX_NOTES));
    assert_eq!(many(MAX_NOTES + 1), Err(format!("damaged drawing: a drawing has at most {MAX_NOTES} notes")));
}

/// A sheet's notes are those on it and on its views, and go with it; a view's go with the view; a
/// new id never takes a note's.
#[test]
fn a_sheets_and_a_views_notes_go_with_them() {
    let (mut d, [on_sheet, on_view, on_edge]) = noted();
    let sheet = d.sheets[0].id;
    let mut all = vec![on_sheet, on_view, on_edge];
    all.sort_unstable();
    assert_eq!(d.on_sheet(sheet).notes, all);
    let mut in_view = vec![on_view, on_edge];
    in_view.sort_unstable();
    let view = d.views[0].id;
    assert_eq!(d.with_contents(vec![view]).notes, in_view);
    let mut id_source = IdSource::seeded(SEED);
    for _ in 0..64 {
        assert!(!all.contains(&d.allocate_id(&mut id_source).unwrap()), "a new id never takes a note's");
    }
    let mut without_view = d.clone();
    without_view.remove_objects(&without_view.with_contents(vec![view]));
    without_view.validate().unwrap();
    assert_eq!(without_view.notes.iter().map(|n| n.id).collect::<Vec<_>>(), [on_sheet]);
    let second = d.allocate_id(&mut id_source).unwrap();
    d.sheets.push(Arc::new(crate::drawing::Sheet { id: second, name: "Sheet 2".into(), ..(*d.sheets[0]).clone() }));
    d.remove_sheet(sheet);
    d.validate().unwrap();
    assert!(d.notes.is_empty() && d.views.is_empty());
}

/// An anchor moved to another view keeps what it names; one on a sheet stays.
#[test]
fn an_anchor_moves_to_a_view() {
    let edge = ModelEdge { body: "B".into(), origin_feature: 4, name: "e".into(), path: vec![] };
    assert_eq!(Anchor::Sheet(1).on_view(9), Anchor::Sheet(1));
    assert_eq!(Anchor::View(1).on_view(9), Anchor::View(9));
    assert_eq!(Anchor::Edge { view: 1, edge: edge.clone() }.on_view(9), Anchor::Edge { view: 9, edge });
    assert_eq!(Anchor::Sheet(1).view(), None);
    assert_eq!(Anchor::View(2).view(), Some(2));
}

/// A note is on its own sheet, or on its view's; a view the drawing doesn't have is on none.
#[test]
fn a_note_is_on_its_sheet_or_its_views() {
    let (mut d, _) = noted();
    let (sheet, view) = (d.sheets[0].id, d.views[0].id);
    let mut id_source = IdSource::seeded(SEED + 1);
    let second = d.allocate_id(&mut id_source).unwrap();
    d.sheets.insert(0, Arc::new(crate::drawing::Sheet { id: second, name: "Sheet 0".into(), ..(*d.sheets[0]).clone() }));
    let edge = ModelEdge { body: "B".into(), origin_feature: 4, name: "e".into(), path: vec![] };
    let anchors = [Anchor::Sheet(second), Anchor::Sheet(sheet), Anchor::View(view), Anchor::Edge { view, edge }];
    assert_eq!(anchors.map(|anchor| d.sheet_of(&anchor)), [Some(0), Some(1), Some(1), Some(1)]);
    assert_eq!([Anchor::Sheet(99), Anchor::View(99)].map(|anchor| d.sheet_of(&anchor)), [None, None]);
}

/// Seeded mutations of a noted drawing's file never panic, and whatever reads again keeps the rules.
#[test]
fn mutated_note_files_read_or_are_refused() {
    let (d, _) = noted();
    let base = file(&d);
    let values =
        [json!(null), json!(0), json!(-1.5), json!(1e308), json!("x"), json!([]), json!({}), json!([1.0, 2.0]), json!({"sheet": "0000000000001"})];
    let keys = ["anchor", "at", "content", "cap_mm", "id"];
    let pick = |seed: u64, salt: u64, n: usize| (((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize) % n;
    for seed in 1..=500u64 {
        let mut v = base.clone();
        let at = pick(seed, 1, 3);
        let value = values[pick(seed, 2, values.len())].clone();
        v["notes"][at][keys[pick(seed, 3, keys.len())]] = value;
        if let Ok(read) = Drawing::from_json(&v.to_string()) {
            read.validate().unwrap();
            assert_eq!(Drawing::from_json(&read.to_json().unwrap()).unwrap(), read, "seed {seed}");
        }
    }
}
