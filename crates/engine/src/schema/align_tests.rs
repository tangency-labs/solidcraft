use serde_json::{Value, json};

use super::AlignParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

/// Box `A` (10 x 10 x 10 at the origin) and `B` (10 x 20 x 10 at (50, 0, 20)), and sketch `S` on
/// XY with a circle `ring` at (0, 5) and a point `p` at (20, 0).
fn scene(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"})),
        ("solid.box", json!({"length": 10, "width": 20, "height": 10, "corner": [50, 0, 20], "body_name": "B", "operation": "new"})),
        ("sketch.create", json!({"plane": "XY", "name": "S"})),
        ("sketch.circle.center", json!({"center": [0, 5], "radius": 3, "id": "ring"})),
        ("sketch.point", json!({"point": [20, 0], "id": "p"})),
        ("sketch.finish", json!({})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

/// `base` for body `B` with `extra`'s keys added.
fn call(extra: Value) -> Value {
    let mut p = json!({"bodies": ["B"]});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    p
}

fn messages(p: &Value) -> Vec<String> {
    issues("solid.align", p).into_iter().map(|i| i.message).collect()
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<AlignParams>(),
        &[
            "angle",
            "axes",
            "bodies",
            "body_name",
            "body_names",
            "flip",
            "from",
            "from_face",
            "from_frame",
            "from_plane",
            "name",
            "participants",
            "to",
            "to_face",
            "to_frame",
            "to_plane",
        ],
    );
    let schema = input_schema_for::<AlignParams>();
    assert_eq!(schema.as_value()["required"], json!(["bodies"]));
}

/// Calls that fail today get the same message in strict, on a design with the bodies, a sketch
/// and its geometry (the handler's first check is that the bodies exist).
#[test]
fn strict_keeps_todays_errors() {
    let long = "q".repeat(300);
    let cases = [
        json!({}),
        json!({"bodies": null, "from": [0, 0, 0], "to": [1, 1, 1]}),
        json!({"bodies": [], "from": [0, 0, 0], "to": [1, 1, 1]}),
        json!({"bodies": 5, "from": [0, 0, 0], "to": [1, 1, 1]}),
        json!({"bodies": ["B", 5], "from": [0, 0, 0], "to": [1, 1, 1]}),
        call(json!({})),
        call(json!({"from": [0, 0, 0]})),
        call(json!({"to": [0, 0, 0]})),
        call(json!({"from": [0, 0], "to": [0, 0, 0]})),
        call(json!({"from": null, "to": [0, 0, 0]})),
        call(json!({"from": "nowhere", "to": [0, 0, 0]})),
        call(json!({"from": {"x": 1, "y": 2, "z": 3}, "to": [0, 0, 0]})),
        call(json!({"from": {"snap": "vertex"}, "to": [0, 0, 0]})),
        call(json!({"from": {"at": "here"}, "to": [0, 0, 0]})),
        call(json!({"from": [0, 0, 0], "to": {"at": [1, 2]}})),
        call(json!({"from": [0, 0, 0], "to": "centre"})),
        call(json!({"from": "origin", "to_face": null})),
        call(json!({"from_plane": null, "to": [0, 0, 0]})),
        call(json!({"from_plane": true, "to": [0, 0, 0]})),
        call(json!({"from_plane": -1, "to": [0, 0, 0]})),
        call(json!({"from_plane": {"origin": [0, 0]}, "to": [0, 0, 0]})),
        call(json!({"from_plane": {}, "to": [0, 0, 0]})),
        call(json!({"from": [0, 0, 0], "to_plane": [1]})),
        call(json!({"from_frame": 5, "to": [0, 0, 0]})),
        call(json!({"from_frame": {}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"frame": {}}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": true, "point": "p"}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": null, "point": "p"}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S"}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S", "point": "p", "curve": "ring"}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S", "point": ""}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S", "point": long}, "to": [0, 0, 0]})),
        call(json!({"from": [0, 0, 0], "to_frame": {"sketch": "S", "point": 5}})),
        call(json!({"from": [0, 0, 0], "to_frame": {"face": [1, 2]}})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "axes": "x"})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "axes": [true, false]})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "axes": [true, 1, false]})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "axes": [false, false, false]})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "participants": 5})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "participants": ["Nope"]})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "participants": ["B", false]})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "participants": 5, "body_names": 6})),
        call(json!({"from": [0], "to": [1]})),
        call(json!({"from_plane": 7.5, "to_plane": true})),
        call(json!({"from": [0, 0, 0], "to": [1, 1, 1], "body_names": [7]})),
    ];
    for p in &cases {
        let error = |level: ParamCheck| scene(level).execute("solid.align", p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
    // The ones that fail before any body is looked up, with the shared helper's session.
    assert_strict_keeps_todays_errors(&[
        ("solid.align", json!({})),
        ("solid.align", json!({"bodies": []})),
        ("solid.align", json!({"bodies": 5})),
        ("solid.align", json!({"bodies": [1]})),
    ]);
}

/// A side that has a plane or a frame is read from that, and one that has a face that is a point
/// is read from the face: the keys the handler does not read in that case are not checked, so a
/// call that works today gets no warning.
#[test]
fn keys_the_handler_does_not_read_are_not_checked() {
    let calls = [
        // `from` is not read when `from_face` is a point.
        call(json!({"from_face": [55, 10, 20], "from": "garbage", "to_plane": "XY"})),
        call(json!({"from_face": [55, 10, 20], "from": null, "to_plane": "XY"})),
        call(json!({"from_face": [55, 10, 20], "from": {"snap": 5}, "to_plane": "XY"})),
        call(json!({"from_face": [55, 10, 20], "to_face": [5, 5, 0], "to": 17})),
        // A null face is a missing one.
        call(json!({"from_face": null, "from": [50, 0, 20], "to": [0, 0, 0]})),
        // A null frame is a missing one; so is a face that is null beside a frame.
        call(json!({"from_frame": null, "from": [50, 0, 20], "to": "origin"})),
        call(json!({"from": [50, 0, 20], "to_frame": null, "to": [1, 2, 3]})),
        // A snap with no `snap` snaps to a vertex; the object's other keys are not read.
        call(json!({"from": {"at": [50, 0, 20], "note": 1}, "to": {"snap": null, "at": [0, 0, 0]}})),
        // `axes` and `angle` may be `null`.
        call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "axes": null, "angle": null, "flip": null})),
    ];
    for p in &calls {
        assert!(scene(ParamCheck::Off).execute("solid.align", p).is_ok(), "{p} works today");
        assert!(messages(p).is_empty(), "{p}: {:?}", messages(p));
        let reply = scene(ParamCheck::Warn).execute("solid.align", p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(reply.get("param_warnings").is_none(), "{p}: {reply}");
        assert!(scene(ParamCheck::Strict).execute("solid.align", p).is_ok(), "{p}");
    }
    // With a plane or a frame the point and face are not read: the handler refuses the call, in
    // its own words, and the contract leaves that to it.
    for p in [
        call(json!({"from_plane": "XY", "from": "garbage", "to": [0, 0, 0]})),
        call(json!({"from_plane": "XY", "from_face": "garbage", "to": [0, 0, 0]})),
        call(json!({"from_frame": {"face": [55, 10, 20]}, "from": 5, "to": [0, 0, 0]})),
    ] {
        assert!(messages(&p).is_empty(), "{p}: {:?}", messages(&p));
        let error = |level: ParamCheck| scene(level).execute("solid.align", &p).err().map(|e| e.to_string());
        assert!(error(ParamCheck::Off).is_some_and(|e| e.contains("not both")), "{p}");
        assert_eq!(error(ParamCheck::Strict), error(ParamCheck::Off), "{p}");
    }
}

/// Keys of the wrong type that the handler reads as left out are refused by the contract: in Warn
/// the call still works, with a warning naming the key.
#[test]
fn the_contract_refuses_what_the_handler_ignores() {
    let cases = [
        (call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "flip": "yes"})), "flip"),
        (call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "angle": true})), "angle"),
        (call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "angle": []})), "angle"),
        (call(json!({"from_face": "junk", "from": [50, 0, 20], "to": [0, 0, 0]})), "from_face"),
        (call(json!({"from": {"snap": 5, "at": [50, 0, 20]}, "to": [0, 0, 0]})), "from.snap"),
    ];
    for (p, key) in cases {
        assert!(scene(ParamCheck::Off).execute("solid.align", &p).is_ok(), "{p} works today");
        let warned = scene(ParamCheck::Warn).execute("solid.align", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{p}: {warned}");
        let error = scene(ParamCheck::Strict).execute("solid.align", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{p}: {error}");
    }
}

/// The messages the check gives, which are the handler's.
#[test]
fn the_check_names_what_is_wrong_with_a_key() {
    let go = |extra: Value| messages(&call(extra));
    assert_eq!(messages(&json!({})), ["`bodies` must list the bodies to move"]);
    assert_eq!(messages(&json!({"bodies": [5]})), ["`bodies` must list names; the entry 1 is 5"]);
    assert_eq!(go(json!({})), ["give `from`, `from_face` or `from_plane` ([x, y, z], \"origin\", or a plane)"]);
    assert_eq!(go(json!({"from": "origin"})), ["give `to`, `to_face` or `to_plane` ([x, y, z], \"origin\", or a plane)"]);
    assert_eq!(go(json!({"from": "ORIGIN", "to": {"snap": "vertex"}})), ["`to.at` must be [x, y, z]"]);
    assert_eq!(go(json!({"from": "origin", "to": "origin", "axes": [false, false, false]})), ["`axes` must enable at least one of X, Y and Z"]);
    assert_eq!(go(json!({"from": "origin", "to": "origin", "axes": [true]})), ["`axes` must be [x, y, z] booleans"]);
    assert_eq!(
        go(json!({"from": "origin", "to": "origin", "angle": "x".repeat(5000)})),
        ["`angle` must be an expression of at most 4096 characters"]
    );
    assert_eq!(go(json!({"from": "origin", "to": "origin", "participants": 5})), ["`participants` must be a list of names; it is 5"]);
    assert_eq!(go(json!({"from": "origin", "to": "origin", "bodies2": 1})).len(), 1);
    assert_eq!(go(json!({"from": "origin", "to": "origin", "flipp": true})), ["unknown key `flipp`; did you mean `flip`?"]);
    assert_eq!(go(json!({"from": "origin", "to": "origin", "flip": 1})), ["`flip` must be true or false"]);
}

/// The schema takes what the handler takes where the keys alone decide it. Each call is checked
/// against the schema and against the handler, with the check off.
#[test]
fn the_schema_takes_what_the_handler_takes() {
    let schema = input_schema_for::<AlignParams>();
    let schema = schema.as_value();
    let ring = json!({"sketch": "S", "curve": "ring"});
    let calls = [
        call(json!({"from": [50, 0, 20], "to": [0, 0, 0]})),
        call(json!({"from": "Origin", "to": {"snap": "vertex", "at": [0, 0, 0]}})),
        call(json!({"from": {"at": {"x": 50, "y": 0, "z": 20}}, "to": [0, 0, 0], "axes": [false, false, true]})),
        call(json!({"from_face": [55, 10, 20], "to_plane": "XY"})),
        call(json!({"from_face": [55, 10, 20], "to_face": [5, 5, 0], "flip": true, "angle": "30 deg"})),
        call(json!({"from_plane": {"origin": [0, 0, 5], "normal": [0, 0, 1]}, "to": [0, 0, 7]})),
        call(json!({"from_face": {"x": 55, "y": 10, "z": 20}, "to_plane": "XY"})),
        call(json!({"from_frame": {"face": [55, 10, 20]}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"frame": {"origin": [55, 10, 20], "z": [0, 0, -1]}}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"face": [55, 10, 20]}, "to_frame": ring, "angle": 90})),
        call(json!({"from_frame": {"face": [55, 10, 20]}, "to_frame": ring, "angle": 90})),
        call(json!({"from_frame": {"sketch": "S", "point": "p"}, "to": [20, 0, 5]})),
        json!({"bodies": "B", "from": [50, 0, 20], "to": [0, 0, 0], "name": "Move", "participants": ["B"]}),
        json!({}),
        json!({"bodies": ["B"]}),
        json!({"bodies": [], "from": [50, 0, 20], "to": [0, 0, 0]}),
        call(json!({"from": [50, 0, 20]})),
        call(json!({"to": [50, 0, 20]})),
        call(json!({"from": [50, 0], "to": [0, 0, 0]})),
        call(json!({"from": {"x": 50, "y": 0, "z": 20}, "to": [0, 0, 0]})),
        call(json!({"from": {"snap": "vertex"}, "to": [0, 0, 0]})),
        call(json!({"from_plane": 7.5, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S"}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S", "point": "p", "curve": "ring"}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S", "point": ""}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"sketch": "S", "point": "q".repeat(257)}, "to": [0, 0, 0]})),
        call(json!({"from_frame": 5, "to": [0, 0, 0]})),
        call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "axes": [true]})),
        call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "flip": "yes"})),
    ];
    for p in &calls {
        // The schema takes a call when the handler does and the check has nothing to say (a
        // `flip` that is not true or false runs today, and is a warning).
        let takes = scene(ParamCheck::Off).execute("solid.align", p).is_ok();
        assert_eq!(conforms(schema, schema, p), takes && messages(p).is_empty(), "{p}: {:?}", messages(p));
    }
}

/// Each form of the command, in Warn: a reply that matches the output schema and carries no
/// warning, since Warn runs a call the contract refuses as well. The same calls pass strict.
#[test]
fn replies_match_the_output_schema() {
    let ring = json!({"sketch": "S", "curve": "ring"});
    let calls = [
        call(json!({"from": [50, 0, 20], "to": [0, 0, 0]})),
        call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "axes": [true, false, true]})),
        call(json!({"from": "ORIGIN", "to": {"snap": "vertex", "at": [10, 20, 30]}})),
        call(json!({"from": {"snap": "edge_mid", "at": [50, 0, 25]}, "to": {"at": [0, 0, 0]}})),
        call(json!({"from_face": [55, 10, 20], "to_face": [5, 5, 10]})),
        call(json!({"from_face": [55, 10, 20], "to_face": [5, 5, 10], "flip": true, "angle": "30 deg"})),
        call(json!({"from_face": {"x": 55, "y": 10, "z": 20}, "to_plane": "XY"})),
        call(json!({"from_plane": {"origin": [0, 0, 5], "normal": [0, 0, 1]}, "to": [0, 0, 7]})),
        call(json!({"from_plane": "XY", "to_plane": {"origin": [0, 0, 5], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        call(json!({"from_frame": {"face": [55, 10, 20]}, "to_frame": ring, "angle": 90})),
        call(json!({"from_frame": {"sketch": "S", "point": "p"}, "to": [20, 0, 5], "flip": true})),
        call(json!({"from_frame": {"point": [55, 10, 20]}, "to": [0, 0, 0]})),
        call(json!({"from_frame": {"frame": {"origin": [55, 10, 20], "z": [0, 0, -1]}}, "to": [0, 0, 0]})),
        call(json!({"from": [50, 0, 20], "to_frame": {"sketch": "S", "point": "p"}})),
        json!({"bodies": "B", "from": [50, 0, 20], "to": [0, 0, 0], "name": "Move B", "participants": ["B", "A"]}),
        json!({"bodies": ["A", "B"], "from": [0, 0, 0], "to": [0, 0, 5], "body_name": "unused", "participants": "*"}),
    ];
    let schema = output_schema_for::<FeatureResult>();
    for p in &calls {
        let reply = scene(ParamCheck::Warn).execute("solid.align", p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{p}: {reply}");
        assert!(reply.get("param_warnings").is_none(), "{p}: {reply}");
        assert!(messages(p).is_empty(), "{p}: {:?}", messages(p));
        let strict = scene(ParamCheck::Strict).execute("solid.align", p).unwrap_or_else(|e| panic!("{p}: {e}"));
        // The replies are the same but for the time it took.
        let same = |mut r: Value| {
            r.as_object_mut().map(|o| o.remove("ms"));
            r
        };
        let off = scene(ParamCheck::Off).execute("solid.align", p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(same(strict), same(reply.clone()), "{p}");
        assert_eq!(same(off), same(reply), "{p}");
    }
    // A misspelt key still runs in Warn, and says so, with a reply that matches.
    let p = call(json!({"from": [50, 0, 20], "to": [0, 0, 0], "axis": [true, true, true]}));
    let warned = scene(ParamCheck::Warn).execute("solid.align", &p).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(warned["param_warnings"], json!(["unknown key `axis`; did you mean `axes`?"]), "{warned}");
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
}

/// The patterns and schemas the tests above can't read (`conforms` skips patterns and `contains`).
#[test]
fn the_schema_text_is_pinned() {
    let schema = input_schema_for::<AlignParams>();
    let all = schema.as_value().to_string();
    assert!(all.contains(r#""pattern":"^[Oo][Rr][Ii][Gg][Ii][Nn]$""#), "{all}");
    assert!(all.contains(r#""contains":{"const":true}"#), "{all}");
    assert!(all.contains(r#""maxLength":256"#), "{all}");
    assert!(schema.as_value()["properties"]["from"]["description"].is_string());
    for key in ["from", "to"] {
        let any = format!(
            r#"{{"anyOf":[{{"required":["{key}"]}},{{"required":["{key}_face"]}},{{"required":["{key}_plane"]}},{{"required":["{key}_frame"]}}]}}"#
        );
        assert!(all.contains(&any), "{key}: {all}");
    }
    // `origin` in any case is what the handler reads, and nothing around it.
    let origin = |t: &str| scene(ParamCheck::Off).execute("solid.align", &call(json!({"from": t, "to": [0, 0, 0]}))).is_ok();
    assert!(origin("origin") && origin("OrIgIn") && !origin(" origin") && !origin("origin\n"));
}
