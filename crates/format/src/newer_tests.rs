use super::*;

fn newer() -> Newer {
    Newer { format: 9, reads_up_to: 8, unknown_changes: Vec::new() }
}

/// What typed reading dropped is named and goes back on saving, where it was.
#[test]
fn dropped_fields_are_named_and_put_back() {
    let file = json!({"name": "A", "format": "app/9", "items": [{"id": 1, "size": 2, "colour": "red"}], "extra": {"x": 1}});
    let known = json!({"name": "A", "items": [{"id": 1, "size": 2}]});
    let mut said = NewerFile::new("App", &newer());
    said.compare(&file, &known, "placeholder", &[]);
    assert_eq!(said.not_understood, ["field `extra`", "field `items[1].colour`"]);
    let mut saved = json!({"name": "A", "items": [{"id": 1, "size": 3}]});
    assert!(said.restore_into(&mut saved).is_empty());
    assert_eq!(saved, json!({"name": "A", "items": [{"id": 1, "size": 3, "colour": "red"}], "extra": {"x": 1}}));
    assert_eq!(said.notice(), "Saved by a newer App (format 9, this version reads up to 8). If something's missing, update.");
}

/// A section that can't be read is left out, kept, and goes back while it is unchanged here.
#[test]
fn unreadable_sections_are_left_out_and_kept() {
    let mut top = match json!({"name": "A", "good": 1, "bad": 2}) {
        Value::Object(m) => m,
        _ => unreachable!(),
    };
    let mut said = NewerFile::new("App", &newer());
    leave_out_unreadable(&mut top, &mut said, "design", |key: &str, _: &Value| if key == "bad" { Err("design file: no") } else { Ok(()) });
    assert!(!top.contains_key("bad"));
    assert_eq!(said.not_understood, ["section `bad` (no)"]);
    let read =
        read_around(
            &mut top,
            &mut said,
            &["name"],
            "design",
            |top: &Map<String, Value>| {
                if top.contains_key("good") { Err("design file: clash") } else { Ok(top.len()) }
            },
        );
    assert_eq!(read, Ok(1));
    assert_eq!(said.not_understood, ["section `bad` (no)", "section `good` (clash)"]);
    let mut saved = json!({"name": "A"});
    said.compare(&saved.clone(), &saved.clone(), "placeholder", &[]);
    assert!(said.restore_into(&mut saved).is_empty());
    assert_eq!(saved, json!({"name": "A", "good": 1, "bad": 2}));
}
