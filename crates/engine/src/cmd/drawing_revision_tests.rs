use serde_json::json;
use solidcraft_doc::drawing::Drawing;

use super::*;
use crate::schema::tests::conforms;
use crate::tests::Folder;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let schema = output_schema_for::<RevisionsResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// `id` refuses `p`, leaving the drawing and the undo steps as they were: why.
fn refused(s: &mut Session, id: &str, p: Value) -> String {
    let (record, steps) = (s.drawing("test").ok().cloned(), s.undo.len());
    let message = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} accepted {p}"));
    assert_eq!((s.drawing("test").ok().cloned(), s.undo.len()), (record, steps), "{id} {p}");
    message
}

/// A box's drawing, open.
fn drawing_session() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10})).unwrap();
    s.execute("drawing.create", &json!({})).unwrap();
    s
}

/// Adds a row with `set`: its id.
fn add(s: &mut Session, set: Value) -> String {
    run(s, ADD, json!({"set": set}))["row"].as_str().unwrap().to_string()
}

fn revisions(reply: &Value) -> Vec<&str> {
    reply["rows"].as_array().unwrap().iter().map(|r| r["revision"].as_str().unwrap()).collect()
}

/// A new row gets the next revision, after the highest the drawing has had: 1, 2, 3; after a
/// revision typed as 5, 6; after deleting row 2, still after the highest. The texts given are the
/// row's own; each command is one undo step, and an edit that changes nothing is none.
#[test]
fn rows_take_the_next_revision_and_keep_what_is_typed() {
    let mut s = drawing_session();
    assert_eq!(run(&mut s, LIST, json!({})), json!({"rows": [], "next": "1"}));
    let steps = s.undo.len();
    let first = add(&mut s, json!({"description": "First issue", "designer": "CW", "date": "2026-10-29"}));
    assert_eq!(s.undo.len(), steps + 1, "one undo step, texts and all");
    let second = add(&mut s, json!({}));
    let third = add(&mut s, json!({}));
    let listed = run(&mut s, LIST, json!({}));
    assert_eq!((revisions(&listed), listed["next"].as_str()), (vec!["1", "2", "3"], Some("4")));
    assert_eq!(
        listed["rows"][0],
        json!({"row": first, "revision": "1", "description": "First issue", "designer": "CW", "checker": "", "date": "2026-10-29", "hidden": false})
    );
    let edited = run(&mut s, EDIT, json!({"row": third, "set": {"revision": "5", "checker": "JB", "hidden": true}}));
    assert_eq!(edited["changed"], json!(["revision", "checker", "hidden"]));
    assert_eq!(edited["next"], "6");
    let steps = s.undo.len();
    let same = run(&mut s, EDIT, json!({"row": third, "set": {"revision": "5"}}));
    assert!(same.get("changed").is_none() && s.undo.len() == steps, "nothing changed, no step: {same}");
    let deleted = run(&mut s, DELETE, json!({"row": second}));
    assert_eq!((revisions(&deleted), deleted["next"].as_str()), (vec!["1", "5"], Some("6")));
    run(&mut s, DELETE, json!({"row": third}));
    assert_eq!(run(&mut s, LIST, json!({}))["next"], "2", "the highest left is 1");
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(revisions(&run(&mut s, LIST, json!({}))), ["1", "2", "5"], "undo puts both back where they were");
    let record = &s.drawing("test").unwrap().revisions;
    assert_eq!(record.newest(None).map(|r| r.revision.as_str()), Some("5"), "a hidden row is still the newest");
    assert_eq!(record.table_rows(None).len(), 2, "but the table leaves it out");
}

/// The rows are saved with the drawing, their ids as every drawing id is written, and read back as
/// they were; a drawing with no rows saves no revisions, as one saved before them.
#[test]
fn rows_are_saved_with_the_drawing() {
    let folder = Folder::new("drawing-revisions");
    let mut s = drawing_session();
    let empty: Value = serde_json::from_str(&s.drawing("test").unwrap().to_json().unwrap()).unwrap();
    assert!(empty.get("revisions").is_none(), "{empty}");
    let row = add(&mut s, json!({"description": "Holes added"}));
    let saved = s.drawing("test").unwrap().to_json().unwrap();
    let v: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(v["revisions"]["rows"][0]["id"], json!(row));
    assert!(v["format_pending"].as_array().unwrap().contains(&json!("revisions")), "{}", v["format_pending"]);
    assert_eq!(Drawing::from_json(&saved).unwrap().revisions, s.drawing("test").unwrap().revisions);
    // A damaged record is refused as the drawing is read, and one kept per sheet isn't a drawing's yet.
    for (path, bad) in [("/revisions/rows/0/revision", json!("A\nB")), ("/revisions/per_sheet", json!(true))] {
        let mut damaged = v.clone();
        *damaged.pointer_mut(path).unwrap_or_else(|| panic!("{path}")) = bad;
        assert!(Drawing::from_json(&damaged.to_string()).is_err(), "{path}");
    }
    s.execute("file.save", &json!({"path": folder.path("box.solidcraft")})).unwrap();
    let mut opened = Session::default();
    opened.execute("doc.open", &json!({"path": folder.path("box.solidcraft")})).unwrap();
    assert_eq!(opened.doc.drawings[0].revisions, s.drawing("test").unwrap().revisions);
    // A row's id is one of the drawing's: no other object may take it, so no new id is it either.
    let mut d = (**s.drawing("test").unwrap()).clone();
    Arc::make_mut(&mut d.sheets[0]).id = ids::decode(&row).unwrap();
    assert!(d.validate().unwrap_err().to_string().contains("two objects have the id"));
}

/// Deleting a row names the revision tags bound to it, which keep the revision they showed.
#[test]
fn a_deleted_row_names_the_tags_it_leaves() {
    let mut s = drawing_session();
    let row = add(&mut s, json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let tag = d.allocate_id(&mut s.ids).unwrap();
    Arc::make_mut(&mut d.revisions).add_tag(ids::decode(&row).unwrap(), tag).unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    let deleted = run(&mut s, DELETE, json!({"row": row}));
    assert_eq!(deleted["tags_lost"], json!([ids::encode(tag)]));
    let record = &s.drawing("test").unwrap().revisions;
    assert_eq!(record.tags()[0].lost.as_deref(), Some("1"));
}

/// What the revision commands refuse, each with the drawing and the undo steps unchanged.
#[test]
fn hostile_revision_calls_are_refused() {
    let mut s = Session::default();
    assert!(refused(&mut s, ADD, json!({})).contains("open a drawing first"));
    let mut s = drawing_session();
    let row = add(&mut s, json!({}));
    let long = "x".repeat(70_000);
    for (cmd, p) in [
        (EDIT, json!({"row": ids::encode(99), "set": {"date": "today"}})),
        (EDIT, json!({"row": 7, "set": {}})),
        (EDIT, json!({"row": null, "set": {}})),
        (EDIT, json!({"row": row})),
        (EDIT, json!({"row": row, "set": {"revision": "A\nB"}})),
        (EDIT, json!({"row": row, "set": {"description": "bell \u{7}"}})),
        (EDIT, json!({"row": row, "set": {"description": long}})),
        (EDIT, json!({"row": row, "set": {"hidden": "yes"}})),
        (EDIT, json!({"row": row, "set": {"date": null}})),
        (EDIT, json!({"row": row, "set": {"colour": "red"}})),
        (ADD, json!({"set": {"revision": "1\n2"}})),
        (ADD, json!({"set": null})),
        (ADD, json!({"row": row})),
        (DELETE, json!({"row": ids::encode(99)})),
        (DELETE, json!({})),
    ] {
        refused(&mut s, cmd, p);
    }
    // A record holds at most 1000 rows.
    for _ in 1..solidcraft_geom::revision::MAX_ROWS {
        add(&mut s, json!({}));
    }
    assert!(refused(&mut s, ADD, json!({})).contains("at most 1000 rows"));
}

/// Sessions seeded alike give rows the same ids.
#[test]
fn a_rows_id_comes_from_the_sessions_seed() {
    let made = |seed: u64| {
        let mut s = drawing_session();
        s.seed_ids(seed);
        [add(&mut s, json!({})), add(&mut s, json!({}))]
    };
    assert_eq!(made(7), made(7));
    assert_ne!(made(7), made(8));
}
