use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::joints::JointKind;

use super::{
    AsBuiltJointParams, CamParams, CamResult, CouplingResult, CreateJointParams, DeleteJointParams, DeleteJointResult, DriveJointParams,
    DriveJointResult, EditJointParams, GearParams, InspectJointParams, InspectJointResult, JointAddedResult, JointKindName, JointLimitsParams,
    ListJointsParams, ListJointsResult, MakeJointOriginParams, MakeJointOriginResult, MotionLinkParams, RackPinionParams, ReachParams, ReachResult,
    RigidGroupParams, RigidGroupResult, ScrewParams, SolveJointsParams, SolveResult,
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
    assert_keys(input_schema_for::<DriveJointParams>(), &["continuous", "index", "joint", "quiet", "stop_at_budget", "value", "values"]);
    assert_keys(input_schema_for::<JointLimitsParams>(), &["index", "joint", "max", "min", "quiet"]);
    assert_keys(input_schema_for::<DeleteJointParams>(), &["joint", "link", "origin", "quiet"]);
    assert_keys(input_schema_for::<ListJointsParams>(), &["quiet"]);
    assert_keys(input_schema_for::<InspectJointParams>(), &["joint"]);
    assert_keys(input_schema_for::<SolveJointsParams>(), &["quiet"]);
    assert_keys(
        input_schema_for::<MakeJointOriginParams>(),
        &["body", "circle", "corner", "edge", "end", "face", "frame", "name", "occurrence", "on", "point", "x", "z"],
    );
    assert_keys(input_schema_for::<RigidGroupParams>(), &["occurrences"]);
    assert_keys(input_schema_for::<ReachParams>(), &["at", "axis", "continuous", "joints", "occurrence", "point", "quiet", "z"]);
    assert_keys(input_schema_for::<MotionLinkParams>(), &["a", "b", "ia", "ib", "offset", "quiet", "ratio"]);
    assert_keys(input_schema_for::<GearParams>(), &["a", "b", "internal", "link", "quiet", "ra", "rb", "reverse"]);
    assert_keys(input_schema_for::<RackPinionParams>(), &["a", "b", "link", "quiet", "radius", "reverse"]);
    assert_keys(input_schema_for::<ScrewParams>(), &["a", "b", "joint", "link", "pitch", "quiet", "reverse"]);
    assert_keys(input_schema_for::<CamParams>(), &["a", "b", "flip", "follower", "link", "profile", "quiet", "radius", "tip"]);
}

#[test]
fn the_commands_publish_their_schemas() {
    for id in [
        "joint.create",
        "joint.as_built",
        "joint.drive",
        "joint.limits",
        "joint.delete",
        "joint.list",
        "joint.inspect",
        "joint.solve",
        "joint.origin",
        "joint.rigid_group",
        "joint.reach",
        "joint.motion_link",
        "joint.gear",
        "joint.rack_pinion",
        "joint.screw",
        "joint.cam",
    ] {
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
        // joint.origin
        ("joint.origin", json!({})),
        ("joint.origin", json!({"face": [1, 2]})),
        ("joint.origin", json!({"face": 7, "name": "N"})),
        ("joint.origin", json!({"frame": {"z": [0, 0, 1]}})),
        ("joint.origin", json!({"frame": null})),
        ("joint.origin", json!({"face": "", "occurrence": 0})),
        ("joint.origin", json!({"circle": "x".repeat(16385)})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": null})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": true})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": 0, "x": "nonsense"})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": 0, "x": [1, 0, 0], "end": "F1"})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": 0, "end": 5})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": 0, "end": null})),
        ("joint.origin", json!({"point": [0, 0, 0], "occurrence": 0, "x": [0, 0, 0]})),
        ("joint.origin", json!({"face": [0, 0, 0], "occurrence": 0, "x": {"face": null}})),
        ("joint.origin", json!({"face": [0, 0, 0], "occurrence": 0, "x": {}})),
        ("joint.origin", json!({"face": [0, 0, 0], "occurrence": 0, "end": "F1"})),
        ("joint.origin", json!({"circle": [0, 0, 0], "occurrence": 0, "z": [0, 0, 0]})),
        ("joint.origin", json!({"circle": [0, 0, 0], "occurrence": 0, "z": "up"})),
        ("joint.origin", json!({"edge": [0, 0, 0], "occurrence": 0, "z": 5})),
        ("joint.origin", json!({"frame": {"origin": [0, 0, 0]}, "occurrence": 0})),
        // joint.rigid_group
        ("joint.rigid_group", json!({})),
        ("joint.rigid_group", json!({"occurrences": null})),
        ("joint.rigid_group", json!({"occurrences": "A"})),
        ("joint.rigid_group", json!({"occurrences": [0]})),
        ("joint.rigid_group", json!({"occurrences": vec![0; 1001]})),
        ("joint.rigid_group", json!({"occurrences": [0, true]})),
        // joint.reach
        ("joint.reach", json!({})),
        ("joint.reach", json!({"occurrence": null, "point": [0, 0, 0]})),
        ("joint.reach", json!({"occurrence": [0], "point": [0, 0, 0]})),
        ("joint.reach", json!({"occurrence": 0})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0]})),
        ("joint.reach", json!({"occurrence": 0, "point": [1e9, 0, 0]})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "at": null})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "at": "here"})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "z": [0, 0, 0]})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "z": null})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "z": [0, 0, 1], "axis": 5})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "axis": [0, 0, 1]})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "joints": "Hinge"})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "joints": null})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "joints": [true]})),
        ("joint.reach", json!({"occurrence": 0, "point": [0, 0, 0], "joints": vec![1; 1001]})),
        // joint.motion_link, joint.gear, joint.rack_pinion, joint.screw and joint.cam
        ("joint.motion_link", json!({})),
        ("joint.motion_link", json!({"a": true, "b": 1, "ratio": 1})),
        ("joint.motion_link", json!({"a": null, "b": 1, "ratio": 1})),
        ("joint.gear", json!({})),
        ("joint.gear", json!({"a": [1], "b": 1})),
        ("joint.rack_pinion", json!({})),
        ("joint.rack_pinion", json!({"a": {}, "b": 1})),
        ("joint.screw", json!({})),
        ("joint.screw", json!({"joint": null})),
        ("joint.screw", json!({"joint": true, "pitch": 1})),
        ("joint.screw", json!({"a": true, "b": 1, "pitch": 1})),
        ("joint.cam", json!({})),
        ("joint.cam", json!({"a": false, "b": 1})),
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
            (
                json!({"type": "revolute", "a": {"edge": {"x": 0, "y": 0, "z": 0}, "z": {"x": 0, "y": 0, "z": 1}}, "b": {"point": {"x": 1, "y": 2, "z": 3}}}),
                true,
            ),
            (json!({"type": "revolute", "a": {"edge": {"x": 0, "y": 0}}, "b": "Origin"}), false),
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

// ---------------------------------------------------------------------------------------------
// joint.origin, joint.rigid_group, joint.reach, joint.motion_link, joint.gear, joint.rack_pinion,
// joint.screw and joint.cam (#13185)
// ---------------------------------------------------------------------------------------------

fn put(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A joint between two parts as they are, at a world point with its z along `z`.
fn pin(s: &mut Session, kind: &str, name: &str, base: &str, part: &str, p: [f64; 3], z: [f64; 3]) {
    let at = |o: &str| json!({"occurrence": o, "point": p, "z": z});
    put(s, "joint.create", json!({"type": kind, "a": at(base), "b": at(part), "flip": true, "name": name}));
}

/// A grounded base with two discs on hinges (G1, G2), a pinion's hinge (P) and its rack's slider
/// (Rack), a screw's hinge and slider (K, Slide2), a cylindrical joint (Nut) and a spare hinge (X):
/// a joint named for each part, for the couplings to tie.
fn rig(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    let root = |s: &mut Session| put(s, "component.activate", json!({"component": "root"}));
    root(&mut s);
    let base = put(&mut s, "component.create", json!({"name": "Base", "grounded": false}));
    put(&mut s, "solid.box", json!({"length": 200, "width": 120, "height": 20, "corner": [-100, -60, -20]}));
    put(&mut s, "occurrence.ground", json!({"occurrence": base["occurrence"], "grounded": true}));
    for (name, x, r) in [("G1", 0.0, 10.0), ("G2", 30.0, 20.0)] {
        root(&mut s);
        put(&mut s, "component.create", json!({"name": name, "grounded": false}));
        put(&mut s, "solid.cylinder", json!({"base": [x, 0, 0], "axis": [0, 0, 1], "radius": r, "height": 5}));
    }
    for (name, x, y) in
        [("P", -40.0, -5.0), ("Rack", -40.0, -35.0), ("K", 70.0, -5.0), ("Slide2", 70.0, 25.0), ("Nut", 100.0, -5.0), ("X", 130.0, -5.0)]
    {
        root(&mut s);
        put(&mut s, "component.create", json!({"name": name, "grounded": false}));
        put(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, y, 0]}));
    }
    let (z, x) = ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    pin(&mut s, "revolute", "G1", "Base", "G1", [0.0, 0.0, 0.0], z);
    pin(&mut s, "revolute", "G2", "Base", "G2", [30.0, 0.0, 0.0], z);
    pin(&mut s, "revolute", "P", "Base", "P", [-40.0, 0.0, 0.0], z);
    pin(&mut s, "slider", "Rack", "Base", "Rack", [-40.0, -30.0, 0.0], x);
    pin(&mut s, "revolute", "K", "Base", "K", [70.0, 0.0, 0.0], z);
    pin(&mut s, "slider", "Slide2", "Base", "Slide2", [70.0, 30.0, 0.0], x);
    pin(&mut s, "cylindrical", "Nut", "Base", "Nut", [100.0, 0.0, 0.0], z);
    pin(&mut s, "revolute", "X", "Base", "X", [130.0, 0.0, 0.0], z);
    s
}

/// Grounded base, a cam (a disc of radius 30 about (10, 0), in sketch `CamSketch`) on a hinge
/// `Cam`, a tappet on a slider `Tappet` and an arm on a hinge `Arm`, for `joint.cam`.
fn cam_rig(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    put(&mut s, "component.activate", json!({"component": "root"}));
    let base = put(&mut s, "component.create", json!({"name": "Base", "grounded": false}));
    put(&mut s, "solid.box", json!({"length": 180, "width": 180, "height": 20, "corner": [-90, -90, -20]}));
    put(&mut s, "occurrence.ground", json!({"occurrence": base["occurrence"], "grounded": true}));
    put(&mut s, "component.activate", json!({"component": "root"}));
    put(&mut s, "component.create", json!({"name": "Cam"}));
    put(&mut s, "sketch.create", json!({"plane": "XY", "name": "CamSketch"}));
    put(&mut s, "sketch.circle.center", json!({"center": [10, 0], "radius": 30, "id": "c1"}));
    put(&mut s, "sketch.finish", json!({}));
    put(&mut s, "solid.extrude", json!({"distance": 5}));
    pin(&mut s, "revolute", "Cam", "Base", "Cam", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    put(&mut s, "component.activate", json!({"component": "root"}));
    put(&mut s, "component.create", json!({"name": "Tappet"}));
    put(&mut s, "solid.box", json!({"length": 20, "width": 4, "height": 5, "corner": [45, -2, 0]}));
    pin(&mut s, "slider", "Tappet", "Base", "Tappet", [0.0, 0.0, 2.5], [1.0, 0.0, 0.0]);
    put(&mut s, "component.activate", json!({"component": "root"}));
    put(&mut s, "component.create", json!({"name": "Arm"}));
    put(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 5, "corner": [-52, 38, 0]}));
    pin(&mut s, "revolute", "Arm", "Base", "Arm", [-50.0, 40.0, 2.5], [0.0, 0.0, 1.0]);
    s
}

/// Both of `rig`'s sessions give a call the same error, with the check off and in strict.
fn assert_strict_keeps_todays_errors_in(rig: fn(ParamCheck) -> Session, cases: &[(&str, Value)]) {
    let (mut off, mut strict) = (rig(ParamCheck::Off), rig(ParamCheck::Strict));
    for (id, p) in cases {
        let today = off.execute(id, p).err().map(|e| e.to_string());
        assert!(today.is_some(), "{id} {p} should fail today");
        assert_eq!(strict.execute(id, p).err().map(|e| e.to_string()), today, "{id} {p}");
    }
}

/// What the handlers refuse with a design to refuse it for: strict says the same.
#[test]
fn strict_keeps_todays_errors_with_the_couplings() {
    let long = "x".repeat(1001);
    assert_strict_keeps_todays_errors_in(
        rig,
        &[
            ("joint.motion_link", json!({"a": "G1", "b": "G1", "ratio": 1})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2"})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2", "ratio": null})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2", "ratio": "2"})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2", "ratio": 1e12})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2", "ratio": 1, "offset": true})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2", "ratio": 1, "offset": null})),
            ("joint.motion_link", json!({"a": "G1", "b": "G2", "ratio": 1, "offset": 1e12})),
            ("joint.motion_link", json!({"a": "Missing", "b": "G2", "ratio": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G1", "ra": 1, "rb": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G2"})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": true, "rb": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": null, "rb": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": long, "rb": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": {"circle": 5}})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": {"circle": ""}})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": {"point": [1, 2, 3]}})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": 1, "link": -1})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": 1, "link": null})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": 1, "link": "0"})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": 1, "link": 0})),
            ("joint.gear", json!({"a": "G1", "b": "Rack", "ra": 1, "rb": 1})),
            ("joint.rack_pinion", json!({"a": "P", "b": "Rack"})),
            ("joint.rack_pinion", json!({"a": "P", "b": "Rack", "radius": []})),
            ("joint.rack_pinion", json!({"a": "P", "b": "Rack", "radius": 1, "link": 1.5})),
            ("joint.rack_pinion", json!({"a": "P", "b": "G1", "radius": 1})),
            ("joint.screw", json!({"joint": "Nut"})),
            ("joint.screw", json!({"joint": "Nut", "pitch": {}})),
            ("joint.screw", json!({"joint": "Nut", "pitch": 1, "link": "x"})),
            ("joint.screw", json!({"joint": "G1", "pitch": 1})),
            ("joint.screw", json!({"joint": 99, "pitch": 1})),
            ("joint.screw", json!({"a": "K", "b": "Slide2", "pitch": null})),
            ("joint.screw", json!({"a": "K", "b": "K", "pitch": 1})),
            ("joint.screw", json!({"a": "K", "pitch": 1})),
            ("joint.reach", json!({"occurrence": "G1", "point": [0, 0, 0], "joints": ["G1", true]})),
            ("joint.reach", json!({"occurrence": "Missing", "point": [0, 0, 0]})),
            ("joint.origin", json!({"occurrence": "Base", "face": "Missing"})),
            ("joint.origin", json!({"occurrence": "Missing", "point": [0, 0, 0]})),
        ],
    );
}

/// What `joint.cam` refuses, with a cam and its followers in the design.
#[test]
fn strict_keeps_todays_errors_for_the_cam() {
    let edges = |n: usize| json!({"edges": vec![[10, 30, 5]; n]});
    assert_strict_keeps_todays_errors_in(
        cam_rig,
        &[
            ("joint.cam", json!({"a": "Cam", "b": "Cam", "profile": {"sketch": "CamSketch"}})),
            ("joint.cam", json!({"a": "Tappet", "b": "Cam", "profile": {"sketch": "CamSketch"}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet"})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": null})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": "CamSketch"})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": null}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": true}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch", "index": "0"}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch", "index": -1}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch", "index": null}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch", "index": 7}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "Missing"}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": []}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": "x"}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": edges(65)})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[1, 2]]}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[10, 30, 5]], "occurrence": null}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[10, 30, 5]], "occurrence": [1]}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "round"})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller"})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": true})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": null})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": {"circle": 5}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "tip": 5})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "tip": null})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "tip": {"face": ""}})),
            ("joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "link": 3})),
            ("joint.cam", json!({"a": "Cam", "b": "Arm", "profile": {"sketch": "CamSketch"}})),
        ],
    );
}

/// The couplings, reach, rigid groups and joint origins read each key as their handlers do, and in
/// the cases the handlers read it.
#[test]
fn the_remaining_types_read_each_key_as_the_handlers_do() {
    let none = Vec::<String>::new;
    let why = "a joint origin needs face, circle, edge, point or frame [x, y, z], or a face's name";
    let x_form = "`x` is a direction [x, y, z], a face's name, or {face: [x, y, z]}";
    let end_name = "`end` is the name of a face a round end lies on";
    let end_round = "`end` is for a round origin (`circle`)";
    let origin = |p: Value| issues("joint.origin", p);

    // joint.origin: the forms, each reading only its own keys.
    for p in [
        json!({"face": [1, 2, 3]}),
        json!({"face": {"x": 1, "y": 2, "z": 3}, "occurrence": "A", "x": [1, 0, 0], "name": "N"}),
        json!({"face": "F1"}),
        json!({"face": "F1", "x": "F2", "body": "B", "occurrence": 3, "name": null}),
        json!({"face": "F1", "x": {"face": [1, 2, 3]}}),
        json!({"face": "F1", "x": {"face": "F2"}}),
        json!({"face": "F1", "x": null, "z": "junk", "on": 1, "corner": 2, "frame": 3, "edge": 4, "point": 5}),
        json!({"circle": "F1", "end": "F2", "x": "F3"}),
        json!({"circle": [1, 2, 3], "end": "F2", "x": [0, 1, 0]}),
        // A circle with a z: its end is tried and then a point, and neither refuses a bad end.
        json!({"circle": [1, 2, 3], "z": [0, 0, 1], "end": 5}),
        json!({"circle": [1, 2, 3], "z": {"x": 0, "y": 0, "z": 1}, "end": "F2"}),
        json!({"circle": [1, 2, 3], "z": null}),
        // An edge reads on, corner and z; with a z, end is not read.
        json!({"edge": [1, 2, 3]}),
        json!({"edge": [1, 2, 3], "on": [0, 0, 0], "corner": [1, 1, 1], "z": [0, 0, 1], "end": 7, "x": "junk", "body": 3}),
        json!({"edge": [1, 2, 3], "on": null, "corner": null, "z": null}),
        json!({"point": [1, 2, 3]}),
        json!({"point": [1, 2, 3], "z": [0, 0, 1], "x": [1, 0, 0], "on": "junk"}),
        json!({"point": [1, 2, 3], "z": [0, 0, 0], "x": null}),
        json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}),
        json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}, "x": "junk", "z": "junk", "on": 1}),
        json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1], "x": null}}),
        // A point wins over a frame; a name over a point; the first of face and circle given by name.
        json!({"point": [1, 2, 3], "frame": 5}),
        json!({"face": [1, 2], "point": [1, 2, 3]}),
        json!({"face": [1, 2, 3], "circle": "C", "end": "E"}),
        json!({"face": null, "circle": null, "point": [1, 2, 3]}),
    ] {
        assert_eq!(origin(p.clone()), none(), "{p}");
    }
    let refused = [
        (json!({}), why),
        (json!({"name": 5}), why),
        (json!({"face": [1, 2]}), why),
        (json!({"face": 7, "circle": null}), why),
        (json!({"point": "x"}), why),
        (json!({"frame": null}), "`frame` needs origin and z"),
        (json!({"frame": {"z": [0, 0, 1]}}), "`frame` needs origin and z"),
        (json!({"frame": {"origin": [0, 0, 0]}}), "`frame` needs a z axis"),
        (json!({"frame": {"origin": [0, 0, 0], "z": [0, 0]}}), "`frame` needs a z axis"),
        (json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1], "x": 5}}), "`frame`'s x must be a direction [x, y, z]"),
        (json!({"face": ""}), "a face name is 1…16384 characters"),
        (json!({"circle": "x".repeat(16385)}), "a face name is 1…16384 characters"),
        (json!({"face": "F", "body": 5}), "`body` must be text"),
        (json!({"face": "F", "occurrence": true}), "an occurrence must be an id or a name"),
        (json!({"face": "F", "occurrence": null}), "an occurrence must be an id or a name"),
        (json!({"point": [1, 2, 3], "occurrence": [1]}), "an occurrence must be an id or a name"),
        (json!({"face": "F", "x": 5}), x_form),
        (json!({"face": "F", "x": ""}), x_form),
        (json!({"face": "F", "x": [0, 0, 0]}), x_form),
        (json!({"face": "F", "x": {}}), x_form),
        (json!({"face": "F", "x": {"face": null}}), x_form),
        (json!({"face": "F", "x": {"face": ""}}), x_form),
        (json!({"face": [1, 2, 3], "x": true}), x_form),
        (json!({"circle": [1, 2, 3], "x": "x".repeat(16385)}), x_form),
        (json!({"face": "F", "end": "E"}), end_round),
        (json!({"face": [1, 2, 3], "end": "E"}), end_round),
        (json!({"point": [1, 2, 3], "end": "E"}), end_round),
        (json!({"edge": [1, 2, 3], "end": "E"}), end_round),
        (json!({"edge": [1, 2, 3], "z": null, "end": "E"}), end_round),
        (json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}, "end": "E"}), end_round),
        (json!({"point": [1, 2, 3], "end": 5}), end_name),
        (json!({"point": [1, 2, 3], "end": null}), end_name),
        (json!({"circle": "C", "end": ""}), end_name),
        (json!({"circle": "C", "end": 5}), end_name),
        (json!({"circle": [1, 2, 3], "end": null}), end_name),
        (json!({"circle": [1, 2, 3], "z": null, "end": 5}), end_name),
        (json!({"circle": [1, 2, 3], "z": [0, 0, 0]}), "`z` is a direction [x, y, z]"),
        (json!({"circle": [1, 2, 3], "z": "up", "end": "E"}), "`z` is a direction [x, y, z]"),
        (json!({"edge": [1, 2, 3], "z": [1, 2]}), "`z` is a direction [x, y, z]"),
        (json!({"edge": [1, 2, 3], "z": [0, 0, 0]}), "`z` is a direction [x, y, z]"),
        (json!({"edge": [1, 2, 3], "on": 5}), "`on` must be a point [x, y, z]"),
        (json!({"edge": [1, 2, 3], "corner": "c"}), "`corner` must be a point [x, y, z]"),
        (json!({"point": [1, 2, 3], "z": "up"}), "`z` is a direction [x, y, z]"),
        (json!({"point": [1, 2, 3], "x": 5}), "a point origin's `x` is a direction [x, y, z]"),
        (json!({"point": [1, 2, 3], "x": "F"}), "a point origin's `x` is a direction [x, y, z]"),
        (json!({"point": [1, 2, 3], "name": 5}), "`name` must be text"),
    ];
    for (p, message) in refused {
        assert_eq!(origin(p.clone()), [message], "{p}");
    }
    assert_eq!(origin(json!({"face": "F", "circle": "C", "end": "E"})), [end_round]);
    assert_eq!(origin(json!({"point": [1, 2, 3], "nme": "N"})), ["unknown key `nme`; did you mean `name`?"]);

    // joint.rigid_group.
    let group = |p: Value| issues("joint.rigid_group", p);
    let list = "`occurrences` must list 2…1000 occurrences";
    assert_eq!(group(json!({"occurrences": [0, "A", 3]})), none());
    assert_eq!(group(json!({})), none());
    assert_eq!(group(json!({"occurrences": vec![1; 1000]})), none());
    for p in [json!(null), json!("A"), json!([]), json!([1]), json!({"0": 1}), json!(vec![1; 1001])] {
        assert_eq!(group(json!({ "occurrences": p })), [list], "{p}");
    }
    assert_eq!(group(json!({"occurrences": [1, true]})), ["an occurrence must be an id or a name"]);
    assert_eq!(group(json!({"occurrences": [1, null, 2]})), ["an occurrence must be an id or a name"]);

    // joint.reach: a point and a direction as finite numbers under 1e9, `null` refused.
    let reach = |extra: Value| {
        let mut p = json!({"occurrence": "Arm", "point": [1, 2, 3]});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        issues("joint.reach", p)
    };
    assert_eq!(reach(json!({})), none());
    assert_eq!(
        reach(json!({"at": {"x": 1, "y": 2, "z": 3}, "z": [0, 0, 5], "axis": [1, 1, 0], "joints": ["A", 1], "quiet": true, "continuous": false})),
        none()
    );
    assert_eq!(reach(json!({"joints": []})), none());
    assert_eq!(reach(json!({"axis": [1, 0, 0]})), none());
    assert_eq!(reach(json!({"quiet": null, "continuous": null})), none());
    assert_eq!(issues("joint.reach", json!({"point": [1, 2, 3]})), none());
    assert_eq!(issues("joint.reach", json!({"occurrence": 2})), none());
    assert_eq!(issues("joint.reach", json!({"occurrence": null, "point": [1, 2, 3]})), ["an occurrence must be an id or a name"]);
    assert_eq!(issues("joint.reach", json!({"occurrence": 2, "point": null})), ["`point` must be [x, y, z]"]);
    for bad in [json!([1, 2]), json!("x"), json!([1e9, 0, 0]), json!({"x": 1, "y": 2}), json!([1, 2, "3"])] {
        assert_eq!(reach(json!({"point": bad.clone()})), ["`point` must be [x, y, z]"], "{bad}");
        assert_eq!(reach(json!({"at": bad.clone()})), ["`at` must be [x, y, z]"], "{bad}");
    }
    assert_eq!(reach(json!({"at": null})), ["`at` must be [x, y, z]"]);
    for key in ["z", "axis"] {
        let message = format!("`{key}` must be a direction [x, y, z]");
        for bad in [json!(null), json!([0, 0, 0]), json!([1, 2]), json!("up")] {
            assert_eq!(reach(json!({ key: bad.clone() })), [message.as_str()], "{key} {bad}");
        }
    }
    let joints = "`joints` must be a list of joints (ids or names), at most 1000";
    for bad in [json!(null), json!("A"), json!(5), json!(vec![1; 1001])] {
        assert_eq!(reach(json!({"joints": bad.clone()})), [joints], "{bad}");
    }
    assert_eq!(reach(json!({"joints": [true]})), ["`joint` must be an id or a name"]);
    assert_eq!(reach(json!({"quiet": 1})), ["`quiet` must be true or false"]);
    assert_eq!(reach(json!({"continuous": "yes"})), ["`continuous` must be true or false"]);
    assert_eq!(reach(json!({"pont": 1})), ["unknown key `pont`; did you mean `point`?"]);

    // joint.motion_link.
    let link = |extra: Value| {
        let mut p = json!({"a": "A", "b": 2, "ratio": 0.5});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        issues("joint.motion_link", p)
    };
    assert_eq!(link(json!({})), none());
    assert_eq!(link(json!({"ia": 1, "ib": -1.5, "offset": "w / 2", "quiet": true})), none());
    assert_eq!(link(json!({"ia": null, "ib": null, "quiet": null})), none());
    assert_eq!(link(json!({"ratio": -1e8})), none());
    assert_eq!(issues("joint.motion_link", json!({})), none());
    assert_eq!(issues("joint.motion_link", json!({"a": 1, "b": 2})), none());
    for bad in [json!(null), json!("2"), json!(true), json!(1e9), json!(-1e12)] {
        assert_eq!(link(json!({"ratio": bad.clone()})), ["`ratio` must be a number"], "{bad}");
    }
    assert_eq!(link(json!({"a": null})), ["`joint` must be an id or a name"]);
    assert_eq!(link(json!({"b": [1]})), ["`joint` must be an id or a name"]);
    assert_eq!(link(json!({"ia": "0"})), ["`ia` must be a number"]);
    assert_eq!(link(json!({"ib": true})), ["`ib` must be a number"]);
    assert_eq!(link(json!({"offset": null})), ["a joint value must be a number or an expression"]);
    assert_eq!(link(json!({"offset": true})), ["a joint value must be a number or an expression"]);
    assert_eq!(link(json!({"offset": 1e12})), ["a joint value must be a number between -1e9 and 1e9"]);
    assert_eq!(link(json!({"quiet": "no"})), ["`quiet` must be true or false"]);

    // The couplings: a length, a round face, a link to replace.
    let size = "must be a length (mm or an expression) or a round face ({circle: [x, y, z]})";
    let gear = |extra: Value| {
        let mut p = json!({"a": "A", "b": "B", "ra": 20, "rb": "15 mm"});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        issues("joint.gear", p)
    };
    assert_eq!(gear(json!({})), none());
    assert_eq!(gear(json!({"internal": true, "reverse": false, "link": 0, "quiet": true})), none());
    assert_eq!(gear(json!({"internal": null, "reverse": null})), none());
    assert_eq!(gear(json!({"ra": {"circle": [10, 0, 2.5]}, "rb": {"circle": "F1", "occurrence": "G2"}})), none());
    assert_eq!(gear(json!({"ra": {"circle": [10, 0, 2.5], "z": "junk"}})), none());
    assert_eq!(gear(json!({"ra": "x".repeat(1000), "rb": -5})), none());
    assert_eq!(issues("joint.gear", json!({"a": 1, "b": 2})), none());
    assert_eq!(issues("joint.gear", json!({})), none());
    for bad in [json!(null), json!(true), json!([1]), json!({}), json!({"point": [1, 2, 3]}), json!("x".repeat(1001))] {
        assert_eq!(gear(json!({"ra": bad.clone()})), [format!("`ra` {size}")], "{bad}");
        assert_eq!(gear(json!({"rb": bad.clone()})), [format!("`rb` {size}")], "{bad}");
    }
    assert_eq!(gear(json!({"ra": {"circle": 5}})), [why]);
    assert_eq!(gear(json!({"rb": {"circle": ""}})), ["a face name is 1…16384 characters"]);
    assert_eq!(gear(json!({"ra": {"circle": {"x": 1, "y": 2}}})), [why]);
    for bad in [json!(-1), json!(null), json!("0"), json!(1.5), json!(true)] {
        assert_eq!(gear(json!({"link": bad.clone()})), ["no such motion link"], "{bad}");
    }
    assert_eq!(gear(json!({"internal": 1})), ["`internal` must be true or false"]);
    assert_eq!(gear(json!({"reverse": "yes"})), ["`reverse` must be true or false"]);
    assert_eq!(gear(json!({"a": true})), ["`joint` must be an id or a name"]);
    assert_eq!(gear(json!({"ra": 1, "a": null, "link": -1})), ["`joint` must be an id or a name"]);
    let rack = |extra: Value| {
        let mut p = json!({"a": "P", "b": "Rack", "radius": "r"});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        issues("joint.rack_pinion", p)
    };
    assert_eq!(rack(json!({})), none());
    assert_eq!(rack(json!({"radius": {"circle": [0, 0, 0]}, "reverse": true, "link": 3, "quiet": false})), none());
    assert_eq!(rack(json!({"radius": null})), [format!("`radius` {size}")]);
    assert_eq!(rack(json!({"link": 2.5})), ["no such motion link"]);
    assert_eq!(issues("joint.rack_pinion", json!({"a": "P", "b": "Rack"})), none());

    // joint.screw: `joint`, once given, is the form, and `a` and `b` are not read.
    let screw = |p: Value| issues("joint.screw", p);
    assert_eq!(screw(json!({"joint": "Nut", "pitch": 2})), none());
    assert_eq!(screw(json!({"joint": 3, "a": true, "b": null, "pitch": "2 mm", "reverse": true, "link": 1, "quiet": true})), none());
    assert_eq!(screw(json!({"a": "K", "b": 4, "pitch": {"circle": [1, 2, 3]}})), none());
    assert_eq!(screw(json!({})), none());
    assert_eq!(screw(json!({"joint": null, "pitch": 1})), ["`joint` must be an id or a name"]);
    assert_eq!(screw(json!({"joint": true, "a": "K", "b": 4, "pitch": 1})), ["`joint` must be an id or a name"]);
    assert_eq!(screw(json!({"a": true, "b": 4, "pitch": 1})), ["`joint` must be an id or a name"]);
    assert_eq!(screw(json!({"a": "K", "b": null, "pitch": 1})), ["`joint` must be an id or a name"]);
    assert_eq!(screw(json!({"joint": "Nut", "pitch": false})), [format!("`pitch` {size}")]);
    assert_eq!(screw(json!({"joint": "Nut", "pitch": 1, "link": "0"})), ["no such motion link"]);

    // joint.cam: the follower, its radius only for a roller, the tip, the profile.
    let cam = |extra: Value| {
        let mut p = json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "S"}});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        issues("joint.cam", p)
    };
    assert_eq!(cam(json!({})), none());
    assert_eq!(cam(json!({"follower": "knife", "radius": true, "flip": true, "link": 0, "quiet": true})), none());
    assert_eq!(cam(json!({"follower": "flat", "radius": null})), none());
    assert_eq!(cam(json!({"follower": null, "radius": []})), none());
    assert_eq!(cam(json!({"follower": "roller", "radius": "5 mm", "tip": {"circle": [1, 2, 3]}})), none());
    assert_eq!(cam(json!({"follower": "roller", "radius": {"circle": [1, 2, 3]}})), none());
    assert_eq!(cam(json!({"tip": "Origin1"})), none());
    assert_eq!(cam(json!({"tip": {"point": [1, 2, 3]}})), none());
    assert_eq!(cam(json!({"flip": null})), none());
    assert_eq!(issues("joint.cam", json!({"a": 1, "b": 2})), none());
    assert_eq!(issues("joint.cam", json!({})), none());
    assert_eq!(cam(json!({"follower": "round"})), ["`follower` is knife, roller or flat"]);
    assert_eq!(cam(json!({"follower": "Roller"})), ["`follower` is knife, roller or flat"]);
    assert_eq!(cam(json!({"follower": 5})), ["`follower` must be text"]);
    assert_eq!(cam(json!({"follower": "roller"})), none());
    assert_eq!(cam(json!({"follower": "roller", "radius": true})), [format!("`radius` {size}")]);
    assert_eq!(cam(json!({"follower": "roller", "radius": null})), [format!("`radius` {size}")]);
    assert_eq!(cam(json!({"tip": null})), [why]);
    assert_eq!(cam(json!({"tip": 5})), [why]);
    assert_eq!(cam(json!({"tip": {}})), [why]);
    assert_eq!(cam(json!({"tip": {"face": ""}})), ["a face name is 1…16384 characters"]);
    assert_eq!(cam(json!({"flip": "yes"})), ["`flip` must be true or false"]);
    assert_eq!(cam(json!({"link": null})), ["no such motion link"]);
    for ok in [
        json!({"sketch": "S"}),
        json!({"sketch": 4, "index": 2}),
        json!({"sketch": "S", "edges": [], "occurrence": true}),
        json!({"sketch": "S", "index": 0, "x": 1}),
        json!({"edges": [[1, 2, 3]]}),
        json!({"edges": [{"x": 1, "y": 2, "z": 3}], "occurrence": "Cam", "index": "x"}),
        json!({"edges": vec![[1, 2, 3]; 64]}),
    ] {
        assert_eq!(cam(json!({"profile": ok.clone()})), none(), "{ok}");
    }
    let forms = "`profile` is {sketch: id|name} or {edges: [[x, y, z], …]}";
    for (bad, message) in [
        (json!(null), forms),
        (json!("S"), forms),
        (json!([1]), forms),
        (json!({}), forms),
        (json!({"edges": "x"}), forms),
        (json!({"sketch": null}), "`profile.sketch` must be a sketch id or name"),
        (json!({"sketch": [1]}), "`profile.sketch` must be a sketch id or name"),
        (json!({"sketch": "S", "index": -1}), "the sketch has no such closed profile (an open curve makes none)"),
        (json!({"sketch": "S", "index": "0"}), "the sketch has no such closed profile (an open curve makes none)"),
        (json!({"sketch": "S", "index": null}), "the sketch has no such closed profile (an open curve makes none)"),
        (json!({"edges": []}), "pick 1 to 64 edges"),
        (json!({"edges": vec![[1, 2, 3]; 65]}), "pick 1 to 64 edges"),
        (json!({"edges": [[1, 2]]}), "`edges` are finite [x, y, z] points"),
        (json!({"edges": [[1, 2, 3], "x"]}), "`edges` are finite [x, y, z] points"),
        (json!({"edges": [[1, 2, 3]], "occurrence": null}), "an occurrence must be an id or a name"),
        (json!({"edges": [[1, 2, 3]], "occurrence": {}}), "an occurrence must be an id or a name"),
    ] {
        assert_eq!(cam(json!({"profile": bad.clone()})), [message], "{bad}");
    }
    assert_eq!(cam(json!({"profle": 1})), ["unknown key `profle`; did you mean `profile`?"]);
}

/// The schemas say what the handlers need, and take what they read.
#[test]
fn the_remaining_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<MakeJointOriginParams>(),
        &[
            (json!({"face": [1, 2, 3]}), true),
            (json!({"face": "F1", "x": "F2", "name": "Top", "body": "B", "occurrence": "A"}), true),
            (json!({"circle": "F1", "end": "F2", "x": {"face": [1, 2, 3]}}), true),
            (json!({"circle": [1, 2, 3], "z": [0, 0, 1], "x": null}), true),
            (json!({"edge": [1, 2, 3], "on": [0, 0, 0], "corner": {"x": 1, "y": 2, "z": 3}, "z": [1, 0, 0]}), true),
            (json!({"point": [1, 2, 3], "z": [0, 0, 1], "x": [1, 0, 0]}), true),
            (json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}}), true),
            (json!({}), false),
            (json!({"name": "N"}), false),
            (json!({"face": [1, 2]}), false),
            (json!({"point": [1, 2, 3], "end": 5}), false),
            (json!({"frame": {"origin": [0, 0, 0]}}), false),
            (json!({"point": [1, 2, 3], "x": 5}), false),
            (json!({"point": [1, 2, 3], "nme": "N"}), false),
        ],
    );
    takes(
        input_schema_for::<RigidGroupParams>(),
        &[
            (json!({"occurrences": ["A", 2]}), true),
            (json!({"occurrences": ["A"]}), false),
            (json!({"occurrences": vec![1; 1001]}), false),
            (json!({"occurrences": [1, true]}), false),
            (json!({}), false),
        ],
    );
    takes(
        input_schema_for::<ReachParams>(),
        &[
            (json!({"occurrence": "Arm", "point": [1, 2, 3]}), true),
            (
                json!({"occurrence": 2, "point": {"x": 1, "y": 2, "z": 3}, "at": [0, 0, 0], "z": [0, 0, 1], "axis": [1, 0, 0], "joints": ["A", 3], "quiet": true, "continuous": true}),
                true,
            ),
            (json!({"point": [1, 2, 3]}), false),
            (json!({"occurrence": "Arm"}), false),
            (json!({"occurrence": "Arm", "point": [1, 2]}), false),
            (json!({"occurrence": "Arm", "point": [1, 2, 3], "joints": "A"}), false),
            (json!({"occurrence": "Arm", "point": [1, 2, 3], "at": null}), false),
        ],
    );
    takes(
        input_schema_for::<MotionLinkParams>(),
        &[
            (json!({"a": "A", "b": 2, "ratio": 0.5}), true),
            (json!({"a": "A", "b": 2, "ratio": -2, "ia": 1, "ib": 0, "offset": "5 mm", "quiet": true}), true),
            (json!({"a": "A", "b": 2}), false),
            (json!({"a": "A", "ratio": 1}), false),
            (json!({"a": "A", "b": 2, "ratio": "2"}), false),
            (json!({"a": "A", "b": 2, "ratio": 1e12}), false),
            (json!({"a": "A", "b": 2, "ratio": 1, "offset": true}), false),
        ],
    );
    let gear = json!({"a": "A", "b": "B", "ra": 20, "rb": {"circle": [1, 2, 3], "occurrence": "G"}});
    takes(
        input_schema_for::<GearParams>(),
        &[
            (gear.clone(), true),
            (json!({"a": 1, "b": 2, "ra": "r1", "rb": "15 mm", "internal": true, "reverse": true, "link": 0, "quiet": false}), true),
            (json!({"a": "A", "b": "B", "ra": 20}), false),
            (json!({"a": "A", "b": "B", "ra": 20, "rb": true}), false),
            (json!({"a": "A", "b": "B", "ra": 20, "rb": {"point": [1, 2, 3]}}), false),
            (json!({"a": "A", "b": "B", "ra": 20, "rb": 1, "link": -1}), false),
            (json!({"a": "A", "b": "B", "ra": 20, "rb": 1, "link": "0"}), false),
        ],
    );
    takes(
        input_schema_for::<RackPinionParams>(),
        &[
            (json!({"a": "P", "b": "Rack", "radius": 10}), true),
            (json!({"a": "P", "b": "Rack", "radius": "r", "reverse": true, "link": 2}), true),
            (json!({"a": "P", "b": "Rack"}), false),
            (json!({"a": "P", "radius": 10}), false),
        ],
    );
    takes(
        input_schema_for::<ScrewParams>(),
        &[
            (json!({"joint": "Nut", "pitch": 2}), true),
            (json!({"a": "K", "b": "S", "pitch": "2 mm", "reverse": true, "link": 1}), true),
            (json!({"joint": "Nut"}), false),
            (json!({"pitch": 2}), false),
            (json!({"a": "K", "pitch": 2}), false),
            (json!({"joint": "Nut", "pitch": 2, "link": -1}), false),
        ],
    );
    takes(
        input_schema_for::<CamParams>(),
        &[
            (json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "S"}}), true),
            (
                json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": 3, "index": 1}, "follower": "flat", "flip": true, "link": 0, "quiet": true}),
                true,
            ),
            (
                json!({"a": "Cam", "b": "Arm", "profile": {"edges": [[1, 2, 3]], "occurrence": "Cam"}, "follower": "roller", "radius": 5, "tip": {"circle": [1, 2, 3]}}),
                true,
            ),
            (json!({"a": "Cam", "b": "Tappet", "profile": {"edges": []}}), false),
            (json!({"a": "Cam", "b": "Tappet", "profile": {"edges": vec![[1, 2, 3]; 65]}}), false),
            (json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "S", "index": -1}}), false),
            (json!({"a": "Cam", "b": "Tappet", "profile": {}}), false),
            (json!({"a": "Cam", "b": "Tappet"}), false),
            (json!({"a": "Cam", "profile": {"sketch": "S"}}), false),
        ],
    );
    // A roller needs its radius, a screw its joint or its two, a reach its axis's z, a joint
    // origin one of its forms: said in the schema (`conforms` doesn't read these).
    let cam = input_schema_for::<CamParams>();
    assert_eq!(cam.as_value()["if"], json!({"properties": {"follower": {"const": "roller"}}, "required": ["follower"]}));
    assert_eq!(cam.as_value()["then"], json!({"required": ["radius"]}));
    let screw = input_schema_for::<ScrewParams>();
    assert_eq!(screw.as_value()["anyOf"], json!([{"required": ["joint"]}, {"required": ["a", "b"]}]));
    assert_eq!(input_schema_for::<ReachParams>().as_value()["dependentRequired"], json!({"axis": ["z"]}));
    assert_eq!(input_schema_for::<MakeJointOriginParams>().as_value()["anyOf"].as_array().map(Vec::len), Some(5));
}

/// A design with the pieces the couplings join, and a tray with a hole for the joint origins by
/// name: each command run in Warn in every form it takes gives a reply that matches its output
/// schema, with no warning.
#[test]
fn replies_of_the_rest_match_the_output_schema() {
    let ok = |schema: schemars::Schema, id: &str, reply: &Value| matches(schema, id, reply);

    // joint.origin, in the forms of a face, a circle, an edge, a point and a frame.
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    put(&mut s, "component.create", json!({"name": "Tray"}));
    put(&mut s, "solid.box", json!({"length": 300, "width": 100, "height": 1, "corner": [0, 0, 0]}));
    put(&mut s, "solid.hole", json!({"position": [280, 50, 1], "diameter": 6}));
    let name_at = |s: &mut Session, p: Vec3, circle: bool| {
        let st = s.model.state();
        let c = s.doc.find_component("Tray").unwrap_or(0);
        solidcraft_doc::joints::face_name_at(&s.doc, &st, c, p, circle).unwrap_or_default()
    };
    let wall = name_at(&mut s, Vec3::new(283.0, 50.0, 0.5), false);
    let top = name_at(&mut s, Vec3::new(150.0, 50.0, 1.0), false);
    let right = name_at(&mut s, Vec3::new(300.0, 50.0, 0.5), false);
    assert!(![&wall, &top, &right].iter().any(|n| n.is_empty()));
    let schema = || output_schema_for::<MakeJointOriginResult>();
    let mut made = Vec::new();
    for p in [
        json!({"occurrence": "Tray", "face": [150, 50, 1], "name": "TopPoint"}),
        json!({"face": [150, 50, 1], "x": [0, 1, 0]}),
        json!({"face": top.clone(), "name": "TopName", "x": right.clone()}),
        json!({"face": top.clone(), "occurrence": "Tray", "body": null, "x": {"face": [300, 50, 0.5]}, "name": "  Padded  "}),
        json!({"circle": wall.clone(), "end": top.clone(), "x": right.clone(), "name": "Hole"}),
        json!({"circle": [283, 50, 0.5], "name": "HoleAt"}),
        json!({"circle": [283, 50, 0.5], "z": [0, 0, 1], "name": "HoleOut", "end": "not checked here"}),
        json!({"circle": [283, 50, 0.5], "end": top.clone(), "x": [1, 0, 0], "name": "HoleEnd"}),
        json!({"edge": [150, 0, 1], "name": "Edge"}),
        json!({"edge": [150, 0, 1], "on": [150, 50, 1], "corner": [299, 0, 1], "name": "Corner"}),
        json!({"edge": [150, 0, 1], "z": [1, 0, 0], "end": 5, "name": "EdgeZ"}),
        json!({"point": [10, 10, 1], "z": [0, 0, 1], "x": [1, 0, 0], "name": "Point"}),
        json!({"point": [10, 10, 1], "occurrence": "Tray", "name": null}),
        json!({"frame": {"origin": [5, 5, 1], "z": [0, 0, 1], "x": [1, 0, 0]}, "occurrence": "Tray", "name": "Frame"}),
        json!({"frame": {"origin": [5, 5, 1], "z": [0, 0, 1]}, "occurrence": 0}),
    ] {
        let r = put(&mut s, "joint.origin", p.clone());
        ok(schema(), "joint.origin", &r);
        made.push(r["origin"].as_str().unwrap_or_default().to_string());
    }
    assert_eq!(made[0], "TopPoint");
    assert_eq!(made[1], "Joint Origin1");
    assert_eq!(made[3], "Padded");
    assert_eq!(made[12], "Joint Origin2");
    assert_eq!(s.doc.assembly.origins.len(), made.len());

    // joint.rigid_group, by name and by id.
    let mut g = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut ids = Vec::new();
    for (name, x) in [("A", 0), ("B", 20), ("C", 40), ("D", 60)] {
        put(&mut g, "component.activate", json!({"component": "root"}));
        let made = put(&mut g, "component.create", json!({"name": name, "grounded": false}));
        put(&mut g, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
        ids.push(made["occurrence"].clone());
    }
    let group = put(&mut g, "joint.rigid_group", json!({"occurrences": ["A", "B", "C"]}));
    ok(output_schema_for::<RigidGroupResult>(), "joint.rigid_group", &group);
    assert!(group["joints"].as_array().is_some_and(|j| j.len() == 2 && j.iter().all(Value::is_u64)), "{group}");
    let by_id = put(&mut g, "joint.rigid_group", json!({"occurrences": [ids[3].clone(), 0]}));
    ok(output_schema_for::<RigidGroupResult>(), "joint.rigid_group", &by_id);
    assert!(by_id["joints"].as_array().is_some_and(|j| j.len() == 1), "{by_id}");

    // The couplings and motion links.
    let mut s = rig(ParamCheck::Warn);
    let coupled = |s: &mut Session, id: &str, p: Value| {
        let r = put(s, id, p.clone());
        ok(output_schema_for::<CouplingResult>(), id, &r);
        let quiet = p["quiet"] == json!(true);
        assert!(r["link"].is_u64() && r["ratio"].is_number() && r.get("dof").is_some() != quiet, "{id} {p}: {r}");
        r
    };
    let gear = coupled(&mut s, "joint.gear", json!({"a": "G1", "b": "G2", "ra": 10, "rb": "20 mm"}));
    assert!(gear["link"] == 0 && gear["ratio"].as_f64().is_some_and(|r| (r + 0.5).abs() < 1e-9), "{gear}");
    let again = coupled(
        &mut s,
        "joint.gear",
        json!({"a": "G1", "b": "G2", "ra": {"occurrence": "G1", "circle": [10, 0, 2.5]}, "rb": {"occurrence": "G2", "circle": [50, 0, 2.5]},
            "internal": true, "reverse": true, "link": 0}),
    );
    assert_eq!(again["link"], 0, "{again}");
    let quiet = coupled(&mut s, "joint.gear", json!({"a": "G1", "b": "G2", "ra": 10, "rb": 20, "link": 0, "quiet": true, "internal": null}));
    assert!(quiet.get("dof").is_none() && quiet.get("fixed").is_none(), "{quiet}");
    let rack = coupled(&mut s, "joint.rack_pinion", json!({"a": "P", "b": "Rack", "radius": 10}));
    assert_eq!(rack["link"], 1, "{rack}");
    coupled(&mut s, "joint.rack_pinion", json!({"a": "Rack", "b": "P", "radius": "10 mm", "reverse": true, "link": 1}));
    let screw = coupled(&mut s, "joint.screw", json!({"joint": "Nut", "pitch": 2}));
    assert_eq!(screw["link"], 2, "{screw}");
    coupled(&mut s, "joint.screw", json!({"joint": "Nut", "pitch": "2 mm", "reverse": true, "link": 2, "a": true}));
    let apart = coupled(&mut s, "joint.screw", json!({"a": "K", "b": "Slide2", "pitch": 3}));
    assert_eq!(apart["link"], 3, "{apart}");
    let linked = put(&mut s, "joint.motion_link", json!({"a": "G1", "b": "X", "ratio": 2}));
    ok(output_schema_for::<SolveResult>(), "joint.motion_link", &linked);
    let quiet = put(&mut s, "joint.motion_link", json!({"a": "G1", "b": "K", "ratio": 1, "offset": "5 deg", "ia": 0, "ib": 0, "quiet": true}));
    ok(output_schema_for::<SolveResult>(), "joint.motion_link", &quiet);
    assert!(quiet.get("dof").is_none() && quiet.get("fixed").is_none(), "{quiet}");
    let list = put(&mut s, "joint.list", json!({}));
    assert!(list["links"].as_array().is_some_and(|l| l.len() == 6), "{list}");

    // joint.reach: an arm.
    let mut a = Session { param_check: ParamCheck::Warn, ..Session::default() };
    put(&mut a, "component.activate", json!({"component": "root"}));
    let base = put(&mut a, "component.create", json!({"name": "Base", "grounded": false}));
    put(&mut a, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [-1, -1, -1]}));
    put(&mut a, "occurrence.ground", json!({"occurrence": base["occurrence"], "grounded": true}));
    for (name, x) in [("Upper", 30), ("Fore", 80)] {
        put(&mut a, "component.activate", json!({"component": "root"}));
        put(&mut a, "component.create", json!({"name": name, "grounded": false}));
        put(&mut a, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [x, -1, -1]}));
    }
    pin(&mut a, "revolute", "Shoulder", "Base", "Upper", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    pin(&mut a, "revolute", "Elbow", "Upper", "Fore", [60.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    put(&mut a, "joint.drive", json!({"joint": "Elbow", "value": 30}));
    let reach = |a: &mut Session, p: Value| {
        let r = put(a, "joint.reach", p.clone());
        ok(output_schema_for::<ReachResult>(), "joint.reach", &r);
        r
    };
    // Fore's tip, with the elbow at 30°.
    let tip = json!([60.0 + 40.0 * 30f64.to_radians().cos(), 20.0, 0.0]);
    let moved = reach(&mut a, json!({"occurrence": "Fore", "at": tip, "point": [50, 50, 0]}));
    assert!(moved["reached"] == json!(true) && moved["moved"] == json!(true) && moved["occurrence"] == "Fore:1", "{moved}");
    assert!(moved["stopped_by_limit"].is_null() && moved["stopped_by_loop"].is_null() && moved.get("error_deg").is_none(), "{moved}");
    assert!(moved["values"].as_array().is_some_and(|v| v.len() == 2 && v[0]["values"][0]["unit"] == "deg"), "{moved}");
    let default_at =
        reach(&mut a, json!({"occurrence": 2, "point": {"x": 40, "y": 30, "z": 0}, "quiet": true, "continuous": true, "joints": ["Shoulder"]}));
    assert!(default_at["continuous"] == json!(true) && default_at.get("dof").is_none(), "{default_at}");
    let turned = reach(&mut a, json!({"occurrence": "Fore", "point": [30, 70, 0], "z": [0, 1, 0], "axis": [1, 0, 0]}));
    assert!(turned["error_deg"].is_number(), "{turned}");
    let turned = reach(&mut a, json!({"occurrence": "Fore", "point": [0, 80, 0], "z": [0, 1, 0]}));
    assert!(turned["error_deg"].is_number() && turned["reached"].is_boolean(), "{turned}");
    put(&mut a, "joint.limits", json!({"joint": "Elbow", "min": 20, "max": 40}));
    let limited = reach(&mut a, json!({"occurrence": "Fore", "point": [-150, 0, 0]}));
    assert!(limited["stopped_by_limit"].as_array().is_some_and(|l| !l.is_empty() && l[0]["limit"] == "min"), "{limited}");
    // A four-bar's rocker pulled past its toggle position: the loop that stops it is named.
    let mut f = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let (o4, p0) = ((45.0f64, 0.0f64), (0.0f64, 20.0f64));
    let (dx, dy) = (o4.0 - p0.0, o4.1 - p0.1);
    let l = dx.hypot(dy);
    let along = (50.0f64.powi(2) - 40.0f64.powi(2) + l * l) / (2.0 * l);
    let off = (50.0f64.powi(2) - along * along).sqrt();
    let q0 = (p0.0 + dx * along / l - dy * off / l, p0.1 + dy * along / l + dx * off / l);
    for (name, c) in [("Ground", (0.0, 0.0)), ("Crank", p0), ("Coupler", ((p0.0 + q0.0) / 2.0, (p0.1 + q0.1) / 2.0)), ("Rocker", q0)] {
        put(&mut f, "component.activate", json!({"component": "root"}));
        let made = put(&mut f, "component.create", json!({"name": name, "grounded": false}));
        put(&mut f, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [c.0, c.1, -5]}));
        if name == "Ground" {
            put(&mut f, "occurrence.ground", json!({"occurrence": made["occurrence"], "grounded": true}));
        }
    }
    let z = [0.0, 0.0, 1.0];
    pin(&mut f, "revolute", "Crank", "Ground", "Crank", [0.0, 0.0, 0.0], z);
    pin(&mut f, "revolute", "Crank pin", "Crank", "Coupler", [p0.0, p0.1, 0.0], z);
    pin(&mut f, "revolute", "Rocker pin", "Coupler", "Rocker", [q0.0, q0.1, 0.0], z);
    pin(&mut f, "revolute", "Rocker", "Ground", "Rocker", [o4.0, o4.1, 0.0], z);
    let start = (q0.1 - o4.1).atan2(q0.0 - o4.0) + 100f64.to_radians();
    let past = [o4.0 + 40.0 * start.cos(), o4.1 + 40.0 * start.sin(), 0.0];
    let stuck = reach(&mut f, json!({"occurrence": "Rocker", "at": [q0.0, q0.1, 0.0], "point": past}));
    assert_eq!(stuck["stopped_by_loop"], json!({"joint": "Rocker pin"}), "{stuck}");
    let fixed = reach(&mut a, json!({"occurrence": "Base", "point": [50, 50, 0]}));
    assert!(fixed["reason"].is_string() && fixed["reached"] == json!(false) && fixed["values"] == json!([]), "{fixed}");
}

/// A cam in every form: a sketch's profile or picked edges, a knife, a roller and a flat face, a
/// translating and an oscillating follower, a link replaced. The replies match the output schema.
#[test]
fn cam_replies_match_the_output_schema() {
    let mut s = cam_rig(ParamCheck::Warn);
    let cam = |s: &mut Session, p: Value| {
        let r = put(s, "joint.cam", p.clone());
        matches(output_schema_for::<CamResult>(), "joint.cam", &r);
        assert!(r["link"].is_u64() && r["lift"]["unit"].is_string() && r["reverse"].is_boolean() && r.get("ratio").is_none(), "{p}: {r}");
        r
    };
    let knife = cam(&mut s, json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    assert!(knife["lift"]["unit"] == "mm" && knife["follower_offset"].is_number(), "{knife}");
    let sized = cam(
        &mut s,
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch", "index": 0}, "follower": "roller", "radius": "5 mm", "link": 0, "quiet": true}),
    );
    assert!(sized.get("dof").is_none(), "{sized}");
    cam(
        &mut s,
        json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[10, 30, 5]], "occurrence": "Cam"}, "follower": "flat", "flip": true, "link": 0}),
    );
    cam(
        &mut s,
        json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[10, 30, 5]]}, "follower": "knife", "radius": true, "tip": {"occurrence": "Tappet", "point": [45, 0, 2.5]}, "link": 0}),
    );
    // An oscillating follower: its tip, no offset in the reply.
    // The roller's centre is where the arm's circle (50 about the pivot) meets the circle of
    // radius 30 + 5 about the cam's centre.
    let (pivot, centre) = ((-50.0f64, 40.0f64), (10.0f64, 0.0f64));
    let (dx, dy) = (centre.0 - pivot.0, centre.1 - pivot.1);
    let l = dx.hypot(dy);
    let a = (50.0f64.powi(2) - 35.0f64.powi(2) + l * l) / (2.0 * l);
    let h = (50.0f64.powi(2) - a * a).sqrt();
    let start = [pivot.0 + dx * a / l - dy * h / l, pivot.1 + dy * a / l + dx * h / l, 2.5];
    let swing = cam(
        &mut s,
        json!({"a": "Cam", "b": "Arm", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": 5, "tip": {"occurrence": "Arm", "point": start}}),
    );
    assert!(swing["lift"]["unit"] == "deg" && swing.get("follower_offset").is_none(), "{swing}");
}

/// A misspelt key still runs in Warn, says so, and the reply still matches its schema.
#[test]
fn a_misspelt_key_is_reported_in_warn_for_the_rest() {
    let mut s = rig(ParamCheck::Warn);
    let calls = [
        ("joint.origin", json!({"occurrence": "Base", "point": [0, 0, 0], "nme": "A"}), "name"),
        ("joint.rigid_group", json!({"occurrences": ["G1", "G2"], "occurences": 1}), "occurrences"),
        ("joint.reach", json!({"occurrence": "G2", "point": [30, 20, 0], "quiet": true, "quet": true}), "quiet"),
        ("joint.motion_link", json!({"a": "P", "b": "X", "ratio": 1, "ratjo": 1}), "ratio"),
        ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 10, "rb": 20, "internl": true}), "internal"),
        ("joint.rack_pinion", json!({"a": "P", "b": "Rack", "radius": 10, "reverce": true}), "reverse"),
        ("joint.screw", json!({"joint": "Nut", "pitch": 2, "pich": 2}), "pitch"),
    ];
    for (id, p, near) in calls {
        let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let warnings = reply["param_warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.len() == 1 && warnings[0].as_str().is_some_and(|w| w.contains("unknown key") && w.contains(near)), "{id}: {reply}");
        let out = crate::cmd::find_command(id).and_then(|c| c.output).map(|f| f()).unwrap_or_else(|| panic!("{id}"));
        assert!(conforms(out.as_value(), out.as_value(), &reply), "{id}: {reply} doesn't match {}", out.as_value());
    }
    let mut c = cam_rig(ParamCheck::Warn);
    let p = json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follwer": "flat"});
    let reply = put(&mut c, "joint.cam", p);
    assert!(reply["param_warnings"].as_array().is_some_and(|w| w.len() == 1 && w[0].as_str().is_some_and(|w| w.contains("follower"))), "{reply}");
    let out = output_schema_for::<CamResult>();
    assert!(conforms(out.as_value(), out.as_value(), &reply), "{reply}");
}

/// A wrong-typed key the handlers read as if it were left out is refused in strict (the stricter
/// reading), and the call works with the check off.
#[test]
fn strict_refuses_wrong_types_the_handlers_ignore_in_the_rest() {
    let cases = [
        ("joint.origin", json!({"occurrence": "Base", "point": [0, 0, 0], "name": 5})),
        ("joint.origin", json!({"occurrence": "Base", "point": [0, 0, 0], "z": "up"})),
        ("joint.reach", json!({"occurrence": "G2", "point": [30, 20, 0], "quiet": "yes"})),
        ("joint.motion_link", json!({"a": "G1", "b": "X", "ratio": 1, "ia": "0"})),
        ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 10, "rb": 20, "reverse": "no"})),
        ("joint.rack_pinion", json!({"a": "P", "b": "Rack", "radius": 10, "quiet": 1})),
        ("joint.screw", json!({"joint": "Nut", "pitch": 2, "reverse": "x"})),
    ];
    for (id, p) in cases {
        let works = |level| rig(level).execute(id, &p).is_ok();
        assert!(works(ParamCheck::Off) && !works(ParamCheck::Strict), "{id} {p}");
    }
    let cam = json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": 5});
    let works = |level| cam_rig(level).execute("joint.cam", &cam).is_ok();
    assert!(works(ParamCheck::Off) && !works(ParamCheck::Strict), "{cam}");
}
