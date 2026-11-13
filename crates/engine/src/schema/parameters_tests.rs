use serde_json::{Value, json};

use super::{
    ExprCompleteParams, ExprCompletions, ExprEvaluateParams, ExprEvaluated, ParamAddParams, ParamAdded, ParamChangeParams, ParamChanged,
    ParamChangedBody, ParamCommentParams, ParamCommented, ParamDeleted, ParamExportParams, ParamExportResult, ParamFavoriteParams, ParamFavorited,
    ParamGraphParams, ParamGraphResult, ParamImportParams, ParamImportResult, ParamListParams, ParamListResult, ParamMoveParams, ParamMoved,
    ParamNameParams, ParamRenameParams, ParamRenamed, ParamUsersParams, ParamUsersResult, SwitchedFeature,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ParamAddParams>(), &["comment", "expression", "favorite", "name", "unit", "value"]);
    assert_keys(input_schema_for::<ParamRenameParams>(), &["name", "new_name"]);
    assert_keys(input_schema_for::<ParamNameParams>(), &["name"]);
    assert_keys(input_schema_for::<ParamCommentParams>(), &["comment", "name"]);
    assert_keys(input_schema_for::<ParamFavoriteParams>(), &["favorite", "name"]);
    assert_keys(input_schema_for::<ParamMoveParams>(), &["after", "before", "name"]);
    assert_keys(
        input_schema_for::<ParamChangeParams>(),
        &["changes", "comment", "delete", "expression", "favorite", "name", "new_name", "unit", "value"],
    );
    assert_keys(input_schema_for::<ParamListParams>(), &["favorites", "filter"]);
    assert_keys(input_schema_for::<ParamUsersParams>(), &["name"]);
    assert_keys(input_schema_for::<ParamGraphParams>(), &[]);
    assert_keys(input_schema_for::<ParamExportParams>(), &["format", "path"]);
    assert_keys(input_schema_for::<ParamImportParams>(), &["path", "text"]);
    assert_keys(input_schema_for::<ExprEvaluateParams>(), &["expression", "kind", "unit"]);
    assert_keys(input_schema_for::<ExprCompleteParams>(), &["prefix"]);
}

/// The calls fail on a design without parameters, with the handlers' messages or the
/// document's.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("parameters.add", json!({})),
        ("parameters.add", json!({"name": "  ", "expression": 1})),
        ("parameters.add", json!({"name": 5, "expression": 1})),
        ("parameters.add", json!({"name": "W"})),
        ("parameters.add", json!({"name": "W", "expression": true})),
        ("parameters.add", json!({"name": "W", "expression": null})),
        ("parameters.add", json!({"name": "W", "value": [1]})),
        ("parameters.add", json!({"name": "1W", "expression": 1})),
        ("parameters.add", json!({"name": "W", "expression": "1 +"})),
        ("parameters.add", json!({"name": "W", "expression": 1, "unit": "furlong"})),
        ("parameters.rename", json!({"new_name": "X"})),
        ("parameters.rename", json!({"name": "Nope", "new_name": "X"})),
        ("parameters.rename", json!({"name": "W"})),
        ("parameters.rename", json!({"name": "W", "new_name": 5})),
        ("parameters.delete", json!({})),
        ("parameters.delete", json!({"name": "Nope"})),
        ("parameters.comment", json!({"name": "Nope", "comment": "x"})),
        ("parameters.comment", json!({"name": "W"})),
        ("parameters.comment", json!({"name": "W", "comment": null})),
        ("parameters.comment", json!({"name": "W", "comment": 5})),
        ("parameters.favorite", json!({"name": []})),
        ("parameters.favorite", json!({"name": "Nope"})),
        ("parameters.move", json!({"name": "W"})),
        ("parameters.move", json!({"name": "W", "before": "A", "after": "B"})),
        ("parameters.move", json!({"name": "W", "before": 5})),
        ("parameters.move", json!({"name": "Nope", "before": "A"})),
        ("parameters.change", json!({})),
        ("parameters.change", json!({"name": "W"})),
        ("parameters.change", json!({"name": " ", "expression": 1})),
        ("parameters.change", json!({"name": "W", "expression": true})),
        ("parameters.change", json!({"name": "W", "expression": null})),
        ("parameters.change", json!({"changes": null})),
        ("parameters.change", json!({"changes": [1]})),
        ("parameters.change", json!({"changes": {}})),
        ("parameters.change", json!({"changes": {"W": true}})),
        ("parameters.change", json!({"changes": {"W": [1]}, "name": "W", "expression": 2})),
        ("parameters.change", json!({"changes": {"Nope": 1}})),
        ("parameters.change", json!({"delete": "Nope"})),
        ("parameters.change", json!({"delete": "  "})),
        ("parameters.change", json!({"name": "Nope", "favorite": true})),
        ("parameters.change", json!({"name": "Nope", "new_name": "X"})),
        ("parameters.users", json!({})),
        ("parameters.users", json!({"name": 5})),
        ("parameters.users", json!({"name": "Nope"})),
        ("parameters.export", json!({"format": "xml"})),
        ("parameters.export", json!({"format": "CSV", "path": "/nonexistent-dir/x/y.csv"})),
        ("parameters.import", json!({})),
        ("parameters.import", json!({"text": null, "path": null})),
        ("parameters.import", json!({"text": 5})),
        ("parameters.import", json!({"path": 5})),
        ("parameters.import", json!({"text": "name,expression\n1W,1"})),
        ("parameters.import", json!({"path": "/nonexistent-dir/x/y.csv"})),
        ("expr.evaluate", json!({})),
        ("expr.evaluate", json!({"expression": null})),
        ("expr.evaluate", json!({"expression": true})),
        ("expr.evaluate", json!({"expression": ["1"]})),
        ("expr.evaluate", json!({"expression": "1", "unit": "furlong"})),
        ("expr.evaluate", json!({"expression": "1", "kind": "colour"})),
        ("expr.evaluate", json!({"expression": "1", "unit": "mm", "kind": "colour"})),
    ]);
}

/// The schemas take what the handlers read: `expression` or `value` as a number or text (`null`
/// is read as left out, so it doesn't count), a non-blank name, and exactly one of `before` and
/// `after` as text.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let add = input_schema_for::<ParamAddParams>();
    let add = add.as_value();
    for (p, takes) in [
        (json!({"name": "W", "expression": "10 mm"}), true),
        (json!({"name": "W", "expression": 10, "unit": "mm", "comment": "Wall", "favorite": true}), true),
        (json!({"name": "W", "value": 10}), true),
        (json!({"name": "W", "expression": null, "value": 3}), true),
        (json!({"name": "W", "expression": null}), false),
        (json!({"name": "W", "expression": true}), false),
        (json!({"name": "", "expression": 10}), false),
    ] {
        assert_eq!(conforms(add, add, &p), takes, "{p}");
    }
    assert_eq!(
        add["anyOf"],
        json!([
            {"required": ["expression"], "properties": {"expression": {"type": ["number", "string"]}}},
            {"required": ["value"], "properties": {"value": {"type": ["number", "string"]}}}
        ]),
        "{add}"
    );
    let comment = input_schema_for::<ParamCommentParams>();
    let comment = comment.as_value();
    assert!(!conforms(comment, comment, &json!({"name": "W"})), "{comment}");
    assert!(!conforms(comment, comment, &json!({"name": "W", "comment": null})), "{comment}");
    // `conforms` doesn't read `oneOf`, so `parameters.move`'s is pinned, and counted here.
    let moved = input_schema_for::<ParamMoveParams>();
    let moved = moved.as_value();
    assert_eq!(
        moved["oneOf"],
        json!([
            {"required": ["before"], "properties": {"before": {"type": "string"}}},
            {"required": ["after"], "properties": {"after": {"type": "string"}}}
        ]),
        "{moved}"
    );
    let one_of = |p: &Value| moved["oneOf"].as_array().into_iter().flatten().filter(|b| conforms(moved, b, p)).count() == 1;
    for (p, takes) in [
        (json!({"name": "W", "before": "A"}), true),
        (json!({"name": "W", "after": "A"}), true),
        (json!({"name": "W", "before": null, "after": "B"}), true),
        (json!({"name": "W"}), false),
        (json!({"name": "W", "before": null}), false),
        (json!({"name": "W", "before": "A", "after": "B"}), false),
        (json!({"name": "W", "before": 5}), false),
    ] {
        assert_eq!(conforms(moved, moved, &p) && one_of(&p), takes, "{p}");
    }
}

/// #11941: adding a number and a text parameter, then commenting, favouriting, moving,
/// renaming and deleting, reply as the output schemas say, in Warn without warnings. A
/// misspelt key gets `param_warnings` in Warn, since the handlers ignore unknown keys, and
/// strict refuses it.
#[test]
fn parameter_edits_reply_with_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run("parameters.add", json!({"name": "W", "expression": "10 mm", "comment": "Wall"}), output_schema_for::<ParamAdded>());
    assert_eq!((r["unit"].as_str(), r["value"].as_f64(), r["display_value"].as_f64()), (Some("mm"), Some(10.0), Some(10.0)), "{r}");
    let r = run("parameters.add", json!({"name": "Label", "expression": "Rev A", "unit": "text"}), output_schema_for::<ParamAdded>());
    assert!(r["text"] == "Rev A" && r["value"].is_null() && r.get("display_value").is_none(), "{r}");
    run("parameters.add", json!({"name": "H", "value": 5, "unit": "mm", "favorite": true}), output_schema_for::<ParamAdded>());
    run("parameters.comment", json!({"name": "H", "comment": "Height"}), output_schema_for::<ParamCommented>());
    let r = run("parameters.favorite", json!({"name": "H", "favorite": false}), output_schema_for::<ParamFavorited>());
    assert_eq!(r["favorite"], false, "{r}");
    let r = run("parameters.move", json!({"name": "H", "before": "W"}), output_schema_for::<ParamMoved>());
    let at = |name: &str| r["order"].as_array().and_then(|order| order.iter().position(|n| n == name));
    assert!(at("H").is_some() && at("H") < at("W"), "{r}");
    let r = run("parameters.rename", json!({"name": "W", "new_name": " Width "}), output_schema_for::<ParamRenamed>());
    assert_eq!((r["from"].as_str(), r["to"].as_str()), (Some("W"), Some("Width")), "{r}");
    run("parameters.delete", json!({"name": "Label"}), output_schema_for::<ParamDeleted>());
    let p = json!({"name": "Width", "favourite": true});
    let mut warned = Session { param_check: ParamCheck::Warn, ..Session::default() };
    warned.execute("parameters.add", &json!({"name": "Width", "expression": 3})).unwrap_or_else(|e| panic!("{e}"));
    let r = warned.execute("parameters.favorite", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("favourite"), "{r}");
    warned.param_check = ParamCheck::Strict;
    let error = warned.execute("parameters.favorite", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`favourite`"), "{error}");
}

/// Whether the contract reports anything for the call, and what: the warnings of Warn.
fn issues(id: &str, p: Value) -> Vec<String> {
    crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect()
}

/// `parameters.change` reads `changes` alone when it is there, `delete` alone when that is text,
/// else the keys of one change: a call that works today in any of the forms gets no warning for
/// a key of a form it doesn't read, and each form has the handler's messages.
#[test]
fn change_reads_only_the_keys_of_the_form_it_is_in() {
    let none = Vec::<String>::new();
    // The keys of another form are not read: a wrong one is no problem.
    assert_eq!(
        issues("parameters.change", json!({"changes": {"W": 1, "H": "2 mm"}, "name": 5, "expression": true, "unit": 1, "favorite": "x"})),
        none
    );
    assert_eq!(issues("parameters.change", json!({"delete": "W", "name": 5, "expression": true, "favorite": "x", "unit": 3})), none);
    assert_eq!(issues("parameters.change", json!({"changes": {"W": 1}, "delete": 5})), none);
    // A `delete` that isn't text is read as left out: the single change form it falls to
    // reports it.
    assert_eq!(issues("parameters.change", json!({"delete": 5, "name": "W", "expression": 1})), ["`delete` must be text"]);
    assert_eq!(issues("parameters.change", json!({"delete": null, "name": "W", "expression": 1})), none);
    // The single change form.
    for p in [
        json!({"name": "W", "expression": 5}),
        json!({"name": " W ", "value": "2 mm", "unit": "mm", "comment": "c", "favorite": true}),
        json!({"name": "W", "new_name": "X"}),
        json!({"name": "W", "favorite": false}),
        json!({"name": "W", "comment": "x"}),
        json!({"name": "W", "expression": null, "value": 3}),
        json!({"name": "W", "favorite": null}),
    ] {
        assert_eq!(issues("parameters.change", p.clone()), none, "{p}");
    }
    for (p, message) in [
        (json!({"name": 5, "expression": 1}), "`name` is required"),
        (json!({"name": null, "expression": 1}), "`name` is required"),
        (json!({"name": "W", "expression": true}), "`expression` must be a number or text"),
        (json!({"name": "W", "new_name": 5}), "`new_name` must be text"),
        (json!({"name": "W", "expression": 1, "unit": 5}), "`unit` must be text"),
        (json!({"name": "W", "expression": 1, "comment": 5}), "`comment` must be text"),
        (json!({"name": "W", "expression": 1, "favorite": "yes"}), "`favorite` must be true or false"),
        (json!({"changes": null}), "`changes` must be an object of name: expression"),
        (json!({"changes": ["W"]}), "`changes` must be an object of name: expression"),
        (json!({"changes": {}}), "`changes` is empty"),
        (json!({"changes": {"W": 1, "H": null}}), "`changes.H` must be a number or text"),
    ] {
        assert_eq!(issues("parameters.change", p.clone()), [message], "{p}");
    }
    // `value` is read only when `expression` isn't a number or text.
    assert_eq!(issues("parameters.change", json!({"name": "W", "expression": "5", "value": true})), none);
    assert_eq!(issues("parameters.change", json!({"name": "W", "expression": null, "value": true})), ["`expression` must be a number or text"]);
    // A missing key is the handler's to refuse, with its own message.
    for p in [json!({}), json!({"name": "W"}), json!({"expression": 1})] {
        assert_eq!(issues("parameters.change", p.clone()), Vec::<String>::new(), "{p}");
    }
}

/// `parameters.list` doesn't read `filter` when `favorites` is true; `parameters.import` doesn't
/// read `path` when `text` is text; `expr.evaluate` doesn't read `kind` when `unit` is `text`.
#[test]
fn list_import_and_evaluate_read_keys_only_where_the_handlers_do() {
    let none = Vec::<String>::new();
    assert_eq!(issues("parameters.list", json!({"favorites": true, "filter": 5})), none);
    assert_eq!(issues("parameters.list", json!({"favorites": false, "filter": 5})), ["`filter` must be text"]);
    assert_eq!(issues("parameters.list", json!({"filter": ["user"]})), ["`filter` must be text"]);
    assert_eq!(issues("parameters.list", json!({"favorites": "yes"})), ["`favorites` must be true or false"]);
    assert_eq!(issues("parameters.list", json!({"filter": null, "favorites": null})), none);
    assert_eq!(issues("parameters.list", json!({"filter": "bogus"})), none);

    assert_eq!(issues("parameters.import", json!({"text": "name,expression\nW,1", "path": 5})), none);
    assert_eq!(issues("parameters.import", json!({"text": "", "path": []})), none);
    assert_eq!(issues("parameters.import", json!({"path": "p.csv"})), none);
    assert_eq!(issues("parameters.import", json!({"text": null, "path": "p.csv"})), none);
    assert_eq!(issues("parameters.import", json!({"path": 5})), ["give `text` or `path`"]);
    assert_eq!(issues("parameters.import", json!({"text": 5})), ["give `text` or `path`"]);
    assert_eq!(issues("parameters.import", json!({"text": 5, "path": "p.csv"})), ["`text` must be text"]);

    assert_eq!(issues("expr.evaluate", json!({"expression": "a b", "unit": "text", "kind": 5})), none);
    assert_eq!(issues("expr.evaluate", json!({"expression": "a b", "unit": " TEXT ", "kind": "bogus"})), none);
    assert_eq!(issues("expr.evaluate", json!({"expression": 5, "unit": "mm", "kind": 5})), ["`kind` must be text"]);
    assert_eq!(issues("expr.evaluate", json!({"expression": 5, "unit": 5})), ["`unit` must be text"]);
    assert_eq!(issues("expr.evaluate", json!({"expression": 5, "kind": "angle", "unit": null})), none);
    assert_eq!(issues("expr.evaluate", json!({"expression": 2.5})), none);
    assert_eq!(issues("expr.evaluate", json!({"expression": {}})), ["`expression` must be text or a number"]);
    assert_eq!(issues("parameters.complete", json!({"prefix": 5})), ["`prefix` must be text"]);
    assert_eq!(issues("parameters.complete", json!({"prefix": null})), none);
    assert_eq!(issues("parameters.export", json!({"format": "JSON", "path": "x.csv"})), none);
    assert_eq!(issues("parameters.users", json!({"name": " "})), ["`name` is required"]);
    assert_eq!(issues("parameters.graph", json!({"nodes": 1})), ["unknown key `nodes`"]);
}

/// The schemas say which forms and keys the handlers need: `parameters.change`'s three forms,
/// `parameters.import`'s `text` or `path`, `expr.evaluate`'s `expression`.
#[test]
fn the_new_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<ParamChangeParams>(),
        &[
            (json!({"name": "W", "expression": "10 mm"}), true),
            (json!({"name": "W", "new_name": "X"}), true),
            (json!({"name": "W", "favorite": true}), true),
            (json!({"changes": {"W": 1}}), true),
            (json!({"delete": "W"}), true),
            (json!({}), false),
            (json!({"name": "W"}), false),
            (json!({"expression": 1}), false),
            (json!({"delete": 5}), false),
            (json!({"name": "W", "expression": true}), false),
        ],
    );
    takes(
        input_schema_for::<ParamImportParams>(),
        &[(json!({"text": "x"}), true), (json!({"path": "p.csv"}), true), (json!({}), false), (json!({"text": 5}), false)],
    );
    takes(
        input_schema_for::<ExprEvaluateParams>(),
        &[
            (json!({"expression": "1 + 2"}), true),
            (json!({"expression": 3, "unit": "mm", "kind": "length"}), true),
            (json!({}), false),
            (json!({"expression": true}), false),
        ],
    );
    takes(input_schema_for::<ParamUsersParams>(), &[(json!({"name": "W"}), true), (json!({}), false), (json!({"name": ""}), false)]);
    takes(
        input_schema_for::<ParamListParams>(),
        &[(json!({}), true), (json!({"favorites": true, "filter": "user"}), true), (json!({"favorites": "x"}), false)],
    );
    takes(
        input_schema_for::<ParamExportParams>(),
        &[(json!({}), true), (json!({"format": "json", "path": "p.json"}), true), (json!({"path": 1}), false)],
    );
    takes(input_schema_for::<ExprCompleteParams>(), &[(json!({}), true), (json!({"prefix": "si"}), true), (json!({"prefix": 1}), false)]);
    // `conforms` doesn't read nested `anyOf` inside a branch's `properties`; the forms are pinned.
    let change = input_schema_for::<ParamChangeParams>();
    assert_eq!(
        change.as_value()["anyOf"],
        json!([
            {"required": ["changes"]},
            {"required": ["delete"], "properties": {"delete": {"type": "string"}}},
            {
                "required": ["name"],
                "properties": {"name": {"type": "string"}},
                "anyOf": [
                    {"required": ["expression"], "properties": {"expression": {"type": ["number", "string"]}}},
                    {"required": ["value"], "properties": {"value": {"type": ["number", "string"]}}},
                    {"required": ["new_name"]},
                    {"required": ["favorite"]},
                    {"required": ["comment"]}
                ]
            }
        ])
    );
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// #12497: every command of the group, run in every form in Warn, replies as its output schema
/// says and without warnings (Warn runs a call the contract refuses anyway, so the absence of
/// `param_warnings` is what shows the contract accepts the call).
#[test]
fn the_parameter_commands_reply_with_their_schemas() {
    let dir = std::env::temp_dir().join(format!("solidcraft-params-schema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("parameters.add", json!({"name": "W", "expression": "10 mm", "favorite": true}));
    run("parameters.add", json!({"name": "H", "expression": 5, "unit": "mm"}));
    run("parameters.add", json!({"name": "Label", "expression": "Rev A", "unit": "text"}));
    run("solid.box", json!({"length": "W", "width": "H * 2", "height": 4, "body_name": "Block"}));

    // Change: one parameter in each of its forms, several at once, a delete.
    let r = run("parameters.change", json!({"name": "W", "expression": "12 mm", "comment": "wall"}));
    assert!(r["value"] == 12.0 && r["expression"] == "12 mm" && r.get("renamed").is_none(), "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    let r = run("parameters.change", json!({"name": "W", "value": 14, "unit": "mm", "favorite": false}));
    assert_eq!(r["value"], 14.0, "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    let r = run("parameters.change", json!({"name": "W", "new_name": "Wall"}));
    assert!(r["renamed"]["from"] == "W" && r["renamed"]["to"] == "Wall" && r["expression"].is_null(), "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    let r = run("parameters.change", json!({"name": "Wall", "comment": "thin", "favorite": true}));
    assert!(r["expression"].is_null() && r["name"] == "Wall", "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    let r = run("parameters.change", json!({"name": "Wall", "expression": "Wide = 16"}));
    assert!(r["renamed"]["to"] == "Wide" && r["name"] == "Wide", "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    let r = run("parameters.change", json!({"name": "Label", "expression": "Rev B"}));
    assert_eq!(r["text"], "Rev B", "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    let r = run("parameters.change", json!({"changes": {"Wide": 20, "H": "6 mm"}}));
    assert!(r["changed"]["Wide"] == 20.0 && r["changed"]["H"] == 6.0, "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    // The other forms' keys are not read, so a bad one doesn't warn.
    let r = run("parameters.change", json!({"changes": {"H": 7}, "name": 5, "expression": true}));
    assert_eq!(r["changed"]["H"], 7.0, "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);
    run("parameters.add", json!({"name": "Spare", "expression": 1}));
    let r = run("parameters.change", json!({"delete": " Spare ", "name": 5}));
    assert_eq!(r["deleted"], "Spare", "{r}");
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &r);

    // List: all forms of the filter, favourites first.
    let all = run("parameters.list", json!({}));
    assert_eq!(all["count"], all["parameters"].as_array().map_or(0, Vec::len), "{all}");
    matches(output_schema_for::<ParamListResult>(), "parameters.list", &all);
    for p in [
        json!({"favorites": true}),
        json!({"favorites": true, "filter": 5}),
        json!({"filter": "USER"}),
        json!({"filter": "feature"}),
        json!({"filter": "sketch"}),
        json!({"filter": "model"}),
        json!({"filter": "bogus"}),
    ] {
        let r = run("parameters.list", p);
        matches(output_schema_for::<ParamListResult>(), "parameters.list", &r);
    }
    let favorites = run("parameters.list", json!({"favorites": true}));
    assert!(favorites["parameters"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|r| r["favorite"] == true)), "{favorites}");
    let feature = run("parameters.list", json!({"filter": "feature"}));
    assert!(feature["parameters"].as_array().is_some_and(|a| a.iter().all(|r| r["source"] == "feature")), "{feature}");

    // Users and graph.
    let users = run("parameters.users", json!({"name": " H "}));
    assert_eq!(users["name"], "H", "{users}");
    assert!(users["users"].as_array().is_some_and(|u| !u.is_empty()) && users["drives"][0]["name"] == "Box1", "{users}");
    matches(output_schema_for::<ParamUsersResult>(), "parameters.users", &users);
    let graph = run("parameters.graph", json!({}));
    assert!(graph["nodes"].as_array().is_some_and(|n| n.contains(&json!("H"))), "{graph}");
    matches(output_schema_for::<ParamGraphResult>(), "parameters.graph", &graph);

    // Export in both formats, to text and to a file; import them back.
    let csv = run("parameters.export", json!({}));
    assert!(csv["format"] == "csv" && csv["path"].is_null(), "{csv}");
    matches(output_schema_for::<ParamExportResult>(), "parameters.export", &csv);
    let json_text = run("parameters.export", json!({"format": "JSON"}));
    assert_eq!(json_text["format"], "json", "{json_text}");
    matches(output_schema_for::<ParamExportResult>(), "parameters.export", &json_text);
    let file = dir.join("parameters.json");
    let written = run("parameters.export", json!({"path": file}));
    assert!(written["format"] == "json" && written["path"].is_string(), "{written}");
    matches(output_schema_for::<ParamExportResult>(), "parameters.export", &written);
    let mut other = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let imported = other.execute("parameters.import", &json!({"path": file})).unwrap_or_else(|e| panic!("{e}"));
    assert!(imported["imported"].as_array().is_some_and(|n| n.len() == 3), "{imported}");
    matches(output_schema_for::<ParamImportResult>(), "parameters.import", &imported);
    let from_text = other.execute("parameters.import", &json!({"text": "name,expression\nZ,3", "path": 5})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(from_text["imported"], json!(["Z"]), "{from_text}");
    matches(output_schema_for::<ParamImportResult>(), "parameters.import", &from_text);

    // Evaluate: a length, an angle, a unitless number, a definition, an error, a text.
    for p in [
        json!({"expression": "H * 2"}),
        json!({"expression": 3, "unit": "in"}),
        json!({"expression": "90 deg"}),
        json!({"expression": "30", "unit": "deg"}),
        json!({"expression": "2 * 3", "kind": "unitless"}),
        json!({"expression": "Wide", "kind": "length"}),
        json!({"expression": "len = 3 mm"}),
        json!({"expression": "nope + 1"}),
        json!({"expression": "Rev \"A\"", "unit": "text", "kind": 5}),
    ] {
        let r = run("expr.evaluate", p.clone());
        matches(output_schema_for::<ExprEvaluated>(), "expr.evaluate", &r);
        assert_eq!(r["ok"], !p["expression"].as_str().is_some_and(|e| e.starts_with("nope")), "{r}");
    }
    let r = run("expr.evaluate", json!({"expression": "len = 3 mm"}));
    assert!(r["defines"] == "len" && r["kind"] == "length" && r["value"] == 3.0, "{r}");
    let r = run("expr.evaluate", json!({"expression": "'Rev A'", "unit": "text"}));
    assert!(r["kind"] == "text" && r["text"] == "Rev A" && r["references"] == json!([]), "{r}");
    let r = run("expr.evaluate", json!({"expression": "nope + 1"}));
    assert!(r["ok"] == false && r["error"].is_string() && r["references"] == json!(["nope"]), "{r}");

    // Complete: parameters, functions, constants, units.
    for p in
        [json!({}), json!({"prefix": "H"}), json!({"prefix": "2 * si"}), json!({"prefix": "p"}), json!({"prefix": "3 m"}), json!({"prefix": null})]
    {
        let r = run("parameters.complete", p);
        matches(output_schema_for::<ExprCompletions>(), "parameters.complete", &r);
    }
    let r = run("parameters.complete", json!({"prefix": "3 m"}));
    assert!(r["suggestions"].as_array().is_some_and(|a| a.iter().any(|c| c["kind"] == "unit" && c["detail"] == "length")), "{r}");
    let r = run("parameters.complete", json!({"prefix": "p"}));
    assert!(r["suggestions"].as_array().is_some_and(|a| a.iter().any(|c| c["kind"] == "constant")), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A misspelt key runs in Warn and is reported, with a reply that still matches; strict refuses
/// it. Each command of the group.
#[test]
fn a_misspelt_key_is_reported_for_each_command() {
    let calls = [
        ("parameters.change", json!({"name": "W", "comment": "c", "favorit": true}), "favorit"),
        ("parameters.list", json!({"favourites": true}), "favourites"),
        ("parameters.users", json!({"name": "W", "nam": 1}), "nam"),
        ("parameters.graph", json!({"node": 1}), "node"),
        ("parameters.export", json!({"formt": "csv"}), "formt"),
        ("parameters.import", json!({"text": "name,expression\nQ,1", "pth": "x"}), "pth"),
        ("expr.evaluate", json!({"expression": "1", "unt": "mm"}), "unt"),
        ("parameters.complete", json!({"prefx": "W"}), "prefx"),
    ];
    for (id, p, key) in calls {
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        s.execute("parameters.add", &json!({"name": "W", "expression": "10 mm"})).unwrap_or_else(|e| panic!("{e}"));
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r["param_warnings"].to_string().contains(key), "{id}: {r}");
        let schema = crate::cmd::find_command(id).and_then(|c| c.output).map(|f| f()).unwrap_or_else(|| panic!("{id} has no output schema"));
        assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r} doesn't match {}", schema.as_value());
        s.param_check = ParamCheck::Strict;
        let error = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id}: {error}");
    }
}

/// A configuration rule's switch has the shape the schema describes.
#[test]
fn a_switched_feature_serializes_as_the_schema_says() {
    let body = ParamChangedBody::Many {
        changed: [("W".to_string(), Some(1.0))].into(),
        recomputed: 2,
        restored: 0,
        errors: Vec::new(),
        switched: vec![SwitchedFeature { feature: 3, name: Some("Hole".into()), suppressed: true }],
    };
    let reply = serde_json::to_value(ParamChanged { body, param_warnings: Vec::new() }).unwrap_or_default();
    assert_eq!(
        reply,
        json!({"changed": {"W": 1.0}, "recomputed": 2, "restored": 0, "errors": [], "switched": [{"feature": 3, "name": "Hole", "suppressed": true}]})
    );
    matches(output_schema_for::<ParamChanged>(), "parameters.change", &reply);
}
