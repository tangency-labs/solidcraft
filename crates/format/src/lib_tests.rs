use super::*;
use serde_json::json;

/// The app's error, as an app has one.
#[derive(Debug, PartialEq)]
struct AppError(String);

impl From<FormatError> for AppError {
    fn from(e: FormatError) -> Self {
        AppError(e.to_string())
    }
}

/// Each step appends its name to the file's `log`, so the order steps ran in can be read back.
fn log(v: &mut Value, step: &str) {
    if let Some(list) = v.get_mut("log").and_then(Value::as_array_mut) {
        list.push(json!(step));
    }
}

const HAND: [Step<AppError>; 2] = [
    |v| {
        log(v, "1to2");
        Ok(())
    },
    |v| {
        log(v, "2to3");
        Ok(())
    },
];
const V4: [Part<AppError>; 2] = [
    Part {
        name: "a",
        step: Some(|v| {
            log(v, "a");
            Ok(())
        }),
    },
    Part { name: "b", step: None },
];
const V5: [Part<AppError>; 1] = [Part {
    name: "c",
    step: Some(|v| {
        log(v, "c");
        Ok(())
    }),
}];
const FOLDED: [(u32, &[Part<AppError>]); 2] = [(4, &V4), (5, &V5)];
const PENDING: [Part<AppError>; 2] = [
    Part {
        name: "d",
        step: Some(|v| {
            log(v, "d");
            Ok(())
        }),
    },
    Part {
        name: "e",
        step: Some(|v| {
            log(v, "e");
            Ok(())
        }),
    },
];
const ALIASES: [Alias; 1] = [Alias { format: 5, name: "old-c", known_as: "c" }];

/// Format 1 untagged; hand steps to 3; folded 4 and 5; pending changes make 6.
const FAMILY: Family<'static, AppError> = Family {
    app: "App",
    noun: "design",
    prefix: "app/",
    legacy_prefix: Some("old/"),
    untagged: Some(1),
    aliases: &ALIASES,
    formats: Formats { hand: &HAND, folded: &FOLDED, pending: &PENDING },
};

fn steps_run(version: u32, applied: Option<&[String]>) -> Vec<String> {
    let mut v = json!({"log": []});
    FAMILY.upgrade(&mut v, version, applied).unwrap();
    v["log"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn tags_and_numbers() {
    assert_eq!(FAMILY.current(), 6);
    assert_eq!(FAMILY.format_string(6), "app/6");
    assert_eq!(FAMILY.version_of("app/4"), Some(4));
    assert_eq!(FAMILY.version_of("old/3"), Some(3), "the legacy tag is read");
    assert_eq!(FAMILY.version_of("other/3"), None);
    assert_eq!(FAMILY.pending_names(), vec!["d".to_string(), "e".to_string()]);
}

#[test]
fn a_file_is_taken_apart_or_refused_with_the_apps_words() {
    let mut v = json!({"format": "app/6", "format_pending": ["d"], "x": 1});
    assert_eq!(FAMILY.take_version(&mut v), Ok((6, Some(vec!["d".to_string()]))));
    assert!(v.get("format_pending").is_none(), "taken out");
    assert_eq!(FAMILY.take_version(&mut json!({})), Ok((1, None)), "untagged");
    let refused = |mut v: Value| FAMILY.take_version(&mut v).unwrap_err().0;
    assert_eq!(refused(json!([])), "not a App design (expected a JSON object)");
    assert_eq!(refused(json!({"format": "other/2"})), "not a App design (format `other/2`)");
    assert_eq!(refused(json!({"format": 2})), "not a App design (format is not a string)");
    assert_eq!(refused(json!({"format": "app/0"})), "not a App design (format 0)");
    assert_eq!(refused(json!({"format": "app/2", "format_pending": [1]})), "damaged design: `format_pending` is not a short list of names");
}

#[test]
fn every_step_runs_once_in_order() {
    assert_eq!(steps_run(1, None), ["1to2", "2to3", "a", "c", "d", "e"]);
    assert_eq!(steps_run(4, None), ["c", "d", "e"]);
    assert_eq!(steps_run(6, None), Vec::<String>::new());
    // A file written while `d` was pending has it: only `e` runs.
    assert_eq!(steps_run(6, Some(&["d".to_string()])), ["e"]);
    // A file of format 5 written while `c` was still pending, without it: `c` runs first.
    assert_eq!(steps_run(5, Some(&[])), ["c", "d", "e"]);
    // Under its other spelling `c` counts as had.
    assert_eq!(steps_run(5, Some(&["old-c".to_string()])), ["d", "e"]);
    // A list beside a hand-written format means nothing.
    assert_eq!(steps_run(2, Some(&["d".to_string()])), ["2to3", "a", "c", "d", "e"]);
    let mut v = json!({});
    assert_eq!(FAMILY.upgrade(&mut v, 0, None), Err(AppError("not a App design (format 0)".into())));
}

#[test]
fn a_newer_file_is_told_by_its_number_or_an_unknown_change() {
    assert_eq!(FAMILY.newer(6, None), None);
    assert_eq!(FAMILY.newer(6, Some(&["d".to_string()])), None);
    assert_eq!(FAMILY.newer(7, None), Some(Newer { format: 7, reads_up_to: 6, unknown_changes: Vec::new() }));
    assert_eq!(FAMILY.newer(6, Some(&["z".to_string()])), Some(Newer { format: 6, reads_up_to: 6, unknown_changes: vec!["z".to_string()] }));
    assert_eq!(FAMILY.newer(5, Some(&["old-c".to_string()])), None, "an alias of a known change");
    assert_eq!(FAMILY.newer(4, Some(&["old-c".to_string()])), Some(Newer { format: 4, reads_up_to: 6, unknown_changes: vec!["old-c".to_string()] }));
    assert_eq!(FAMILY.newer(2, Some(&["z".to_string()])), None, "a list beside a hand-written format means nothing");
}
