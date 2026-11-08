use serde_json::{Value, json};

use super::{
    BodyFeaturesParams, BodyListParams, CommandEntry, CommandsParams, DescribeParams, DocumentInspectParams, DocumentInspectResult,
    MaterialAssignParams, MaterialAssignResult, MaterialListParams, MaterialListResult, MeasureParams, MeasureResult, ModelBodyFeatures, ModelEdges,
    ModelFaces, ModelThreads, ModelThreadsParams, SketchInspectParams, SketchInspected,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DescribeParams>(), &["command"]);
    assert_keys(input_schema_for::<BodyListParams>(), &["body"]);
    assert_keys(input_schema_for::<ModelThreadsParams>(), &[]);
    assert_keys(input_schema_for::<BodyFeaturesParams>(), &["body"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("engine.describe", json!({"command": 5})),
        ("engine.describe", json!({"command": null})),
        ("engine.describe", json!({"command": "solid.extrud"})),
        ("model.edges", json!({})),
        ("model.edges", json!({"body": "Nope"})),
        ("model.faces", json!({})),
        ("model.faces", json!({"body": "Nope"})),
        ("model.body_features", json!({"body": "Nope"})),
    ]);
}

/// #10942: a body's edges and faces and the design's threads reply as their output schemas say,
/// in Warn without warnings for good calls and with them for a misspelt key. A cylindrical face
/// carries `cylinder` and a folded sheet's face `sheet_face`; strict refuses a `body` that isn't
/// text, which the handlers read as no body.
#[test]
fn model_lists_reply_with_their_schemas() {
    let (edges, faces, threads) = (output_schema_for::<ModelEdges>(), output_schema_for::<ModelFaces>(), output_schema_for::<ModelThreads>());
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "designation": "M8"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "R1", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [60, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    let sheet = s.model.state().bodies.iter().map(|b| b.name.clone()).find(|n| n != "Shaft").unwrap_or_default();

    let r = run(&mut s, "model.edges", json!({"body": "Shaft"}));
    matches(&edges, &r, false);
    assert!(r["edges"].as_array().is_some_and(|e| !e.is_empty()), "{r}");
    let r = run(&mut s, "model.faces", json!({"body": "Shaft"}));
    matches(&faces, &r, false);
    assert!(r["faces"].as_array().into_iter().flatten().any(|f| f["cylinder"]["radius"].as_f64().is_some_and(|r| (r - 4.0).abs() < 1e-9)), "{r}");
    let r = run(&mut s, "model.faces", json!({"body": sheet}));
    matches(&faces, &r, false);
    assert!(r["faces"].as_array().into_iter().flatten().any(|f| f["sheet_face"].as_str().is_some_and(|n| n.starts_with("sheet:"))), "{r}");
    let r = run(&mut s, "model.threads", json!({}));
    matches(&threads, &r, false);
    assert_eq!((r["threads"][0]["designation"].as_str(), r["threads"][0]["hand"].as_str()), (Some("M8"), Some("right")), "{r}");

    matches(&edges, &run(&mut s, "model.edges", json!({"body": "Shaft", "bdy": 1})), true);
    matches(&faces, &run(&mut s, "model.faces", json!({"body": "Shaft", "bdy": 1})), true);
    matches(&threads, &run(&mut s, "model.threads", json!({"body": "Shaft"})), true);
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    strict.execute("solid.cylinder", &json!({"radius": 4, "height": 20, "body_name": "Shaft"})).unwrap_or_else(|e| panic!("{e}"));
    for id in ["model.edges", "model.faces"] {
        let error = strict.execute(id, &json!({"body": 5})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("`body` must be text"), "{id}: {error}");
    }
}

#[test]
fn describe_returns_both_schemas() {
    let mut s = Session::default();
    let d = s.execute("engine.describe", &json!({"command": "solid.box"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["id"], "solid.box");
    assert!(d["input"]["properties"].get("length").is_some());
    assert!(d["output"]["properties"].get("created_bodies").is_some());
    // Its own output schema lists the warnings warn mode can add.
    let own = s.execute("engine.describe", &json!({"command": "engine.describe"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(own["output"]["properties"].get("param_warnings").is_some(), "{own}");
    let d = s.execute("engine.describe", &json!({"command": "solid.emboss"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(d["input"].is_null() && d["output"].is_null(), "unconverted commands have no schemas yet");
    let e = s.execute("engine.describe", &json!({"command": "solid.extrud"})).err().map(|e| e.to_string());
    assert!(e.as_deref().is_some_and(|e| e.contains("did you mean `solid.extrude`")), "{e:?}");
}

#[test]
fn inspect_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DocumentInspectParams>(), &["measure", "sketch_counts"]);
    assert_keys(input_schema_for::<MeasureParams>(), &["bodies", "items", "names_only"]);
    assert_keys(input_schema_for::<CommandsParams>(), &[]);
    assert_keys(input_schema_for::<MaterialListParams>(), &[]);
    assert_keys(input_schema_for::<MaterialAssignParams>(), &["bodies", "material"]);
    assert_keys(input_schema_for::<SketchInspectParams>(), &["sketch"]);
}

#[test]
fn strict_keeps_todays_errors_of_the_inspect_commands() {
    assert_strict_keeps_todays_errors(&[
        ("material.assign", json!({})),
        ("material.assign", json!({"bodies": ["A"]})),
        ("material.assign", json!({"bodies": ["A"], "material": 4})),
        ("material.assign", json!({"bodies": ["A"], "material": null})),
        ("material.assign", json!({"bodies": ["A"], "material": ["Steel"]})),
        ("material.assign", json!({"bodies": 5, "material": "Steel"})),
        ("material.assign", json!({"bodies": [5], "material": "Steel"})),
        ("material.assign", json!({"bodies": [], "material": "Steel"})),
        ("material.assign", json!({"bodies": null, "material": "Steel"})),
        ("material.assign", json!({"material": "Steel"})),
        ("material.assign", json!({"bodies": ["Nope"], "material": "Steel"})),
        ("material.assign", json!({"bodies": ["Nope"], "material": "Unobtainium"})),
        ("inspect.measure", json!({"bodies": 5})),
        ("inspect.measure", json!({"bodies": [5]})),
        ("inspect.measure", json!({"bodies": ["Nope"]})),
        ("inspect.measure", json!({"items": null})),
        ("inspect.measure", json!({"items": "x"})),
        ("inspect.measure", json!({"items": {}})),
        ("inspect.measure", json!({"items": []})),
        ("inspect.measure", json!({"items": "x", "bodies": 5})),
        ("inspect.measure", json!({"items": [1], "bodies": 5, "names_only": "yes"})),
        ("inspect.measure", json!({"items": [{"type": "body", "name": "Nope"}]})),
        ("sketch.inspect", json!({"sketch": true})),
        ("sketch.inspect", json!({"sketch": -1})),
        ("sketch.inspect", json!({"sketch": 1.5})),
        ("sketch.inspect", json!({"sketch": [1]})),
        ("sketch.inspect", json!({"sketch": {}})),
        ("sketch.inspect", json!({"sketch": "Nope"})),
        ("sketch.inspect", json!({"sketch": 99999})),
    ]);
}

/// Each type reads a key as its handler does: with `items` only `items` is read, without it
/// `bodies` and `names_only`; `material.assign` leaves a missing or empty `bodies` and a missing
/// `material` to the handler's own message; `sketch` is an id, a name or `null`.
#[test]
fn the_types_read_each_key_as_the_handler_does() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let none = Vec::<String>::new();
    // inspect.measure: the keys read depend on the form.
    let pick = json!([{"type": "face", "body": "A", "point": [1, 2, 3]}]);
    assert_eq!(issues("inspect.measure", json!({})), none);
    assert_eq!(issues("inspect.measure", json!({"bodies": ["A"], "names_only": true})), none);
    assert_eq!(issues("inspect.measure", json!({"bodies": null, "names_only": null})), none);
    assert_eq!(issues("inspect.measure", json!({"items": pick})), none);
    assert_eq!(issues("inspect.measure", json!({"items": pick, "bodies": 5, "names_only": "yes"})), none);
    assert_eq!(issues("inspect.measure", json!({"bodies": 5})), ["`bodies` must be a list of names; it is 5"]);
    assert_eq!(issues("inspect.measure", json!({"bodies": ["A", 7]})), ["`bodies` must list names; the entry 2 is 7"]);
    assert_eq!(issues("inspect.measure", json!({"names_only": "yes"})), ["`names_only` must be true or false"]);
    assert_eq!(issues("inspect.measure", json!({"items": null})), ["`items` must be a list of selections"]);
    assert_eq!(issues("inspect.measure", json!({"items": "x", "bodies": 5})), ["`items` must be a list of selections"]);
    assert_eq!(issues("inspect.measure", json!({"items": []})), ["measure one or two items"]);
    assert_eq!(issues("inspect.measure", json!({"items": vec![json!({}); 1001]})), ["measure one or two items"]);
    // material.assign.
    assert_eq!(issues("material.assign", json!({"bodies": ["A"], "material": "Steel"})), none);
    assert_eq!(issues("material.assign", json!({"bodies": "A", "material": " steel "})), none);
    assert_eq!(issues("material.assign", json!({"material": "Steel"})), none);
    assert_eq!(issues("material.assign", json!({"bodies": [], "material": "Steel"})), none);
    assert_eq!(issues("material.assign", json!({"bodies": null})), none);
    assert_eq!(issues("material.assign", json!({"bodies": 5, "material": "Steel"})), ["`bodies` must be a list of names; it is 5"]);
    assert_eq!(issues("material.assign", json!({"bodies": ["A"], "material": 4})), ["`material` must be a material name"]);
    assert_eq!(issues("material.assign", json!({"bodies": ["A"], "material": null})), ["`material` must be a material name"]);
    // sketch.inspect.
    for ok in [json!({}), json!({"sketch": null}), json!({"sketch": 3}), json!({"sketch": "Base"}), json!({"sketch": "7"})] {
        assert_eq!(issues("sketch.inspect", ok.clone()), none, "{ok}");
    }
    for bad in [json!(true), json!(-1), json!(1.5), json!([1]), json!({"id": 1})] {
        assert_eq!(issues("sketch.inspect", json!({ "sketch": bad.clone() })), ["`sketch` must be an id or a name"], "{bad}");
    }
    // document.inspect's flags are plain booleans, `null` is left out.
    assert_eq!(issues("document.inspect", json!({"measure": true, "sketch_counts": null})), none);
    assert_eq!(issues("document.inspect", json!({"measure": "yes"})), ["`measure` must be true or false"]);
    // Neither list command takes a key.
    assert_eq!(issues("engine.commands", json!({"x": 1})), ["unknown key `x`"]);
    assert_eq!(issues("material.list", json!({"x": 1})), ["unknown key `x`"]);
}

/// The schemas say what the handlers need: `material.assign` can't do without `bodies` and
/// `material`, though its type leaves them to the handler's message; `inspect.measure` takes
/// bodies or items.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<MaterialAssignParams>(),
        &[
            (json!({"bodies": ["A"], "material": "Steel"}), true),
            (json!({"bodies": "A", "material": "Steel"}), true),
            (json!({"material": "Steel"}), false),
            (json!({"bodies": ["A"]}), false),
            (json!({"bodies": [], "material": "Steel"}), false),
            (json!({"bodies": ["A"], "material": 4}), false),
        ],
    );
    takes(
        input_schema_for::<MeasureParams>(),
        &[
            (json!({}), true),
            (json!({"bodies": ["A"], "names_only": true}), true),
            (json!({"bodies": "A"}), true),
            (json!({"bodies": null}), true),
            (json!({"items": [{"type": "body", "name": "A"}]}), true),
            (json!({"items": []}), false),
            (json!({"items": "x"}), false),
            (json!({"bodies": 5}), false),
            (json!({"names_only": "yes"}), false),
        ],
    );
    takes(
        input_schema_for::<SketchInspectParams>(),
        &[
            (json!({}), true),
            (json!({"sketch": 3}), true),
            (json!({"sketch": "Base"}), true),
            (json!({"sketch": null}), true),
            (json!({"sketch": -1}), false),
            (json!({"sketch": true}), false),
        ],
    );
    takes(input_schema_for::<DocumentInspectParams>(), &[(json!({"measure": true, "sketch_counts": false}), true), (json!({"measure": 1}), false)]);
    takes(input_schema_for::<CommandsParams>(), &[(json!({}), true), (json!({"x": 1}), false)]);
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A reply matches its output schema and, in Warn, carries `param_warnings` only when told to.
fn matches<T: schemars::JsonSchema>(id: &str, reply: &Value, warned: bool) {
    let schema = output_schema_for::<T>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert_eq!(reply.get("param_warnings").is_some(), warned, "{id}: {reply}");
}

/// A design with a box, a cylinder, a sheet metal body, a sketch of every curve kind with a
/// dimension, and a sketch that projects a face: returns the first sketch's feature id.
fn design(s: &mut Session) -> u64 {
    run(s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"}));
    run(s, "solid.cylinder", json!({"radius": 4, "height": 20, "base": [40, 0, 0], "body_name": "Shaft"}));
    run(s, "material.create", json!({"name": "Alloy", "density_g_cm3": 2.5, "color": "#ff0000"}));
    run(
        s,
        "material.create",
        json!({"name": "Painted", "density_g_cm3": 1.5, "color": "#00ff00", "pbr": {"metallic": 0.5, "roughness": 0.5, "dielectric_ior": 1.5}}),
    );
    run(s, "sheet.manage_rules", json!({"name": "R1", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [60, 0], "p1": [100, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
    run(s, "sketch.create", json!({"plane": "XZ", "name": "Shapes"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 20]]}));
    run(s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    run(s, "sketch.circle.center", json!({"center": [10, 10], "radius": 1}));
    run(s, "sketch.arc.three_point", json!({"start": [50, 0], "end": [60, 10], "through": [58, 3]}));
    run(s, "sketch.ellipse", json!({"center": [-30, 0], "major": [-20, 0], "minor_radius": 5}));
    run(s, "sketch.spline.fit_point", json!({"points": [[0, 40], [10, 50], [20, 40], [30, 50]]}));
    run(s, "sketch.spline.control_point", json!({"points": [[0, 60], [10, 70], [20, 60], [30, 70]]}));
    run(s, "sketch.conic", json!({"start": [0, 80], "end": [30, 80], "apex": [15, 100], "rho": 0.4}));
    run(s, "sketch.dimension", json!({"entities": ["c1"]}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(s, "sketch.construction", json!({"curves": ["l2"]}));
    let id = run(s, "sketch.inspect", json!({}))["id"].as_u64().unwrap_or_default();
    run(s, "sketch.finish", json!({}));
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Top"}));
    run(s, "sketch.project", json!({"refs": [{"face": [5, 10, 30], "body": "Block"}]}));
    run(s, "sketch.finish", json!({}));
    id
}

/// Real calls of `document.inspect` in Warn, each form: the replies match the output schema
/// without `param_warnings`, and say what the design has.
#[test]
fn document_inspect_replies_match_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let shapes = design(&mut s);
    run(&mut s, "sketch.edit", json!({"sketch": shapes}));
    for p in [
        json!({}),
        json!({"measure": true}),
        json!({"sketch_counts": true}),
        json!({"measure": true, "sketch_counts": true}),
        json!({"measure": false, "sketch_counts": null}),
    ] {
        let r = run(&mut s, "document.inspect", p.clone());
        matches::<DocumentInspectResult>("document.inspect", &r, false);
        assert_eq!(r["active_sketch"], json!(shapes), "{r}");
        assert_eq!(r["bodies"].as_array().map(Vec::len), Some(3), "{p}: {r}");
        assert_eq!(r["bodies"][0].get("area_mm2").is_some(), p["measure"] == true, "{p}: {r}");
        assert_eq!(r["bodies"][0].get("feature").is_some(), p["measure"] != true, "{p}: {r}");
        assert_eq!(r["sketches"][0].get("plane").is_some(), p["sketch_counts"] == true, "{p}: {r}");
    }
    let r = run(&mut s, "document.inspect", json!({"measure": true}));
    assert!(r["params"].as_array().is_some_and(|p| p.iter().any(|x| x["name"].is_string() && x["model"] == true)), "{r}");
    assert!(r["timeline"].as_array().is_some_and(|t| t.iter().any(|x| x["type"].is_string() && x["ms"].is_number())), "{r}");
    run(&mut s, "browser.visibility", json!({"items": ["Shaft"], "visible": false}));
    let r = run(&mut s, "document.inspect", json!({}));
    matches::<DocumentInspectResult>("document.inspect", &r, false);
    assert_eq!(r["bodies"][1]["visible"], false, "{r}");
    // A body that a rolled-back later feature deletes says so.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.remove", json!({"bodies": ["Shaft"]}));
    run(&mut s, "timeline.roll_to", json!({"feature": "Shapes"}));
    run(&mut s, "sketch.edit", json!({"sketch": shapes}));
    let r = run(&mut s, "document.inspect", json!({}));
    matches::<DocumentInspectResult>("document.inspect", &r, false);
    assert!(r["bodies"].as_array().is_some_and(|b| b.iter().any(|b| b["removed_later"] == true)), "{r}");
}

/// `inspect.measure` in each form: bodies (with sheet metal and a material), names only, one
/// item and two, in Warn without warnings. Everything it ignores with `items` is ignored.
#[test]
fn measure_replies_match_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let shapes = design(&mut s);
    run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Alloy"}));
    let id = "inspect.measure";
    let all = run(&mut s, id, json!({}));
    matches::<MeasureResult>(id, &all, false);
    assert_eq!(all["body_count"], 3, "{all}");
    assert_eq!(all["bodies"][0]["material"], "Alloy", "{all}");
    assert!((all["bodies"][0]["mass_g"].as_f64().unwrap_or_default() - 15.0).abs() < 1e-6, "{all}");
    assert!(all["bodies"].as_array().is_some_and(|b| b.iter().any(|b| b["sheet_metal"] == true)), "{all}");
    assert_eq!(all["total"]["open_bodies_skipped"], 0, "{all}");
    for p in [json!({"bodies": ["Block"]}), json!({"bodies": "Shaft", "names_only": false}), json!({"bodies": null})] {
        matches::<MeasureResult>(id, &run(&mut s, id, p), false);
    }
    let names = run(&mut s, id, json!({"names_only": true}));
    assert_eq!(names["bodies"][0], json!({"name": "Block", "definition": "Block"}), "{names}");
    matches::<MeasureResult>(id, &names, false);
    matches::<MeasureResult>(id, &run(&mut s, id, json!({"bodies": ["Block"], "names_only": true})), false);

    let face = |body: &str, p: [i32; 3]| json!({"type": "face", "body": body, "point": p});
    let curve = |c: &str| json!({"type": "sketch_curve", "id": c, "sketch": shapes});
    for items in [
        json!([face("Block", [5, 10, 30])]),
        json!([face("Shaft", [44, 0, 10])]),
        json!([face("Block", [5, 10, 30]), face("Block", [5, 10, 0])]),
        json!([face("Block", [5, 10, 30]), face("Block", [10, 10, 15])]),
        json!([face("Shaft", [44, 0, 10]), face("Block", [5, 10, 30])]),
        json!([{"type": "edge", "body": "Block", "point": [5, 0, 30]}, {"type": "edge", "body": "Block", "point": [5, 20, 30]}]),
        json!([{"type": "body", "name": "Block"}]),
        json!([{"type": "body", "name": "Block"}, {"type": "body", "name": "Shaft"}]),
        json!([{"type": "vertex", "body": "Block", "point": [0, 0, 0]}, {"type": "vertex", "body": "Block", "point": [10, 20, 30]}]),
        json!([curve("c1")]),
        json!([curve("a1")]),
        json!([curve("l1")]),
        json!([curve("c1"), curve("c2")]),
        json!([curve("l1"), curve("l2")]),
        json!([curve("c1"), curve("l1")]),
        json!([curve("a1"), face("Block", [5, 10, 30])]),
    ] {
        let r = run(&mut s, id, json!({ "items": items }));
        matches::<MeasureResult>(id, &r, false);
        assert!(r["items"].as_array().is_some_and(|i| i.len() == items.as_array().map_or(0, Vec::len)), "{r}");
    }
    let one = run(&mut s, id, json!({"items": [curve("c1")]}));
    assert!(one["diameter"] == 6.0 && one["circumference"].is_number() && one["center"].is_array(), "{one}");
    let arc = run(&mut s, id, json!({"items": [curve("a1")]}));
    assert!(arc["arc_length"].is_number() && arc["angle_deg"].is_number() && arc.get("circumference").is_none(), "{arc}");
    let two = run(&mut s, id, json!({"items": [face("Block", [5, 10, 30]), face("Block", [5, 10, 0])]}));
    assert!(two["distance_mm"] == 30.0 && two["min"] == 30.0 && two["measure"] == "min" && two["angle_deg"].is_number(), "{two}");
    let pair = run(&mut s, id, json!({"items": [curve("c1"), curve("c2")]}));
    assert!(pair["measure"] == "center" && pair["center_distance"].is_number(), "{pair}");
    // With `items`, `bodies` and `names_only` are not read at all.
    let r = run(&mut s, id, json!({"items": [face("Block", [5, 10, 30])], "bodies": 7, "names_only": "yes"}));
    matches::<MeasureResult>(id, &r, false);
}

/// `sketch.inspect` of every curve kind, a dimension and a projected link, by id, by name and as
/// the sketch being edited, in Warn without warnings.
#[test]
fn sketch_inspect_replies_match_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let shapes = design(&mut s);
    let id = "sketch.inspect";
    for p in [json!({"sketch": shapes}), json!({"sketch": "Shapes"}), json!({"sketch": "Top"}), json!({"sketch": "Base"})] {
        matches::<SketchInspected>(id, &run(&mut s, id, p), false);
    }
    let r = run(&mut s, id, json!({"sketch": "Shapes"}));
    let kinds: Vec<&str> = r["curves"].as_array().into_iter().flatten().filter_map(|c| c["type"].as_str()).collect();
    for kind in ["line", "circle", "arc", "ellipse", "spline", "conic"] {
        assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
    }
    assert!(
        r["constraints"].as_array().is_some_and(|c| c.iter().any(|c| c["param"].is_string() && c["expression"].is_string() && c["type"].is_string())),
        "{r}"
    );
    assert!(r["profiles"][0]["holes"].is_array() && r["points"][0]["link"].is_null(), "{r}");
    let top = run(&mut s, id, json!({"sketch": "Top"}));
    assert!(top["links"][0]["id"].is_string() && top["curves"][0]["link"].is_string(), "{top}");
    run(&mut s, "sketch.edit", json!({"sketch": "Shapes"}));
    for p in [json!({}), json!({"sketch": null}), json!({"sketch": shapes.to_string()})] {
        let r = run(&mut s, id, p);
        matches::<SketchInspected>(id, &r, false);
        assert_eq!(r["name"], "Shapes", "{r}");
    }
}

/// `material.assign` (a name, a list, the default material) and `material.list` (built-in
/// materials and the design's, with and without a look) in Warn without warnings.
#[test]
fn material_replies_match_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    design(&mut s);
    let id = "material.assign";
    let r = run(&mut s, id, json!({"bodies": ["Block"], "material": "alloy"}));
    assert_eq!(r, json!({"bodies": ["Block"], "material": "Alloy"}));
    matches::<MaterialAssignResult>(id, &r, false);
    let r = run(&mut s, id, json!({"bodies": "Shaft", "material": " steel "}));
    assert_eq!(r["material"], "Steel", "{r}");
    matches::<MaterialAssignResult>(id, &r, false);
    matches::<MaterialAssignResult>(id, &run(&mut s, id, json!({"bodies": ["Block", "Shaft"], "material": "Painted"})), false);
    matches::<MaterialAssignResult>(id, &run(&mut s, id, json!({"bodies": ["Block"], "material": "Steel"})), false);

    let listed = run(&mut s, "material.list", json!({}));
    matches::<MaterialListResult>("material.list", &listed, false);
    let materials = listed["materials"].as_array().cloned().unwrap_or_default();
    assert_eq!(materials[0].as_object().map(|m| m.len()), Some(2), "{listed}");
    let mine = |name: &str| materials.iter().find(|m| m["name"] == name).cloned().unwrap_or_default();
    assert!(mine("Alloy")["user"] == true && mine("Alloy")["appearance"]["color"] == "#ff0000", "{listed}");
    assert!(mine("Painted")["appearance"]["pbr"]["metallic"] == 0.5, "{listed}");
    assert!(mine("Steel").get("user").is_none() && mine("Steel").get("appearance").is_none(), "{listed}");
    run(&mut s, "material.create", json!({"name": "Bare", "density_g_cm3": 1.0}));
    let listed = run(&mut s, "material.list", json!({}));
    matches::<MaterialListResult>("material.list", &listed, false);
    let bare = listed["materials"].as_array().into_iter().flatten().find(|m| m["name"] == "Bare").cloned().unwrap_or_default();
    assert!(bare["appearance"].is_null() && bare["user"] == true, "{listed}");
}

/// `engine.commands` replies with a list, which carries no `param_warnings`: Strict is where a
/// stray key is refused. The list has every command, and each entry matches its schema.
#[test]
fn the_command_list_matches_its_schema() {
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let list = run(&mut strict, "engine.commands", json!({}));
    let schema = output_schema_for::<Vec<CommandEntry>>();
    assert!(conforms(schema.as_value(), schema.as_value(), &list), "{list}");
    assert_eq!(list.as_array().map(Vec::len), Some(crate::command_specs().len()));
    assert!(list.as_array().is_some_and(|l| l.iter().any(|c| c["id"] == "engine.commands" && c["enabled"] == true && c["shortcut"].is_null())));
    assert!(list.as_array().is_some_and(|l| l.iter().any(|c| c["id"] == "inspect.measure" && c["shortcut"] == "I")));
    let error = strict.execute("engine.commands", &json!({"lst": 1})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("unknown key `lst`"), "{error}");
    let mut warn = Session { param_check: ParamCheck::Warn, ..Session::default() };
    assert!(run(&mut warn, "engine.commands", json!({"lst": 1})).is_array());
}

/// A misspelt key runs in Warn and is reported, with a reply that still matches its schema.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let shapes = design(&mut s);
    let schemas = |id: &str| match id {
        "document.inspect" => output_schema_for::<DocumentInspectResult>(),
        "inspect.measure" => output_schema_for::<MeasureResult>(),
        "material.assign" => output_schema_for::<MaterialAssignResult>(),
        "material.list" => output_schema_for::<MaterialListResult>(),
        _ => output_schema_for::<SketchInspected>(),
    };
    for (id, p, suggestion) in [
        ("document.inspect", json!({"mesure": true}), "measure"),
        ("document.inspect", json!({"sketch_count": true}), "sketch_counts"),
        ("inspect.measure", json!({"bodes": ["Block"]}), "bodies"),
        ("inspect.measure", json!({"name_only": true}), "names_only"),
        ("material.assign", json!({"bodies": ["Block"], "materal": "Steel", "material": "Steel"}), "material"),
        ("material.list", json!({"materials": true}), ""),
        ("sketch.inspect", json!({"sketch": shapes, "sketchs": 1}), "sketch"),
    ] {
        let r = run(&mut s, id, p.clone());
        let warnings: Vec<&str> = r["param_warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        assert_eq!(warnings.len(), 1, "{id} {p}: {r}");
        assert!(warnings[0].starts_with("unknown key `") && warnings[0].contains(suggestion), "{id}: {warnings:?}");
        let schema = schemas(id);
        assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r}");
    }
}

/// Shapes the real designs above don't reach: a body whose measurement failed (a sheet metal
/// one carries its corrected counts) and the null coordinates of two planes that meet.
#[test]
fn rare_reply_shapes_serialize_as_before_and_match_the_schema() {
    use super::{BodyFailure, BodyMeasurement, BodyOutcome, Measured, MeasuredItems};
    let failed = BodyMeasurement {
        name: "B".into(),
        outcome: BodyOutcome::Failed(BodyFailure { error: "boom".into(), faces: Some(2), edges: None, vertices: None }),
    };
    assert_eq!(serde_json::to_value(&failed).unwrap_or_default(), json!({"name": "B", "error": "boom", "faces": 2}));
    let items = MeasuredItems { from: Some([f64::NAN, 0.0, 0.0]), distance_mm: Some(0.0), ..MeasuredItems::default() };
    let reply = serde_json::to_value(MeasureResult { measured: Measured::Items(Box::new(items)), param_warnings: Vec::new() }).unwrap_or_default();
    assert_eq!(reply["from"], json!([null, 0.0, 0.0]));
    let schema = output_schema_for::<MeasureResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
}

/// #11945: each body lists the features that built it, in timeline order: its own box, the fillet
/// that changed it, and the cutting box a Combine consumed into it with that Combine. Another body
/// keeps only its own. Rolled back before the Combine, neither it nor the tool's box are listed.
#[test]
fn body_features_list_what_built_each_body() {
    let schema = output_schema_for::<ModelBodyFeatures>();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let id = |r: Value| r["feature"].as_u64().unwrap_or_else(|| panic!("{r}"));
    let a = id(run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "A"})));
    let b =
        id(run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "operation": "new", "body_name": "B"})));
    let fillet = id(run(&mut s, "solid.fillet", json!({"body": "A", "edges": [[10, 0, 10]], "radius": 1})));
    let tool =
        id(run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 20, "corner": [5, 5, -5], "operation": "new", "body_name": "T"})));
    let combine = id(run(&mut s, "solid.combine", json!({"target": "A", "tools": ["T"], "operation": "cut"})));

    let listed = |r: &Value, body: &str| -> Vec<u64> {
        let entry = r["bodies"].as_array().into_iter().flatten().find(|e| e["body"] == body).unwrap_or_else(|| panic!("no {body} in {r}"));
        entry["features"].as_array().into_iter().flatten().filter_map(|f| f["id"].as_u64()).collect()
    };
    let r = run(&mut s, "model.body_features", json!({}));
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    assert_eq!(listed(&r, "A"), vec![a, fillet, tool, combine], "{r}");
    assert_eq!(listed(&r, "B"), vec![b], "{r}");
    assert_eq!(r["bodies"].as_array().map(Vec::len), Some(2), "the consumed tool is gone: {r}");
    assert_eq!(r["bodies"][0]["features"][1]["name"], s.doc.feature(fillet).map(|f| f.name.clone()).unwrap_or_default().as_str());

    // One body, and the Session data the browser reads agree.
    let one = run(&mut s, "model.body_features", json!({"body": "B"}));
    assert_eq!((one["bodies"].as_array().map(Vec::len), listed(&one, "B")), (Some(1), vec![b]), "{one}");
    assert_eq!(s.body_features().get("A"), Some(&vec![a, fillet, tool, combine]));

    run(&mut s, "timeline.rollback", json!({"position": 3}));
    assert_eq!(s.body_features().get("A"), Some(&vec![a, fillet]));
    assert_eq!(s.body_features().get("B"), Some(&vec![b]));

    // A misspelt key warns; an unknown body is refused.
    let warned = run(&mut s, "model.body_features", json!({"bdy": "A"}));
    assert!(warned.get("param_warnings").is_some(), "{warned}");
    assert!(s.execute("model.body_features", &json!({"body": "Nope"})).is_err());
}
