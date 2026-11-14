use serde_json::{Value, json};

use super::{SlicerInfo, SlicersParams, SlicersResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SlicersParams>(), &[]);
}

/// The handler reads no key, so there is no value of one to refuse: strict refuses a key it does
/// not know, with the check's own message, and the handler never refuses anything.
#[test]
fn the_type_reads_no_key_as_the_handler_reads_none() {
    assert!(serde_json::from_value::<SlicersParams>(json!({})).is_ok());
    // An unknown key is the key check's to report, not the type's.
    assert!(serde_json::from_value::<SlicersParams>(json!({"path": "x"})).is_ok());
    let mut s = Session { param_check: ParamCheck::Off, ..Session::default() };
    for p in [json!({}), json!({"anything": 1, "path": null})] {
        assert!(s.execute("file.slicers", &p).is_ok(), "{p}");
    }
    let schema = input_schema_for::<SlicersParams>();
    assert!(conforms(schema.as_value(), schema.as_value(), &json!({})));
    assert!(!conforms(schema.as_value(), schema.as_value(), &json!({"slicer": "prusa"})));
}

/// Strict has nothing of today's to keep: no call of `file.slicers` is refused by the handler.
/// Called with no key it runs, as it always did, and a misspelt one is the check's to refuse.
#[test]
fn strict_keeps_todays_behaviour() {
    assert_strict_keeps_todays_errors(&[]);
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    assert!(s.execute("file.slicers", &json!({})).is_ok());
    assert!(s.execute("file.slicers", &json!({"slicer": "prusa"})).is_err());
}

/// A call with no key has nothing to warn about; one with a key is told it is unknown.
#[test]
fn the_check_names_an_unknown_key() {
    assert!(issues("file.slicers", &json!({})).is_empty());
    assert_eq!(issues("file.slicers", &json!({"slicer": "prusa"})).len(), 1);
}

fn matches(reply: &Value) {
    let schema = output_schema_for::<SlicersResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), reply), "{reply} doesn't match {schema:?}");
    assert!(reply.get("param_warnings").is_none(), "{reply}");
}

/// What the entries serialize to: every key is there, and a missing program or icon is null, as
/// the handler has always answered.
#[test]
fn an_entry_serializes_with_null_for_what_is_missing() {
    let full = SlicerInfo { id: "prusa".into(), name: "PrusaSlicer".into(), program: Some("/usr/bin/prusa".into()), icon: Some("/i.png".into()) };
    assert_eq!(
        serde_json::to_value(full).unwrap_or_default(),
        json!({"id": "prusa", "name": "PrusaSlicer", "program": "/usr/bin/prusa", "icon": "/i.png"})
    );
    let bare = SlicerInfo { id: "x".into(), name: "X".into(), program: None, icon: None };
    assert_eq!(serde_json::to_value(bare).unwrap_or_default(), json!({"id": "x", "name": "X", "program": null, "icon": null}));
    let none = SlicersResult { slicers: Vec::new(), param_warnings: Vec::new() };
    let reply = serde_json::to_value(none).unwrap_or_default();
    assert_eq!(reply, json!({"slicers": []}));
    matches(&reply);
}

/// The real command in Warn, on this machine's own list, then on made-up ones (one with an icon
/// and a program, one with neither): each reply matches the output schema and carries no
/// warning. A misspelt key still runs, says so, and the reply still matches.
#[cfg(unix)]
#[test]
fn replies_match_the_output_schema() {
    use crate::slicers::{IconSource, Slicer, TEST_DETECTED};
    use crate::tests::mesh_convert::{CHOSEN_PROGRAMS, MadeUp};
    let _turn = CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _reset = MadeUp;
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let own = s.execute("file.slicers", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert!(own["slicers"].is_array(), "{own}");
    matches(&own);

    let entry = |id: &str, command: Vec<String>, icon: Option<IconSource>| Slicer {
        id: id.into(),
        name: format!("{id} name"),
        command,
        icon,
        prusa_major: None,
    };
    *TEST_DETECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(vec![
        entry("prusa", vec!["/opt/prusa".into(), "--arg".into()], Some(IconSource::Image("/icons/prusa.png".into()))),
        entry("bare", Vec::new(), None),
    ]);
    let listed = s.execute("file.slicers", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        listed,
        json!({"slicers": [
            {"id": "prusa", "name": "prusa name", "program": "/opt/prusa", "icon": "/icons/prusa.png"},
            {"id": "bare", "name": "bare name", "program": null, "icon": null},
        ]})
    );
    matches(&listed);

    let warned = s.execute("file.slicers", &json!({"slicr": true})).unwrap_or_else(|e| panic!("{e}"));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    assert_eq!(warned["slicers"], listed["slicers"]);
    let schema = output_schema_for::<SlicersResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
}
