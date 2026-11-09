use serde_json::{Value, json};

use super::{
    ContactCheck, ContactCreateParams, ContactEditParams, ContactEnableParams, ContactNoParams, ContactSetCreated, ContactSetEdited, ContactSets,
    ContactSwitch, ExplodeCreateParams, ExplodeCreated, ExplodeDeleteParams, ExplodeDeleted, ExplodeListParams, ExplodeShowParams, ExplodeShown,
    ExplodeViews, FindLimitsParams, FindLimitsResult, MotionListParams, MotionStudies, MotionStudyDeleteParams, MotionStudyDeleted,
    MotionStudyParams, MotionStudySaved,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FindLimitsParams>(), &["against", "contact_sets", "index", "joint", "occurrences", "refine", "step"]);
    assert_keys(input_schema_for::<ContactEnableParams>(), &["enabled"]);
    assert_keys(input_schema_for::<ContactNoParams>(), &[]);
    assert_keys(input_schema_for::<ContactCreateParams>(), &["name", "occurrences"]);
    assert_keys(input_schema_for::<ContactEditParams>(), &["delete", "name", "occurrences", "set", "suppressed"]);
    assert_keys(input_schema_for::<MotionStudyParams>(), &["keys", "name", "steps", "study"]);
    assert_keys(input_schema_for::<MotionListParams>(), &[]);
    assert_keys(input_schema_for::<MotionStudyDeleteParams>(), &["study"]);
    assert_keys(input_schema_for::<ExplodeCreateParams>(), &["moves", "name", "scale"]);
    assert_keys(input_schema_for::<ExplodeShowParams>(), &["view"]);
    assert_keys(input_schema_for::<ExplodeListParams>(), &[]);
    assert_keys(input_schema_for::<ExplodeDeleteParams>(), &["view"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let many = vec!["A"; 10_001];
    assert_strict_keeps_todays_errors(&[
        ("joint.find_limits", json!({"joint": true})),
        ("joint.find_limits", json!({"joint": null})),
        ("joint.find_limits", json!({"joint": "Hinge"})),
        ("contact.create", json!({})),
        ("contact.create", json!({"occurrences": null})),
        ("contact.create", json!({"occurrences": "Door"})),
        ("contact.create", json!({"occurrences": many})),
        ("contact.create", json!({"occurrences": [true, "Door"]})),
        ("contact.create", json!({"occurrences": ["Nope", "Door"]})),
        ("contact.create", json!({"occurrences": ["root", "Door"]})),
        ("contact.create", json!({"occurrences": [7]})),
        ("contact.edit", json!({})),
        ("contact.edit", json!({"set": true})),
        ("contact.edit", json!({"set": null})),
        ("contact.edit", json!({"set": "Nope", "delete": true})),
    ]);
}

/// The contact contracts say what their handlers need: `occurrences` and `set` are required in
/// the schemas, though the types leave them to the handlers' messages, and a list names two or
/// more occurrences.
#[test]
fn the_contact_schemas_take_what_the_handlers_read() {
    let create = input_schema_for::<ContactCreateParams>();
    let create = create.as_value();
    for (p, takes) in [
        (json!({"occurrences": ["Door", 7], "name": "Swing"}), true),
        (json!({"occurrences": ["Door"]}), false),
        (json!({"occurrences": [true, "Door"]}), false),
        (json!({"name": "Swing"}), false),
    ] {
        assert_eq!(conforms(create, create, &p), takes, "{p}");
    }
    let edit = input_schema_for::<ContactEditParams>();
    let edit = edit.as_value();
    for (p, takes) in [(json!({"set": 3, "suppressed": true}), true), (json!({"set": null}), false), (json!({"delete": true}), false)] {
        assert_eq!(conforms(edit, edit, &p), takes, "{p}");
    }
}

/// #11949: every contact command replies as its output schema says, in Warn without warnings:
/// sets made, listed, changed, suppressed and deleted, contact switched on and off, and a check
/// that finds two parts passing through each other. A misspelt key gets `param_warnings` in Warn,
/// since the handlers ignore unknown keys, and strict refuses it.
#[test]
fn contact_commands_reply_with_their_schemas() {
    let mut s = door();
    // A part through the door, so a check finds them.
    for (id, p) in [
        ("component.activate", json!({"component": "root"})),
        ("component.create", json!({"name": "Clash", "grounded": false})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [10, -3, 0]})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s.param_check = ParamCheck::Warn;
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    run("contact.list", json!({}), output_schema_for::<ContactSets>());
    let r = run("contact.create", json!({"occurrences": ["Door", "Clash"], "name": "Swing"}), output_schema_for::<ContactSetCreated>());
    assert_eq!(r["name"], "Swing", "{r}");
    let other = run("contact.create", json!({"occurrences": ["Door", "Slide", "Door"]}), output_schema_for::<ContactSetCreated>());
    let r = run("contact.check", json!({}), output_schema_for::<ContactCheck>());
    assert!(r["collisions"].as_array().is_some_and(|c| !c.is_empty()), "{r}");
    run(
        "contact.edit",
        json!({"set": "Swing", "suppressed": true, "occurrences": ["Door", "Clash", "Block"], "name": "Swing 2"}),
        output_schema_for::<ContactSetEdited>(),
    );
    let r = run("contact.list", json!({}), output_schema_for::<ContactSets>());
    assert!(r["sets"][0]["suppressed"] == true && r["sets"][1].get("suppressed").is_none(), "{r}");
    let r = run("contact.edit", json!({"set": other["set"], "delete": true}), output_schema_for::<ContactSetEdited>());
    assert_eq!(r["deleted"], true, "{r}");
    for (id, p) in [("contact.enable_sets", json!({"enabled": false})), ("contact.enable_all", json!({})), ("contact.disable_all", json!({}))] {
        run(id, p, output_schema_for::<ContactSwitch>());
    }
    let p = json!({"enable": false});
    let r = s.execute("contact.enable_sets", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("enable"), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("contact.enable_sets", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`enable`"), "{error}");
}

/// A 100 × 4 × 50 door hinged about Z at the origin, a block in its way and a slider beside it.
fn door() -> Session {
    let mut s = Session::default();
    for (name, corner, size) in [("Door", [0, 0, 0], [100, 4, 50]), ("Block", [60, 30, 0], [20, 20, 50]), ("Slide", [0, -40, 0], [10, 10, 10])] {
        for (id, p) in [
            ("component.activate", json!({"component": "root"})),
            ("component.create", json!({"name": name, "grounded": false})),
            ("solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner})),
        ] {
            s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
    for p in [
        json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]}, "flip": true, "name": "Hinge"}),
        json!({"type": "slider", "a": {"occurrence": 0, "point": [0, -40, 0], "z": [1, 0, 0]}, "b": {"occurrence": "Slide", "point": [0, -40, 0], "z": [1, 0, 0]}, "flip": true, "name": "Rail"}),
    ] {
        s.execute("joint.create", &p).unwrap_or_else(|e| panic!("{e}"));
    }
    s
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FindLimitsResult>();
    let schema = schema.as_value();
    let mut s = door();
    s.param_check = ParamCheck::Warn;
    for p in [json!({"joint": "Hinge"}), json!({"joint": "Rail", "index": 0, "refine": 0.1, "occurrences": ["Slide", "Door"]})] {
        let r = s.execute("joint.find_limits", &p).unwrap_or_else(|e| panic!("{e}"));
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    // A misspelt key still runs in warn, and says so.
    let r = s.execute("joint.find_limits", &json!({"joint": "Hinge", "refin": 0.1})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()) && conforms(schema, schema, &r), "{r}");
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
}

/// A key driving the door's hinge from 0 to `to` degrees over the steps.
fn hinge_key(to: Value) -> Value {
    json!({"joint": "Hinge", "points": [[0, 0], [100, to]]})
}

/// #12155: the motion study commands refuse, in strict, what they refuse today, with the same
/// message. They need the door's joints, so the shared helper doesn't fit.
#[test]
fn strict_keeps_the_motion_studys_errors() {
    let many_keys = vec![hinge_key(json!(10)); 1001];
    let many_points: Vec<Value> = (0..10_001).map(|i| json!([i, 0])).collect();
    for (id, p) in [
        ("motion.study", json!({"study": true})),
        ("motion.study", json!({"study": null})),
        ("motion.study", json!({"study": "Nope"})),
        ("motion.study", json!({"steps": 0})),
        ("motion.study", json!({"steps": "100"})),
        ("motion.study", json!({"steps": null})),
        ("motion.study", json!({"keys": {"joint": "Hinge"}})),
        ("motion.study", json!({"keys": null})),
        ("motion.study", json!({"keys": many_keys})),
        ("motion.study", json!({"keys": [{"points": [[0, 0]]}]})),
        ("motion.study", json!({"keys": [{"joint": "Nope", "points": [[0, 0]]}]})),
        ("motion.study", json!({"keys": [{"joint": "Hinge", "index": 3, "points": [[0, 0]]}]})),
        ("motion.study", json!({"keys": [{"joint": "Hinge"}]})),
        ("motion.study", json!({"keys": [{"joint": "Hinge", "points": []}]})),
        ("motion.study", json!({"keys": [{"joint": "Hinge", "points": many_points}]})),
        ("motion.study", json!({"keys": [{"joint": "Hinge", "points": [[0]]}]})),
        ("motion.study", json!({"keys": [{"joint": "Hinge", "points": [["a", 0]]}]})),
        ("motion.study", json!({"keys": [hinge_key(json!(true))]})),
        ("motion.study", json!({"keys": [hinge_key(json!("nope + 1"))]})),
        ("motion.delete", json!({})),
        ("motion.delete", json!({"study": [1]})),
        ("motion.delete", json!({"study": "Nope"})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = door();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #12155: a key's points, in `motion.study`'s schema, take what the handler reads: a step that
/// is a number (not text), and a value that is a number or an expression, each smaller than 1e9
/// in size; elements after those two are ignored. Each case is checked against the contract's
/// own key check too, so the schema and the handler can't drift apart.
#[test]
fn a_studys_points_schema_takes_what_the_handler_reads() {
    let study = input_schema_for::<MotionStudyParams>();
    let study = study.as_value();
    for (point, takes) in [
        (json!([0, 20]), true),
        (json!([12.5, "10 mm"]), true),
        (json!([0, 20, "ignored"]), true),
        (json!(["0", 20]), false),
        (json!([null, 20]), false),
        (json!([2e9, 20]), false),
        (json!([0, 2e9]), false),
        (json!([0, true]), false),
        (json!([0]), false),
    ] {
        let key = json!({"joint": "Hinge", "points": [point]});
        assert_eq!(conforms(study, study, &json!({"keys": [key]})), takes, "schema: {point}");
        assert_eq!(super::study_key_problem(&key).is_none(), takes, "key check: {point}");
    }
}

/// #12155: making, changing, listing and deleting motion studies reply as the output schemas
/// say, in Warn without warnings; a listed study's keys go back into `motion.study` unchanged.
/// A misspelt key gets `param_warnings` in Warn, since the handlers ignore unknown keys, and
/// strict refuses it.
#[test]
fn motion_studies_reply_with_their_schemas() {
    let mut s = door();
    s.param_check = ParamCheck::Warn;
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run("motion.study", json!({"name": "Swing", "steps": 50, "keys": [hinge_key(json!(30))]}), output_schema_for::<MotionStudySaved>());
    assert_eq!((r["name"].as_str(), r["steps"].as_u64(), r["keys"].as_u64()), (Some("Swing"), Some(50), Some(1)), "{r}");
    let slide = json!({"joint": "Rail", "index": 0, "points": [[0, 0], [50, "10 mm"]]});
    run("motion.study", json!({"study": "Swing", "keys": [hinge_key(json!(20)), slide]}), output_schema_for::<MotionStudySaved>());
    run("motion.study", json!({}), output_schema_for::<MotionStudySaved>());
    let list = run("motion.list", json!({}), output_schema_for::<MotionStudies>());
    let swing = &list["studies"][0];
    assert_eq!((swing["keys"][0]["unit"].as_str(), swing["keys"][1]["unit"].as_str()), (Some("deg"), Some("mm")), "{list}");
    assert_eq!(swing["keys"][0]["points"], json!([[0.0, 0.0], [100.0, 20.0]]), "degrees, as given");
    // The listed keys go back in as they are.
    run("motion.study", json!({"study": swing["id"], "keys": swing["keys"]}), output_schema_for::<MotionStudySaved>());
    let r = run("motion.delete", json!({"study": "Swing"}), output_schema_for::<MotionStudyDeleted>());
    assert_eq!(r["deleted"], "Swing", "{r}");
    let p = json!({"name": "Again", "step": 10});
    let r = s.execute("motion.study", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("step"), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("motion.study", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`step`"), "{error}");
}

/// The exploded view commands in a design with three occurrences, in Off and in Strict: each call
/// the handler refuses is refused with the same message. They need the door's occurrences, so
/// the shared helper doesn't fit.
#[test]
fn strict_keeps_the_exploded_views_errors() {
    let many: Vec<Value> = (0..10_001).map(|_| move_of(json!("Door"), json!([1, 2, 3]))).collect();
    for (id, p) in [
        ("explode.create", json!({"moves": null})),
        ("explode.create", json!({"moves": "Door"})),
        ("explode.create", json!({"moves": {"occurrence": "Door", "translate": [1, 2, 3]}})),
        ("explode.create", json!({"moves": many})),
        ("explode.create", json!({"moves": [{}]})),
        ("explode.create", json!({"moves": [true]})),
        ("explode.create", json!({"moves": [move_of(json!(null), json!([1, 2, 3]))]})),
        ("explode.create", json!({"moves": [move_of(json!(true), json!([1, 2, 3]))]})),
        ("explode.create", json!({"moves": [move_of(json!(["Door"]), json!([1, 2, 3]))]})),
        ("explode.create", json!({"moves": [move_of(json!("Nope"), json!([1, 2, 3]))]})),
        ("explode.create", json!({"moves": [{"occurrence": "Door"}]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!(null))]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!([1, 2]))]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!({"x": 1, "y": 2}))]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!(["a", 2, 3]))]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!([1e7, 0, 0]))]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!([8e6, 8e6, 0]))]})),
        ("explode.create", json!({"moves": [move_of(json!("Door"), json!([1, 2, 3])), {"translate": [1, 2, 3]}]})),
        ("explode.create", json!({"scale": "2"})),
        ("explode.create", json!({"scale": null})),
        ("explode.create", json!({"scale": true})),
        ("explode.create", json!({"scale": 100.5})),
        ("explode.create", json!({"scale": -101})),
        ("explode.show", json!({"view": true})),
        ("explode.show", json!({"view": [1]})),
        ("explode.show", json!({"view": {"id": 1}})),
        ("explode.show", json!({"view": "Nope"})),
        ("explode.show", json!({"view": 7})),
        ("explode.delete", json!({})),
        ("explode.delete", json!({"view": null})),
        ("explode.delete", json!({"view": false})),
        ("explode.delete", json!({"view": ["Apart"]})),
        ("explode.delete", json!({"view": "Nope"})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = door();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

fn move_of(occurrence: Value, translate: Value) -> Value {
    json!({"occurrence": occurrence, "translate": translate})
}

/// The exploded view types read each key as the handlers do: `scale` only without `moves`, a move
/// by an occurrence's id or name with a point in either form, `view` left out, `null` or `none`
/// to show the assembled placement but refused by `explode.delete`, and keys a move carries
/// besides its two ignored. Each case is also checked against the schema.
#[test]
fn the_exploded_view_types_read_the_keys_as_the_handlers_do() {
    let issues = |id: &str, p: &Value| crate::schema::issues(id, p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let ok = |id: &str, p: Value, takes: bool| assert_eq!(issues(id, &p).is_empty(), takes, "{id} {p}: {:?}", issues(id, &p));
    let schema_takes = |schema: schemars::Schema, p: Value, takes: bool| {
        let schema = schema.as_value();
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    };
    // `scale` is read only when `moves` is not there; `moves` is read when it is there at all.
    let one = move_of(json!(3), json!({"x": 1, "y": 2, "z": 3}));
    for (p, takes) in [
        (json!({}), true),
        (json!({"name": "Apart", "scale": 2.5}), true),
        (json!({"scale": -100}), true),
        (json!({"scale": 101}), false),
        (json!({"scale": "2"}), false),
        (json!({"scale": null}), false),
        (json!({"moves": [], "scale": "ignored"}), true),
        (json!({"moves": [one.clone()], "scale": null}), true),
        (json!({"moves": [{"occurrence": "Door", "translate": [0, 0, 5], "note": "ignored"}]}), true),
        (json!({"moves": null, "scale": 2}), false),
        (json!({"moves": "Door"}), false),
        (json!({"moves": [{"occurrence": "Door"}]}), false),
        (json!({"moves": [{"occurrence": true, "translate": [0, 0, 5]}]}), false),
        (json!({"moves": [{"occurrence": "Door", "translate": [0, 0, 1e7]}]}), false),
    ] {
        ok("explode.create", p, takes);
    }
    schema_takes(input_schema_for::<ExplodeCreateParams>(), json!({"moves": [one], "name": "N"}), true);
    schema_takes(input_schema_for::<ExplodeCreateParams>(), json!({"moves": [{"occurrence": "Door"}]}), false);
    schema_takes(input_schema_for::<ExplodeCreateParams>(), json!({"scale": 101}), false);
    // Showing: no view, `null` and `none` in any case show the assembled placement.
    for (p, takes) in [
        (json!({}), true),
        (json!({"view": null}), true),
        (json!({"view": "None"}), true),
        (json!({"view": "Apart"}), true),
        (json!({"view": 4}), true),
        (json!({"view": true}), false),
        (json!({"view": ["Apart"]}), false),
    ] {
        ok("explode.show", p.clone(), takes);
        schema_takes(input_schema_for::<ExplodeShowParams>(), p, takes);
    }
    // Deleting needs a view, by id or name.
    for (p, takes) in [(json!({"view": "Apart"}), true), (json!({"view": 4}), true), (json!({}), false), (json!({"view": null}), false)] {
        schema_takes(input_schema_for::<ExplodeDeleteParams>(), p, takes);
    }
    ok("explode.delete", json!({"view": "Apart"}), true);
    assert_eq!(issues("explode.delete", &json!({"view": null})), ["`view` must be an id or a name"]);
    assert_eq!(issues("explode.delete", &json!({})), Vec::<String>::new(), "the handler gives its own message for a missing view");
    assert_eq!(issues("explode.show", &json!({"view": true})), ["`view` must be an id or a name"]);
    assert_eq!(issues("explode.create", &json!({"scale": "x"})), ["`scale` must be a number up to 100"]);
    assert_eq!(issues("explode.create", &json!({"moves": 1})), ["`moves` must be a list"]);
    assert_eq!(issues("explode.create", &json!({"moves": [{}]})), ["an occurrence must be an id or a name"]);
    assert_eq!(issues("explode.create", &json!({"moves": [{"occurrence": 1}]})), ["each move needs `translate`: [x, y, z]"]);
    assert_eq!(issues("explode.list", &json!({"x": 1})).len(), 1);
}

/// Making, showing, listing and deleting exploded views reply as the output schemas say, in Warn
/// without warnings, with the JSON the handlers have always returned. A misspelt key gets
/// `param_warnings` in Warn, since the handlers ignore unknown keys, and strict refuses it.
#[test]
fn exploded_views_reply_with_their_schemas() {
    let mut s = door();
    s.param_check = ParamCheck::Warn;
    let run = |s: &mut Session, id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let auto = run(&mut s, "explode.create", json!({}), output_schema_for::<ExplodeCreated>());
    assert_eq!(auto["name"], "Exploded View1", "{auto}");
    assert_eq!(auto["moves"].as_array().map(Vec::len), Some(3), "{auto}");
    assert!(auto["moves"][0]["occurrence"].is_u64() && auto["moves"][0]["translate"].as_array().is_some_and(|t| t.len() == 3), "{auto}");
    let scaled = run(&mut s, "explode.create", json!({"name": "Far", "scale": 2.5}), output_schema_for::<ExplodeCreated>());
    assert_eq!(scaled["name"], "Far", "{scaled}");
    run(&mut s, "explode.create", json!({"scale": -1}), output_schema_for::<ExplodeCreated>());
    // `scale` is not read with `moves`, so a bad one is no problem.
    let by_hand = run(
        &mut s,
        "explode.create",
        json!({"name": "Hand", "scale": "ignored", "moves": [move_of(json!("Door"), json!([0, 0, 50])), move_of(json!("Slide"), json!({"x": 1, "y": 2, "z": 3}))]}),
        output_schema_for::<ExplodeCreated>(),
    );
    let (door_id, slide_id) = (by_hand["moves"][0]["occurrence"].clone(), by_hand["moves"][1]["occurrence"].clone());
    let expected = json!({
        "view": by_hand["view"],
        "name": "Hand",
        "moves": [{"occurrence": door_id, "translate": [0.0, 0.0, 50.0]}, {"occurrence": slide_id, "translate": [1.0, 2.0, 3.0]}]
    });
    assert_eq!(by_hand, expected);
    run(&mut s, "explode.create", json!({"name": "Empty", "moves": []}), output_schema_for::<ExplodeCreated>());

    let list = run(&mut s, "explode.list", json!({}), output_schema_for::<ExplodeViews>());
    assert_eq!(list["views"].as_array().map(Vec::len), Some(5), "{list}");
    // Exactly what the design's own views serialize to, as the handler returned them before.
    assert_eq!(list["views"], serde_json::to_value(&s.doc.assembly.exploded).unwrap_or_default());
    assert_eq!(list["views"][3], json!({"id": by_hand["view"], "name": "Hand", "moves": [[door_id, [0.0, 0.0, 50.0]], [slide_id, [1.0, 2.0, 3.0]]]}));

    // Shown by name, by id and, to put the assembled placement back, by nothing, `null` or `none`.
    let r = run(&mut s, "explode.show", json!({"view": "Hand"}), output_schema_for::<ExplodeShown>());
    assert_eq!(r, json!({"view": by_hand["view"], "name": "Hand"}));
    let r = run(&mut s, "explode.show", json!({"view": scaled["view"]}), output_schema_for::<ExplodeShown>());
    assert_eq!(r, json!({"view": scaled["view"], "name": "Far"}));
    for p in [json!({}), json!({"view": null}), json!({"view": "None"})] {
        run(&mut s, "explode.show", json!({"view": "Hand"}), output_schema_for::<ExplodeShown>());
        let r = run(&mut s, "explode.show", p, output_schema_for::<ExplodeShown>());
        assert_eq!(r, json!({"view": null}));
    }

    let r = run(&mut s, "explode.delete", json!({"view": "Hand"}), output_schema_for::<ExplodeDeleted>());
    assert_eq!(r, json!({"deleted": "Hand"}));
    let r = run(&mut s, "explode.delete", json!({"view": scaled["view"]}), output_schema_for::<ExplodeDeleted>());
    assert_eq!(r, json!({"deleted": "Far"}));

    // A misspelt key runs in Warn and says so, with a reply that still matches; strict refuses it.
    run(&mut s, "explode.create", json!({"name": "Typo"}), output_schema_for::<ExplodeCreated>());
    for (id, p, schema) in [
        ("explode.create", json!({"nme": "Typo"}), output_schema_for::<ExplodeCreated>()),
        ("explode.show", json!({"veiw": "Exploded View"}), output_schema_for::<ExplodeShown>()),
        ("explode.list", json!({"view": 1}), output_schema_for::<ExplodeViews>()),
        ("explode.delete", json!({"view": "Typo", "name": 1}), output_schema_for::<ExplodeDeleted>()),
    ] {
        s.param_check = ParamCheck::Warn;
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()) && conforms(schema, schema, &r), "{id}: {r}");
        s.param_check = ParamCheck::Strict;
        let error = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("unknown key"), "{id}: {error}");
    }
    let before = s.doc.assembly.exploded.len();
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("explode.create", &json!({"nme": "Typo"})).is_err());
    assert_eq!(s.doc.assembly.exploded.len(), before, "strict ran nothing");
}
