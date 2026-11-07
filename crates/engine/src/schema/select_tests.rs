use serde_json::{Value, json};

use super::{SelectClearParams, SelectClearResult, SelectItem, SelectSetParams, SelectSetResult, read_item};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use crate::{Sel, Session};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SelectSetParams>(), &["add", "items"]);
    assert_keys(input_schema_for::<SelectClearParams>(), &[]);
}

/// Strict gives the handler's message for the items it refuses on their own.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("select.set", json!({"items": [{"type": "bogus"}]})),
        ("select.set", json!({"items": [{}]})),
        ("select.set", json!({"items": [7]})),
        ("select.set", json!({"items": [{"type": "body"}]})),
        ("select.set", json!({"items": [{"type": "body", "name": 5}]})),
        ("select.set", json!({"items": [{"type": "feature", "id": -1}]})),
        ("select.set", json!({"items": [{"type": "occurrence", "path": "1"}]})),
        ("select.set", json!({"items": [{"type": "face"}]})),
        ("select.set", json!({"items": [{"type": "face", "body": "Box1"}]})),
        ("select.set", json!({"items": [{"type": "face", "index": 0}]})),
        ("select.set", json!({"items": [{"type": "edge", "body": "Box1", "index": -1}]})),
        ("select.set", json!({"items": [{"type": "edge", "body": "Box1", "point": [1, 2]}]})),
        ("select.set", json!({"items": [{"type": "edge", "body": "Box1", "index": 0, "point": null}]})),
        ("select.set", json!({"items": [{"type": "face", "body": "Box1", "index": null, "point": [1, 2, 3]}]})),
        ("select.set", json!({"items": [{"type": "body", "name": "Missing"}]})),
        ("select.set", json!({"items": [{"type": "face", "body": "Missing", "point": [1, 2, 3]}]})),
    ]);
}

/// The item type reads each item as the handler does: face and edge items with one of `index`
/// and `point` are partial picks, everything else a `Sel`.
#[test]
fn the_item_type_reads_like_the_handler() {
    let item = |v: Value| serde_json::from_value::<SelectItem>(v);
    assert!(matches!(item(json!({"type": "body", "name": "A", "colour": 1})), Ok(SelectItem::Exact(_))));
    assert!(matches!(item(json!({"type": "face", "body": "A", "index": 2})), Ok(SelectItem::Pick { face: true, .. })));
    assert!(matches!(item(json!({"type": "edge", "body": "A", "point": [1, 2, 3], "extra": 1})), Ok(SelectItem::Pick { face: false, .. })));
    assert!(matches!(item(json!({"type": "edge", "body": "A", "point": null})), Ok(SelectItem::Pick { .. })));
    // Neither: the handler refuses it against the design, after it has looked up the body.
    assert!(matches!(item(json!({"type": "face", "body": "A"})), Ok(SelectItem::Pick { .. })));
    assert!(matches!(item(json!({"type": "face", "body": "A", "index": 2, "point": [1, 2, 3]})), Ok(SelectItem::Exact(_))));
    assert!(matches!(item(json!({"type": "sketch_curve", "id": "c1"})), Ok(SelectItem::Exact(_))));
    assert!(matches!(item(json!({"type": "profile", "sketch": 3, "index": 0})), Ok(SelectItem::Exact(_))));
    // Both given: read whole, as a Sel, so a null refuses.
    assert!(item(json!({"type": "face", "body": "A", "index": 2, "point": null})).is_err());
    assert!(item(json!({"type": "face", "index": 2})).is_err());
    assert!(item(json!({"type": "face", "body": "A", "index": "2"})).is_err());
    assert!(item(json!({"type": "nothing"})).is_err() && item(json!(null)).is_err() && item(json!("body")).is_err());
    // The messages are the handler's (what serde says), whole.
    for bad in [json!({"type": "nothing"}), json!({"type": "face"}), json!({"type": "body"}), json!(3)] {
        let handler = read_item(bad.clone()).err().unwrap_or_default();
        let typed = item(bad.clone()).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(typed.ends_with(&handler), "{bad}: {typed} / {handler}");
    }
}

/// The schema takes what the type takes, and the keys the params type reads.
#[test]
fn the_schemas_take_what_the_handler_reads() {
    let check = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value().clone();
        for (p, takes) in cases {
            assert_eq!(conforms(&schema, &schema, p), *takes, "{p}");
        }
    };
    let cases = [
        (json!({}), true),
        (json!({"items": []}), true),
        (json!({"items": null, "add": null}), true),
        (json!({"items": [{"type": "body", "name": "A"}], "add": true}), true),
        (json!({"items": [{"type": "face", "body": "A", "index": 1}, {"type": "edge", "body": "A", "point": [1, 2, 3]}]}), true),
        (json!({"items": [{"type": "face", "body": "A", "index": 1, "point": [1, 2, 3]}]}), true),
        (json!({"items": [{"type": "plane", "name": "XY"}, {"type": "occurrence", "path": [1, 2]}, {"type": "feature", "id": 4}]}), true),
        (json!({"items": [{"type": "sketch_curve", "id": "c1", "sketch": 2}, {"type": "profile", "sketch": 2, "index": 0}]}), true),
        (json!({"items": [{"type": "face", "body": "A"}]}), false),
        (json!({"items": [{"type": "bogus"}]}), false),
        (json!({"items": [{"type": "body"}]}), false),
        (json!({"items": "A"}), false),
        (json!({"add": "yes"}), false),
        (json!({"item": []}), false),
    ];
    check(input_schema_for::<SelectSetParams>(), &cases);
    check(input_schema_for::<SelectClearParams>(), &[(json!({}), true), (json!({"add": true}), false)]);
    // The schema differs from the type in two cases: a face or edge with neither `index` nor
    // `point`, which the type leaves to the handler (its message depends on the design), and an
    // unknown key, which the key check reports, not the type.
    let apart = [json!({"items": [{"type": "face", "body": "A"}]}), json!({"item": []})];
    for (p, takes) in cases.iter().filter(|(p, _)| !apart.contains(p)) {
        assert_eq!(serde_json::from_value::<SelectSetParams>(p.clone()).is_ok(), *takes, "{p}");
    }
}

/// Real calls of both commands in Warn: no warnings, and the replies match the output schemas
/// and are what the handlers have always returned. A misspelt key gets its warning and the
/// reply still matches; strict refuses it.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box1"})).unwrap();
    s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [30, 0, 0], "body_name": "Box2"})).unwrap();
    let matches = |schema: schemars::Schema, r: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    };
    let run = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        r
    };
    let set = |s: &mut Session, p: Value| {
        let r = run(s, "select.set", p);
        matches(output_schema_for::<SelectSetResult>(), &r);
        r
    };
    assert_eq!(set(&mut s, json!({})), json!({"selected": 0}));
    assert_eq!(set(&mut s, json!({"items": [{"type": "body", "name": "Box1"}]})), json!({"selected": 1}));
    assert_eq!(set(&mut s, json!({"items": [{"type": "body", "name": "Box2"}], "add": true})), json!({"selected": 2}));
    assert_eq!(set(&mut s, json!({"items": [{"type": "body", "name": "Box2"}], "add": true})), json!({"selected": 2}));
    assert_eq!(set(&mut s, json!({"items": null})), json!({"selected": 0}));
    assert_eq!(set(&mut s, json!({"items": [{"type": "face", "body": "Box1", "index": 0}], "add": null})), json!({"selected": 1}));
    assert_eq!(
        set(&mut s, json!({"items": [{"type": "face", "body": "Box1", "point": [5, 5, 10]}, {"type": "edge", "body": "Box1", "index": 0}]})),
        json!({"selected": 2})
    );
    let complete = s.selection[0].clone();
    assert_eq!(set(&mut s, json!({"items": [complete, {"type": "plane", "name": "XY", "colour": "red"}], "add": true})), json!({"selected": 3}));
    let r = run(&mut s, "select.clear", json!({}));
    matches(output_schema_for::<SelectClearResult>(), &r);
    assert_eq!(r, json!({"selected": 0}));
    assert!(s.selection.is_empty());
    // A misspelt key runs in warn and says so; the reply still matches.
    let p = json!({"items": [{"type": "body", "name": "Box1"}], "ad": true});
    let r = s.execute("select.set", &p).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    assert_eq!(r["selected"], 1);
    matches(output_schema_for::<SelectSetResult>(), &r);
    let r = s.execute("select.clear", &json!({"all": true})).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    matches(output_schema_for::<SelectClearResult>(), &r);
    s.param_check = ParamCheck::Strict;
    let error = s.execute("select.set", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`ad`"), "{error}");
    let error = s.execute("select.clear", &json!({"all": true})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`all`"), "{error}");
}

/// One item of every `Sel` variant, as `Sel` serializes it (and the forms it also reads): each is
/// a `Sel`, is read by the item type, matches the item schema, and is accepted in strict.
#[test]
fn every_selection_kind_is_taken() {
    let items = [
        json!({"type": "occurrence", "path": [1, 2]}),
        json!({"type": "body", "name": "A"}),
        json!({"type": "edge", "body": "A", "index": 1, "point": [1, 2, 3]}),
        json!({"type": "face", "body": "A", "index": 1, "point": [1, 2, 3]}),
        json!({"type": "feature", "id": 4}),
        json!({"type": "sketch_curve", "id": "c1"}),
        json!({"type": "sketch_curve", "id": "c1", "sketch": 2}),
        json!({"type": "sketch_curve", "id": "c1", "sketch": null}),
        json!({"type": "sketch_point", "id": "p1"}),
        json!({"type": "sketch_midpoint", "id": "l1"}),
        json!({"type": "external_sketch_curve", "sketch": 2, "id": "c1"}),
        json!({"type": "external_sketch_point", "sketch": 2, "id": "p1"}),
        json!({"type": "sketch_constraint", "id": "k1"}),
        json!({"type": "profile", "sketch": 2, "index": 0}),
        json!({"type": "plane", "name": "XY"}),
        json!({"type": "construction_point", "name": "P"}),
        json!({"type": "axis", "name": "X"}),
        json!({"type": "vertex", "body": "A", "point": [0, 0, 0]}),
    ];
    let schema = input_schema_for::<SelectSetParams>();
    let schema = schema.as_value();
    for item in items {
        assert!(serde_json::from_value::<Sel>(item.clone()).is_ok(), "{item} isn't a Sel");
        assert!(serde_json::from_value::<SelectItem>(item.clone()).is_ok(), "{item}");
        let p = json!({"items": [item]});
        assert!(conforms(schema, schema, &p), "{p} doesn't match {schema}");
    }
}

/// Valid calls are not refused in Strict, and a wrong-typed `items` or `add`, which the handler
/// reads as if left out, is reported in Warn and refused in Strict.
#[test]
fn strict_takes_valid_calls_and_warn_reports_wrong_types() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box1"})).unwrap();
    for p in [
        json!({}),
        json!({"items": null, "add": null}),
        json!({"items": [{"type": "body", "name": "Box1"}], "add": true}),
        json!({"items": [{"type": "face", "body": "Box1", "index": 0}]}),
        json!({"items": [{"type": "edge", "body": "Box1", "point": [0, 0, 0]}], "add": true}),
        json!({"items": [{"type": "plane", "name": "XY", "colour": "red"}]}),
    ] {
        let r = s.execute("select.set", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
    }
    assert_eq!(s.execute("select.clear", &json!({})).unwrap(), json!({"selected": 0}));
    for p in [json!({"items": "Box1"}), json!({"add": "yes"})] {
        s.param_check = ParamCheck::Warn;
        let r = s.execute("select.set", &p).unwrap();
        assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{p}: {r}");
        s.param_check = ParamCheck::Strict;
        assert!(s.execute("select.set", &p).is_err(), "{p}");
    }
}
