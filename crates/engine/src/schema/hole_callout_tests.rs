use serde_json::{Value, json};

use super::CalloutParams;
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CalloutParams>(), &["decimals", "feature", "language", "point", "units"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("drawing.hole_callout", json!({})),
        ("drawing.hole_callout", json!({"feature": 0})),
        ("drawing.hole_callout", json!({"feature": "Hole1"})),
        ("drawing.hole_callout", json!({"feature": 1, "decimals": 7})),
        ("drawing.hole_callout", json!({"feature": 1, "decimals": 2.5})),
        ("drawing.hole_callout", json!({"feature": 1, "units": 5})),
        ("drawing.hole_callout", json!({"feature": 1, "language": 5})),
    ]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<super::CalloutResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    let blind = run(&mut s, "solid.hole", json!({"position": [10, 15, 10], "thread": "M8", "thread_depth": 6, "depth": 8}))["feature"].clone();
    let through = run(
        &mut s,
        "solid.hole",
        json!({"position": [30, 15, 10], "diameter": 6, "type": "counterbore", "cb_diameter": 11, "cb_depth": 4}),
    )["feature"]
        .clone();
    let replies = [
        run(&mut s, "drawing.hole_callout", json!({"feature": blind})),
        run(&mut s, "drawing.hole_callout", json!({"feature": through, "units": "in", "decimals": 3, "language": "de"})),
        run(&mut s, "drawing.hole_callout", json!({"feature": through, "point": null, "units": null, "decimals": null, "language": null})),
    ];
    for r in &replies {
        assert_eq!(r["status"], "resolved", "{r}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    }
    run(&mut s, "timeline.suppress", json!({"feature": through, "suppressed": true}));
    let suppressed = run(&mut s, "drawing.hole_callout", json!({"feature": through}));
    assert_eq!(suppressed["status"], "suppressed", "{suppressed}");
    assert!(conforms(schema, schema, &suppressed), "{suppressed}");
    run(&mut s, "timeline.delete", json!({"feature": blind}));
    let lost = run(&mut s, "drawing.hole_callout", json!({"feature": blind}));
    assert_eq!(lost["status"], "lost", "{lost}");
    assert!(conforms(schema, schema, &lost), "{lost}");
}
