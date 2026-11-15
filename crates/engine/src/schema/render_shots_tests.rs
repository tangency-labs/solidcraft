use serde_json::{Value, json};

use super::{ShotChange, ShotParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A design with one saved render shot, `Shot`, at the parameter-check level `level`.
fn with_a_shot(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("render.settings.set", &json!({"name": "Shot", "set": {"width": 96, "height": 64}})).unwrap_or_else(|e| panic!("{e}"));
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ShotParams>(), &["expected_revision", "name", "new_name"]);
}

/// The handler refuses these calls after reading them; strict refuses them with its message.
/// They need a saved shot and the design's revision, so the shared helper, which has neither,
/// doesn't fit.
#[test]
fn strict_keeps_todays_errors() {
    let cases: [(&str, fn(u64) -> Value); 9] = [
        ("render.shot.duplicate", |_| json!({"name": "Shot", "new_name": "Copy", "expected_revision": 999_999})),
        ("render.shot.duplicate", |r| json!({"name": " Shot", "new_name": "Copy", "expected_revision": r})),
        ("render.shot.duplicate", |r| json!({"name": "Nope", "new_name": "Copy", "expected_revision": r})),
        ("render.shot.duplicate", |r| json!({"name": "Shot", "expected_revision": r})),
        ("render.shot.duplicate", |r| json!({"name": "Shot", "new_name": "x".repeat(129), "expected_revision": r})),
        ("render.shot.duplicate", |r| json!({"name": "Shot", "new_name": "Shot", "expected_revision": r})),
        ("render.shot.rename", |r| json!({"name": "Shot", "new_name": null, "expected_revision": r})),
        ("render.shot.rename", |r| json!({"name": "Shot", "new_name": "a\u{7}b", "expected_revision": r})),
        ("render.shot.remove", |r| json!({"name": "Shot", "new_name": "Other", "expected_revision": r})),
    ];
    for (id, p) in cases {
        let error = |level: ParamCheck| {
            let mut s = with_a_shot(level);
            let p = p(s.revision);
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {} should fail today", p(0));
        assert_eq!(error(ParamCheck::Strict), off, "{id} {}", p(0));
    }
}

/// #11450: duplicating, renaming and removing a shot reply as the output schema says, in Warn
/// without warnings. Like the handler, warn and strict refuse a misspelt key.
#[test]
fn shot_commands_reply_with_their_schema() {
    let schema = output_schema_for::<ShotChange>();
    let schema = schema.as_value();
    let mut s = with_a_shot(ParamCheck::Warn);
    let mut run = |id: &str, p: Value| {
        let mut p = p;
        p["expected_revision"] = json!(s.revision);
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run("render.shot.duplicate", json!({"name": "Shot", "new_name": "Copy"}));
    assert_eq!((r["operation"].as_str(), r["name"].as_str()), (Some("duplicate"), Some("Copy")), "{r}");
    let r = run("render.shot.rename", json!({"name": "Copy", "new_name": "Side"}));
    assert_eq!((r["operation"].as_str(), r["source"].as_str()), (Some("rename"), Some("Copy")), "{r}");
    let r = run("render.shot.remove", json!({"name": "Side", "new_name": null}));
    assert!(r["operation"] == "remove" && r["name"].is_null(), "{r}");
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = with_a_shot(level);
        let p = json!({"name": "Shot", "new_name": "Copy", "expected_revison": s.revision});
        let error = s.execute("render.shot.duplicate", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("expected_revison"), "{level:?}: {error}");
    }
}
