use serde_json::json;

use super::*;
use crate::Vec2;

/// A fresh id, as the design's random ids are: never one handed out before in this run.
fn fresh() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Adds a row with a fresh id: its id.
fn add(record: &mut RevisionRecord, sheet: Option<u64>) -> Result<u64> {
    let id = fresh();
    record.add(sheet, id).map(|()| id)
}

/// Adds a tag with a fresh id: its id.
fn add_tag(record: &mut RevisionRecord, row: u64) -> Result<u64> {
    let id = fresh();
    record.add_tag(row, id).map(|()| id)
}

fn labels(record: &RevisionRecord) -> Vec<&str> {
    record.rows().iter().map(|r| r.revision.as_str()).collect()
}

#[test]
fn numbers_run_from_1_or_0_and_letters_skip_six() {
    let mut record = RevisionRecord::default();
    for _ in 0..3 {
        add(&mut record, None).unwrap();
    }
    assert_eq!(labels(&record), ["1", "2", "3"]);
    let mut zero = RevisionRecord { numbering: Numbering::Numbers { start: 0 }, ..RevisionRecord::default() };
    add(&mut zero, None).unwrap();
    add(&mut zero, None).unwrap();
    assert_eq!(labels(&zero), ["0", "1"]);
    let mut lettered = RevisionRecord { numbering: Numbering::Letters, ..RevisionRecord::default() };
    for _ in 0..22 {
        add(&mut lettered, None).unwrap();
    }
    assert_eq!(labels(&lettered).concat(), "ABCDEFGHJKLMNPRTUVWYAAAB", "I, O, Q, S, X and Z skipped, then AA");
    // Past AY comes BA; past YY, AAA.
    for (n, label) in [(0, "A"), (19, "Y"), (20, "AA"), (39, "AY"), (40, "BA"), (419, "YY"), (420, "AAA")] {
        assert_eq!(letters(n), label);
        assert_eq!(letters_index(label), Some(n), "{label}");
    }
    for not in ["", "I", "A1", "Ä"] {
        assert_eq!(letters_index(not), None, "{not:?}");
    }
}

/// An issued revision is never given out again: a typed lower-case letter counts as its capital,
/// the lettering counts on past eight letters, and past `u64` nothing is given out.
#[test]
fn the_next_revision_is_never_one_given_out_before() {
    let mut lettered = RevisionRecord { numbering: Numbering::Letters, ..RevisionRecord::default() };
    let a = add(&mut lettered, None).unwrap();
    assert_eq!(lettered.set(a, Field::Revision, "c").unwrap(), None);
    add(&mut lettered, None).unwrap();
    assert_eq!(labels(&lettered), ["c", "D"]);
    lettered.set(a, Field::Revision, "YYYYYYYY").unwrap();
    assert_eq!(lettered.next_revision(None).as_deref(), Some("AAAAAAAAA"));
    lettered.set(a, Field::Revision, &letters(u64::MAX)).unwrap();
    assert_eq!(letters_index(&letters(u64::MAX)), Some(u64::MAX));
    assert_eq!(lettered.next_revision(None), None);
    assert_eq!(add(&mut lettered, None).unwrap_err().0, "no revision follows the highest one");
    let mut numbered = RevisionRecord::default();
    let first = add(&mut numbered, None).unwrap();
    numbered.set(first, Field::Revision, &(u64::MAX - 1).to_string()).unwrap();
    add(&mut numbered, None).unwrap();
    assert_eq!(numbered.rows()[1].revision, u64::MAX.to_string());
    assert_eq!(add(&mut numbered, None).unwrap_err().0, "no revision follows the highest one");
}

#[test]
fn a_row_is_added_with_its_revision_only_and_typed_text_is_kept() {
    let mut record = RevisionRecord { numbering: Numbering::Letters, ..RevisionRecord::default() };
    let a = add(&mut record, None).unwrap();
    let row = &record.rows()[0];
    assert_eq!(
        (row.revision.as_str(), row.description.as_str(), row.designer.as_str(), row.checker.as_str(), row.date.as_str()),
        ("A", "", "", "", "")
    );
    assert_eq!(record.set(a, Field::Description, "First issue\nfor tender").unwrap(), None);
    assert_eq!(record.set(a, Field::Checker, "J. Smith").unwrap(), None, "a skipped letter outside the revision is no matter");
    // A typed revision is kept; one with a skipped letter is warned about, and the next follows it.
    assert_eq!(record.set(a, Field::Revision, "C").unwrap(), None);
    let b = add(&mut record, None).unwrap();
    assert_eq!(record.rows()[1].revision, "D");
    assert_eq!(record.set(b, Field::Revision, "I").unwrap(), Some(SkippedLetter('I')));
    assert_eq!(record.set(b, Field::Revision, "o").unwrap(), Some(SkippedLetter('o')));
    assert_eq!(record.rows()[1].revision, "o");
    // Numbers don't warn about letters, and typed text that isn't a number doesn't move the next one.
    let mut numbered = RevisionRecord::default();
    let first = add(&mut numbered, None).unwrap();
    assert_eq!(numbered.set(first, Field::Revision, "Issue 1").unwrap(), None);
    add(&mut numbered, None).unwrap();
    assert_eq!(numbered.rows()[1].revision, "1");
    for (field, text, message) in [
        (Field::Revision, "1\n2", "a label is one line of at most 4096 bytes"),
        (Field::Date, "\u{0}", "a cell's text has no control character but a line break"),
        (Field::Description, &"x".repeat(4097), "a cell's text is at most 4096 bytes"),
    ] {
        assert_eq!(record.set(a, field, text), Err(Invalid(message)));
    }
    assert_eq!(record.set(99, Field::Date, "x"), Err(Invalid("no row has that id")));
}

#[test]
fn hiding_and_deleting_keep_the_issued_history() {
    let mut record = RevisionRecord::default();
    let one = add(&mut record, None).unwrap();
    let two = add(&mut record, None).unwrap();
    record.set(one, Field::Description, "First").unwrap();
    record.issue(one, None).unwrap();
    record.set(one, Field::Description, "First, corrected").unwrap();
    record.issue(two, None).unwrap();
    record.hide(one, true).unwrap();
    record.delete(two).unwrap();
    let issued: Vec<_> = record.issued().iter().map(|r| (r.revision.as_str(), r.description.as_str())).collect();
    assert_eq!(issued, [("1", "First"), ("2", "")], "copies as they read when issued");
    // An issued revision is never given out again, though its row is gone.
    add(&mut record, None).unwrap();
    assert_eq!(labels(&record), ["1", "3"]);
    // A deleted row that was never issued gives its revision back.
    let mut fresh = RevisionRecord::default();
    add(&mut fresh, None).unwrap();
    let gone = add(&mut fresh, None).unwrap();
    fresh.delete(gone).unwrap();
    add(&mut fresh, None).unwrap();
    assert_eq!(labels(&fresh), ["1", "2"]);
    // An id in use is refused: a row's, an issued copy's whose row is gone, or a tag's.
    let tag = add_tag(&mut record, one).unwrap();
    for taken in [one, two, tag] {
        assert_eq!(record.add(None, taken), Err(Invalid("that id is taken")), "{taken}");
    }
    assert_eq!(record.add_tag(one, tag), Err(Invalid("that id is taken")));
    assert_eq!(record.delete(two), Err(Invalid("no row has that id")));
    assert_eq!(record.issue(two, None), Err(Invalid("no row has that id")));
}

#[test]
fn deleting_a_row_reports_its_tags_which_keep_what_they_showed() {
    let mut record = RevisionRecord::default();
    let one = add(&mut record, None).unwrap();
    let two = add(&mut record, None).unwrap();
    let a = add_tag(&mut record, one).unwrap();
    let b = add_tag(&mut record, two).unwrap();
    let c = add_tag(&mut record, two).unwrap();
    record.tags.iter_mut().find(|t| t.id == c).unwrap().text = Some("2*".into());
    let shown = |record: &RevisionRecord| record.tags().iter().map(|t| record.tag_text(t)).collect::<Vec<_>>();
    assert_eq!(shown(&record), [TagText::Revision("1".into()), TagText::Revision("2".into()), TagText::Override("2*".into())]);
    // A tag follows its row's revision as it is retyped.
    record.set(one, Field::Revision, "1A").unwrap();
    assert_eq!(record.tag_text(&record.tags()[0]), TagText::Revision("1A".into()));
    assert_eq!(record.delete(two).unwrap(), [b, c]);
    assert_eq!(shown(&record), [TagText::Revision("1A".into()), TagText::Lost("2".into()), TagText::Lost("2*".into())]);
    assert_eq!(record.tags().len(), 3, "never removed");
    assert_eq!(record.delete(one).unwrap(), [a]);
    assert_eq!(add_tag(&mut record, one), Err(Invalid("no row has that id")));
    assert_eq!(record.tags()[0].size_mm, 5.0);
    record.validate().unwrap();
}

#[test]
fn the_title_block_and_the_table_read_the_same_fields() {
    let mut record = RevisionRecord::default();
    let one = add(&mut record, None).unwrap();
    let two = add(&mut record, None).unwrap();
    for (id, designer, date) in [(one, "A. Lee", "2026-10-01"), (two, "B. Ode", "2026-10-26")] {
        record.set(id, Field::Designer, designer).unwrap();
        record.set(id, Field::Date, date).unwrap();
    }
    // The newest row, hidden or not, answers a title block's revision keys.
    record.hide(two, true).unwrap();
    let newest = record.newest(None).unwrap();
    assert_eq!((newest.field("revision"), newest.field("designer"), newest.field("date")), (Some("2"), Some("B. Ode"), Some("2026-10-26")));
    assert_eq!(newest.field("checker"), Some(""), "nothing filled in for a person");
    assert_eq!(newest.field("mass"), None);
    // The table shows the rows not hidden, each field by the same function.
    let rows = record.table_rows(None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, one.to_string());
    for f in Field::ALL {
        assert_eq!(rows[0].fields.get(f.key()).map(String::as_str), record.rows()[0].field(f.key()));
    }
    // Laid out by the sheet table: five columns, the heading at the top.
    let layout = table().layout(&rows, Vec2::new(400.0, 280.0), &|t, cap| t.chars().count() as f64 * 0.6 * cap).unwrap();
    let headings: Vec<_> = layout.heading.cells.iter().map(|c| c.lines[0].text.as_str()).collect();
    assert_eq!(headings, ["Revision", "Description", "Designer", "Checker", "Date"]);
    let row: Vec<_> = layout.rows[0].cells.iter().map(|c| c.lines.first().map_or("", |l| l.text.as_str())).collect();
    assert_eq!(row, ["1", "", "A. Lee", "", "2026-10-01"]);
    assert!(layout.heading.rect.min.y > layout.rows[0].rect.min.y);
}

#[test]
fn a_record_per_sheet_numbers_each_sheet_apart() {
    let mut record = RevisionRecord { per_sheet: true, ..RevisionRecord::default() };
    for sheet in [1, 1, 2, 1] {
        add(&mut record, Some(sheet)).unwrap();
    }
    assert_eq!(labels(&record), ["1", "2", "1", "3"]);
    assert_eq!(record.newest(Some(2)).unwrap().revision, "1");
    assert_eq!(record.newest(Some(9)), None);
    assert_eq!(record.table_rows(Some(1)).len(), 3);
    assert_eq!(add(&mut record, None), Err(Invalid("a row has a sheet when the record is kept per sheet, and only then")));
    assert_eq!(add(&mut RevisionRecord::default(), Some(1)), Err(Invalid("a row has a sheet when the record is kept per sheet, and only then")));
}

#[test]
fn a_record_round_trips_through_json_and_damaged_ones_are_refused() {
    let mut record = RevisionRecord { numbering: Numbering::Letters, ..RevisionRecord::default() };
    let a = add(&mut record, None).unwrap();
    record.issue(a, None).unwrap();
    record.issue(a, Some("3f2a9c1e")).unwrap();
    add_tag(&mut record, a).unwrap();
    // The issued copies keep the model version they were issued against; the row has none.
    let versions: Vec<_> = record.issued().iter().map(|r| r.model_version.as_deref()).collect();
    assert_eq!((versions, record.rows()[0].model_version.as_deref()), (vec![None, Some("3f2a9c1e")], None));
    assert_eq!(record.issue(a, Some("x\ny")), Err(Invalid("a label is one line of at most 4096 bytes")));
    assert_eq!(record.issued().len(), 2, "a refused version issues nothing");
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["numbering"], json!("letters"));
    let id = json!(crate::ids::encode(a));
    assert_eq!(
        (&json["rows"][0]["id"], &json["tags"][0]["row"], &json["rows"][0]["sheet"]),
        (&id, &id, &json!(null)),
        "ids as every drawing writes them"
    );
    let back: RevisionRecord = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back, record);
    back.validate().unwrap();
    assert_eq!(serde_json::to_value(RevisionRecord::default()).unwrap()["numbering"], json!({"numbers": {"start": 1}}));
    for path in ["", "/rows/0", "/issued/0", "/tags/0"] {
        let mut bad = json.clone();
        bad.pointer_mut(path).unwrap().as_object_mut().unwrap().insert("colour".into(), json!(1));
        assert!(serde_json::from_value::<RevisionRecord>(bad).is_err(), "{path}");
    }
    let damaged = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut v = json.clone();
        change(&mut v);
        serde_json::from_value::<RevisionRecord>(v).unwrap().validate()
    };
    let ids = "each row and tag has its own id";
    for (change, message) in [
        (
            &(|v: &mut serde_json::Value| v["numbering"] = json!({"numbers": {"start": 2}})) as &dyn Fn(&mut serde_json::Value),
            "numbers start at 0 or 1",
        ),
        (&|v| v["tags"][0]["id"] = v["rows"][0]["id"].clone(), ids),
        (&|v| v["rows"] = json!([v["rows"][0], v["rows"][0]]), ids),
        (&|v| v["rows"][0]["sheet"] = json!(crate::ids::encode(3)), "a row has a sheet when the record is kept per sheet, and only then"),
        (&|v| v["tags"][0]["size_mm"] = json!(0.5), "a tag's side is 1 to 50 mm"),
        (&|v| v["tags"][0]["size_mm"] = json!(50.001), "a tag's side is 1 to 50 mm"),
        (&|v| v["tags"][0]["row"] = json!(crate::ids::encode(99)), "a tag is bound to a row"),
        (&|v| v["rows"] = json!([]), "a tag is bound to a row"),
        (&|v| v["rows"][0]["revision"] = json!("A\nB"), "a label is one line of at most 4096 bytes"),
        (&|v| v["issued"][0]["date"] = json!("\u{1}"), "a cell's text has no control character but a line break"),
        (&|v| v["tags"][0]["text"] = json!("x\ny"), "a label is one line of at most 4096 bytes"),
        (&|v| v["issued"][1]["model_version"] = json!("x\ny"), "a label is one line of at most 4096 bytes"),
        (&|v| v["rows"] = json!(vec![json!({"id": crate::ids::encode(1), "revision": "A"}); 1001]), "a record has at most 1000 rows"),
    ] {
        assert_eq!(damaged(change), Err(Invalid(message)), "{message}");
    }
}

#[test]
fn the_caps_hold() {
    let mut record = RevisionRecord::default();
    for _ in 0..MAX_ROWS {
        add(&mut record, None).unwrap();
    }
    assert_eq!(add(&mut record, None), Err(Invalid("a record has at most 1000 rows")));
    assert_eq!(record.rows().last().unwrap().revision, "1000");
    let first = record.rows()[0].id;
    for _ in 0..MAX_ISSUED {
        record.issue(first, None).unwrap();
        add_tag(&mut record, first).unwrap();
    }
    assert_eq!(record.issue(first, None), Err(Invalid("a record's issued history holds at most 10,000 entries")));
    assert_eq!(add_tag(&mut record, first), Err(Invalid("a record has at most 10,000 tags")));
    record.validate().unwrap();
}
