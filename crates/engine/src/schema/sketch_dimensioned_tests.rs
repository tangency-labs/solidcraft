use serde_json::{Value, json};

use super::{CREATORS, DrawDimensionedParams, DrawDimensionedResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

const ID: &str = "sketch.draw.dimensioned";

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn scene(level: ParamCheck) -> Session {
    let mut s = Session { param_check: ParamCheck::Off, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s.param_check = level;
    s
}

fn matches(reply: &Value) {
    let schema = output_schema_for::<DrawDimensionedResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{reply}");
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DrawDimensionedParams>(), &["as_drawn", "command", "dimensions", "locate", "params"]);
}

/// The creators in the schema are exactly the ones `dimension_roles` knows.
#[test]
fn the_creator_list_is_the_one_dimension_roles_has() {
    for c in crate::cmd::command_specs() {
        assert_eq!(crate::dimension_roles(c.id).is_some(), CREATORS.contains(&c.id), "{}", c.id);
    }
    for creator in CREATORS {
        assert!(crate::cmd::find_command(creator).is_some(), "{creator}");
        assert!(crate::dimension_roles(creator).is_some(), "{creator}");
    }
}

#[test]
fn strict_keeps_todays_errors() {
    let line = json!({"points": [[0, 0], [10, 0]]});
    assert_strict_keeps_todays_errors(&[
        (ID, json!({})),
        (ID, json!({"command": null, "params": line, "dimensions": {"length": 5}})),
        (ID, json!({"command": 5, "params": line, "dimensions": {"length": 5}})),
        (ID, json!({"command": "sketch.point", "params": line, "dimensions": {"length": 5}})),
        (ID, json!({"command": "", "params": line, "dimensions": {"length": 5}})),
        (ID, json!({"command": "sketch.line", "dimensions": {"length": 5}})),
        (ID, json!({"command": "sketch.line", "params": null, "dimensions": {"length": 5}})),
        (ID, json!({"command": "sketch.line", "params": [1], "dimensions": {"length": 5}})),
        (ID, json!({"command": "sketch.line", "params": line})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": null})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": null, "locate": true})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": [1]})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": {"length": null}})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": {"length": true}})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": {"length": ""}})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": {"length": "x".repeat(4097)}})),
        (ID, json!({"command": "sketch.line", "params": line, "dimensions": {"length": [5]}})),
    ]);
}

#[test]
fn the_check_names_what_is_wrong_with_a_key() {
    let said = |p: Value| issues(ID, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let line = json!({"points": [[0, 0], [10, 0]]});
    assert_eq!(said(json!({"command": "sketch.point", "params": line})), ["unsupported sketch creator"]);
    assert_eq!(said(json!({"command": 3, "params": line})), ["`command` must name a supported sketch creator"]);
    assert_eq!(said(json!({"command": "sketch.line", "params": 1})), ["`params` must be an object"]);
    assert_eq!(said(json!({"command": "sketch.line", "params": line, "dimensions": 1})), ["`dimensions` must be an object"]);
    assert_eq!(
        said(json!({"command": "sketch.line", "params": line, "dimensions": {"length": []}})),
        ["dimension values must be bounded expressions or finite numbers"]
    );
    assert_eq!(said(json!({"command": "sketch.line", "params": line, "dimensions": {"length": 5}, "lenght": 1})).len(), 1);
    // Left out, the handler's message is what a caller gets, not the check's.
    assert!(said(json!({})).is_empty());
}

/// The types read each key as the handler does, and the schema takes what the handler takes.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<DrawDimensionedParams>();
    let schema = schema.as_value();
    let line = json!({"points": [[0, 0], [10, 0]]});
    for (p, takes) in [
        (json!({"command": "sketch.line", "params": line, "dimensions": {"length": 5, "angle": "30 deg"}}), true),
        (json!({"command": "sketch.circle.center", "params": {"center": [0, 0], "radius": 2}, "dimensions": {"diameter": "width = 20"}}), true),
        (json!({"command": "sketch.line", "params": line, "as_drawn": true}), true),
        (json!({"command": "sketch.line", "params": line, "locate": true}), true),
        (json!({"command": "sketch.line", "params": line, "as_drawn": false, "locate": true}), true),
        (json!({"command": "sketch.line", "params": line}), false),
        (json!({"command": "sketch.line", "params": line, "as_drawn": false}), false),
        (json!({"params": line, "as_drawn": true}), false),
        (json!({"command": "sketch.line", "as_drawn": true}), false),
        (json!({"command": "sketch.point", "params": line, "as_drawn": true}), false),
        (json!({"command": "sketch.line", "params": line, "dimensions": {"length": []}}), false),
    ] {
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
        // The type agrees with the schema about the keys it reads.
        if takes {
            assert!(issues(ID, &p).is_empty(), "{p}");
        }
    }
}

/// Every creator, in every form the tests use (typed, as drawn, located), in Warn: the reply
/// matches the output schema and carries no warning, so the contract accepts what works today.
#[test]
fn replies_match_the_output_schema() {
    let calls = [
        ("sketch.line", json!({"points": [[0, 0], [10, 3]]}), json!({"length": 12, "angle": "20"})),
        ("sketch.line.midpoint", json!({"mid": [3, 4], "end": [9, 12]}), json!({"length": 12})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 5]}), json!({"width": 20, "height": "h = 8"})),
        ("sketch.rectangle.center", json!({"center": [0, 0], "corner": [5, 5]}), json!({"width": 20})),
        ("sketch.rectangle.rounded", json!({"p0": [0, 0], "p1": [20, 10], "radius": 2}), json!({"width": 30})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 3}), json!({"diameter": 20})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 3}), json!({"radius": "r = 4"})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 6], "minor_radius": 4}), json!({"major_radius": 12})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 5, "sides": 6}), json!({"radius": 8})),
        ("sketch.polygon.edge", json!({"p0": [0, 0], "p1": [5, 0], "sides": 6}), json!({"length": 7})),
    ];
    for (command, params, dimensions) in calls {
        for extra in [json!({}), json!({"as_drawn": true}), json!({"as_drawn": true, "locate": true}), json!({"locate": true})] {
            let mut s = scene(ParamCheck::Warn);
            let mut p = json!({"command": command, "params": params, "dimensions": dimensions});
            p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
            let reply = s.execute(ID, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
            matches(&reply);
            assert!(reply["dimensions"].is_object(), "{reply}");
            assert_eq!(reply.get("located").is_some(), p.get("locate") == Some(&json!(true)), "{reply}");
        }
    }
    // Typed values alone (no flags), a definition and `locate`: `defined` and `located` appear.
    let mut s = scene(ParamCheck::Warn);
    let reply = run(
        &mut s,
        ID,
        json!({"command": "sketch.circle.center", "params": {"center": [3, 4], "radius": 3}, "dimensions": {"diameter": "w = 20"}, "locate": true}),
    );
    matches(&reply);
    assert_eq!(reply["defined"], json!(["w"]), "{reply}");
    assert!(reply["located"].as_array().is_some_and(|l| !l.is_empty()), "{reply}");
}

/// A drawing for each creator, as the handler's own tests use.
fn sample(creator: &str) -> Value {
    match creator {
        "sketch.line" => json!({"points":[[0,0],[6,8]]}),
        "sketch.line.midpoint" => json!({"mid":[3,4],"end":[9,12]}),
        "sketch.rectangle.two_point" => json!({"p0":[0,0],"p1":[30,20]}),
        "sketch.rectangle.center" => json!({"center":[0,0],"corner":[15,10]}),
        "sketch.rectangle.three_point" => json!({"p0":[0,0],"p1":[30,10],"p2":[25,25]}),
        "sketch.rectangle.rounded" => json!({"p0":[0,0],"p1":[30,20],"radius":3}),
        "sketch.circle.center" => json!({"center":[0,0],"radius":5}),
        "sketch.circle.two_point" => json!({"p0":[0,0],"p1":[10,0]}),
        "sketch.circle.three_point" => json!({"p0":[0,0],"p1":[10,0],"p2":[5,5]}),
        "sketch.arc.center_point" => json!({"center":[0,0],"start":[10,0],"end":[0,10]}),
        "sketch.ellipse" => json!({"center":[0,0],"major":[10,6],"minor_radius":4}),
        "sketch.slot.center_to_center" | "sketch.slot.overall" => json!({"p0":[2,3],"p1":[22,3],"width":4}),
        "sketch.slot.center_point" => json!({"center":[12,3],"end":[22,3],"width":4}),
        "sketch.slot.arc_center" => json!({"center":[0,0],"start":[20,0],"end":[0,20],"width":6}),
        "sketch.slot.arc_three_point" => json!({"start":[20,0],"through":[14.142136,14.142136],"end":[0,20],"width":6}),
        "sketch.polygon.inscribed" | "sketch.polygon.circumscribed" => json!({"center":[4,5],"radius":10,"sides":6}),
        "sketch.polygon.edge" => json!({"p0":[0,0],"p1":[10,0],"sides":6}),
        other => panic!("{other}: add a sample drawing for it"),
    }
}

/// Every creator, as drawn and located, in Warn: a reply that matches, with no warning.
#[test]
fn every_creator_replies_as_drawn_and_located() {
    for creator in CREATORS {
        for flags in [json!({"as_drawn": true}), json!({"locate": true}), json!({"as_drawn": true, "locate": true})] {
            let mut s = scene(ParamCheck::Warn);
            let mut p = json!({"command": creator, "params": sample(creator)});
            p.as_object_mut().into_iter().for_each(|o| o.extend(flags.as_object().cloned().unwrap_or_default()));
            let reply = s.execute(ID, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
            matches(&reply);
            assert!(reply["dimensions"].is_object() && reply["curves"].is_array(), "{reply}");
        }
    }
}

#[test]
fn every_creator_passes_the_check() {
    for creator in CREATORS {
        // The creator refuses empty parameters in its own words; the check says nothing.
        assert!(issues(ID, &json!({"command": creator, "params": {}, "as_drawn": true})).is_empty(), "{creator}");
    }
}

/// A misspelt key still runs in Warn and says so, with a reply that matches; strict refuses it.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let p = json!({"command": "sketch.circle.center", "params": {"center": [0, 0], "radius": 3}, "dimensions": {"diameter": 10}, "as_draw": true});
    let mut s = scene(ParamCheck::Warn);
    let reply = run(&mut s, ID, p.clone());
    assert!(reply["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{reply}");
    let schema = output_schema_for::<DrawDimensionedResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert!(scene(ParamCheck::Strict).execute(ID, &p).is_err());
}

/// The reply is what the handler returned before: the creator's keys, `dimensions`, `defined`,
/// `located`, and the creator's own `param_warnings` kept.
#[test]
fn the_reply_keeps_the_creators_keys() {
    let creator = json!({"curves": ["c1"], "sketch": {"solved": true, "dof": 1}, "param_warnings": ["a"], "dimensions": "old", "defined": ["x"]});
    let none = DrawDimensionedResult::new(creator.clone(), serde_json::Map::new(), Vec::new(), None);
    let json = serde_json::to_value(&none).unwrap_or_default();
    assert_eq!(json, json!({"curves": ["c1"], "sketch": {"solved": true, "dof": 1}, "param_warnings": ["a"], "dimensions": {}, "defined": ["x"]}));
    let all = DrawDimensionedResult::new(creator, serde_json::Map::new(), vec!["w".into()], Some(vec![json!(1)]));
    let json = serde_json::to_value(&all).unwrap_or_default();
    assert_eq!(json["defined"], json!(["w"]));
    assert_eq!(json["located"], json!([1]));
}
