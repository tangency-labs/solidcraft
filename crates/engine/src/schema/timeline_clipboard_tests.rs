use serde_json::{Value, json};

use super::{AddParams, AddResult, CopyParams, CopyResult, ExportScriptParams, ExportScriptResult, PasteParams, PasteResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn warn_session() -> Session {
    Session { param_check: ParamCheck::Warn, ..Session::default() }
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A dimensioned sketch named S, extruded into Body, for the commands to work on.
fn plate(s: &mut Session) {
    run(s, "parameters.change", json!({"name": "base", "expression": "5 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": "base * 4"}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "S", "distance": 5, "name": "E"}));
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

fn issues(id: &str, p: Value) -> Vec<String> {
    crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect()
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CopyParams>(), &["features"]);
    assert_keys(input_schema_for::<PasteParams>(), &["component", "component_path", "from_design", "translate"]);
    assert_keys(input_schema_for::<AddParams>(), &["feature", "params"]);
    assert_keys(input_schema_for::<ExportScriptParams>(), &["path"]);
}

/// Strict gives the handlers' own messages for what they already refuse (a missing `features` or
/// `feature` included: the handlers' words, not "is required").
#[test]
fn strict_keeps_todays_errors() {
    let key = |k: Value| json!({"feature": {"type": "box", "length": "1", "width": "1", "height": "1", "corner": [0, 0, 0]}, "params": k});
    assert_strict_keeps_todays_errors(&[
        ("timeline.copy", json!({})),
        ("timeline.copy", json!({"features": null})),
        ("timeline.copy", json!({"features": []})),
        ("timeline.copy", json!({"features": 3})),
        ("timeline.copy", json!({"features": true})),
        ("timeline.copy", json!({"features": [true]})),
        ("timeline.copy", json!({"features": ["S", -1]})),
        ("timeline.copy", json!({"features": ["S", 1.5]})),
        ("timeline.copy", json!({"features": vec!["S"; 1001]})),
        ("timeline.copy", json!({"features": vec!["S"; 10_001]})),
        ("timeline.copy", json!({"features": ["Nope"]})),
        ("timeline.paste", json!({"from_design": null})),
        ("timeline.paste", json!({"from_design": 1})),
        ("timeline.paste", json!({"from_design": "true"})),
        ("timeline.add", json!({})),
        ("timeline.add", json!({"feature": null})),
        ("timeline.add", json!({"feature": [1, 2, 3]})),
        ("timeline.add", json!({"feature": "box"})),
        ("timeline.add", key(json!("q"))),
        ("timeline.add", key(json!({"name": "q"}))),
        ("timeline.add", key(json!(vec![json!({"name": "q", "expression": "1"}); 40_001]))),
        ("timeline.add", key(json!([1]))),
        ("timeline.add", key(json!([{"expression": "1"}]))),
        ("timeline.add", key(json!([{"name": 4, "expression": "1"}]))),
        ("timeline.add", key(json!([{"name": "q"}]))),
        ("timeline.add", key(json!([{"name": "q", "expression": "  "}]))),
        ("timeline.add", key(json!([{"name": "q", "expression": true}]))),
        ("timeline.add", key(json!([{"name": "q", "expression": "1", "unit": 3}]))),
        ("timeline.add", key(json!([{"name": "q", "expression": "1", "comment": []}]))),
        ("timeline.export_script", json!({"path": 7})),
        ("timeline.export_script", json!({"path": true})),
        ("timeline.export_script", json!({"path": ["a"]})),
    ]);
}

/// A paste with something to paste (in this design, or from another one): strict gives the
/// handler's message for each key it refuses. Without anything copied the handler says so before
/// it reads the other keys.
#[test]
fn paste_strict_keeps_todays_errors() {
    let mut source = Session::default();
    plate(&mut source);
    run(&mut source, "timeline.copy", json!({"features": ["S"]}));
    let cases = [
        json!({"translate": [1, 2]}),
        json!({"translate": null}),
        json!({"translate": "1,2,3"}),
        json!({"translate": [1e9, 0, 0]}),
        json!({"component": true}),
        json!({"component": null}),
        json!({"component_path": "0"}),
        json!({"component_path": null}),
        json!({"component_path": [-1]}),
        json!({"component_path": vec![0; 65]}),
    ];
    for design in [false, true] {
        for case in &cases {
            let mut p = case.clone();
            p["from_design"] = json!(design);
            let error = |level: ParamCheck| {
                let mut s = Session { param_check: level, ..Session::default() };
                plate(&mut s);
                s.foreign_copy = source.foreign_copy.clone();
                run(&mut s, "timeline.copy", json!({"features": ["S"]}));
                s.foreign_copy = source.foreign_copy.clone();
                s.execute("timeline.paste", &p).err().map(|e| e.to_string())
            };
            let off = error(ParamCheck::Off);
            assert!(off.is_some_and(|e| !e.contains("nothing copied")), "{p} should fail on the key");
            assert_eq!(error(ParamCheck::Strict), error(ParamCheck::Off), "{p}");
        }
    }
}

/// The types read each key as the handlers do: what the handler's reader takes the type takes,
/// and a refusal has the handler's message.
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let ok = |id: &str, p: Value| assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{p}");
    let refused = |id: &str, p: Value, message: &str| assert_eq!(issues(id, p.clone()), [message], "{p}");

    // timeline.copy: a name, or names and ids in any mix; null is not absent.
    ok("timeline.copy", json!({"features": "S"}));
    ok("timeline.copy", json!({"features": ["S", 3, "E"]}));
    ok("timeline.copy", json!({"features": vec!["S"; 1000]}));
    refused("timeline.copy", json!({"features": []}), "`features` must list 1…1000 features");
    refused("timeline.copy", json!({"features": null}), "`features` must list 1…1000 features");
    refused("timeline.copy", json!({"features": vec!["S"; 1001]}), "`features` must list 1…1000 features");
    refused("timeline.copy", json!({"features": 3}), "`features` must be a list of names or ids; it is 3");
    refused("timeline.copy", json!({"features": ["S", true]}), "`features` must list names or ids; the entry 2 is true");

    // timeline.paste: the same keys with or without from_design.
    for design in [json!({}), json!({"from_design": false}), json!({"from_design": true})] {
        let with = |k: &str, v: Value| {
            let mut p = design.clone();
            p[k] = v;
            p
        };
        ok("timeline.paste", design.clone());
        ok("timeline.paste", with("translate", json!([1, 2.5, -3])));
        ok("timeline.paste", with("translate", json!({"x": 1, "y": 2, "z": 3})));
        ok("timeline.paste", with("component", json!("Plate")));
        ok("timeline.paste", with("component", json!(4)));
        ok("timeline.paste", with("component_path", json!([])));
        ok("timeline.paste", with("component_path", json!([1, 2])));
        refused("timeline.paste", with("translate", json!([1, 2])), "`translate` must be [x, y, z]");
        refused("timeline.paste", with("translate", json!(null)), "`translate` must be [x, y, z]");
        refused("timeline.paste", with("component", json!(null)), "`component` must be a component id or name");
        refused("timeline.paste", with("component", json!([1])), "`component` must be a component id or name");
        refused("timeline.paste", with("component_path", json!(null)), "`component_path` must list 0..64 occurrence ids");
        refused("timeline.paste", with("component_path", json!([1.5])), "`component_path` must list 0..64 occurrence ids");
        refused("timeline.paste", with("component_path", json!(vec![0; 65])), "`component_path` must list 0..64 occurrence ids");
    }
    ok("timeline.paste", json!({"component_path": vec![0; 64]}));
    refused("timeline.paste", json!({"from_design": null}), "`from_design` must be true or false");
    refused("timeline.paste", json!({"from_design": 0}), "`from_design` must be true or false");

    // timeline.add: the feature is an object; params are read by the handler's own reader.
    let feature = json!({"type": "box", "length": "1", "width": "1", "height": "1", "corner": [0, 0, 0]});
    ok("timeline.add", json!({"feature": feature}));
    ok("timeline.add", json!({"feature": feature, "params": null}));
    ok("timeline.add", json!({"feature": feature, "params": []}));
    ok("timeline.add", json!({"feature": feature, "params": [{"name": "q", "expression": 5}]}));
    ok("timeline.add", json!({"feature": feature, "params": [{"name": "q", "expression": "5", "unit": null, "comment": null, "extra": 1}]}));
    ok("timeline.add", json!({"feature": feature, "params": [{"name": " q ", "expression": " 5 ", "unit": "mm", "comment": "c"}]}));
    let message = "`feature` must be a feature object, as a design file or timeline.inspect has it";
    refused("timeline.add", json!({"feature": 1}), message);
    refused("timeline.add", json!({"feature": null}), message);
    refused("timeline.add", json!({"feature": feature, "params": {}}), "`params` must be a list of parameters");
    refused("timeline.add", json!({"feature": feature, "params": [{}]}), "each parameter needs a `name`");
    refused("timeline.add", json!({"feature": feature, "params": [{"name": "q"}]}), "parameter `q` needs an `expression`");
    refused(
        "timeline.add",
        json!({"feature": feature, "params": [{"name": "q", "expression": "1", "unit": 1}]}),
        "parameter `q`: `unit` must be text",
    );
    refused(
        "timeline.add",
        json!({"feature": feature, "params": [{"name": "q", "expression": "1", "comment": 1}]}),
        "parameter `q`: `comment` must be text",
    );
    // The feature's own keys are the design file's to check, not the contract's.
    ok("timeline.add", json!({"feature": {"type": "warp_drive", "anything": [1, 2]}}));

    // timeline.export_script: text, or null for no file.
    ok("timeline.export_script", json!({}));
    ok("timeline.export_script", json!({"path": null}));
    ok("timeline.export_script", json!({"path": "script.json"}));
    refused("timeline.export_script", json!({"path": 1}), "`path` must be a file path");
}

/// The schemas say what the handlers need: the keys a handler refuses without are required, and
/// each value is the form its reader takes.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<CopyParams>(),
        &[
            (json!({"features": "S"}), true),
            (json!({"features": ["S", 2]}), true),
            (json!({}), false),
            (json!({"features": []}), false),
            (json!({"features": 2}), false),
            (json!({"features": [true]}), false),
            (json!({"features": [-1]}), false),
            (json!({"features": vec!["S"; 1001]}), false),
        ],
    );
    takes(
        input_schema_for::<PasteParams>(),
        &[
            (json!({}), true),
            (json!({"translate": [1, 2, 3], "component": "P", "component_path": [1], "from_design": true}), true),
            (json!({"translate": {"x": 1, "y": 2, "z": 3}, "component": 2}), true),
            (json!({"translate": [1, 2]}), false),
            (json!({"translate": [1e9, 0, 0]}), false),
            (json!({"component": true}), false),
            (json!({"component_path": [-1]}), false),
            (json!({"component_path": vec![0; 65]}), false),
            (json!({"from_design": null}), false),
            (json!({"from_design": "yes"}), false),
        ],
    );
    takes(
        input_schema_for::<AddParams>(),
        &[
            (json!({"feature": {"type": "box"}}), true),
            (json!({"feature": {"type": "box"}, "params": [{"name": "q", "expression": 5, "unit": "mm", "comment": null}]}), true),
            (json!({"feature": {"type": "box"}, "params": [{"name": "q", "expression": "5"}]}), true),
            (json!({}), false),
            (json!({"feature": []}), false),
            (json!({"feature": {"type": "box"}, "params": [{"name": "q"}]}), false),
            (json!({"feature": {"type": "box"}, "params": {"name": "q"}}), false),
        ],
    );
    takes(
        input_schema_for::<ExportScriptParams>(),
        &[(json!({}), true), (json!({"path": "a.json"}), true), (json!({"path": 3}), false), (json!({"file": "a.json"}), false)],
    );
}

/// Copy (a name, an id, both), paste (as it is, moved, into a component, from another design),
/// in Warn: each real reply matches its output schema and carries no warning.
#[test]
fn copy_and_paste_replies_match_the_output_schema() {
    let mut s = warn_session();
    plate(&mut s);
    let sketch_id = s.doc.find_feature("S").map(|f| f.id).unwrap_or_default();

    for features in [json!("S"), json!(["S"]), json!([sketch_id]), json!(["S", "E"]), json!(["E", sketch_id])] {
        let copied = run(&mut s, "timeline.copy", json!({"features": features}));
        assert!(copied["copied"].as_array().is_some_and(|c| !c.is_empty()), "{copied}");
        matches(output_schema_for::<CopyResult>(), "timeline.copy", &copied);
    }
    assert_eq!(run(&mut s, "timeline.copy", json!({"features": ["E", "S"]}))["copied"], json!(["S", "E"]), "timeline order");

    let pasted = run(&mut s, "timeline.paste", json!({}));
    assert_eq!(pasted["pasted"].as_array().map(Vec::len), Some(2), "{pasted}");
    assert_eq!(pasted["pasted"][0]["from"], "S", "{pasted}");
    assert_eq!(pasted["pasted"][0]["name"], "S (2)", "{pasted}");
    assert!(pasted["pasted"][0]["id"].is_u64(), "{pasted}");
    assert_eq!(pasted["errors"], json!([]), "{pasted}");
    assert_eq!(pasted["notices"], json!([]), "{pasted}");
    matches(output_schema_for::<PasteResult>(), "timeline.paste", &pasted);

    for p in [json!({"translate": [30, 0, 0]}), json!({"translate": {"x": 0, "y": 30, "z": 0}, "from_design": false})] {
        let moved = run(&mut s, "timeline.paste", p);
        matches(output_schema_for::<PasteResult>(), "timeline.paste", &moved);
    }

    // Into a component, by name and by id, with its placement.
    let made = run(&mut s, "component.create", json!({"name": "Plate", "grounded": false}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let (component, occurrence) = (made["component"].clone(), made["occurrence"].clone());
    let into = run(&mut s, "timeline.paste", json!({"component": "Plate", "component_path": [occurrence.clone()]}));
    matches(output_schema_for::<PasteResult>(), "timeline.paste", &into);
    let by_id = run(&mut s, "timeline.paste", json!({"component": component, "translate": [0, 0, 20]}));
    matches(output_schema_for::<PasteResult>(), "timeline.paste", &by_id);

    // From another design: its copy is in the snapshot, pasted with the same keys.
    let mut source = warn_session();
    plate(&mut source);
    run(&mut source, "timeline.copy", json!({"features": ["S"]}));
    let mut target = warn_session();
    target.foreign_copy = source.foreign_copy.clone();
    let foreign = run(&mut target, "timeline.paste", json!({"from_design": true, "translate": [5, 0, 0], "component_path": []}));
    assert_eq!(foreign["pasted"][0]["from"], "S", "{foreign}");
    assert_eq!(foreign["errors"], json!([]), "{foreign}");
    assert_eq!(foreign["notices"], json!([]), "{foreign}");
    matches(output_schema_for::<PasteResult>(), "timeline.paste", &foreign);

    // A copy of a feature that is not a sketch, by id.
    let mut notices = warn_session();
    run(&mut notices, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    let nothing = run(&mut notices, "timeline.copy", json!({"features": [1]}));
    matches(output_schema_for::<CopyResult>(), "timeline.copy", &nothing);
}

/// Add every step of a design's script in Warn, with its parameters: each `timeline.add` reply
/// matches the output schema without warnings, a feature that does not build says why, and the
/// script is returned and written as before.
#[test]
fn add_and_export_replies_match_the_output_schema() {
    let mut s = warn_session();
    plate(&mut s);
    let exported = run(&mut s, "timeline.export_script", json!({}));
    matches(output_schema_for::<ExportScriptResult>(), "timeline.export_script", &exported);
    assert_eq!(exported["path"], Value::Null, "{exported}");
    assert_eq!(exported["omitted"], json!([]), "{exported}");
    assert_eq!(exported["steps"].as_u64(), exported["script"]["commands"].as_array().map(|c| c.len() as u64), "{exported}");
    let mut about: Vec<_> = exported["script"]["about"].as_object().into_iter().flat_map(|o| o.keys().cloned()).collect();
    about.sort();
    assert_eq!(about, ["design", "format", "replay", "version"], "{exported}");
    assert!(exported["script"]["commands"].as_array().is_some_and(|c| c.iter().all(|c| c["command"].is_string() && c.get("params").is_some())));

    let mut t = warn_session();
    let mut added = 0;
    for step in exported["script"]["commands"].as_array().into_iter().flatten() {
        let reply = run(&mut t, step["command"].as_str().unwrap_or_default(), step["params"].clone());
        if step["command"] == "timeline.add" {
            added += 1;
            assert!(reply["error"].is_null() && reply["warning"].is_null(), "{reply}");
            matches(output_schema_for::<AddResult>(), "timeline.add", &reply);
        }
    }
    assert!(added >= 2, "a sketch and an extrude");
    assert!(
        exported["script"]["commands"].as_array().is_some_and(|c| c.iter().any(|c| c["params"]["params"].is_array())),
        "a step brings parameters"
    );

    // Parameters as a number, with text and null in the other keys; a feature that doesn't build.
    let mut u = warn_session();
    let reply = run(
        &mut u,
        "timeline.add",
        json!({"feature": {"type": "box", "length": "q", "width": "5", "height": "5", "corner": [0, 0, 0]}, "params": [{"name": "q", "expression": 8, "unit": null, "comment": "a size"}]}),
    );
    assert_eq!(reply["name"], "Box1", "{reply}");
    matches(output_schema_for::<AddResult>(), "timeline.add", &reply);
    let broken = run(&mut u, "timeline.add", json!({"feature": {"type": "box", "length": "0", "width": "5", "height": "5", "corner": [0, 0, 0]}}));
    assert!(broken["error"].is_string() || broken["warning"].is_string(), "{broken}");
    matches(output_schema_for::<AddResult>(), "timeline.add", &broken);

    // With a path the script is written, and the reply names the file.
    let dir = std::env::temp_dir().join(format!("solidcraft-timeline-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let file = dir.join("script.json");
    let written = run(&mut s, "timeline.export_script", json!({"path": file}));
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(serde_json::from_str::<Value>(&text).ok(), Some(written["script"].clone()));
    // The file's text is the pretty-printed script with its keys sorted, as it has always been.
    let sorted = serde_json::to_string_pretty(&written["script"]).unwrap_or_default();
    assert_eq!(text, sorted);
    assert_eq!(written["path"], json!(file), "{written}");
    matches(output_schema_for::<ExportScriptResult>(), "timeline.export_script", &written);
}

/// A misspelt key still runs in Warn, and says so, with a reply that matches its schema.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let mut s = warn_session();
    plate(&mut s);
    let sketch = json!({"type": "box", "length": "1", "width": "1", "height": "1", "corner": [0, 0, 0]});
    let cases = [
        ("timeline.copy", json!({"features": ["S"], "featurs": 1}), output_schema_for::<CopyResult>()),
        ("timeline.paste", json!({"translat": [1, 0, 0]}), output_schema_for::<PasteResult>()),
        ("timeline.add", json!({"feature": sketch, "param": []}), output_schema_for::<AddResult>()),
        ("timeline.export_script", json!({"paht": "x.json"}), output_schema_for::<ExportScriptResult>()),
    ];
    for (id, p, schema) in cases {
        if id == "timeline.paste" {
            run(&mut s, "timeline.copy", json!({"features": ["S"]}));
        }
        let reply = run(&mut s, id, p);
        let warnings = reply["param_warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.len() == 1 && warnings[0].as_str().is_some_and(|w| w.contains("unknown key")), "{id}: {reply}");
        assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{id}: {reply}");
    }
    // In strict the call is refused before it runs.
    s.param_check = ParamCheck::Strict;
    let refused = s.execute("timeline.export_script", &json!({"paht": "x.json"}));
    assert!(refused.is_err_and(|e| e.to_string().contains("unknown key `paht`")));
}

/// The expression of a parameter that isn't blank once trimmed: the pattern is the one the
/// shared check builds from `trim`'s whitespace.
#[test]
fn a_blank_expression_is_refused_by_the_pattern() {
    let schema = input_schema_for::<AddParams>();
    let pattern = schema.as_value()["$defs"]["StepParams"]["items"]["properties"]["expression"]["anyOf"][0]["pattern"].clone();
    assert_eq!(pattern, json!(crate::schema::types::not_blank_pattern()));
}
