use serde_json::json;

use super::*;
use crate::cmd::drawing_view::tests::three_views;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} accepted {p}"))
}

/// A box's drawing, open, with its one A3 sheet.
fn drawing_session() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    run(&mut s, "drawing.create", json!({}));
    s
}

fn sheets(s: &Session) -> Vec<Arc<Sheet>> {
    s.drawing("test").unwrap().sheets.clone()
}

fn names(s: &Session) -> Vec<String> {
    sheets(s).iter().map(|sheet| sheet.name.clone()).collect()
}

/// A new sheet follows the last, or the one named, with its paper, border and resolution and a
/// name of its own; each add is one undo step.
#[test]
fn a_new_sheet_takes_the_paper_of_the_sheet_it_follows() {
    let mut s = drawing_session();
    let steps = s.undo.len();
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "dpi": 600, "size": {"standard": "a2"}}));
    let added = run(&mut s, "drawing.sheet.add", json!({}));
    let all = sheets(&s);
    assert_eq!(added["sheet"], json!(ids::encode(all[1].id)));
    assert_eq!((all[1].name.as_str(), all[1].dpi, all[1].sheet), ("Sheet 2", 600.0, all[0].sheet));
    assert_eq!(all[1].border, all[0].border);
    run(&mut s, "drawing.sheet.add", json!({"after": "Sheet 1", "name": "Details", "orientation": "portrait"}));
    assert_eq!(names(&s), ["Sheet 1", "Details", "Sheet 2"]);
    assert_eq!(sheets(&s)[1].sheet.orientation, Orientation::Portrait);
    assert_eq!(s.undo.len(), steps + 3);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["Sheet 1", "Sheet 2"]);
    // After a sheet by id; an unknown one is refused.
    let first = ids::encode(sheets(&s)[0].id);
    run(&mut s, "drawing.sheet.add", json!({"after": first, "name": "Second"}));
    assert_eq!(names(&s), ["Sheet 1", "Second", "Sheet 2"]);
    assert!(refused(&mut s, "drawing.sheet.add", json!({"after": "Nowhere"})).contains("no sheet `Nowhere`"));
    // A new sheet documents nothing and leaves nothing out unless told, whatever the one it follows does.
    let source = ids::encode(s.drawing("test").unwrap().sources[0].id);
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 2", "documents": {"source": source}, "excluded": {"count": true}}));
    run(&mut s, "drawing.sheet.add", json!({"after": "Sheet 2"}));
    let added = sheets(&s).last().cloned().unwrap();
    assert_eq!((added.documents, added.excluded.count, added.excluded.print), (None, false, false));
    let told = json!({"documents": {"source": source, "component": 0}, "excluded": {"print": true}});
    let id = run(&mut s, "drawing.sheet.add", told)["sheet"].clone();
    let added = sheets(&s).into_iter().find(|sheet| json!(ids::encode(sheet.id)) == id).unwrap();
    assert!(added.documents.is_some() && added.excluded.print && !added.excluded.count);
}

/// A sheet's id comes from the session's id source: two sessions seeded alike and run alike give
/// the same ids (where a crash replay starts), another seed others, and a failed add draws none.
#[test]
fn sheet_ids_come_from_the_sessions_seed() {
    let made = |seed: u64, fail: bool| {
        let mut s = Session::default();
        s.seed_ids(seed);
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
        run(&mut s, "drawing.create", json!({}));
        if fail {
            refused(&mut s, "drawing.sheet.add", json!({"after": "Nowhere"}));
        }
        run(&mut s, "drawing.sheet.add", json!({}));
        let record = s.drawing("test").unwrap().clone();
        record.sheets.iter().map(|sheet| sheet.id).chain(record.sources.iter().map(|source| source.id)).collect::<Vec<_>>()
    };
    assert_eq!(made(7, false), made(7, false));
    assert_eq!(made(7, true), made(7, false), "the failed add drew none");
    assert_ne!(made(7, false), made(8, false));
}

/// 1000 sheets at most: the 1000th is added, the 1001st refused with nothing changed.
#[test]
fn a_drawing_has_at_most_1000_sheets() {
    let mut s = drawing_session();
    for _ in 1..solidcraft_doc::drawing::MAX_SHEETS {
        run(&mut s, "drawing.sheet.add", json!({}));
    }
    assert_eq!(sheets(&s).len(), 1000);
    let before = s.drawing("test").unwrap().clone();
    assert!(refused(&mut s, "drawing.sheet.add", json!({})).contains("1 to 1000 sheets"));
    assert!(Arc::ptr_eq(s.drawing("test").unwrap(), &before));
}

/// A sheet's settings change one at a time, the rest kept; what the sheet's rules refuse is
/// refused with the drawing unchanged.
#[test]
fn a_sheets_settings_change_and_keep_its_rules() {
    let mut s = drawing_session();
    let source = ids::encode(s.drawing("test").unwrap().sources[0].id);
    let id = ids::encode(sheets(&s)[0].id);
    run(
        &mut s,
        "drawing.sheet.set",
        json!({"sheet": id, "name": "Assembly", "documents": {"source": source, "component": 0}, "excluded": {"print": true}}),
    );
    let sheet = sheets(&s)[0].clone();
    assert_eq!(sheet.name, "Assembly");
    assert_eq!(sheet.documents.map(|d| d.component), Some(0));
    assert_eq!((sheet.excluded.count, sheet.excluded.print), (false, true));
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Assembly", "excluded": {"count": true}, "documents": null}));
    let sheet = sheets(&s)[0].clone();
    assert_eq!((sheet.excluded.count, sheet.excluded.print, sheet.documents), (true, true, None), "the other flag kept");
    let before = s.drawing("test").unwrap().clone();
    for (p, says) in [
        (json!({"sheet": "Assembly", "dpi": 24}), "dpi"),
        (json!({"sheet": "Assembly", "size": {"standard": "b7"}}), "`size`"),
        (json!({"sheet": "Assembly", "size": {"custom": {"sides_mm": [0, 100]}}}), "a custom side is finite, above 0 and at most 5000 mm"),
        (json!({"sheet": "Assembly", "size": {"custom": {"sides_mm": [15, 15]}}}), "the margins leave no drawing space"),
        (json!({"sheet": "Assembly", "dpi": 2400.001}), "dpi"),
        (json!({"sheet": "Assembly", "excluded": true}), "`excluded` is {count?"),
        (json!({"sheet": "Assembly", "excluded": {"cuont": true}}), "`excluded` is {count?"),
        (json!({"sheet": "Assembly", "orientation": "upright"}), "landscape or portrait"),
        (json!({"sheet": "Assembly", "name": ""}), "name"),
        (json!({"sheet": "Assembly", "documents": {"source": "0000000000001"}}), "documents source"),
        (json!({"sheet": "Assembly", "excluded": {"count": "yes"}}), "true or false"),
        (json!({"sheet": "Nowhere"}), "no sheet `Nowhere`"),
        (json!({}), "`sheet`"),
    ] {
        let e = refused(&mut s, "drawing.sheet.set", p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(Arc::ptr_eq(s.drawing("test").unwrap(), &before), "{p}");
    }
    // The resolution's bounds are in; the same settings again are no step; a name two sheets
    // share is allowed, and then the sheet is named by id.
    let steps = s.undo.len();
    for dpi in [25.0, 2400.0] {
        run(&mut s, "drawing.sheet.set", json!({"sheet": "Assembly", "dpi": dpi}));
        assert_eq!(sheets(&s)[0].dpi, dpi);
    }
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Assembly", "dpi": 2400}));
    assert_eq!(s.undo.len(), steps + 2, "no change, no step");
    run(&mut s, "drawing.sheet.add", json!({}));
    let second = ids::encode(sheets(&s)[1].id);
    run(&mut s, "drawing.sheet.set", json!({"sheet": second, "name": "Assembly"}));
    assert!(refused(&mut s, "drawing.sheet.set", json!({"sheet": "Assembly", "dpi": 300})).contains("give its id"));
}

/// Sheets reorder by every id once, a name shared by two is refused in favour of the id, and the
/// last sheet can't be deleted.
#[test]
fn sheets_reorder_and_delete_but_one_always_stays() {
    let mut s = drawing_session();
    run(&mut s, "drawing.sheet.add", json!({}));
    run(&mut s, "drawing.sheet.add", json!({"name": "Sheet 1"}));
    let ids: Vec<String> = sheets(&s).iter().map(|sheet| ids::encode(sheet.id)).collect();
    assert!(refused(&mut s, "drawing.sheet.delete", json!({"sheet": "Sheet 1"})).contains("give its id"));
    let steps = s.undo.len();
    run(&mut s, "drawing.sheet.reorder", json!({"sheets": [ids[0], ids[1], ids[2]]}));
    assert_eq!(s.undo.len(), steps, "the same order is no step");
    run(&mut s, "drawing.sheet.reorder", json!({"sheets": [ids[2], ids[0], ids[1]]}));
    assert_eq!(sheets(&s).iter().map(|sheet| ids::encode(sheet.id)).collect::<Vec<_>>(), [ids[2].clone(), ids[0].clone(), ids[1].clone()]);
    for order in [json!([ids[0], ids[1]]), json!([ids[0], ids[0], ids[1]]), json!([ids[0], ids[1], ids[2], "0000000000001"]), json!("all")] {
        let e = refused(&mut s, "drawing.sheet.reorder", json!({"sheets": order}));
        assert!(e.contains("every sheet's id once"), "{order}: {e}");
    }
    run(&mut s, "drawing.sheet.delete", json!({"sheet": ids[0]}));
    // An undone delete puts the sheet back where it was.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sheets(&s).iter().map(|sheet| ids::encode(sheet.id)).collect::<Vec<_>>(), [ids[2].clone(), ids[0].clone(), ids[1].clone()]);
    run(&mut s, "drawing.sheet.delete", json!({"sheet": ids[0]}));
    run(&mut s, "drawing.sheet.delete", json!({"sheet": ids[1]}));
    assert!(refused(&mut s, "drawing.sheet.delete", json!({"sheet": ids[2]})).contains("at least one sheet"));
    assert_eq!(sheets(&s).len(), 1);
}

/// A sheet with views on it is refused, naming them, unless `contents` is `delete`: then it goes
/// with them in one undo step, which an undo brings back whole. Nothing on another sheet goes with
/// it, so a view there projected from one of its views is refused by name.
#[test]
fn a_sheet_with_views_on_it_goes_only_with_them() {
    let (mut s, views) = three_views();
    let [_, _, top] = views;
    let first = ids::encode(sheets(&s)[0].id);
    let second = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].clone();
    let e = refused(&mut s, "drawing.sheet.delete", json!({"sheet": first}));
    assert!(e.contains("has 3 views (") && views.iter().all(|v| e.contains(&ids::encode(*v))) && e.contains(r#""contents": "delete""#), "{e}");
    for contents in [json!("keep"), json!(true), json!(null)] {
        let e = refused(&mut s, "drawing.sheet.delete", json!({"sheet": first, "contents": contents}));
        assert!(e.contains("`contents` is delete"), "{contents}: {e}");
    }
    // The top view, placed apart, on the second sheet: its parent is on the first.
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(top), "sheet": second}));
    let e = refused(&mut s, "drawing.sheet.delete", json!({"sheet": first, "contents": "delete"}));
    assert!(e.contains(&ids::encode(top)) && e.contains("projected"), "{e}");
    run(&mut s, "edit.undo", json!({}));
    let steps = s.undo.len();
    run(&mut s, "drawing.sheet.delete", json!({"sheet": first, "contents": "delete"}));
    assert_eq!(s.undo.len(), steps + 1);
    let d = s.drawing("test").unwrap();
    assert_eq!((d.sheets.len(), d.views.len()), (1, 0));
    run(&mut s, "edit.undo", json!({}));
    let mut every = views.to_vec();
    every.sort_unstable();
    let d = s.drawing("test").unwrap();
    assert_eq!(ids::encode(d.sheets[0].id), first, "back in its place");
    assert_eq!(d.views.iter().map(|v| v.id).collect::<Vec<_>>(), every, "with its views");
    // An empty sheet goes without being asked twice.
    run(&mut s, "drawing.sheet.delete", json!({"sheet": second}));
}

/// A 180 x 54 mm block of two cells, as a command takes it: the title, and the projection symbol.
fn block() -> Value {
    json!({
        "size_mm": [180.0, 54.0], "outline_line_mm": 0.7, "cell_line_mm": 0.35,
        "cells": [
            {"name": "title", "rect_mm": [-180.0, 0.0, -30.0, 54.0], "label": {"text": "Title", "cap_mm": 2.5},
             "kind": {"text": {"content": [{"field": {"source": "component", "key": "name"}}], "cap_mm": 5.0}}},
            {"name": "projection", "rect_mm": [-30.0, 0.0, 0.0, 54.0], "label": {"text": "Projection", "cap_mm": 2.5}, "kind": "projection_symbol"},
        ],
    })
}

/// A sheet's title block by command: given, listed by `drawing.inspect`, taken by a new sheet from
/// the one it follows, and taken away with `null`. A block or a paper the sheet can't hold is
/// refused, with the drawing unchanged and no undo step.
#[test]
fn a_sheets_title_block_by_command() {
    let mut s = drawing_session();
    let steps = s.undo.len();
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "title_block": block()}));
    let given: TitleBlock = serde_json::from_value(block()).unwrap();
    assert_eq!(sheets(&s)[0].title_block.as_ref(), Some(&given));
    let inspected = run(&mut s, "drawing.inspect", json!({}));
    assert_eq!(inspected["sheets"][0]["title_block"], serde_json::to_value(&given).unwrap());
    run(&mut s, "drawing.sheet.add", json!({}));
    assert_eq!(sheets(&s)[1].title_block.as_ref(), Some(&given), "a new sheet takes it");
    assert_eq!(s.undo.len(), steps + 2);
    // A5 portrait's drawing border is 128 mm wide; the block is 180.
    let unchanged = |s: &mut Session, p: Value, why: &str| {
        let (before, steps) = (s.drawing("test").unwrap().clone(), s.undo.len());
        let refusal = refused(s, "drawing.sheet.set", p);
        assert!(refusal.contains(why), "{refusal}");
        assert_eq!((s.drawing("test").unwrap(), s.undo.len()), (&before, steps));
    };
    let fits = "a title block fits inside the drawing border";
    unchanged(&mut s, json!({"sheet": "Sheet 1", "size": {"standard": "a5"}, "orientation": "portrait"}), fits);
    let mut wide = block();
    wide["size_mm"] = json!([400.5, 54.0]);
    unchanged(&mut s, json!({"sheet": "Sheet 1", "title_block": wide}), fits);
    let mut unknown = block();
    unknown["colour"] = json!("red");
    unchanged(&mut s, json!({"sheet": "Sheet 1", "title_block": unknown}), "unknown field `colour`");
    run(&mut s, "drawing.sheet.set", json!({"sheet": "Sheet 2", "title_block": null}));
    assert_eq!((sheets(&s)[0].title_block.is_some(), sheets(&s)[1].title_block.is_some()), (true, false));
}
