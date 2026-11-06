use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::joints::JointKind;

use super::{
    AsBuiltJointParams, CreateJointParams, DeleteJointParams, DeleteJointResult, DriveJointParams, DriveJointResult, EditJointParams,
    InspectJointParams, InspectJointResult, JointAddedResult, JointKindName, JointLimitsParams, ListJointsParams, ListJointsResult,
    SolveJointsParams, SolveResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EditJointParams>(), &["a", "angle", "b", "flip", "joint", "name", "offset", "quiet", "suppressed", "type"]);
    assert_keys(
        input_schema_for::<CreateJointParams>(),
        &["a", "angle", "b", "flip", "limits", "name", "offset", "quiet", "type", "value", "values"],
    );
    assert_keys(input_schema_for::<AsBuiltJointParams>(), &["a", "at", "b", "limits", "name", "quiet", "type", "value", "values"]);
    assert_keys(input_schema_for::<DriveJointParams>(), &["continuous", "index", "joint", "quiet", "value", "values"]);
    assert_keys(input_schema_for::<JointLimitsParams>(), &["index", "joint", "max", "min", "quiet"]);
    assert_keys(input_schema_for::<DeleteJointParams>(), &["joint", "link", "origin", "quiet"]);
    assert_keys(input_schema_for::<ListJointsParams>(), &["quiet"]);
    assert_keys(input_schema_for::<InspectJointParams>(), &["joint"]);
    assert_keys(input_schema_for::<SolveJointsParams>(), &["quiet"]);
}

#[test]
fn the_commands_publish_their_schemas() {
    for id in ["joint.create", "joint.as_built", "joint.drive", "joint.limits", "joint.delete", "joint.list", "joint.inspect", "joint.solve"] {
        let spec = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id}"));
        assert!(spec.contract.is_some() && spec.output.is_some(), "{id}");
    }
}

/// The replies name a joint's type as the document does.
#[test]
fn every_joint_type_has_its_reply_name() {
    let names = output_schema_for::<JointKindName>();
    let names: Vec<&str> = names.as_value()["oneOf"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(names.as_value()["enum"].as_array().into_iter().flatten())
        .filter_map(|v| v.get("const").or(Some(v)).and_then(Value::as_str))
        .collect();
    for spelling in [
        "rigid",
        "revolute",
        "slider",
        "cylindrical",
        "pin_slot",
        "planar",
        "ball",
        "parallel",
        "perpendicular",
        "angle",
        "point_on_plane",
        "point_on_line",
        "line_on_plane",
    ] {
        let kind = JointKind::parse(spelling).unwrap_or_else(|| panic!("{spelling}"));
        assert_eq!(serde_json::to_value(kind).ok(), Some(json!(spelling)));
        assert!(names.contains(&spelling), "{spelling} in {names:?}");
    }
}

#[test]
fn strict_keeps_todays_errors() {
    let at = |o: u64, x: f64| json!({"occurrence": o, "point": [x, 0, 0]});
    assert_strict_keeps_todays_errors(&[
        ("joint.create", json!({})),
        ("joint.create", json!({"type": "nope"})),
        ("joint.create", json!({"type": 5})),
        ("joint.create", json!({"type": null})),
        ("joint.create", json!({"type": "rigid"})),
        ("joint.create", json!({"type": "rigid", "a": 5, "b": at(0, 1.0)})),
        ("joint.create", json!({"type": "rigid", "a": null, "b": at(0, 1.0)})),
        ("joint.create", json!({"type": "rigid", "a": {}, "b": at(0, 1.0)})),
        ("joint.create", json!({"type": "rigid", "a": {"frame": 1}, "b": at(0, 1.0)})),
        ("joint.create", json!({"type": "rigid", "a": {"face": [1, 2]}, "b": at(0, 1.0)})),
        ("joint.create", json!({"type": "rigid", "a": at(0, 0.0)})),
        ("joint.create", json!({"type": "rigid", "a": at(0, 0.0), "b": at(0, 1.0), "offset": true})),
        ("joint.create", json!({"type": "rigid", "a": at(0, 0.0), "b": at(0, 1.0), "offset": null})),
        ("joint.create", json!({"type": "rigid", "a": at(0, 0.0), "b": at(0, 1.0), "angle": [1]})),
        ("joint.create", json!({"type": "revolute", "a": at(0, 0.0), "b": at(0, 1.0), "values": [true]})),
        ("joint.create", json!({"type": "revolute", "a": at(0, 0.0), "b": at(0, 1.0), "value": {}})),
        ("joint.create", json!({"type": "revolute", "a": at(0, 0.0), "b": at(0, 1.0), "values": [1e12]})),
        ("joint.create", json!({"type": "revolute", "a": at(0, 0.0), "b": at(0, 1.0), "limits": [[true, 1]]})),
        ("joint.create", json!({"type": "revolute", "a": at(0, 0.0), "b": at(0, 1.0), "limits": [[0, 1e12]]})),
        ("joint.as_built", json!({})),
        ("joint.as_built", json!({"type": "rigid"})),
        ("joint.as_built", json!({"type": "rigid", "a": true, "b": 0})),
        ("joint.as_built", json!({"type": "rigid", "a": 0, "b": null})),
        ("joint.as_built", json!({"type": "rigid", "a": [1], "b": 0})),
        ("joint.as_built", json!({"type": "rigid", "a": 0, "b": 0, "at": null})),
        ("joint.as_built", json!({"type": "rigid", "a": 0, "b": 0, "at": 3})),
        ("joint.as_built", json!({"type": "rigid", "a": 0, "b": 0, "at": {"frame": {}}})),
        ("joint.as_built", json!({"type": "revolute", "a": 0, "b": 0, "limits": [[true, 1]]})),
        ("joint.drive", json!({})),
        ("joint.drive", json!({"joint": true})),
        ("joint.drive", json!({"joint": null})),
        ("joint.drive", json!({"joint": [1]})),
        ("joint.limits", json!({})),
        ("joint.limits", json!({"joint": {}})),
        ("joint.delete", json!({})),
        ("joint.delete", json!({"joint": true})),
        ("joint.delete", json!({"joint": null})),
        ("joint.delete", json!({"origin": "nothing"})),
        ("joint.delete", json!({"link": 4})),
        ("joint.delete", json!({"link": null})),
        ("joint.inspect", json!({"joint": true})),
        ("joint.inspect", json!({"joint": [1]})),
    ]);
}

/// A design with a grounded plate and a hinged leaf, a joint origin, and a third part joined as
/// built, for the calls that need joints to fail on.
fn hinged(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    for name in ["LeafA", "LeafB"] {
        run("component.activate", json!({"component": "root"}));
        let made = run("component.create", json!({"name": name, "grounded": false}));
        run("solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
        run("solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
        if name == "LeafA" {
            run("occurrence.ground", json!({"occurrence": made["occurrence"], "grounded": true}));
        }
    }
    run(
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]},
            "flip": true, "name": "Hinge", "limits": [[-90, 90]]}),
    );
    run("joint.origin", json!({"occurrence": "LeafA", "face": [20, 15, 3], "name": "Top"}));
    s
}

/// What the handlers refuse, with a joint in the design to refuse it for: strict says the same.
#[test]
fn strict_keeps_todays_errors_with_joints() {
    let cases = [
        ("joint.drive", json!({"joint": "Hinge"})),
        ("joint.drive", json!({"joint": "Hinge", "value": true})),
        ("joint.drive", json!({"joint": "Hinge", "values": [true]})),
        ("joint.drive", json!({"joint": "Hinge", "values": [1e12]})),
        ("joint.drive", json!({"joint": "Hinge", "index": null, "value": 5})),
        ("joint.drive", json!({"joint": "Hinge", "index": "0", "value": 5})),
        ("joint.drive", json!({"joint": "Hinge", "index": -1, "value": 5})),
        ("joint.drive", json!({"joint": "Hinge", "index": 0, "values": [5]})),
        ("joint.drive", json!({"joint": "Hinge", "index": 0, "values": null, "value": 5})),
        ("joint.drive", json!({"joint": "Hinge", "index": 0, "value": null})),
        ("joint.drive", json!({"joint": "Hinge", "index": 0, "value": "1 +"})),
        ("joint.limits", json!({"joint": "Hinge", "min": 1})),
        ("joint.limits", json!({"joint": "Hinge", "max": 1})),
        ("joint.limits", json!({"joint": "Hinge", "min": null, "max": 1})),
        ("joint.limits", json!({"joint": "Hinge", "min": 1, "max": true})),
        ("joint.limits", json!({"joint": "Hinge", "min": true, "max": 1})),
        ("joint.limits", json!({"joint": "Hinge", "min": 1e12, "max": 1})),
        ("joint.limits", json!({"joint": "Hinge", "index": 3, "min": 1, "max": 2})),
        ("joint.delete", json!({"joint": "Hinge", "origin": "Missing"})),
        ("joint.delete", json!({"link": 7, "joint": "Hinge"})),
        ("joint.create", json!({"type": "rigid", "a": "Top", "b": "Top"})),
        ("joint.create", json!({"type": "rigid", "a": "Missing", "b": "Top"})),
        ("joint.as_built", json!({"type": "rigid", "a": "LeafA", "b": "LeafA"})),
        ("joint.as_built", json!({"type": "rigid", "a": "LeafA", "b": "Missing"})),
        ("joint.inspect", json!({"joint": "Missing"})),
    ];
    for (id, p) in cases {
        let off = hinged(ParamCheck::Off).execute(id, &p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(hinged(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()), off, "{id} {p}");
    }
}

fn issues(id: &str, p: Value) -> Vec<String> {
    crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect()
}

/// The types read each key as its handler does, and in the cases the handler reads it.
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let none = Vec::<String>::new;
    let sides = json!({"a": {"occurrence": 0, "point": [0, 0, 0]}, "b": "Origin"});
    let create = |extra: Value| {
        let mut p = json!({"type": "revolute"});
        p.as_object_mut().into_iter().for_each(|o| {
            o.extend(sides.as_object().cloned().unwrap_or_default());
            o.extend(extra.as_object().cloned().unwrap_or_default());
        });
        issues("joint.create", p)
    };
    // The forms of a side, and the key left out, which the handler refuses with its own message.
    assert_eq!(create(json!({})), none());
    for side in [
        json!("Name"),
        json!({"face": "F1"}),
        json!({"circle": "F1", "occurrence": "A"}),
        json!({"face": [0, 0, 0]}),
        json!({"edge": {"x": 0, "y": 0, "z": 0}, "z": [0, 0, 1]}),
        json!({"point": [1, 2, 3], "x": [1, 0, 0]}),
        json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}),
    ] {
        assert_eq!(create(json!({"a": side})), none(), "{side}");
    }
    assert_eq!(issues("joint.create", json!({"type": "rigid"})), none());
    let why = "a joint origin needs face, circle, edge, point or frame [x, y, z], or a face's name";
    for side in [json!(5), json!(null), json!([1, 2, 3]), json!({}), json!({"face": [1, 2]}), json!({"face": 7})] {
        assert_eq!(create(json!({"a": side})), [why], "{side}");
    }
    assert_eq!(create(json!({"b": {"frame": {"z": [0, 0, 1]}}})), ["`frame` needs origin and z"]);
    // The type: any spelling `JointKind::parse` takes.
    for t in ["Revolute", "pin-slot", "POINT ON PLANE", "rotate", "square"] {
        assert_eq!(issues("joint.create", json!({"type": t, "a": "A", "b": "B"})), none(), "{t}");
    }
    assert_eq!(issues("joint.create", json!({"type": "hinge", "a": "A", "b": "B"})).len(), 1);
    // `value` is read only when `values` is not a list; `values` and `limits` only as lists.
    assert_eq!(create(json!({"values": [1, "w / 2"], "value": true})), none());
    assert_eq!(create(json!({"value": true})), ["a joint value must be a number or an expression"]);
    assert_eq!(create(json!({"value": "5 mm"})), none());
    assert_eq!(create(json!({"values": null, "value": 4})), none());
    assert_eq!(create(json!({"values": [true]})), ["a joint value must be a number or an expression"]);
    assert_eq!(create(json!({"values": [1e12]})), ["a joint value must be a number between -1e9 and 1e9"]);
    assert_eq!(create(json!({"values": 5})), ["`values` must be a list of joint values"]);
    for limits in [json!([[0, 90]]), json!([null, ["-1 mm", "w"]]), json!([[1]]), json!([[]]), json!([[0, 1, true]]), json!(null), json!([])] {
        assert_eq!(create(json!({"limits": limits})), none(), "{limits}");
    }
    for limits in [json!([[true, 1]]), json!([[0, {}]]), json!([5]), json!({"a": 1}), json!("x")] {
        assert_eq!(create(json!({"limits": limits})).len(), 1, "{limits}");
    }
    // Numbers: `offset` and `angle` read `null` as a value, the flags and the name as absent.
    assert_eq!(create(json!({"offset": "w", "angle": 30, "flip": true, "name": "J", "quiet": false})), none());
    assert_eq!(create(json!({"offset": null})), ["a joint value must be a number or an expression"]);
    assert_eq!(create(json!({"angle": {}})), ["a joint value must be a number or an expression"]);
    assert_eq!(create(json!({"flip": null, "name": null, "quiet": null})), none());
    assert_eq!(create(json!({"flip": "yes"})), ["`flip` must be true or false"]);
    assert_eq!(create(json!({"name": 3})), ["`name` must be text"]);
    assert_eq!(create(json!({"quiet": 1})), ["`quiet` must be true or false"]);
    assert_eq!(create(json!({"nme": "J"})), ["unknown key `nme`; did you mean `name`?"]);

    // joint.as_built: occurrences by id or name, `at` a joint origin, `values` only by being there.
    let built = |extra: Value| {
        let mut p = json!({"type": "rigid", "a": 0, "b": "Part"});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        issues("joint.as_built", p)
    };
    assert_eq!(built(json!({})), none());
    assert_eq!(built(json!({"at": {"face": "F"}, "name": "N", "limits": [[0, 1]], "quiet": true, "values": "anything", "value": {"x": 1}})), none());
    assert_eq!(built(json!({"a": true})), ["an occurrence must be an id or a name"]);
    assert_eq!(built(json!({"b": null})), ["an occurrence must be an id or a name"]);
    assert_eq!(built(json!({"at": null})), [why]);
    assert_eq!(built(json!({"at": []})), [why]);
    assert_eq!(issues("joint.as_built", json!({"type": "rigid"})), none());

    // joint.drive: one coordinate with `index`, else `value` or a list of `values`.
    let drive = |p: Value| {
        let mut all = json!({"joint": "Hinge"});
        all.as_object_mut().into_iter().for_each(|o| o.extend(p.as_object().cloned().unwrap_or_default()));
        issues("joint.drive", all)
    };
    assert_eq!(drive(json!({"value": 45})), none());
    assert_eq!(drive(json!({"value": "pi rad", "quiet": true, "continuous": true})), none());
    assert_eq!(drive(json!({"values": [10, "w"]})), none());
    assert_eq!(drive(json!({"values": [10], "value": true})), none());
    assert_eq!(drive(json!({"values": null, "value": 5})), none());
    assert_eq!(drive(json!({})), none());
    assert_eq!(drive(json!({"index": 1, "value": 5})), none());
    assert_eq!(drive(json!({"index": 1, "value": 5, "values": [1]})), ["index goes with value, not values"]);
    assert_eq!(drive(json!({"index": 1, "values": null})), ["index goes with value, not values"]);
    assert_eq!(drive(json!({"index": 1})), none());
    assert_eq!(drive(json!({"index": "1", "value": 5})), ["index is outside this joint's coordinates"]);
    assert_eq!(drive(json!({"index": null, "value": 5})), ["index is outside this joint's coordinates"]);
    assert_eq!(drive(json!({"index": -1, "value": 5})), ["index is outside this joint's coordinates"]);
    assert_eq!(drive(json!({"index": 0, "value": true})), ["a joint value must be a number or an expression"]);
    assert_eq!(drive(json!({"value": null})), ["a joint value must be a number or an expression"]);
    assert_eq!(drive(json!({"values": [null]})), ["a joint value must be a number or an expression"]);
    assert_eq!(drive(json!({"values": "x"})), ["`values` must be a list of joint values"]);
    assert_eq!(issues("joint.drive", json!({"value": 1})), none());
    assert_eq!(issues("joint.drive", json!({"joint": null, "value": 1})), ["`joint` must be an id or a name"]);
    assert_eq!(issues("joint.drive", json!({"joint": 3, "value": 1})), none());

    // joint.limits: both or neither, the minimum read first; `index` a number.
    let limits = |p: Value| {
        let mut all = json!({"joint": 2});
        all.as_object_mut().into_iter().for_each(|o| o.extend(p.as_object().cloned().unwrap_or_default()));
        issues("joint.limits", all)
    };
    assert_eq!(limits(json!({})), none());
    assert_eq!(limits(json!({"min": -90, "max": "w / 2", "index": 0, "quiet": true})), none());
    assert_eq!(limits(json!({"index": -3.5})), none());
    assert_eq!(limits(json!({"min": 1})), ["give both `min` and `max`, or neither to clear"]);
    assert_eq!(limits(json!({"max": null})), ["give both `min` and `max`, or neither to clear"]);
    assert_eq!(limits(json!({"min": null, "max": 1})), ["a joint value must be a number or an expression"]);
    assert_eq!(limits(json!({"min": 0, "max": true})), ["a joint value must be a number or an expression"]);
    assert_eq!(limits(json!({"index": "0"})), ["`index` must be a number"]);
    assert_eq!(limits(json!({"index": null})), none());

    // joint.delete: a joint origin's name, else a motion link, else a joint; each mode reads only
    // its own keys.
    assert_eq!(issues("joint.delete", json!({"origin": "Top"})), none());
    assert_eq!(issues("joint.delete", json!({"origin": "Top", "joint": true, "link": "x", "quiet": 5})), none());
    assert_eq!(issues("joint.delete", json!({"link": 1})), none());
    assert_eq!(issues("joint.delete", json!({"link": 1, "joint": [1], "quiet": "no"})), none());
    assert_eq!(issues("joint.delete", json!({"link": 1e12, "joint": "Hinge"})), none());
    assert_eq!(issues("joint.delete", json!({"joint": "Hinge", "quiet": true})), none());
    assert_eq!(issues("joint.delete", json!({"origin": null, "link": null, "joint": 4})), none());
    assert_eq!(issues("joint.delete", json!({})), none());
    assert_eq!(issues("joint.delete", json!({"joint": true})), ["`joint` must be an id or a name"]);
    assert_eq!(issues("joint.delete", json!({"link": 1e12, "joint": true})), ["`joint` must be an id or a name"]);
    assert_eq!(issues("joint.delete", json!({"joint": 1, "quiet": 5})), ["`quiet` must be true or false"]);
    assert_eq!(issues("joint.delete", json!({"origin": 5, "joint": 1})), ["`origin` must be the name of a joint origin"]);
    assert_eq!(issues("joint.delete", json!({"link": "1", "joint": 1})), ["`link` must be a motion link's index, a number"]);

    // joint.list, joint.solve and joint.inspect.
    for id in ["joint.list", "joint.solve"] {
        assert_eq!(issues(id, json!({"quiet": true})), none());
        assert_eq!(issues(id, json!({"quiet": null})), none());
        assert_eq!(issues(id, json!({"quiet": "x"})), ["`quiet` must be true or false"]);
        assert_eq!(issues(id, json!({"quet": true})), ["unknown key `quet`; did you mean `quiet`?"]);
    }
    assert_eq!(issues("joint.inspect", json!({})), none());
    assert_eq!(issues("joint.inspect", json!({"joint": null})), none());
    assert_eq!(issues("joint.inspect", json!({"joint": 4})), none());
    assert_eq!(issues("joint.inspect", json!({"joint": "Hinge"})), none());
    assert_eq!(issues("joint.inspect", json!({"joint": false})), ["`joint` must be an id or a name"]);
}

/// The schemas say what the handlers need: the keys a handler refuses without are required, the
/// modes are shown, and the values a handler reads are the ones the schemas take.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    let sides = json!({"a": {"face": [0, 0, 0]}, "b": "Origin"});
    let mut full = sides.clone();
    full["type"] = json!("revolute");
    let with = |extra: Value| {
        let mut p = full.clone();
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        p
    };
    takes(
        input_schema_for::<CreateJointParams>(),
        &[
            (full.clone(), true),
            (with(json!({"values": [1, "w"], "limits": [[0, 90], null], "flip": true, "offset": "w", "angle": 5, "name": "J", "quiet": true})), true),
            (with(json!({"value": 3})), true),
            (json!({"a": {"face": [0, 0, 0]}, "b": "Origin"}), false),
            (json!({"type": "revolute", "b": "Origin"}), false),
            (json!({"type": "revolute", "a": 5, "b": "Origin"}), false),
            (with(json!({"values": 5})), false),
            // The schema can't spell the type's leniency; the type refuses "hinge" (see above).
            (with(json!({"type": 5})), false),
            (with(json!({"type": "Pin Slot"})), true),
            (with(json!({"type": "REVOLUTE"})), true),
            (with(json!({"offset": true})), false),
            (with(json!({"limits": [[true, 1]]})), false),
            (with(json!({"nme": "J"})), false),
        ],
    );
    takes(
        input_schema_for::<AsBuiltJointParams>(),
        &[
            (json!({"type": "rigid", "a": 0, "b": "Part"}), true),
            (json!({"type": "rigid", "a": "A", "b": 3, "at": {"circle": [0, 0, 0]}, "limits": [null], "values": [1]}), true),
            (json!({"type": "rigid", "a": 0}), false),
            (json!({"type": "rigid", "a": true, "b": 0}), false),
        ],
    );
    takes(
        input_schema_for::<DriveJointParams>(),
        &[
            (json!({"joint": "Hinge", "value": 45}), true),
            (json!({"joint": 1, "values": [1, "w"], "quiet": true, "continuous": true}), true),
            (json!({"joint": 1, "index": 1, "value": "w"}), true),
            (json!({"value": 45}), false),
            (json!({"joint": "Hinge"}), false),
            (json!({"joint": "Hinge", "index": 0, "values": [1]}), false),
            (json!({"joint": "Hinge", "index": -1, "value": 1}), false),
        ],
    );
    takes(
        input_schema_for::<JointLimitsParams>(),
        &[
            (json!({"joint": "Hinge"}), true),
            (json!({"joint": 1, "index": 1, "min": -1, "max": "w"}), true),
            (json!({"index": 1}), false),
            (json!({"joint": 1, "min": true, "max": 1}), false),
        ],
    );
    takes(
        input_schema_for::<DeleteJointParams>(),
        &[
            (json!({"joint": "Hinge", "quiet": true}), true),
            (json!({"origin": "Top"}), true),
            (json!({"link": 2}), true),
            (json!({}), false),
            (json!({"quiet": true}), false),
            (json!({"joint": true}), false),
        ],
    );
    takes(
        input_schema_for::<InspectJointParams>(),
        &[(json!({}), true), (json!({"joint": "J"}), true), (json!({"joint": 4}), true), (json!({"joint": true}), false)],
    );
    takes(input_schema_for::<ListJointsParams>(), &[(json!({}), true), (json!({"quiet": false}), true), (json!({"quiet": 1}), false)]);
    takes(input_schema_for::<SolveJointsParams>(), &[(json!({}), true), (json!({"quiet": false}), true), (json!({"quiet": 1}), false)]);
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// Hinge leaves, a slider, a screw, a joint origin and two loose parts joined to each other: each
/// command, run in Warn in every form it takes, gives a reply that matches its output schema and
/// carries no warning.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    for name in ["LeafA", "LeafB", "LeafC", "LeafD", "LeafE"] {
        run("component.activate", json!({"component": "root"}));
        let made = run("component.create", json!({"name": name, "grounded": false}));
        run("solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
        run("solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
        if name == "LeafA" {
            run("occurrence.ground", json!({"occurrence": made["occurrence"], "grounded": true}));
        }
    }
    let hinge = |a: &str, b: &str, name: &str| json!({"type": "revolute", "a": {"occurrence": a, "circle": [1, 37, 4]}, "b": {"occurrence": b, "circle": [1, 37, 4]}, "flip": true, "name": name});

    // joint.create, in the forms of a side, with a value held inside its limits, and as built.
    let made = run("joint.create", with_limits(hinge("LeafA", "LeafB", "Hinge"), json!([[-90, 90]])));
    matches(output_schema_for::<JointAddedResult>(), "joint.create", &made);
    assert!(made.get("warnings").is_none() && made["name"] == "Hinge" && made.get("dof").is_some(), "{made}");
    let quiet = run("joint.create", with_extra(hinge("LeafC", "LeafB", "Quiet"), json!({"quiet": true, "value": 30, "limits": [null]})));
    matches(output_schema_for::<JointAddedResult>(), "joint.create", &quiet);
    assert!(quiet.get("dof").is_none() && quiet.get("fixed").is_none(), "{quiet}");
    run("joint.delete", json!({"joint": "Quiet"}));
    let clamped = run("joint.create", with_extra(hinge("LeafD", "LeafE", "Clamped"), json!({"values": [100], "limits": [[-10, 10]]})));
    assert!(clamped["warnings"].as_array().is_some_and(|w| w.len() == 1), "{clamped}");
    matches(output_schema_for::<JointAddedResult>(), "joint.create", &clamped);
    run("joint.origin", json!({"occurrence": "LeafA", "face": [20, 15, 3], "name": "Top"}));
    let by_origin = run(
        "joint.create",
        json!({"type": "rigid", "a": "Top", "b": {"occurrence": "LeafC", "face": [20, 15, 0]}, "name": "Stack", "offset": "0 mm", "angle": 0}),
    );
    matches(output_schema_for::<JointAddedResult>(), "joint.create", &by_origin);
    let built = run("joint.as_built", json!({"type": "slider", "a": "LeafB", "b": "LeafC", "name": "Slide", "limits": [[0, 10]], "quiet": false}));
    matches(output_schema_for::<JointAddedResult>(), "joint.as_built", &built);
    let at = run("joint.as_built", json!({"type": "rigid", "a": "LeafA", "b": "LeafD", "at": {"point": [0, 0, 0]}, "values": [1]}));
    matches(output_schema_for::<JointAddedResult>(), "joint.as_built", &at);
    run("joint.delete", json!({"joint": at["joint"]}));

    // A screw ties Slide's travel to Hinge's turn: a coupling for the list.
    run("joint.screw", json!({"a": "Hinge", "b": "Slide", "pitch": 2}));

    let list = run("joint.list", json!({}));
    matches(output_schema_for::<ListJointsResult>(), "joint.list", &list);
    assert!(list["links"].as_array().is_some_and(|l| l.len() == 1) && list["links"][0]["coupling"]["type"] == "screw", "{list}");
    assert!(list["joints"].as_array().is_some_and(|j| j.len() == 4) && list["origins"][0]["name"] == "Top", "{list}");
    assert!(list["floating"].as_array().is_some_and(|f| f.len() == 1), "{list}");
    let quiet = run("joint.list", json!({"quiet": true}));
    matches(output_schema_for::<ListJointsResult>(), "joint.list", &quiet);
    assert!(quiet.get("dof").is_none(), "{quiet}");

    for p in [json!({}), json!({"joint": "Slide"}), json!({"joint": null}), json!({"joint": made["joint"]})] {
        let inspected = run("joint.inspect", p);
        matches(output_schema_for::<InspectJointResult>(), "joint.inspect", &inspected);
    }
    let inspected = run("joint.inspect", json!({"joint": "Hinge"}));
    assert!(inspected["joints"][0]["links"][0]["role"] == "driver" && inspected["joints"][0]["a"]["references"].as_array().is_some(), "{inspected}");

    // joint.drive: a value, a list, one coordinate, past a limit, a drag frame, a suppressed joint
    // and one joined to nothing grounded.
    for p in [
        json!({"joint": "Hinge", "value": 45}),
        json!({"joint": "Hinge", "values": [30]}),
        json!({"joint": "Hinge", "index": 0, "value": "20 deg", "quiet": true}),
        json!({"joint": "Hinge", "value": 50, "continuous": true}),
        json!({"joint": "Slide", "value": 3}),
    ] {
        let driven = run("joint.drive", p);
        matches(output_schema_for::<DriveJointResult>(), "joint.drive", &driven);
    }
    let held = run("joint.drive", json!({"joint": "Hinge", "value": 200}));
    assert!(held["held"][0]["why"].as_str().is_some_and(|w| w.contains("limits")), "{held}");
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &held);
    let linked = run("joint.drive", json!({"joint": "Slide", "value": 5}));
    assert!(linked["held"][0]["why"].as_str().is_some_and(|w| w.contains("motion link")), "{linked}");
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &linked);
    let loose = run("joint.drive", json!({"joint": "Clamped", "value": 5}));
    assert!(loose["unconnected"].is_string() && loose["reason"].is_string(), "{loose}");
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &loose);
    run("joint.edit", json!({"joint": "Stack", "type": "revolute"}));
    let turned = run("joint.drive", json!({"joint": "Stack", "value": 1}));
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &turned);
    run("joint.edit", json!({"joint": "Stack", "suppressed": true}));
    let hidden = run("joint.drive", json!({"joint": "Stack", "value": 2}));
    assert!(hidden["reason"].as_str().is_some_and(|r| r.contains("suppressed")), "{hidden}");
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &hidden);

    // joint.limits and joint.solve.
    for p in [
        json!({"joint": "Hinge", "min": -45, "max": "45 deg"}),
        json!({"joint": "Hinge", "index": 0, "min": -10, "max": 10, "quiet": true}),
        json!({"joint": "Hinge"}),
    ] {
        let limited = run("joint.limits", p);
        matches(output_schema_for::<SolveResult>(), "joint.limits", &limited);
    }
    for p in [json!({}), json!({"quiet": true})] {
        let solved = run("joint.solve", p);
        matches(output_schema_for::<SolveResult>(), "joint.solve", &solved);
    }

    // joint.delete: a motion link, a joint origin, a joint.
    let link = run("joint.delete", json!({"link": 0}));
    assert_eq!(link["deleted_link"], 0, "{link}");
    matches(output_schema_for::<DeleteJointResult>(), "joint.delete", &link);
    let origin = run("joint.delete", json!({"origin": "Top", "joint": true, "link": "x"}));
    assert_eq!((origin["deleted_origin"].clone(), origin["joints"].clone()), (json!("Top"), json!(4)), "{origin}");
    matches(output_schema_for::<DeleteJointResult>(), "joint.delete", &origin);
    let deleted = run("joint.delete", json!({"joint": "Stack"}));
    assert!(deleted["deleted"].is_u64() && deleted.get("dof").is_some(), "{deleted}");
    matches(output_schema_for::<DeleteJointResult>(), "joint.delete", &deleted);
    let quiet = run("joint.delete", json!({"joint": "Clamped", "quiet": true}));
    assert!(quiet.get("dof").is_none() && quiet["deleted"].is_u64(), "{quiet}");
    matches(output_schema_for::<DeleteJointResult>(), "joint.delete", &quiet);
}

fn with_limits(mut p: Value, limits: Value) -> Value {
    p["limits"] = limits;
    p
}

fn with_extra(mut p: Value, extra: Value) -> Value {
    if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    p
}

/// A misspelt key still runs in Warn, says so, and the reply still matches its schema.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let mut s = hinged(ParamCheck::Warn);
    let calls = [
        ("joint.create", json!({"type": "rigid", "a": "Top", "b": {"occurrence": "LeafB", "face": [20, 15, 0]}, "nme": "J"}), "name"),
        ("joint.as_built", json!({"type": "rigid", "a": "LeafA", "b": "LeafB", "nam": "As built"}), "name"),
        ("joint.drive", json!({"joint": "Hinge", "valu": 10, "value": 10}), "value"),
        ("joint.limits", json!({"joint": "Hinge", "mn": 1, "min": -10, "max": 10}), "min"),
        ("joint.list", json!({"quet": true}), "quet"),
        ("joint.inspect", json!({"joynt": "Hinge"}), "joint"),
        ("joint.solve", json!({"quet": true}), "quet"),
        ("joint.delete", json!({"origin": "Top", "oringin": 1}), "origin"),
    ];
    for (id, p, near) in calls {
        let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let warnings = reply["param_warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.len() == 1 && warnings[0].as_str().is_some_and(|w| w.contains("unknown key") && w.contains(near)), "{id}: {reply}");
        let out = crate::cmd::find_command(id).and_then(|c| c.output).map(|f| f()).unwrap_or_else(|| panic!("{id}"));
        assert!(conforms(out.as_value(), out.as_value(), &reply), "{id}: {reply} doesn't match {}", out.as_value());
    }
}

/// A cam on a revolute joint: `joint.list` and `joint.inspect` give its follower's lift, a table
/// that is not whole as `null`, and the replies match their schemas.
#[test]
fn a_cam_link_shows_its_lift() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let part = |run: &mut dyn FnMut(&str, Value) -> Value, name: &str, corner: [f64; 3]| {
        run("component.activate", json!({"component": "root"}));
        let made = run("component.create", json!({"name": name, "grounded": false}));
        run("solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": corner}));
        made["occurrence"].clone()
    };
    let pin = |run: &mut dyn FnMut(&str, Value) -> Value, kind: &str, name: &str, a: &str, b: &str, p: Vec3, z: Vec3| {
        let at = |o: &str| json!({"occurrence": o, "point": [p.x, p.y, p.z], "z": [z.x, z.y, z.z]});
        run("joint.create", json!({"type": kind, "a": at(a), "b": at(b), "flip": true, "name": name}));
    };
    let occurrence = part(&mut run, "Base", [-90.0, -90.0, -20.0]);
    run("occurrence.ground", json!({"occurrence": occurrence, "grounded": true}));
    run("component.activate", json!({"component": "root"}));
    run("component.create", json!({"name": "Cam"}));
    run("sketch.create", json!({"plane": "XY", "name": "CamSketch"}));
    run("sketch.circle.center", json!({"center": [10, 0], "radius": 30, "id": "c1"}));
    run("sketch.finish", json!({}));
    run("solid.extrude", json!({"distance": 5}));
    pin(&mut run, "revolute", "Cam", "Base", "Cam", Vec3::ZERO, Vec3::Z);
    run("component.activate", json!({"component": "root"}));
    run("component.create", json!({"name": "Tappet"}));
    run("solid.box", json!({"length": 20, "width": 4, "height": 5, "corner": [40, -2, 0]}));
    pin(&mut run, "slider", "Tappet", "Base", "Tappet", Vec3::new(0.0, 0.0, 2.5), Vec3::X);
    run("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    let list = run("joint.list", json!({}));
    let schema = output_schema_for::<ListJointsResult>();
    assert!(list["links"][0]["lift"]["rise"].is_number() && list["links"][0].get("ratio").is_none(), "{list}");
    assert!(list["links"][0]["coupling"]["type"] == "cam" && conforms(schema.as_value(), schema.as_value(), &list), "{list}");
    let inspected = run("joint.inspect", json!({"joint": "Tappet"}));
    let schema = output_schema_for::<InspectJointResult>();
    assert!(inspected["joints"][0]["links"][0]["role"] == "driven" && conforms(schema.as_value(), schema.as_value(), &inspected), "{inspected}");
    // A table that is not whole moves nothing, and says `null`.
    if let Some(l) = s.doc_mut().assembly.links.first_mut() {
        l.lift.truncate(3);
    }
    let broken = s.execute("joint.list", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<ListJointsResult>();
    assert!(broken["links"][0]["lift"].is_null() && broken["links"][0].get("lift").is_some(), "{broken}");
    assert!(conforms(schema.as_value(), schema.as_value(), &broken), "{broken}");
}

/// A slider stopped by contact: the bodies that touched and how far it got, in the reply.
#[test]
fn a_drive_stopped_by_contact_matches_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    for (name, x) in [("A", 0), ("B", 20)] {
        run("component.activate", json!({"component": 0}));
        let made = run("component.create", json!({"name": name}));
        run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
        if name == "A" {
            run("occurrence.ground", json!({"occurrence": made["occurrence"], "grounded": true}));
        }
    }
    run("joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run("contact.create", json!({"occurrences": ["A", "B"]}));
    let free = run("joint.drive", json!({"joint": "Rail", "value": 10}));
    assert!(free["stopped_by_contact"].is_null() && free["stopped_by_loop"].is_null(), "{free}");
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &free);
    let stopped = run("joint.drive", json!({"joint": "Rail", "value": -30}));
    assert!(stopped["stopped_by_contact"]["fraction"].is_number() && stopped["stopped_by_contact"]["a"].is_string(), "{stopped}");
    assert!(stopped["held"][0]["why"].as_str().is_some_and(|w| w.contains("contact")), "{stopped}");
    matches(output_schema_for::<DriveJointResult>(), "joint.drive", &stopped);
}

/// An origin on an edge names the edge and its two faces; a round origin on an extruded circle
/// names the sketch curve its wall was swept from.
#[test]
fn inspect_names_what_edges_and_swept_faces_refer_to() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let panel = run("component.create", json!({"name": "Panel"}));
    run("solid.box", json!({"length": 400, "width": 300, "height": 2, "corner": [0, 0, 0]}));
    run("occurrence.ground", json!({"occurrence": panel["occurrence"], "grounded": true}));
    run("component.activate", json!({"component": "root"}));
    run("component.create", json!({"name": "Pin"}));
    run("sketch.create", json!({"plane": "XY", "name": "PinSketch"}));
    run("sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c1"}));
    run("sketch.finish", json!({}));
    run("solid.extrude", json!({"distance": 20}));
    run(
        "joint.create",
        json!({"type": "revolute", "name": "Edge", "a": {"occurrence": "Panel", "edge": [200, 0, 2]}, "b": {"occurrence": "Pin", "circle": [105, 0, 20]}}),
    );
    let inspected = run("joint.inspect", json!({"joint": "Edge"}));
    matches(output_schema_for::<InspectJointResult>(), "joint.inspect", &inspected);
    let side = |s: &str, role: &str| inspected["joints"][0][s]["references"].as_array().and_then(|r| r.iter().find(|r| r["role"] == role)).cloned();
    let edge = side("a", "edge").unwrap_or_default();
    assert!(edge["faces"].as_array().is_some_and(|f| f.len() == 2 && f[0]["feature"]["type"] == "BoxFeature"), "{inspected}");
    let wall = side("b", "face").unwrap_or_default();
    assert_eq!((wall["sketch"]["name"].clone(), wall["sketch"]["curve_type"].clone()), (json!("PinSketch"), json!("circle")), "{inspected}");
    // The extrude goes: the names are kept, and say what is gone.
    run("timeline.delete", json!({"feature": "Extrude1"}));
    let gone = run("joint.inspect", json!({"joint": "Edge"}));
    assert!(gone["joints"][0]["b"]["resolves"] == false && gone["joints"][0]["b"]["error"].is_string(), "{gone}");
    matches(output_schema_for::<InspectJointResult>(), "joint.inspect", &gone);
}

/// The contract looks at every key before the handler looks at the design, so a call with two
/// problems may name another one in strict than the handler does (the documented difference), and
/// a call the handler ignores a wrong-typed key in is refused (the stricter reading).
#[test]
fn strict_may_name_another_problem_first_and_refuses_ignored_wrong_types() {
    let strict = |id: &str, p: Value| hinged(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string());
    let off = |id: &str, p: Value| hinged(ParamCheck::Off).execute(id, &p).err().map(|e| e.to_string());
    for (id, p) in
        [("joint.drive", json!({"joint": "nope", "index": "x", "value": 1})), ("joint.limits", json!({"joint": "Hinge", "index": 9, "min": 0}))]
    {
        assert!(off(id, p.clone()).is_some() && strict(id, p.clone()).is_some() && off(id, p.clone()) != strict(id, p), "{id}");
    }
    for (id, p) in [
        ("joint.limits", json!({"joint": "Hinge", "limits": [5]})),
        ("joint.limits", json!({"joint": "Hinge", "index": "0", "min": 1, "max": 2})),
        ("joint.solve", json!({"quiet": "yes"})),
        ("joint.delete", json!({"joint": "Hinge", "link": "x"})),
        ("joint.delete", json!({"joint": "Hinge", "origin": 5})),
    ] {
        let works = |level| hinged(level).execute(id, &p).is_ok();
        assert!(works(ParamCheck::Off) && !works(ParamCheck::Strict), "{id} {p}");
    }
}
