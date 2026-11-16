use serde_json::{Value, json};

use super::{NewDesignParams, NewDesignResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<NewDesignParams>(), &["name"]);
    let issues = |p: Value| crate::schema::issues("file.new", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert!(issues(json!({})).is_empty());
    assert!(issues(json!({"name": "Gearbox"})).is_empty());
    assert_eq!(issues(json!({"nmae": "Gearbox"})).len(), 1);
}

/// `file.new` has no call the handler refuses, so strict has no error of today's to keep: every
/// call that works with the check off still works in strict, with the same reply. A `name` that
/// is not text is the one call strict now refuses, which the handler read as left out.
#[test]
fn strict_keeps_todays_replies() {
    let reply = |level: ParamCheck, p: &Value| {
        let mut s = Session { param_check: level, ..Session::default() };
        s.execute("file.new", p).map_err(|e| e.to_string())
    };
    for p in [
        json!({}),
        json!(null),
        json!({"name": null}),
        json!({"name": "Gearbox lid"}),
        json!({"name": ""}),
        json!({"name": "  "}),
        json!({"name": "Untitled"}),
    ] {
        let off = reply(ParamCheck::Off, &p);
        assert!(off.is_ok(), "{p}");
        assert_eq!(reply(ParamCheck::Strict, &p), off, "{p}");
    }
    assert_eq!(reply(ParamCheck::Off, &json!({"name": 5})), Ok(json!({"name": "Untitled"})));
    assert!(reply(ParamCheck::Strict, &json!({"name": 5})).is_err());
}

/// The type reads `name` as the handler does: text, or `null` and absent as left out.
#[test]
fn the_types_read_each_key_as_the_handler_does() {
    let read = |p: Value| serde_json::from_value::<NewDesignParams>(p).map(|p| p.name);
    assert_eq!(read(json!({})).ok(), Some(None));
    assert_eq!(read(json!({"name": null})).ok(), Some(None));
    assert_eq!(read(json!({"name": "Lid"})).ok(), Some(Some("Lid".to_string())));
    assert_eq!(read(json!({"name": ""})).ok(), Some(Some(String::new())));
    assert!(read(json!({"name": 5})).is_err());
}

#[test]
fn the_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<NewDesignParams>();
    let schema = schema.as_value();
    for (p, takes) in [(json!({}), true), (json!({"name": "Lid"}), true), (json!({"name": 5}), false), (json!({"title": "x"}), false)] {
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// Real calls in Warn reply as before, with no warnings, and match the output schema.
#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<NewDesignResult>();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for (p, name) in [
        (json!({}), "Untitled"),
        (json!({"name": null}), "Untitled"),
        (json!({"name": "Gearbox lid"}), "Gearbox lid"),
        (json!({"name": ""}), ""),
        (json!({"name": "Untitled"}), "Untitled"),
    ] {
        let reply = s.execute("file.new", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(reply, json!({"name": name}), "{p}");
        assert!(reply.get("param_warnings").is_none(), "{p}: {reply}");
        assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{p}: {reply}");
    }
    assert_eq!(s.doc.name, "Untitled");
}

/// A misspelt key still runs in Warn, says so, and the reply still matches.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let reply = s.execute("file.new", &json!({"nmae": "Lid"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reply["name"], "Untitled", "{reply}");
    assert!(reply["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{reply}");
    let schema = output_schema_for::<NewDesignResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
}
