use serde_json::{Value, json};

use super::{AngleReadoutParams, TabStep, TabStepParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AngleReadoutParams>(), &["enabled"]);
}

#[test]
fn status_details_has_checked_pure_query_and_current_typed_diagnostics() {
    let schema = output_schema_for::<crate::status_details::StatusDetails>();
    let root = schema.as_value();
    let mut s = Session::default();
    let spec = crate::find_command("view.status_details").unwrap();
    assert!(!spec.undoable && !spec.reads_model);
    assert!(spec.contract.is_some() && spec.output.is_some());
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    s.execute("inspect.continuity", &json!({"edges":[[5,0,0]],"samples":5})).unwrap();
    for mode in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        s.param_check = mode;
        let before = (s.revision, s.doc.to_json(), s.undo.len(), s.continuity.clone());
        for params in [json!({}), Value::Null] {
            let reply = s.execute("view.status_details", &params).unwrap();
            assert!(conforms(root, root, &reply), "{reply} does not match {root}");
            assert_eq!(reply["continuity"][0]["samples"], 5);
            assert_eq!(reply["continuity"][0]["current"], true);
            assert_eq!(reply["continuity"][0]["verdict"], "sharp");
            assert_eq!(reply["items"][0]["source"]["command"], "inspect.continuity");
            assert_eq!(reply["items"][0]["has_details"], true);
        }
        for params in [json!({"clear":true}), json!([]), json!(false)] {
            assert!(s.execute("view.status_details", &params).is_err());
        }
        assert_eq!((s.revision, s.doc.to_json(), s.undo.len(), s.continuity.clone()), before);
    }
}

#[test]
fn surface_and_section_contracts_match_successful_existing_reply_shapes() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Default::default() };
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    for (command, params) in [
        ("inspect.zebra", json!({"stripes":14.5})),
        ("inspect.draft", json!({"pull":[-2,3,-4],"angle":12.75})),
        ("inspect.environment_map", json!({})),
        ("inspect.accessibility", json!({"direction":[0,0,-1]})),
        ("inspect.curvature_map", json!({"radius":25.4})),
        ("inspect.section", json!({"plane":{"origin":[4,5,6],"normal":[-1,2,-3]},"offset":"-2 mm","flip":true,"hatch":false})),
        (
            "inspect.continuity",
            json!({"edges":[[5,0,0]],"samples":5,"gap_tolerance_mm":0.002,"angle_tolerance_deg":2,"curvature_tolerance_per_mm":0.03}),
        ),
    ] {
        let description = s.execute("engine.describe", &json!({"command":command})).unwrap();
        assert_eq!(description["input"]["additionalProperties"], false);
        for params in [params, json!({"clear":true})] {
            let reply = s.execute(command, &params).unwrap();
            let root = &description["output"];
            assert!(conforms(root, root, &reply), "{command}: {reply} does not match {root}");
        }
        assert!(s.execute(command, &json!({"unknown_setting":true})).is_err(), "checked registration for {command}");
    }
}

#[test]
fn status_items_has_strict_empty_input_and_a_typed_output_schema() {
    use crate::status_items::StatusItems;
    assert_keys(input_schema_for::<super::StatusItemsParams>(), &[]);
    let schema = output_schema_for::<StatusItems>();
    let schema = schema.as_value();
    let mut s = Session::default();
    let described = s.execute("engine.describe", &json!({"command": "view.status_items"})).unwrap();
    assert_eq!(described["input"]["type"], "object");
    assert_eq!(described["input"]["additionalProperties"], false);
    assert_eq!(described["output"]["properties"]["items"]["type"], "array");
    let spec = crate::find_command("view.status_items").unwrap();
    assert!(!spec.undoable && !spec.reads_model);
    for mode in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        s.param_check = mode;
        for input in [json!({}), Value::Null] {
            let reply = s.execute("view.status_items", &input).unwrap();
            assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
        }
        for input in [json!({"extra": true}), json!({"clear": true}), json!([]), json!(false), json!(42), json!("status")] {
            assert!(s.execute("view.status_items", &input).is_err(), "accepted {input} in {mode:?}");
        }
    }
    for (command, params) in [
        ("inspect.zebra", json!({"stripes": 13.5})),
        ("inspect.draft", json!({"angle": 4.5})),
        ("inspect.environment_map", json!({})),
        ("inspect.accessibility", json!({})),
        ("inspect.curvature_map", json!({"radius": 2})),
        ("inspect.section", json!({"plane": "XY", "hatch": false})),
    ] {
        s.execute(command, &params).unwrap();
        let reply = s.execute("view.status_items", &json!({})).unwrap();
        assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
        let typed: StatusItems = serde_json::from_value(reply.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), reply);
    }
    s.set_single_surface_analysis(false);
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    s.execute("inspect.continuity", &json!({"edges":[[5,0,0]],"samples":5})).unwrap();
    let reply = s.execute("view.status_items", &json!({})).unwrap();
    assert!(conforms(schema, schema, &reply), "legacy reply {reply} doesn't match {schema}");
    assert_eq!(reply["items"].as_array().unwrap().len(), 3);
    assert!(reply["items"].as_array().unwrap().iter().any(|item| item["id"] == "continuity"));
    let typed: StatusItems = serde_json::from_value(reply.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), reply);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("view.angle_readout", json!({"enabled": 1})),
        ("view.angle_readout", json!({"enabled": "true"})),
        ("view.angle_readout", json!({"enabled": null})),
    ]);
}

#[test]
fn angle_readout_replies_match_the_output_schema() {
    let schema = output_schema_for::<super::AngleReadoutResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for enabled in [true, false] {
        let r = s.execute("view.angle_readout", &json!({"enabled": enabled})).unwrap_or_else(|e| panic!("{e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings"), None, "{r}");
        assert_eq!(r["enabled"], Value::Bool(enabled));
    }
    assert!(!conforms(schema, schema, &json!({"enabled": "yes"})));
    assert!(!conforms(schema, schema, &json!({})));
}

/// #8565: the next and previous tab take nothing, say which way the window goes, and leave the
/// design and its undo as they were.
#[test]
fn next_and_previous_tab_echo_their_step_without_editing_the_design() {
    assert_keys(input_schema_for::<TabStepParams>(), &[]);
    let schema = output_schema_for::<TabStep>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let (before, undo) = (s.doc.to_json(), s.undo.len());
    for (id, step) in [("view.next_tab", 1), ("view.previous_tab", -1)] {
        let r = s.execute(id, &json!({})).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(r, json!({"step": step}), "{id}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(s.execute(id, &json!({"step": 2})).is_err(), "{id} takes no parameters (strict check)");
    }
    assert_eq!((s.doc.to_json(), s.undo.len()), (before, undo), "nothing in the design changes");
}

#[test]
fn navigator_checks_and_echoes_its_settings_and_refuses_a_missing_body() {
    assert_keys(input_schema_for::<super::NavigatorParams>(), &["arrows", "body", "home", "style", "texture"]);
    let schema = output_schema_for::<super::NavigatorParams>();
    let root = schema.as_value();
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let body = s.world_state().bodies[0].name.clone();
    let before = (s.revision, s.doc.to_json(), s.undo.len());
    for (params, echo) in [
        (json!({}), json!({})),
        (Value::Null, json!({})),
        (
            json!({"style": "custom", "body": body, "texture": "modeled", "arrows": true, "home": false}),
            json!({"style": "custom", "body": body, "texture": "modeled", "arrows": true, "home": false}),
        ),
        (json!({"body": null}), json!({"body": null})),
    ] {
        let reply = s.execute("view.navigator", &params).unwrap();
        assert_eq!(reply, echo, "{params}");
        assert!(conforms(root, root, &reply), "{reply} does not match {root}");
    }
    assert_eq!((s.revision, s.doc.to_json(), s.undo.len()), before, "the navigator is never part of the design");
    for bad in
        [json!({"style": "sphere"}), json!({"body": "No such body"}), json!({"body": 3}), json!({"texture": "glossy"}), json!({"arrows": "yes"})]
    {
        assert!(s.execute("view.navigator", &bad).is_err(), "{bad}");
    }
    let spec = crate::find_command("view.navigator").unwrap();
    assert!(!spec.undoable && spec.contract.is_some() && spec.output.is_some());
}
