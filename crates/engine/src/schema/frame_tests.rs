use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;
use solidcraft_doc::frame::{self, MAX_MEMBERS};

use super::{
    FrameMemberParams, ProfileInsertParams, ProfileInsertResult, ProfileListParams, ProfileListResult, ProfilePropertiesParams,
    ProfilePropertiesResult,
};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

fn tube() -> Value {
    json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3})
}

/// The id of the first sketch of `s`.
fn first_sketch(s: &Session) -> u64 {
    s.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).unwrap_or_else(|| panic!("no sketch"))
}

/// A session with the 600 × 400 rectangle `Frame` (its four line ids returned) and the sketch
/// `Section`, two concentric circles: a tube's wall.
fn frame_session(level: ParamCheck) -> (Session, Vec<String>) {
    let mut s = Session { param_check: level, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("sketch.create", json!({"plane": "XY", "name": "Frame"}));
    let rectangle = run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run("sketch.finish", json!({}));
    run("sketch.create", json!({"plane": "XY", "name": "Section"}));
    run("sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run("sketch.circle.center", json!({"center": [0, 0], "radius": 17}));
    run("sketch.finish", json!({}));
    let lines =
        rectangle["created"]["curves"].as_array().map(|c| c.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()).unwrap_or_default();
    (s, lines)
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<FrameMemberParams>(),
        &[
            "anchor",
            "angle",
            "body_name",
            "exact_regions",
            "lines",
            "locate",
            "mirror",
            "name",
            "names",
            "participants",
            "profile",
            "profile_sketch",
            "profiles",
            "sketch",
        ],
    );
    assert_keys(input_schema_for::<ProfileListParams>(), &["family"]);
    assert_keys(input_schema_for::<ProfilePropertiesParams>(), &["density", "profile"]);
    assert_keys(input_schema_for::<ProfileInsertParams>(), &["anchor", "angle", "at", "fixed", "profile", "sketch"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let sketch = {
        let mut s = Session::default();
        sketch_rectangle(&mut s);
        first_sketch(&s)
    };
    let member = |extra: Value| {
        let mut p = json!({"sketch": sketch, "profile": tube()});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        ("frame.member", p)
    };
    let bar = json!({"kind": "round_bar", "diameter": 10});
    let insert = |extra: Value| {
        let mut p = json!({"profile": bar});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        ("frame.profile.insert", p)
    };
    assert_strict_keeps_todays_errors(&[
        // frame.member
        ("frame.member", json!({})),
        ("frame.member", json!({"sketch": null, "profile": tube()})),
        ("frame.member", json!({"sketch": true, "profile": tube()})),
        ("frame.member", json!({"sketch": [1], "profile": tube()})),
        ("frame.member", json!({"sketch": sketch})),
        ("frame.member", json!({"sketch": sketch, "profile": tube(), "profile_sketch": sketch})),
        ("frame.member", json!({"sketch": sketch, "profile": tube(), "profile_sketch": null})),
        ("frame.member", json!({"sketch": sketch, "profile_sketch": true})),
        ("frame.member", json!({"sketch": sketch, "profile_sketch": null})),
        ("frame.member", json!({"sketch": sketch, "profile_sketch": sketch})),
        ("frame.member", json!({"sketch": sketch, "profile_sketch": "Nowhere"})),
        ("frame.member", json!({"sketch": sketch, "profile_sketch": 99999, "profiles": 3})),
        ("frame.member", json!({"sketch": "Nowhere", "profile": tube()})),
        member(json!({"profile": null})),
        member(json!({"profile": "tube"})),
        member(json!({"profile": {}})),
        member(json!({"profile": {"kind": "hexagon"}})),
        member(json!({"profile": {"kind": 5}})),
        member(json!({"profile": {"kind": "bar", "width": 10}})),
        member(json!({"profile": {"kind": "bar", "width": 10, "height": 5, "diameter": 3}})),
        member(json!({"profile": {"kind": "bar", "width": true, "height": 5}})),
        member(json!({"profile": {"kind": "bar", "width": " ", "height": 5}})),
        member(json!({"profile": {"kind": "rect_tube", "width": 10, "height": 10, "wall": 5}})),
        member(json!({"profile": {"kind": "bar", "width": "nonsense(", "height": 5}})),
        member(json!({"profile": {"size": "RSC 999x99"}})),
        member(json!({"profile": {"size": 5}})),
        member(json!({"profile": {"size": "RSC 203x89", "height": 200}})),
        member(json!({"profile": {"size": "RSC 203x89", "kind": "angle"}})),
        member(json!({"profile": {"kind": "channel", "height": 100, "width": 50, "web": 6, "flange": 8, "flange_slope": 45}})),
        member(json!({"lines": []})),
        member(json!({"lines": "l1"})),
        member(json!({"lines": [1]})),
        member(json!({"lines": ["l1", "l1"]})),
        member(json!({"lines": (0..=MAX_MEMBERS).map(|i| format!("l{i}")).collect::<Vec<_>>()})),
        member(json!({"lines": ["nope"]})),
        member(json!({"body_name": ""})),
        member(json!({"body_name": "   "})),
        member(json!({"body_name": 5})),
        member(json!({"body_name": "x".repeat(129)})),
        member(json!({"anchor": "middle"})),
        member(json!({"anchor": 5})),
        member(json!({"anchor": ["top"]})),
        member(json!({"angle": true})),
        member(json!({"angle": ""})),
        member(json!({"angle": "x".repeat(4097)})),
        member(json!({"angle": "a lot"})),
        member(json!({"mirror": 3})),
        member(json!({"mirror": "yes"})),
        member(json!({"names": ["Rail"]})),
        member(json!({"names": {"l1": " "}})),
        member(json!({"names": {"l1": 5}})),
        member(json!({"names": {"l1": "x".repeat(129)}})),
        member(json!({"participants": 5})),
        member(json!({"participants": [5]})),
        member(json!({"participants": ["Nobody"]})),
        // frame.profile.list
        ("frame.profile.list", json!({"family": "XYZ"})),
        ("frame.profile.list", json!({"family": 5})),
        ("frame.profile.list", json!({"family": "rsc"})),
        // frame.profile.properties
        ("frame.profile.properties", json!({})),
        ("frame.profile.properties", json!({"profile": null})),
        ("frame.profile.properties", json!({"profile": {"kind": "hexagon"}})),
        ("frame.profile.properties", json!({"profile": {"kind": "round_bar"}})),
        ("frame.profile.properties", json!({"profile": {"kind": "round_bar", "diameter": -1}})),
        ("frame.profile.properties", json!({"profile": bar, "density": -1})),
        ("frame.profile.properties", json!({"profile": bar, "density": 0})),
        ("frame.profile.properties", json!({"profile": bar, "density": "steel"})),
        ("frame.profile.properties", json!({"profile": bar, "density": 1e7})),
        ("frame.profile.properties", json!({"profile": bar, "density": true})),
        // frame.profile.insert
        ("frame.profile.insert", json!({})),
        ("frame.profile.insert", json!({"profile": {"kind": "bar", "width": 5}})),
        insert(json!({"at": "here"})),
        insert(json!({"at": [1]})),
        insert(json!({"at": [1e300, 0]})),
        insert(json!({"at": {"x": 1}})),
        insert(json!({"anchor": "middle"})),
        insert(json!({"anchor": 3})),
        insert(json!({"angle": "a lot"})),
        insert(json!({"angle": 1e7})),
        insert(json!({"fixed": 3})),
        insert(json!({"fixed": "false"})),
        insert(json!({"sketch": true})),
        insert(json!({"sketch": -1})),
        insert(json!({"sketch": 1.5})),
        insert(json!({"sketch": "Nowhere"})),
        // No sketch is being edited, and none is named.
        insert(json!({})),
    ]);
}

/// Each key is read as its handler reads it: a call the handler takes has no issue, and a call
/// it refuses over a key has one. (A call with a key the handler ignores is told apart below.)
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let (_, lines) = frame_session(ParamCheck::Off);
    let line = lines.first().cloned().unwrap_or_default();
    let member = |extra: Value| {
        let mut p = json!({"sketch": "Frame", "profile": tube()});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        p
    };
    let section = |extra: Value| {
        let mut p = json!({"sketch": "Frame", "profile_sketch": "Section"});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        p
    };
    // (call, whether the handler takes it)
    let cases = [
        (member(json!({})), true),
        (member(json!({"lines": [line]})), true),
        (member(json!({"lines": null})), true),
        (member(json!({"lines": []})), false),
        (member(json!({"lines": [line, line]})), false),
        (member(json!({"body_name": "Rail"})), true),
        (member(json!({"body_name": "  Rail  "})), true),
        (member(json!({"body_name": null})), true),
        (member(json!({"body_name": "x".repeat(128)})), true),
        (member(json!({"body_name": "x".repeat(129)})), false),
        (member(json!({"body_name": " "})), false),
        (member(json!({"anchor": "bottom_left"})), true),
        (member(json!({"anchor": null})), true),
        (member(json!({"anchor": "Top"})), false),
        (member(json!({"angle": "90 deg"})), true),
        (member(json!({"angle": 45})), true),
        (member(json!({"angle": " 30 deg "})), true),
        (member(json!({"angle": null})), true),
        (member(json!({"angle": ""})), false),
        (member(json!({"angle": 1e9})), false),
        (member(json!({"mirror": true})), true),
        (member(json!({"mirror": null})), true),
        (member(json!({"mirror": 1})), false),
        (member(json!({"names": {line.clone(): " Mine "}})), true),
        (member(json!({"names": {}})), true),
        (member(json!({"names": null})), true),
        (member(json!({"names": {line.clone(): ""}})), false),
        (member(json!({"names": [line]})), false),
        (member(json!({"name": "Frame members"})), true),
        (member(json!({"participants": "*"})), true),
        (member(json!({"participants": ["*"]})), true),
        (member(json!({"participants": null})), true),
        (member(json!({"participants": [3]})), false),
        (member(json!({"participants": {"a": 1}})), false),
        (member(json!({"profile": {"kind": "bar", "width": "40 mm", "height": 10, "corner_radius": 0}})), true),
        (member(json!({"profile": {"kind": "bar", "width": 40}})), false),
        (member(json!({"profile": {"kind": "i_beam", "height": 100, "width": 50, "web": 4.5, "flange": 7, "root_radius": null}})), false),
        (section(json!({})), true),
        (section(json!({"profiles": "all"})), true),
        (section(json!({"profiles": [{"point": [0, 0]}], "exact_regions": true})), true),
        (section(json!({"profiles": [0]})), true),
        (section(json!({"profiles": "none"})), false),
        (section(json!({"profiles": null})), false),
        (section(json!({"profiles": []})), false),
        // Keys the handler never reads in this form: no issue, and the call is as good as without.
        (member(json!({"profiles": "none", "exact_regions": "yes"})), true),
        (member(json!({"profiles": null})), true),
    ];
    for (p, takes) in cases {
        let (mut s, _) = frame_session(ParamCheck::Off);
        let outcome = s.execute("frame.member", &p);
        assert_eq!(outcome.is_ok(), takes, "{p}: {outcome:?}");
        assert_eq!(issues("frame.member", &p).is_empty(), takes, "{p}: {:?}", issues("frame.member", &p));
    }

    // One of `profile` and `profile_sketch`: with both or neither the handler refuses before it
    // reads the section's keys, and the check, which can't see the design, leaves it that.
    for p in [
        json!({"sketch": "Frame", "profile": tube(), "profile_sketch": "Section"}),
        json!({"sketch": "Frame", "profile": tube(), "profile_sketch": "Section", "profiles": "none", "exact_regions": 1}),
        json!({"sketch": "Frame", "profile": null, "profile_sketch": null}),
        json!({"sketch": "Frame"}),
        json!({"sketch": "Frame", "profiles": "none"}),
    ] {
        let (mut s, _) = frame_session(ParamCheck::Off);
        let error = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(error.contains("`profile`") && error.contains("one of them"), "{p}: {error}");
        assert!(issues("frame.member", &p).is_empty(), "{p}: {:?}", issues("frame.member", &p));
    }

    // The properties and insert commands.
    let (mut s, _) = frame_session(ParamCheck::Off);
    let bar = json!({"kind": "round_bar", "diameter": 10});
    for (p, takes) in [
        (json!({"profile": bar}), true),
        (json!({"profile": bar, "density": 7850}), true),
        (json!({"profile": bar, "density": null}), true),
        (json!({"profile": bar, "density": 1e6}), true),
        (json!({"profile": bar, "density": 1e6 + 1.0}), false),
        (json!({"profile": bar, "density": 0}), false),
        (json!({"profile": bar, "density": "7850"}), false),
    ] {
        let outcome = s.execute("frame.profile.properties", &p);
        assert_eq!(outcome.is_ok(), takes, "{p}: {outcome:?}");
        assert_eq!(issues("frame.profile.properties", &p).is_empty(), takes, "{p}");
    }
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap_or_else(|e| panic!("{e}"));
    for (p, takes) in [
        (json!({"profile": bar}), true),
        (json!({"profile": bar, "at": [3, 4], "anchor": "centroid", "angle": 0.5, "fixed": false}), true),
        (json!({"profile": bar, "at": {"x": 3, "y": 4}}), true),
        (json!({"profile": bar, "at": null, "anchor": null, "angle": null, "fixed": null, "sketch": null}), true),
        (json!({"profile": bar, "angle": -1e6}), true),
        (json!({"profile": bar, "angle": 1e6 + 1.0}), false),
        (json!({"profile": bar, "at": [1, 2, 3]}), false),
        (json!({"profile": bar, "fixed": 0}), false),
        (json!({"profile": bar, "anchor": "CENTER"}), false),
        (json!({"profile": bar, "sketch": 1.5}), false),
    ] {
        let outcome = s.execute("frame.profile.insert", &p);
        assert_eq!(outcome.is_ok(), takes, "{p}: {outcome:?}");
        assert_eq!(issues("frame.profile.insert", &p).is_empty(), takes, "{p}");
    }
}

/// With a built-in profile, a `profiles` or `exact_regions` that couldn't be read is no warning;
/// with a profile sketch it is, and says what the handler says.
#[test]
fn a_section_sketchs_keys_count_only_with_a_section_sketch() {
    let messages = |p: Value| issues("frame.member", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(messages(json!({"sketch": 1, "profile": tube(), "profiles": 7, "exact_regions": []})), Vec::<String>::new());
    assert_eq!(messages(json!({"sketch": 1, "profile_sketch": 2, "profiles": "none"})), ["`profiles` must be \"all\" or a list"]);
    assert_eq!(messages(json!({"sketch": 1, "profile_sketch": 2, "exact_regions": "yes"})), ["`exact_regions` must be true or false"]);
}

/// The schemas say what the handlers need: `sketch` and a profile are required, `profile` and
/// `profile_sketch` are alternatives, and values have the shape the handlers read.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    let bar = json!({"kind": "round_bar", "diameter": 10});
    takes(
        input_schema_for::<FrameMemberParams>(),
        &[
            (json!({"sketch": "Frame", "profile": tube()}), true),
            (json!({"sketch": 3, "profile_sketch": "Section"}), true),
            (json!({"sketch": 3, "profile_sketch": 4, "profiles": [{"point": [0, 0]}], "exact_regions": true}), true),
            (
                json!({
                    "sketch": "Frame", "lines": ["l1", "l2"], "profile": {"kind": "bar", "width": "w", "height": 5, "corner_radius": 1},
                    "anchor": "top_left", "angle": "90 deg", "mirror": true, "name": "N", "body_name": "Rail",
                    "names": {"l1": "A"}, "participants": ["*"]
                }),
                true,
            ),
            (json!({"profile": tube()}), false),
            (json!({"sketch": "Frame"}), false),
            (json!({"sketch": "Frame", "profile": tube(), "profile_sketch": "Section"}), false),
            (json!({"sketch": "Frame", "profile": {"kind": "hexagon"}}), false),
            (json!({"sketch": "Frame", "profile": {"kind": "bar", "width": 5}}), false),
            (json!({"sketch": "Frame", "profile": {"kind": "bar", "width": 5, "height": 5, "wall": 1}}), false),
            (json!({"sketch": true, "profile": tube()}), false),
            (json!({"sketch": "Frame", "profile": tube(), "lines": []}), false),
            (json!({"sketch": "Frame", "profile": tube(), "lines": ["a", "a"]}), false),
            (json!({"sketch": "Frame", "profile": tube(), "anchor": "middle"}), false),
            (json!({"sketch": "Frame", "profile": tube(), "angle": true}), false),
            (json!({"sketch": "Frame", "profile": tube(), "body_names": ["a"]}), false),
            (json!({"sketch": "Frame", "profile": {"size": "RSC 203x89"}}), true),
            (json!({"sketch": "Frame", "profile": {"size": "RSC 203x89", "kind": "channel"}}), true),
            (json!({"sketch": "Frame", "profile": {"size": "RSC 203x89", "height": 200}}), false),
            (json!({"sketch": "Frame", "profile": {"size": ""}}), false),
            (
                json!({"sketch": "Frame", "profile": {"kind": "channel", "height": 100, "width": 50, "web": 6, "flange": 8, "flange_slope": "5 deg"}}),
                true,
            ),
        ],
    );
    takes(
        input_schema_for::<ProfileListParams>(),
        &[(json!({}), true), (json!({"family": "RSC"}), true), (json!({"family": "rsc"}), false), (json!({"kind": "bar"}), false)],
    );
    takes(
        input_schema_for::<ProfilePropertiesParams>(),
        &[
            (json!({"profile": bar}), true),
            (json!({"profile": bar, "density": 7850}), true),
            (json!({"profile": bar, "density": null}), true),
            (json!({"profile": bar, "density": 0}), false),
            (json!({"profile": bar, "density": 1e6 + 1.0}), false),
            (json!({"density": 7850}), false),
            (json!({"profile": {"kind": "round_bar"}}), false),
        ],
    );
    takes(
        input_schema_for::<ProfileInsertParams>(),
        &[
            (json!({"profile": bar}), true),
            (json!({"profile": bar, "at": [1, 2], "anchor": "bottom", "angle": 2.5, "fixed": false, "sketch": "S"}), true),
            (json!({"profile": bar, "at": {"x": 1, "y": 2}, "sketch": 4}), true),
            (json!({}), false),
            (json!({"profile": bar, "at": [1, 2, 3]}), false),
            (json!({"profile": bar, "angle": 2e6}), false),
            (json!({"profile": bar, "fixed": "yes"}), false),
            (json!({"profile": bar, "sketch": 1.5}), false),
        ],
    );
}

/// The "not blank" patterns are the shared one, which `conforms` doesn't read: pin where they are.
#[test]
fn the_name_patterns_are_the_blank_pattern() {
    let blank = crate::schema::types::not_blank_pattern();
    let member = input_schema_for::<FrameMemberParams>();
    let member = member.as_value();
    assert_eq!(member["properties"]["body_name"]["anyOf"][0]["$ref"], json!("#/$defs/MemberName"));
    assert_eq!(member["$defs"]["MemberName"]["pattern"], json!(blank));
    assert_eq!(member["$defs"]["MemberNames"]["additionalProperties"]["pattern"], json!(blank));
    let properties = input_schema_for::<ProfilePropertiesParams>();
    let properties = properties.as_value();
    let kinds = properties["$defs"]["ProfileParam"]["anyOf"].as_array().cloned().unwrap_or_default();
    // One schema per kind, and one for a standard size by name.
    assert_eq!(kinds.len(), frame::profile_kinds().len() + 1);
    assert_eq!(kinds[0]["properties"]["width"]["anyOf"][1]["pattern"], json!(blank));
}

/// The warning for what a key gets wrong, and the check that every built-in kind is described.
#[test]
fn the_check_names_what_is_wrong_with_a_key() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues("frame.profile.properties", json!({"profile": {"kind": "hexagon"}})).len(), 1);
    assert_eq!(
        issues("frame.profile.properties", json!({"profile": {"kind": "bar", "width": 1, "height": 1, "wall": 1}})),
        ["`profile`: a bar profile has no dimension `wall`"]
    );
    assert_eq!(issues("frame.profile.properties", json!({"profile": {"kind": "bar"}})), ["`profile`: a bar profile needs `width`"]);
    assert_eq!(
        issues("frame.profile.properties", json!({"profile": {"kind": "round_bar", "diameter": 1}, "density": -3})),
        ["`density` must be a positive number of kg/m³ (at most 1e6)"]
    );
    assert_eq!(
        issues("frame.profile.insert", json!({"profile": {"kind": "round_bar", "diameter": 1}, "at": 3})),
        ["`at` must be [x, y] in sketch millimetres"]
    );
    assert_eq!(
        issues("frame.profile.insert", json!({"profile": {"kind": "round_bar", "diameter": 1}, "fixed": "no"})),
        ["`fixed` must be true or false"]
    );
    assert_eq!(issues("frame.member", json!({"sketch": 1, "profile": tube(), "mirror": "no"})), ["`mirror` must be true or false"]);
    assert_eq!(issues("frame.member", json!({"sketch": 1, "profile": tube(), "lines": ["a", "a"]})), ["line `a` is listed twice"]);
    assert_eq!(
        issues("frame.member", json!({"sketch": 1, "profile": tube(), "body_names": ["a"]})),
        ["unknown key `body_names`; did you mean `body_name`?"]
    );
    // The schemas describe every built-in kind with its dimensions, and its example is one.
    let schema = input_schema_for::<ProfilePropertiesParams>();
    let schema = schema.as_value();
    for kind in frame::profile_kinds() {
        let example = serde_json::to_value(&kind.example).unwrap_or_default();
        let profile = json!({"profile": example});
        assert!(conforms(schema, schema, &profile), "{profile}");
        assert!(crate::schema::issues("frame.profile.properties", &profile).is_empty(), "{profile}");
    }
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// The library's list and properties replies match the output schemas and are what the handlers
/// built before the typed replies: the same JSON.
#[test]
fn profile_library_replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let listed = run("frame.profile.list", json!({}));
    matches(output_schema_for::<ProfileListResult>(), "frame.profile.list", &listed);
    assert_eq!(listed["kinds"], json!(frame::profile_kinds()));
    let families: Vec<&str> = listed["sizes"].as_array().into_iter().flatten().filter_map(|f| f["family"].as_str()).collect();
    assert_eq!(families, ["RSC", "PFC", "RSA", "SHS", "RHS", "CHS"]);
    let only = run("frame.profile.list", json!({"family": "CHS"}));
    matches(output_schema_for::<ProfileListResult>(), "frame.profile.list", &only);
    assert_eq!(only["sizes"].as_array().map(Vec::len), Some(1), "{only}");
    let sized = run("frame.profile.properties", json!({"profile": {"size": "RSC 203x89"}}));
    matches(output_schema_for::<ProfilePropertiesResult>(), "frame.profile.properties", &sized);
    assert_eq!(sized["size"], json!("RSC 203x89"));
    let kinds = listed["kinds"].as_array().cloned().unwrap_or_default();
    assert_eq!(kinds.len(), 7);
    for kind in &kinds {
        let r = run("frame.profile.properties", json!({"profile": kind["example"]}));
        matches(output_schema_for::<ProfilePropertiesResult>(), "frame.profile.properties", &r);
        let profile: frame::Profile = serde_json::from_value(kind["example"].clone()).unwrap_or_else(|e| panic!("{e}"));
        let region = profile.region().unwrap_or_else(|e| panic!("{e}"));
        let sec = frame::section(&region);
        assert_eq!(
            r,
            json!({
                "profile": profile.filled(), "area": sec.area, "centroid": sec.centroid, "ixx": sec.ixx, "iyy": sec.iyy, "ixy": sec.ixy,
                "width": sec.width, "height": sec.height
            })
        );
    }
    let heavy = run("frame.profile.properties", json!({"profile": tube(), "density": 7850}));
    matches(output_schema_for::<ProfilePropertiesResult>(), "frame.profile.properties", &heavy);
    let area = heavy["area"].as_f64().unwrap_or_default();
    assert_eq!(heavy["mass_per_m"], json!(area * 1e-6 * 7850.0), "{heavy}");
    assert_eq!(heavy["profile"]["outer_radius"], json!(6.0), "{heavy}");
    // Dimensions as expressions, in a design with parameters.
    run("parameters.add", json!({"name": "wall", "expression": "3 mm"}));
    let by_parameter =
        run("frame.profile.properties", json!({"profile": {"kind": "rect_tube", "width": "40 mm", "height": 40, "wall": "wall"}, "density": null}));
    matches(output_schema_for::<ProfilePropertiesResult>(), "frame.profile.properties", &by_parameter);
    assert_eq!(by_parameter["area"], run("frame.profile.properties", json!({"profile": tube()}))["area"]);

    // A misspelt key still runs in Warn and says so, with a reply that matches.
    for (id, p) in [("frame.profile.list", json!({"kinds": true})), ("frame.profile.properties", json!({"profile": tube(), "densty": 7850}))] {
        let warned = run(id, p);
        assert!(warned["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{warned}");
        let schema =
            if id == "frame.profile.list" { output_schema_for::<ProfileListResult>() } else { output_schema_for::<ProfilePropertiesResult>() };
        assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
    }
}

/// Insert a profile in every form, in Warn: each reply matches its output schema, carries no
/// warning and is what the handler built before.
#[test]
fn insert_replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let made = run("sketch.create", json!({"plane": "XY", "name": "Drawing"}));
    let id = made["sketch"].clone();
    let plain = run("frame.profile.insert", json!({"profile": tube()}));
    matches(output_schema_for::<ProfileInsertResult>(), "frame.profile.insert", &plain);
    assert_eq!(plain["created"]["curves"].as_array().map(Vec::len), Some(16), "{plain}");
    assert!(plain["sketch"]["solved"].is_boolean() && plain["sketch"]["dof"].is_u64(), "{plain}");
    let mut keys: Vec<&String> = plain.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    keys.sort();
    assert_eq!(keys, ["created", "section", "sketch"]);
    let mut created: Vec<&String> = plain["created"].as_object().map(|o| o.keys().collect()).unwrap_or_default();
    created.sort();
    assert_eq!(created, ["constraints", "curves", "dimensions", "points"]);
    let section = run("frame.profile.properties", json!({"profile": tube()}));
    // The section is what the library computes for the profile, not just what the other command says.
    let profile: frame::Profile = serde_json::from_value(tube()).unwrap_or_else(|e| panic!("{e}"));
    let sec = frame::section(&profile.region().unwrap_or_else(|e| panic!("{e}")));
    assert_eq!(plain["section"]["area"], json!(sec.area), "{plain}");
    assert_eq!(plain["section"]["centroid"], json!(sec.centroid), "{plain}");
    assert_eq!(plain["section"]["profile"], serde_json::to_value(profile.filled()).unwrap_or_default(), "{plain}");
    assert_eq!(plain["section"], section);
    let placed = run(
        "frame.profile.insert",
        json!({
            "profile": {"kind": "channel", "height": "60 mm", "width": 45, "web": 5, "flange": 6},
            "at": [100, 50], "anchor": "bottom_left", "angle": 0.5, "fixed": false, "sketch": "Drawing"
        }),
    );
    matches(output_schema_for::<ProfileInsertResult>(), "frame.profile.insert", &placed);
    let by_id = run(
        "frame.profile.insert",
        json!({"profile": {"kind": "round_bar", "diameter": 20}, "sketch": id, "at": {"x": -50, "y": 5}, "anchor": null}),
    );
    matches(output_schema_for::<ProfileInsertResult>(), "frame.profile.insert", &by_id);
    assert_eq!(by_id["created"]["curves"].as_array().map(Vec::len), Some(1), "{by_id}");
    // A misspelt key still runs in Warn, and says so, with a reply that matches.
    let warned = run("frame.profile.insert", json!({"profile": {"kind": "round_bar", "diameter": 5}, "sketch": "Drawing", "ancor": "center"}));
    assert_eq!(warned["param_warnings"], json!(["unknown key `ancor`; did you mean `anchor`?"]), "{warned}");
    let schema = output_schema_for::<ProfileInsertResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
}

/// Members in every form, in Warn: each reply matches `FeatureResult` and carries no warning.
#[test]
fn member_replies_match_the_output_schema() {
    let (mut s, lines) = frame_session(ParamCheck::Warn);
    let mut run = |p: Value| s.execute("frame.member", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let first = lines.first().cloned().unwrap_or_default();
    let second = lines.get(1).cloned().unwrap_or_default();
    let calls = [
        json!({"sketch": "Frame", "profile": tube()}),
        json!({"sketch": "Frame", "profile": {"kind": "angle", "width": 60, "height": 40, "thickness": "5 mm"}, "lines": [first], "anchor": "bottom_left"}),
        json!({"sketch": "Frame", "profile": tube(), "lines": [first, second], "angle": "90 deg", "mirror": true, "name": "Rails", "body_name": "Rail"}),
        json!({"sketch": "Frame", "profile": tube(), "lines": [first], "angle": 45, "names": {first.clone(): "Chosen"}, "participants": "*"}),
        json!({"sketch": "Frame", "profile": tube(), "lines": null}),
        json!({"sketch": "Frame", "profile_sketch": "Section"}),
        json!({"sketch": "Frame", "profile_sketch": "Section", "profiles": [{"point": [0, 0]}], "exact_regions": true, "lines": [first], "anchor": "center"}),
        json!({"sketch": "Frame", "profile_sketch": "Section", "profiles": "all", "mirror": false, "angle": null, "names": null}),
    ];
    for p in calls {
        let reply = run(p.clone());
        matches(output_schema_for::<FeatureResult>(), "frame.member", &reply);
        assert!(reply["created_bodies"].as_array().is_some_and(|b| !b.is_empty()), "{p}: {reply}");
    }
    // By the id of the sketch.
    let by_id = s_first_sketch_call(&mut s, &first);
    matches(output_schema_for::<FeatureResult>(), "frame.member", &by_id);

    // A misspelt key still runs in Warn, and says so, with a reply that matches.
    let warned =
        s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "lines": [first], "profil": 1})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(warned["param_warnings"], json!(["unknown key `profil`; did you mean `profile`?"]), "{warned}");
    let schema = output_schema_for::<FeatureResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
}

fn s_first_sketch_call(s: &mut Session, line: &str) -> Value {
    let id = first_sketch(s);
    s.execute("frame.member", &json!({"sketch": id, "profile": tube(), "lines": [line]})).unwrap_or_else(|e| panic!("{e}"))
}
