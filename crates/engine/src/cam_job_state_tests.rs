//! Tests of the open job's state: the writer's key order, indent and fixed point, the history,
//! and the structural diff.

use serde_json::{Value, json};

use super::*;

fn bracket() -> Value {
    // As a script might write it: keys out of the writer's order, one-space indent.
    serde_json::from_str(
        r#"{
 "operations": [
  {"type": "face", "tool": 32, "bottom": 0, "name": "face top", "coolant": true, "stepdown": 0.5},
  {"heights": {"bottom": -6, "top": 0, "retract": 3, "clearance": 10}, "name": "boss pocket", "type": "pocket", "tool": 6, "sketch": "PocketSketch", "linking": "stay_down", "stepover": 2.4}
 ],
 "stock": {"box": {"max": [50, 40, 1], "min": [-50, -40, -20]}},
 "origin": [0, 0, 20],
 "machine": "3axis",
 "name": "bracket-80x50",
 "post_path": "mill.post.json",
 "tolerance": 0.01
}"#,
    )
    .unwrap()
}

#[test]
fn the_writer_puts_keys_in_the_jobs_order_and_is_a_fixed_point() {
    let text = write_job(&bracket(), 1);
    let at = |k: &str| text.find(&format!("\"{k}\"")).unwrap_or_else(|| panic!("{k} in {text}"));
    // Job keys.
    assert!(at("name") < at("machine") && at("machine") < at("post_path") && at("post_path") < at("origin"), "{text}");
    assert!(at("origin") < at("tolerance") && at("tolerance") < at("stock") && at("stock") < at("operations"), "{text}");
    // An operation: name, type, tool first; heights, stepdown, stepover next; the rest by name;
    // coolant last.
    let face = &text[at("face top") - 12..];
    let pos = |k: &str| face.find(&format!("\"{k}\"")).unwrap();
    assert!(pos("name") < pos("type") && pos("type") < pos("tool") && pos("tool") < pos("stepdown") && pos("stepdown") < pos("bottom"));
    assert!(pos("bottom") < pos("coolant"), "{face}");
    // Heights in their own order, the box min before max.
    let h = &text[at("heights")..];
    assert!(h.find("clearance").unwrap() < h.find("retract").unwrap() && h.find("top").unwrap() < h.find("bottom").unwrap());
    assert!(text.find("\"min\"").unwrap() < text.find("\"max\"").unwrap());
    // One space a level, a final newline, integers stay integers.
    assert!(text.starts_with("{\n \"name\": \"bracket-80x50\",\n"), "{text}");
    assert!(text.ends_with("}\n") && text.contains("\"bottom\": -6") && text.contains("\"tolerance\": 0.01"), "{text}");
    // Read back and written again: byte for byte the same, and the same job.
    let again: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(again, bracket());
    assert_eq!(write_job(&again, detect_indent(&text)), text);
    // Two spaces, and empty lists and objects on one line.
    let two = write_job(&json!({"name": "x", "operations": [], "default_feeds": {}}), 2);
    assert_eq!(two, "{\n  \"name\": \"x\",\n  \"default_feeds\": {},\n  \"operations\": []\n}\n");
}

#[test]
fn the_indent_is_read_from_the_file() {
    assert_eq!(detect_indent("{\n \"name\": 1\n}"), 1);
    assert_eq!(detect_indent("{\n    \"name\": 1\n}"), 4);
    assert_eq!(detect_indent("{\"name\": 1}"), DEFAULT_INDENT);
    assert_eq!(detect_indent("{\n\t\"name\": 1\n}"), DEFAULT_INDENT);
    assert_eq!(detect_indent(""), DEFAULT_INDENT);
}

#[test]
fn undo_and_redo_walk_the_jobs_steps() {
    let start = json!({"operations": []});
    let mut job = OpenJob::new(None, start.clone(), false, None, 2);
    for k in 0..5 {
        job.change(format!("step {k}"), json!({"name": format!("n{k}"), "operations": []}));
    }
    assert_eq!(job.revision, 6);
    assert_eq!(job.redo().as_deref(), None);
    for k in (0..5).rev() {
        assert_eq!(job.undo().as_deref(), Some(format!("step {k}").as_str()));
    }
    assert_eq!(job.value, start, "five undos are the job as opened");
    assert_eq!(job.undo(), None);
    assert_eq!(job.redo().as_deref(), Some("step 0"));
    assert_eq!(job.value["name"], "n0");
    // A new step drops what could be redone; the history keeps at most its limit.
    job.change("other", json!({"operations": [], "name": "o"}));
    assert!(job.redo.is_empty());
    for k in 0..MAX_HISTORY + 10 {
        job.change(format!("s{k}"), json!({"operations": [], "tolerance": k}));
    }
    assert_eq!(job.undo.len(), MAX_HISTORY);
}

#[test]
fn a_saved_job_is_clean_until_it_changes() {
    let mut job = OpenJob::new(None, json!({"operations": []}), true, None, 2);
    assert!(!job.dirty());
    job.change("x", json!({"operations": [], "name": "a"}));
    assert!(job.dirty());
    job.undo();
    assert!(!job.dirty(), "undone back to what was saved");
    assert!(OpenJob::new(None, json!({"operations": []}), false, None, 2).dirty(), "never saved");
}

#[test]
fn the_diff_names_what_changed() {
    let old = bracket();
    let mut new = bracket();
    new["operations"][1]["stepover"] = json!(2);
    new["operations"].as_array_mut().unwrap().push(json!({"name": "deburr", "type": "chamfer"}));
    new["tolerance"] = json!(0.02);
    new.as_object_mut().unwrap().remove("origin");
    let lines = changes(&old, &new);
    assert!(lines.contains(&"operation 2 'boss pocket': stepover 2.4 -> 2".to_string()), "{lines:?}");
    assert!(lines.contains(&"added operation 3 'deburr'".to_string()), "{lines:?}");
    assert!(lines.contains(&"tolerance 0.01 -> 0.02".to_string()), "{lines:?}");
    assert!(lines.contains(&"origin removed".to_string()), "{lines:?}");
    assert_eq!(lines.len(), 4, "{lines:?}");
    let mut moved = bracket();
    moved["operations"].as_array_mut().unwrap().reverse();
    let lines = changes(&old, &moved);
    assert!(lines.iter().any(|l| l.starts_with("moved operation 'boss pocket' from 2 to 1")), "{lines:?}");
    assert!(changes(&old, &old).is_empty());
}

#[test]
fn deep_values_are_measured_without_going_deeper_than_asked() {
    let mut v = json!(1);
    for _ in 0..100 {
        v = json!([v]);
    }
    assert_eq!(depth(&v, 40), 40);
    assert_eq!(depth(&json!({"a": [1]}), 10), 2);
}
