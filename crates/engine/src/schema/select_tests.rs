use serde_json::{Value, json};

use super::{
    SameLookParams, SelectClearParams, SelectClearResult, SelectFilterParams, SelectFilterResult, SelectItem, SelectMode, SelectSetParams,
    SelectSetResult, read_item,
};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use crate::{Sel, Session};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SelectSetParams>(), &["add", "items"]);
    assert_keys(input_schema_for::<SelectClearParams>(), &[]);
    assert_keys(input_schema_for::<SelectFilterParams>(), &["mode"]);
    assert_keys(input_schema_for::<SameLookParams>(), &["body", "face"]);
}

/// #16536: `select.filter` takes one of three words and answers it, and says so: the published
/// schema requires `mode` and lists the words, the reply is the mode that was given, and the type
/// reads a value exactly when the handler does.
#[test]
fn the_filter_answers_the_mode_it_was_given() {
    let input = input_schema_for::<SelectFilterParams>().as_value().clone();
    let described = Session::default().execute("engine.describe", &json!({"command": "select.filter"})).unwrap();
    assert_eq!(described["input"]["required"], json!(["mode"]), "the handler refuses a call without it");
    assert_eq!(described["input"]["additionalProperties"], false);
    // The words are published as the mode's own schema, inline or through a `$ref`.
    let mode = match described["input"]["properties"]["mode"].get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/")) {
        Some(name) => &described["input"]["$defs"][name],
        None => &described["input"]["properties"]["mode"],
    };
    let words: Vec<&str> = mode["oneOf"].as_array().into_iter().flatten().map(|one| one["const"].as_str().unwrap_or_default()).collect();
    assert_eq!(words, ["body", "face", "component"], "{described}");
    let spec = crate::find_command("select.filter").unwrap();
    assert!(spec.contract.is_some() && spec.output.is_some() && !spec.undoable);
    let typed = |p: &Value| serde_json::from_value::<SelectFilterParams>(p.clone()).is_ok();
    for mode in ["body", "face", "component"] {
        assert!(typed(&json!({"mode": mode})), "{mode}");
    }
    for p in [json!({"mode": "edge"}), json!({"mode": ""}), json!({"mode": 1}), json!({"mode": null})] {
        assert!(!typed(&p), "{p}");
    }
    // A documented enum is published as `oneOf`/`const`, which `conforms` doesn't weigh, so the
    // words are compared above; what the schema decides for itself are the keys around them.
    for p in [json!({"mode": "body", "modes": "body"}), json!({})] {
        assert!(!conforms(&input, &input, &p), "{p}");
    }
    // `mode` is required in the schema and left to the handler in the type, as the other commands
    // whose handler refuses a missing key do.
    assert!(typed(&json!({})));
    assert!(!conforms(&input, &input, &json!({})));
    assert_eq!(SelectMode::parse("component"), Some(SelectMode::Component));
    assert_eq!(SelectMode::parse("edge"), None);

    let schema = output_schema_for::<SelectFilterResult>().as_value().clone();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for mode in ["body", "face", "component"] {
        let r = s.execute("select.filter", &json!({"mode": mode})).unwrap_or_else(|e| panic!("{mode}: {e}"));
        assert_eq!(r, json!({"mode": mode}), "the reply is the JSON the command returned before the type");
        assert!(conforms(&schema, &schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings"), None, "{r}");
    }
    assert!(!conforms(&schema, &schema, &json!({})), "the reply always names the mode");
    // A key the command doesn't take is refused today: the handler's own reader denies unknown keys,
    // so the refusal is its message at every level, and strict only gets there first.
    for level in ParamCheck::ALL {
        let mut strict = Session { param_check: level, ..Session::default() };
        let said = strict.execute("select.filter", &json!({"mode": "body", "mode2": "body"})).err().map(|e| e.to_string());
        assert!(said.as_deref().is_some_and(|e| e.contains("mode2")), "{level:?}: {said:?}");
    }
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
        ("select.filter", json!({})),
        ("select.filter", json!({"mode": ""})),
        ("select.filter", json!({"mode": "edge"})),
        ("select.filter", json!({"mode": "Body"})),
        ("select.same_look", json!({})),
        ("select.same_look", json!({"body": "Missing"})),
        ("select.same_look", json!({"body": "Box1", "face": 99})),
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
    check(
        input_schema_for::<SameLookParams>(),
        &[
            (json!({}), true),
            (json!({"body": "Box1"}), true),
            (json!({"face": 0}), true),
            (json!({"body": "Box1", "face": 2}), true),
            (json!({"body": null, "face": null}), true),
            (json!({"body": 5}), false),
            (json!({"face": -1}), false),
            (json!({"face": 1.5}), false),
            (json!({"bodyy": "Box1"}), false),
        ],
    );
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

/// #16578: `select.same_look` reads its two optional keys through the published type and answers the
/// reply `select.set` publishes. What a body, a face or the current selection means stays the handler's.
#[test]
fn same_look_selects_the_objects_with_that_appearance() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box1"})).unwrap();
    s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [30, 0, 0], "body_name": "Box2"})).unwrap();
    let described = s.execute("engine.describe", &json!({"command": "select.same_look"})).unwrap();
    assert_eq!(described["input"]["additionalProperties"], false, "the command reads two keys and nothing else");
    assert_eq!(described["input"]["properties"].as_object().map(|p| p.len()), Some(2), "both keys are described");
    assert!(described["input"].get("required").is_none(), "neither key is required: the call may name a body, a face, or neither");
    let spec = crate::find_command("select.same_look").unwrap();
    assert!(spec.contract.is_some() && spec.output.is_some(), "the command describes its call and its reply");

    // Both boxes use the design's default look, so naming one of them selects both.
    let output = output_schema_for::<SelectSetResult>().as_value().clone();
    for p in [json!({"body": "Box1"}), json!({"body": "Box2"})] {
        let r = s.execute("select.same_look", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(r, json!({"selected": 2}), "{p} selects the objects with that look");
        assert!(conforms(&output, &output, &r), "{r} doesn't match {output}");
    }
    // The current selection is the default: a face of Box1, then the same look with no keys at all.
    s.execute("select.set", &json!({"items": [{"type": "face", "body": "Box1", "index": 0}]})).unwrap();
    let r = s.execute("select.same_look", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r, json!({"selected": 2}), "the selected face's look is the default too");
    assert_eq!(s.selection.len(), 2);
    // A body or a face that isn't there, and no selection at all, stay the handler's refusals.
    s.execute("select.clear", &json!({})).unwrap();
    for p in [json!({}), json!({"body": "Missing"}), json!({"body": "Box1", "face": 99})] {
        let off = s.execute("select.same_look", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(!off.is_empty(), "{p} should be refused");
        s.param_check = ParamCheck::Strict;
        let strict = s.execute("select.same_look", &p).err().map(|e| e.to_string()).unwrap_or_default();
        s.param_check = ParamCheck::Warn;
        assert_eq!(strict, off, "{p}");
    }
    // A key the command doesn't take is refused at every level: with the check off by the handler's
    // own `deny_unknown_fields`, in warn after the warning, in strict by the contract. Each names
    // the key the caller wrote.
    let p = json!({"bodyy": "Box1"});
    for (level, name) in [(ParamCheck::Off, "off"), (ParamCheck::Warn, "warn"), (ParamCheck::Strict, "strict")] {
        s.param_check = level;
        let e = s.execute("select.same_look", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("bodyy"), "{name}: {e}");
    }
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
    // `conforms` doesn't check `const`, so each item is also matched to the branch of its `type`.
    let branches = schema["$defs"]["SelectItem"]["anyOf"].as_array().cloned().unwrap_or_default();
    assert!(!branches.is_empty(), "no SelectItem branches in {schema}");
    // Every branch is one of the kinds listed here.
    for b in &branches {
        assert!(items.iter().any(|i| i["type"] == b["properties"]["type"]["const"]), "{b} isn't tried");
    }
    for item in items {
        assert!(serde_json::from_value::<Sel>(item.clone()).is_ok(), "{item} isn't a Sel");
        assert!(serde_json::from_value::<SelectItem>(item.clone()).is_ok(), "{item}");
        let kind = item["type"].as_str().unwrap_or_default();
        let branch = branches.iter().find(|b| b["properties"]["type"]["const"] == kind);
        assert!(branch.is_some_and(|b| conforms(schema, b, &item)), "no `{kind}` branch takes {item}");
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
