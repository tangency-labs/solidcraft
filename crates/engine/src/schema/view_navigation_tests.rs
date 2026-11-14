use serde_json::{Value, json};

use super::{
    AmbientOcclusionParams, AmbientOcclusionResult, AngleReadoutParams, CameraReply, DisplayToggleParams, GraphicsPresetParams, GraphicsPresetResult,
    HistoryLayoutParams, HistoryLayoutResult, LayoutParams, LayoutResult, NavigateParams, NavigateResult, NavigationBarParams, NormalToParams,
    RenderPanelVisibilityParams, RenderPanelVisibilityResult, StandardViewParams, TabStep, TabStepParams, ToolParams, ToolResult, VisualStyleParams,
    VisualStyleResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::types::CameraParam;
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AngleReadoutParams>(), &["enabled"]);
    assert_keys(input_schema_for::<super::OriginAxesAcrossViewParams>(), &["enabled"]);
    assert_keys(input_schema_for::<StandardViewParams>(), &["camera"]);
    assert_keys(input_schema_for::<NormalToParams>(), &["camera"]);
    assert_keys(input_schema_for::<LayoutParams>(), &["mode"]);
    assert_keys(input_schema_for::<HistoryLayoutParams>(), &["collapsed", "dock", "width"]);
    assert_keys(input_schema_for::<GraphicsPresetParams>(), &["preset"]);
    assert_keys(input_schema_for::<AmbientOcclusionParams>(), &["enabled"]);
    assert_keys(input_schema_for::<DisplayToggleParams>(), &["on"]);
    assert_keys(input_schema_for::<NavigationBarParams>(), &["visible"]);
    assert_keys(
        input_schema_for::<NavigateParams>(),
        &["action", "anchor", "camera", "degrees", "delta", "direction", "factor", "fit", "free_orbit", "motion", "pivot", "viewport"],
    );
}

const STANDARD_VIEWS: [&str; 7] = ["view.front", "view.back", "view.left", "view.right", "view.top", "view.bottom", "view.isometric"];

/// A camera the view commands take, with `change` merged into it.
fn camera(change: Value) -> Value {
    let mut c = json!({"target": [1, 2, 3], "yaw": 0.3, "pitch": 0.4, "roll": 0, "distance": 150, "fov": 0, "up": "z"});
    if let (Some(c), Some(change)) = (c.as_object_mut(), change.as_object()) {
        c.extend(change.clone());
    }
    c
}

/// #10898: the camera type takes what the view commands' own reader takes, and its schema says
/// the same.
#[test]
fn the_camera_type_matches_the_view_commands_reader() {
    let schema = input_schema_for::<CameraParam>();
    let schema = schema.as_value();
    let without = |key: &str| {
        let mut c = camera(json!({}));
        c.as_object_mut().map(|o| o.remove(key));
        c
    };
    for v in [
        camera(json!({})),
        camera(json!({"up": "y", "fov": 0.8})),
        camera(json!({"pitch": -1.5, "yaw": 12.0, "roll": -3.0})),
        camera(json!({"target": [0.5, -2, 1e6]})),
        without("roll"),
        without("up"),
        camera(json!({"distance": 0})),
        camera(json!({"distance": -5})),
        camera(json!({"fov": std::f64::consts::PI})),
        camera(json!({"fov": -0.1})),
        camera(json!({"up": "x"})),
        camera(json!({"up": "Z"})),
        camera(json!({"target": [1, 2]})),
        camera(json!({"target": {"x": 1, "y": 2, "z": 3}})),
        camera(json!({"yaw": "0.3"})),
        without("target"),
        without("distance"),
        without("fov"),
        json!(null),
        json!(5),
        json!("front"),
        json!([1, 2, 3]),
    ] {
        let read = crate::cmd::camera_form(&v).is_some();
        assert_eq!(serde_json::from_value::<CameraParam>(v.clone()).is_ok(), read, "{v}");
        assert_eq!(conforms(schema, schema, &v), read, "{v}");
    }
}

#[test]
fn standard_views_keep_todays_errors() {
    let mut cases = Vec::new();
    for id in STANDARD_VIEWS {
        for c in [json!(5), json!(null), json!("top"), camera(json!({"distance": 0})), camera(json!({"fov": 4})), camera(json!({"up": "x"}))] {
            cases.push((id, json!({ "camera": c })));
        }
    }
    assert_strict_keeps_todays_errors(&cases);
}

/// A box with its top face selected, for Normal To.
fn top_face_selected(check: ParamCheck) -> Session {
    let mut s = Session { param_check: check, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("select.set", &json!({"items": [{"type": "face", "body": "Body1", "index": 1, "point": [20, 15, 20]}]}))
        .unwrap_or_else(|e| panic!("{e}"));
    s
}

/// #10898: every standard view and Normal To reply with the camera their output schema
/// describes, with or without a camera given, and refuse in strict what they refuse anyway.
#[test]
fn camera_commands_reply_with_the_camera_schema() {
    let schema = output_schema_for::<CameraReply>();
    let schema = schema.as_value();
    let check = |id: &str, r: &Value, warned: bool| {
        assert!(conforms(schema, schema, r), "{id}: {r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{id}: {r}");
        assert!(crate::cmd::camera_form(&r["camera"]).is_some(), "{id}: the reply's camera is one the commands take: {r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for id in STANDARD_VIEWS {
        for p in [json!({}), json!({"camera": camera(json!({}))}), json!({"camera": camera(json!({"up": "y", "fov": 0.6}))})] {
            let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            check(id, &r, false);
            if let Some(up) = p["camera"].get("up") {
                assert_eq!(&r["camera"]["up"], up, "{id} keeps the up axis");
            }
        }
        let r = s.execute(id, &json!({"camera": camera(json!({})), "zoom": 2})).unwrap_or_else(|e| panic!("{id}: {e}"));
        check(id, &r, true);
    }
    let mut s = top_face_selected(ParamCheck::Warn);
    for p in [json!({}), json!({"camera": camera(json!({}))})] {
        let r = s.execute("view.normal_to", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        check("view.normal_to", &r, false);
    }
    let r = s.execute("view.normal_to", &json!({"zoom": 2})).unwrap_or_else(|e| panic!("{e}"));
    check("view.normal_to", &r, true);
    // Strict refuses what the handler refuses, with its message: a camera of the wrong form,
    // and (the handler's own check) one whose angles are out of Normal To's bounds.
    for c in [json!(5), json!(null), camera(json!({"distance": 0})), camera(json!({"yaw": 2e6})), camera(json!({"pitch": 2.0}))] {
        let p = json!({ "camera": c });
        let off = top_face_selected(ParamCheck::Off).execute("view.normal_to", &p).err().map(|e| e.to_string());
        assert!(off.as_deref().is_some_and(|e| e.contains("invalid camera")), "{p}: {off:?}");
        assert_eq!(top_face_selected(ParamCheck::Strict).execute("view.normal_to", &p).err().map(|e| e.to_string()), off, "{p}");
    }
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
        ("view.origin_axes_across_view", json!({"enabled": 1})),
        ("view.origin_axes_across_view", json!({"enabled": "true"})),
        ("view.origin_axes_across_view", json!({"enabled": null})),
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

#[test]
fn origin_axes_across_view_replies_match_the_output_schema() {
    let schema = output_schema_for::<super::OriginAxesAcrossViewResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for enabled in [true, false] {
        let reply = s.execute("view.origin_axes_across_view", &json!({"enabled": enabled})).unwrap();
        assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
        assert_eq!(reply, json!({"enabled": enabled}));
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
    let body = s.world_state().unwrap().bodies[0].name.clone();
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

const LAYOUT_COMMANDS: [&str; 5] = ["view.layout", "view.history_layout", "view.graphics_preset", "view.ambient_occlusion", "view.navigate"];

/// A camera for `view.navigate`.
fn nav_camera() -> Value {
    camera(json!({"target": [0, 0, 0], "yaw": 0.0, "pitch": 0.0, "distance": 100, "fov": 0.0}))
}

/// A `view.navigate` call: the camera and `extra`'s keys.
fn nav(extra: Value) -> Value {
    let mut p = json!({"camera": nav_camera()});
    if let (Some(o), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        o.extend(extra.clone());
    }
    p
}

/// Each command publishes its input and output schemas with a contract, is display only, and its
/// required keys are the ones its handler refuses a call without.
#[test]
fn the_commands_publish_contracts_and_schemas() {
    let mut s = Session::default();
    for id in LAYOUT_COMMANDS {
        let spec = crate::find_command(id).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some() && !spec.undoable, "{id}");
        let d = s.execute("engine.describe", &json!({"command": id})).unwrap();
        assert_eq!(d["input"]["additionalProperties"], false, "{id}");
        assert_eq!(d["output"]["type"], "object", "{id}");
    }
    let mut required = |id: &str| {
        let d = s.execute("engine.describe", &json!({"command": id})).unwrap();
        d["input"]["required"].clone()
    };
    assert_eq!(required("view.layout"), json!(["mode"]));
    assert_eq!(required("view.graphics_preset"), json!(["preset"]));
    assert_eq!(required("view.ambient_occlusion"), json!(["enabled"]));
    assert_eq!(required("view.navigate"), json!(["camera"]));
    assert_eq!(required("view.history_layout"), Value::Null);
    let d = Session::default().execute("engine.describe", &json!({"command": "view.history_layout"})).unwrap();
    assert_eq!(d["input"]["properties"]["width"]["minimum"], 1.0);
    assert_eq!(d["input"]["properties"]["width"]["maximum"], 1000000.0);
    let d = Session::default().execute("engine.describe", &json!({"command": "view.navigate"})).unwrap();
    assert_eq!(d["input"]["properties"]["degrees"]["exclusiveMinimum"], 0.0);
    assert_eq!(d["input"]["properties"]["degrees"]["maximum"], 180.0);
    assert_eq!(d["input"]["properties"]["factor"]["minimum"], 0.05);
    assert_eq!(d["input"]["properties"]["pivot"]["anyOf"][0]["items"]["exclusiveMaximum"], 1e9);
    let d = Session::default().execute("engine.describe", &json!({"command": "view.history_layout"})).unwrap();
    let dock = serde_json::to_string(&d["input"]).unwrap();
    for name in ["bottom", "left", "under_browser", "browser"] {
        assert!(dock.contains(&format!("\"{name}\"")), "dock {name}: {dock}");
    }
}

/// The replies are the JSON the handlers returned before the types: exactly these values.
#[test]
fn replies_are_todays_json() {
    let mut s = Session::default();
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    assert_eq!(run("view.layout", json!({"mode": "compact"})), json!({"mode": "compact"}));
    assert_eq!(run("view.layout", json!({"mode": "floating"})), json!({"mode": "floating"}));
    assert_eq!(run("view.history_layout", json!({})), json!({}));
    assert_eq!(
        run("view.history_layout", json!({"width": 320, "dock": "under_browser", "collapsed": false})),
        json!({"width": 320.0, "dock": "under_browser", "collapsed": false})
    );
    assert_eq!(run("view.history_layout", json!({"dock": "browser"})), json!({"dock": "browser"}));
    assert_eq!(
        run("view.graphics_preset", json!({"preset": "quality"})),
        json!({"preset": "quality", "enabled": true, "display_quality": "fine", "supersampling": 2.0, "supersampling_adaptive": true})
    );
    assert_eq!(
        run("view.graphics_preset", json!({"preset": "performance"})),
        json!({"preset": "performance", "enabled": false, "display_quality": "normal", "supersampling": 1.0, "supersampling_adaptive": true})
    );
    assert_eq!(run("view.ambient_occlusion", json!({"enabled": false})), json!({"enabled": false}));
    let cam: solidcraft_viewport::Camera = serde_json::from_value(nav_camera()).unwrap();
    let turned = run("view.navigate", json!({"camera": nav_camera(), "action": "fit"}));
    assert_eq!(turned, json!({"camera": cam, "fit": true}));
    let moved = run("view.navigate", json!({"camera": nav_camera(), "motion": "zoom", "factor": 2}));
    assert_eq!(moved["fit"], false);
    assert_eq!(moved["continuous"], true);
    assert_eq!(moved.as_object().map(|o| o.len()), Some(3));
}

/// Real replies of each command, in Warn, match their output schema; a misspelt key's warning
/// still matches.
#[test]
fn replies_match_the_output_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let calls: Vec<(&str, Value, schemars::Schema)> = vec![
        ("view.layout", json!({"mode": "compact"}), output_schema_for::<LayoutResult>()),
        ("view.layout", json!({"mode": "floating"}), output_schema_for::<LayoutResult>()),
        ("view.history_layout", json!({}), output_schema_for::<HistoryLayoutResult>()),
        ("view.history_layout", json!({"width": 250.5, "dock": "left", "collapsed": true}), output_schema_for::<HistoryLayoutResult>()),
        ("view.graphics_preset", json!({"preset": "quality"}), output_schema_for::<GraphicsPresetResult>()),
        ("view.graphics_preset", json!({"preset": "performance"}), output_schema_for::<GraphicsPresetResult>()),
        ("view.ambient_occlusion", json!({"enabled": true}), output_schema_for::<AmbientOcclusionResult>()),
        ("view.navigate", nav(json!({"direction": [1, 1, 1]})), output_schema_for::<NavigateResult>()),
        ("view.navigate", nav(json!({"action": "left", "degrees": 15})), output_schema_for::<NavigateResult>()),
        ("view.navigate", nav(json!({"action": "home"})), output_schema_for::<NavigateResult>()),
        ("view.navigate", nav(json!({"action": "clockwise", "fit": true})), output_schema_for::<NavigateResult>()),
        ("view.navigate", nav(json!({"motion": "pan", "delta": [10, -5], "viewport": [800, 600]})), output_schema_for::<NavigateResult>()),
        (
            "view.navigate",
            nav(json!({"motion": "orbit", "delta": [10, -5], "pivot": [1, 2, 3], "viewport": [800, 600]})),
            output_schema_for::<NavigateResult>(),
        ),
        ("view.navigate", nav(json!({"motion": "orbit", "delta": [10, -5], "free_orbit": true})), output_schema_for::<NavigateResult>()),
        ("view.navigate", nav(json!({"motion": "zoom", "factor": 0.5, "anchor": [1, 2, 3]})), output_schema_for::<NavigateResult>()),
    ];
    for (id, p, schema) in calls {
        let reply = run(id, p.clone());
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &reply), "{id} {p}: {reply} doesn't match {schema}");
        assert!(reply.get("param_warnings").is_none(), "{id} {p}: {reply}");
        let again = run(id, {
            let mut q = p.clone();
            q["zoomm"] = json!(1);
            q
        });
        assert!(again["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{id}: {again}");
        assert!(conforms(schema, schema, &again), "{id}: {again} doesn't match {schema}");
    }
}

/// Strict refuses with the handler's own message whatever the handler refuses.
#[test]
fn view_layout_commands_keep_todays_errors() {
    let mut cases: Vec<(&str, Value)> = vec![
        ("view.layout", json!({})),
        ("view.layout", json!({"mode": "grid"})),
        ("view.layout", json!({"mode": 1})),
        ("view.layout", json!({"mode": null})),
        ("view.history_layout", json!({"width": 0})),
        ("view.history_layout", json!({"width": 1000001})),
        ("view.history_layout", json!({"width": "300"})),
        ("view.history_layout", json!({"width": null})),
        ("view.history_layout", json!({"dock": "top"})),
        ("view.history_layout", json!({"dock": 3})),
        ("view.history_layout", json!({"collapsed": "yes"})),
        ("view.history_layout", json!({"collapsed": null})),
        ("view.graphics_preset", json!({})),
        ("view.graphics_preset", json!({"preset": "fast"})),
        ("view.graphics_preset", json!({"preset": true})),
        ("view.graphics_preset", json!({"preset": null})),
        ("view.ambient_occlusion", json!({})),
        ("view.ambient_occlusion", json!({"enabled": 1})),
        ("view.ambient_occlusion", json!({"enabled": "true"})),
        ("view.ambient_occlusion", json!({"enabled": null})),
        ("view.navigate", json!({})),
        ("view.navigate", json!({"camera": null, "action": "fit"})),
        ("view.navigate", json!({"camera": 5, "action": "fit"})),
        ("view.navigate", json!({"action": "fit"})),
    ];
    for c in [
        camera(json!({"yaw": 2e6})),
        camera(json!({"roll": -2e6})),
        camera(json!({"pitch": 2.0})),
        camera(json!({"distance": 0})),
        camera(json!({"fov": 4})),
    ] {
        cases.push(("view.navigate", json!({"camera": c, "action": "fit"})));
    }
    for extra in [
        json!({"action": "fit", "fit": 1}),
        json!({"action": "fit", "fit": null}),
        json!({"direction": [0, 0, 0]}),
        json!({"direction": [3, 0, 0]}),
        json!({"direction": "front"}),
        json!({"direction": null}),
        json!({"action": "spin"}),
        json!({"action": 4}),
        json!({"action": null}),
        json!({"action": "left", "degrees": 0}),
        json!({"action": "left", "degrees": 181}),
        json!({"action": "left", "degrees": "90"}),
        json!({"motion": "slide"}),
        json!({"motion": null}),
        json!({"motion": "zoom", "fit": true, "factor": 2}),
        json!({"motion": "zoom", "factor": 0.01}),
        json!({"motion": "zoom", "factor": 21}),
        json!({"motion": "zoom", "factor": 2, "anchor": [1, 2]}),
        json!({"motion": "zoom", "factor": 2, "anchor": null}),
        json!({"motion": "pan", "delta": [20000, 0], "viewport": [800, 600]}),
        json!({"motion": "pan", "delta": [1], "viewport": [800, 600]}),
        json!({"motion": "pan", "viewport": [800, 600]}),
        json!({"motion": "pan", "delta": [1, 1], "viewport": [0, 600]}),
        json!({"motion": "pan", "delta": [1, 1], "viewport": [800, 2000000]}),
        json!({"motion": "pan", "delta": [1, 1]}),
        json!({"motion": "orbit", "delta": [1, 1], "free_orbit": "yes"}),
        json!({"motion": "orbit", "delta": [1, 1], "pivot": [1, 2]}),
        json!({"motion": "orbit", "delta": [1, 1], "pivot": null}),
    ] {
        cases.push(("view.navigate", nav(extra)));
    }
    assert_strict_keeps_todays_errors(&cases);
}

/// A value of a key is accepted by the type exactly when the handler accepts it (the other keys
/// being fine): the schemas never say yes where today's reader says no, or the other way round.
#[test]
fn the_types_read_each_key_as_its_handler_does() {
    let accepts = |id: &str, p: &Value| Session::default().execute(id, p).is_ok();
    let check = |id: &str, base: Value, key: &str, values: Vec<Value>| {
        for v in values {
            let mut p = base.clone();
            p[key] = v.clone();
            let ok = accepts(id, &p);
            let typed = match id {
                "view.layout" => serde_json::from_value::<LayoutParams>(p.clone()).is_ok(),
                "view.history_layout" => serde_json::from_value::<HistoryLayoutParams>(p.clone()).is_ok(),
                "view.graphics_preset" => serde_json::from_value::<GraphicsPresetParams>(p.clone()).is_ok(),
                "view.ambient_occlusion" => serde_json::from_value::<AmbientOcclusionParams>(p.clone()).is_ok(),
                _ => serde_json::from_value::<NavigateParams>(p.clone()).is_ok(),
            };
            assert_eq!(typed, ok, "{id} {p}");
        }
    };
    let junk = || vec![json!(null), json!(true), json!(0), json!(1.5), json!("x"), json!([]), json!({})];
    let with = |mut v: Vec<Value>| {
        v.extend(junk());
        v
    };
    check("view.layout", json!({}), "mode", with(vec![json!("floating"), json!("compact"), json!("Compact")]));
    check("view.history_layout", json!({}), "width", with(vec![json!(1), json!(1e6), json!(0.5), json!(1000001), json!(-4), json!(300)]));
    check(
        "view.history_layout",
        json!({}),
        "dock",
        with(vec![json!("bottom"), json!("left"), json!("under_browser"), json!("browser"), json!("Left")]),
    );
    check("view.history_layout", json!({}), "collapsed", with(vec![json!(true), json!(false)]));
    check("view.graphics_preset", json!({}), "preset", with(vec![json!("quality"), json!("performance"), json!("Quality")]));
    check("view.ambient_occlusion", json!({}), "enabled", with(vec![json!(true), json!(false)]));
    let base = json!({"camera": nav_camera(), "action": "fit"});
    let all_actions: Vec<Value> =
        ["left", "right", "up", "down", "clockwise", "counterclockwise", "home", "fit", "spin"].iter().map(|a| json!(a)).collect();
    check("view.navigate", json!({"camera": nav_camera()}), "action", with(all_actions));
    check("view.navigate", base.clone(), "fit", with(vec![json!(true), json!(false)]));
    check(
        "view.navigate",
        base.clone(),
        "direction",
        with(vec![
            json!([1, 0, 0]),
            json!([0, 0, 1]),
            json!([2, 0, 0]),
            json!([2.1, 0, 0]),
            json!([0, 0, 0]),
            json!({"x": 0, "y": 1, "z": 0}),
            json!([1, 2]),
            json!([1e10, 0, 0]),
        ]),
    );
    check(
        "view.navigate",
        json!({"camera": nav_camera(), "action": "left"}),
        "degrees",
        with(vec![json!(1), json!(180), json!(90), json!(90.0), json!(0), json!(-1), json!(181), json!(0.001)]),
    );
    let all_keys = json!({"camera": nav_camera(), "delta": [1, 1], "viewport": [800, 600], "factor": 2});
    check("view.navigate", all_keys, "motion", with(vec![json!("zoom"), json!("pan"), json!("orbit")]));
    let pan = json!({"camera": nav_camera(), "motion": "pan", "viewport": [800, 600]});
    check(
        "view.navigate",
        pan.clone(),
        "delta",
        with(vec![json!([1, 2]), json!({"x": 1, "y": 2}), json!([10000, -10000]), json!([10001, 0]), json!([1, 2, 3]), json!([1e10, 0])]),
    );
    let pan = json!({"camera": nav_camera(), "motion": "pan", "delta": [1, 1]});
    check(
        "view.navigate",
        pan,
        "viewport",
        with(vec![json!([1, 1]), json!([1e6, 1e6]), json!([0.5, 5]), json!([5, 1000001]), json!({"x": 3, "y": 4}), json!([3])]),
    );
    let zoom = json!({"camera": nav_camera(), "motion": "zoom", "factor": 2});
    check("view.navigate", zoom.clone(), "anchor", with(vec![json!([1, 2, 3]), json!({"x": 1, "y": 2, "z": 3}), json!([1, 2]), json!([1e10, 0, 0])]));
    check(
        "view.navigate",
        json!({"camera": nav_camera(), "motion": "zoom"}),
        "factor",
        with(vec![json!(0.05), json!(20), json!(0.04), json!(20.5), json!(1)]),
    );
    let orbit = json!({"camera": nav_camera(), "motion": "orbit", "delta": [1, 1], "viewport": [800, 600]});
    check("view.navigate", orbit.clone(), "pivot", with(vec![json!([1, 2, 3]), json!([1, 2]), json!({"x": 1, "y": 2, "z": 3})]));
    check("view.navigate", orbit, "free_orbit", with(vec![json!(true), json!(false)]));
    let camera_values = vec![
        nav_camera(),
        camera(json!({"yaw": 1e6})),
        camera(json!({"yaw": 1.1e6})),
        camera(json!({"roll": 1.1e6})),
        camera(json!({"pitch": std::f64::consts::FRAC_PI_2})),
        camera(json!({"pitch": 1.58})),
        camera(json!({"pitch": -1.58})),
        camera(json!({"distance": 0})),
        camera(json!({"fov": 3.2})),
        camera(json!({"up": "y"})),
        camera(json!({"up": "w"})),
    ];
    check("view.navigate", json!({"action": "fit"}), "camera", with(camera_values));
}

/// A key of another form than the one the handler takes is not used, so the contract does not
/// read it either: such a call replies as it does with the check off, in warn and in strict.
#[test]
fn navigate_reads_only_the_keys_of_its_form() {
    for p in [
        // A motion wins over an action and a direction.
        json!({"camera": nav_camera(), "motion": "zoom", "factor": 2, "action": "bogus", "direction": "up"}),
        // Each motion reads its own keys: a pan has no factor, a zoom no delta.
        json!({"camera": nav_camera(), "motion": "pan", "delta": [3, 4], "viewport": [800, 600], "factor": 99}),
        json!({"camera": nav_camera(), "motion": "zoom", "factor": 2, "delta": "far", "viewport": 0, "pivot": "here"}),
        // An orbit reads the viewport only about a pivot.
        json!({"camera": nav_camera(), "motion": "orbit", "delta": [3, 4], "viewport": 0}),
        // A direction has no degrees or action.
        json!({"camera": nav_camera(), "direction": [0, 0, 1], "degrees": 500, "action": "bogus"}),
    ] {
        let off = Session::default().execute("view.navigate", &p).unwrap();
        for check in [ParamCheck::Warn, ParamCheck::Strict] {
            let checked = Session { param_check: check, ..Session::default() }.execute("view.navigate", &p).unwrap();
            assert_eq!(checked, off, "{check:?} {p}");
        }
    }
    // The keys of the form taken are still checked.
    let p = json!({"camera": nav_camera(), "motion": "pan", "delta": [3, 4], "viewport": [0, 600]});
    let strict = Session { param_check: ParamCheck::Strict, ..Session::default() }.execute("view.navigate", &p);
    assert!(strict.unwrap_err().to_string().contains("viewport dimensions must be in 1..1000000"));
}

/// A call with exactly these parameters.
fn call(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut p = serde_json::Map::new();
    for (key, value) in pairs {
        p.insert(key.to_string(), value);
    }
    Value::Object(p)
}

/// The four viewport display toggles read their one optional flag the same way
/// (`cmd/view_navigation.rs`'s `view_toggle`), so one contract and one result type describe each
/// of their calls: the flag is a boolean or absent, and the reply is the flag it was given. A
/// `null` flag is not an omitted one, which is what these handlers have always refused.
#[test]
fn the_display_toggles_answer_with_their_own_schemas() {
    let mut s = Session::default();
    for (id, key) in [("view.grid", "on"), ("view.ground_shadow", "on"), ("view.spin", "on"), ("view.navigation_bar", "visible")] {
        let spec = crate::find_command(id).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some() && !spec.undoable, "{id}");
        let described = s.execute("engine.describe", &json!({"command": id})).unwrap();
        let (input, out) = (&described["input"], &described["output"]);
        assert_eq!(input["properties"][key]["type"], "boolean", "{id}: a null flag is not an omitted one");
        assert!(input.get("required").is_none(), "{id}: the flag may be omitted");
        for value in [json!(true), json!(false)] {
            let p = call([(key, value)]);
            let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            assert!(conforms(out, out, &reply), "{id} {p}: {reply} doesn't match {out}");
        }
        // With no flag the command asks the UI to toggle the setting and answers with nothing.
        let reply = s.execute(id, &json!({})).unwrap_or_else(|e| panic!("{id} with no flag: {e}"));
        assert_eq!(reply, json!({}), "{id}");
        assert!(conforms(out, out, &reply), "{id}: {reply} doesn't match {out}");
        // A `null` flag is not an omitted one, and no level lets it through: with the check off and
        // in warn the handler's own message answers, in strict the contract speaks first, and the
        // published schema has no null branch for either.
        let null = call([(key, Value::Null)]);
        let handler = format!("invalid parameters for `{id}`: {key} must be a boolean");
        let contract = format!("invalid parameters for `{id}`: `{key}` must be true or false");
        for (level, expected) in [(ParamCheck::Off, &handler), (ParamCheck::Warn, &handler), (ParamCheck::Strict, &contract)] {
            let mut checked = Session { param_check: level, ..Session::default() };
            let e = checked.execute(id, &null).err().unwrap_or_else(|| panic!("{id} {null} runs with the check {level:?}"));
            assert_eq!(e.to_string(), *expected, "{id}");
        }
        // A key the command doesn't take is refused as it has always been: the handler names the
        // key it doesn't read, and in strict the contract names it before the handler runs.
        let hostile = call([(key, json!(true)), ("off", json!(true))]);
        for level in [ParamCheck::Off, ParamCheck::Warn] {
            let mut checked = Session { param_check: level, ..Session::default() };
            let e = checked.execute(id, &hostile).err().unwrap_or_else(|| panic!("{id} {hostile} runs with the check {level:?}"));
            assert_eq!(e.to_string(), format!("invalid parameters for `{id}`: unknown parameter: off"), "{id}");
        }
        let strict = Session { param_check: ParamCheck::Strict, ..Session::default() }.execute(id, &hostile);
        let e = strict.err().unwrap_or_else(|| panic!("{id} {hostile} runs in strict"));
        assert!(e.to_string().contains("unknown key `off`"), "{id}: {e}");
    }
}

/// The two latched display settings: each publishes a contract and an output schema, its only key
/// is required, and the schema names are exactly the names the handler takes.
#[test]
fn the_mode_commands_publish_contracts_and_schemas() {
    assert_keys(input_schema_for::<ToolParams>(), &["tool"]);
    assert_keys(input_schema_for::<VisualStyleParams>(), &["style"]);
    let mut s = Session::default();
    let mut described = |id: &str| s.execute("engine.describe", &json!({"command": id})).unwrap();
    for id in ["view.tool", "view.visual_style"] {
        let spec = crate::find_command(id).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some() && !spec.undoable, "{id}");
        let d = described(id);
        assert_eq!(d["input"]["additionalProperties"], false, "{id}");
        assert_eq!(d["output"]["type"], "object", "{id}");
    }
    assert_eq!(described("view.tool")["input"]["required"], json!(["tool"]));
    assert_eq!(described("view.visual_style")["input"]["required"], json!(["style"]));
    // The window's names are the published names, so a caller reads them off the description.
    let tools = serde_json::to_string(&described("view.tool")["input"]).unwrap();
    for name in ["orbit", "pan", "zoom", "none"] {
        assert!(tools.contains(&format!("\"{name}\"")), "tool {name}: {tools}");
    }
    let styles = serde_json::to_string(&described("view.visual_style")["input"]).unwrap();
    for name in ["shaded_with_edges", "shaded", "wireframe", "shaded_with_hidden_edges"] {
        assert!(styles.contains(&format!("\"{name}\"")), "style {name}: {styles}");
    }
}

/// The replies are the JSON the handlers returned before the types, and in the warn mode they
/// match the published output schemas.
#[test]
fn mode_replies_are_todays_json() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let tool_schema = output_schema_for::<ToolResult>();
    let style_schema = output_schema_for::<VisualStyleResult>();
    let (tool, style) = (tool_schema.as_value(), style_schema.as_value());
    for name in ["orbit", "pan", "zoom", "none"] {
        let p = json!({"tool": name});
        let reply = s.execute("view.tool", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(reply, json!({"tool": name}), "{p}");
        assert!(conforms(tool, tool, &reply), "{reply} doesn't match {tool}");
        assert!(reply.get("param_warnings").is_none(), "{p}: {reply}");
    }
    for (name, index) in [("shaded_with_edges", 0), ("shaded", 1), ("wireframe", 2), ("shaded_with_hidden_edges", 3)] {
        let p = json!({"style": name});
        let reply = s.execute("view.visual_style", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(reply, json!({"style": name, "index": index}), "{p}");
        assert!(conforms(style, style, &reply), "{reply} doesn't match {style}");
        assert!(reply.get("param_warnings").is_none(), "{p}: {reply}");
    }
    // Both commands check their own keys first, so a key they don't know stays today's refusal.
    for (id, key) in [("view.tool", "tools"), ("view.visual_style", "styles")] {
        let e = s.execute(id, &json!({key: "orbit"})).err().unwrap_or_else(|| panic!("{id} {key} runs"));
        assert_eq!(e.to_string(), format!("invalid parameters for `{id}`: unknown parameter: {key}"), "{id} {key}");
    }
}

/// Strict refuses with the handler's own message whatever the handler refuses today.
#[test]
fn mode_commands_keep_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("view.tool", json!({})),
        ("view.tool", json!({"tool": "spin"})),
        ("view.tool", json!({"tool": 1})),
        ("view.tool", json!({"tool": true})),
        ("view.tool", json!({"tool": null})),
        ("view.visual_style", json!({})),
        ("view.visual_style", json!({"style": "hidden"})),
        ("view.visual_style", json!({"style": "Shaded"})),
        ("view.visual_style", json!({"style": 1})),
        ("view.visual_style", json!({"style": null})),
    ]);
}

/// The Render window's two panels take the same required flag and answer with it, so one contract
/// and one result type describe both calls. Neither has a toggle form: `visible` is required, and
/// the reply is the flag, which is the object both handlers have always returned.
#[test]
fn the_render_panels_answer_with_their_own_schemas() {
    assert_keys(input_schema_for::<RenderPanelVisibilityParams>(), &["visible"]);
    let mut s = Session::default();
    for id in ["view.render_inspector", "view.shots_strip"] {
        let spec = crate::find_command(id).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some() && !spec.undoable, "{id}");
        let described = s.execute("engine.describe", &json!({"command": id})).unwrap();
        let (input, out) = (&described["input"], &described["output"]);
        assert_eq!(input["required"], json!(["visible"]), "{id}: the flag has no toggle form");
        assert_eq!(input["additionalProperties"], false, "{id}");
        assert_eq!(input["properties"]["visible"]["type"], "boolean", "{id}: a null flag is not an omitted one");
        for value in [true, false] {
            let p = call([("visible", json!(value))]);
            let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            assert_eq!(reply, json!({"visible": value}), "{id} {p}");
            assert!(conforms(out, out, &reply), "{id} {p}: {reply} doesn't match {out}");
            assert!(reply.get("param_warnings").is_none(), "{id} {p}: {reply}");
        }
    }
    // The published output type is that object, plus the warnings only the warn mode adds.
    let out = output_schema_for::<RenderPanelVisibilityResult>();
    let out = out.as_value();
    assert!(conforms(out, out, &json!({"visible": true, "param_warnings": ["a key the command doesn't take"]})), "{out}");
}

/// A flag that isn't a boolean and a key the command doesn't take keep the messages the handler has
/// always used with the check off and in warn; in strict the contract speaks first and names them
/// the way the published schema does.
#[test]
fn render_panel_refusals_stay_the_handlers_below_strict() {
    for id in ["view.render_inspector", "view.shots_strip"] {
        // The flag is required: no level lets a call without one through. The message is the one the
        // handler's private struct gave before these types, because the parameters are read the same
        // way; in strict the contract speaks first and names the key the way the schema does.
        for level in [ParamCheck::Off, ParamCheck::Warn] {
            let mut s = Session { param_check: level, ..Session::default() };
            let e = s.execute(id, &json!({})).err().unwrap_or_else(|| panic!("{id} with no flag runs with the check {level:?}"));
            assert_eq!(e.to_string(), format!("invalid parameters for `{id}`: missing field `visible`"), "{id}");
        }
        let strict = Session { param_check: ParamCheck::Strict, ..Session::default() }.execute(id, &json!({}));
        let e = strict.err().unwrap_or_else(|| panic!("{id} with no flag runs in strict"));
        assert!(e.to_string().contains("`visible`"), "{id}: {e}");
        let bad_flag = call([("visible", json!("false"))]);
        let handler = format!(r#"invalid parameters for `{id}`: invalid type: string "false", expected a boolean"#);
        for level in [ParamCheck::Off, ParamCheck::Warn] {
            let mut s = Session { param_check: level, ..Session::default() };
            let e = s.execute(id, &bad_flag).err().unwrap_or_else(|| panic!("{id} {bad_flag} runs with the check {level:?}"));
            assert_eq!(e.to_string(), handler, "{id}");
        }
        let strict = Session { param_check: ParamCheck::Strict, ..Session::default() }.execute(id, &bad_flag);
        let e = strict.err().unwrap_or_else(|| panic!("{id} {bad_flag} runs in strict"));
        assert!(e.to_string().contains("`visible` must be true or false"), "{id}: {e}");
        // A key the command doesn't take: the handler has always refused it, and strict names it
        // from the schema instead.
        let hostile = call([("visible", json!(true)), ("off", json!(true))]);
        for level in [ParamCheck::Off, ParamCheck::Warn] {
            let mut s = Session { param_check: level, ..Session::default() };
            let e = s.execute(id, &hostile).err().unwrap_or_else(|| panic!("{id} {hostile} runs with the check {level:?}"));
            assert_eq!(e.to_string(), format!("invalid parameters for `{id}`: unknown field `off`, expected `visible`"), "{id}");
        }
        let strict = Session { param_check: ParamCheck::Strict, ..Session::default() }.execute(id, &hostile);
        let e = strict.err().unwrap_or_else(|| panic!("{id} {hostile} runs in strict"));
        assert!(e.to_string().contains("unknown key `off`"), "{id}: {e}");
    }
}

