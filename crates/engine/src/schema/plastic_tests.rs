use serde_json::{Value, json};

use super::{
    AssignRuleParams, AssignRuleResult, BossParams, LipParams, ManageRulesParams, ManageRulesResult, RestParams, RulesParams, RulesResult,
    SnapFitParams,
};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A design with one box, `Body1`.
fn boxed(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 20, "width": 15, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<ManageRulesParams>(),
        &[
            "active",
            "clearance",
            "delete",
            "draft",
            "from",
            "knife_edge",
            "material",
            "max_thickness",
            "min_draft",
            "min_thickness",
            "name",
            "nominal_radius",
            "reveal_height",
            "thickness",
            "thickness_variation",
        ],
    );
    assert_keys(input_schema_for::<AssignRuleParams>(), &["bodies", "rule"]);
    assert_keys(input_schema_for::<RulesParams>(), &[]);
}

/// #12313: calls that fail today get the same message in strict. (List Plastic Rules takes any
/// parameters today; strict refuses an unknown key in the check's own words, as for every
/// converted command.)
#[test]
fn strict_keeps_todays_errors() {
    let many: Vec<String> = (0..10_001).map(|i| format!("B{i}")).collect();
    assert_strict_keeps_todays_errors(&[
        ("plastic.manage_rules", json!({})),
        ("plastic.manage_rules", json!({"name": null})),
        ("plastic.manage_rules", json!({"name": 5})),
        ("plastic.manage_rules", json!({"name": "  "})),
        ("plastic.manage_rules", json!({"name": "x".repeat(129)})),
        ("plastic.manage_rules", json!({"name": "New", "from": "Nope"})),
        ("plastic.manage_rules", json!({"name": "Nope", "delete": true})),
        ("plastic.manage_rules", json!({"name": "ABS (1.5mm)", "thickness": "nope + 1"})),
        ("plastic.manage_rules", json!({"name": "ABS (1.5mm)", "thickness": "0 mm"})),
        ("plastic.manage_rules", json!({"name": "ABS (1.5mm)", "draft": "2 mm"})),
        ("plastic.assign_rule", json!({})),
        ("plastic.assign_rule", json!({"bodies": null})),
        ("plastic.assign_rule", json!({"bodies": []})),
        ("plastic.assign_rule", json!({"bodies": 5})),
        ("plastic.assign_rule", json!({"bodies": ["Body1", 5]})),
        ("plastic.assign_rule", json!({"bodies": many})),
        ("plastic.assign_rule", json!({"bodies": ["Nope"]})),
    ]);
    // A rule that doesn't exist, for a body that does.
    let p = json!({"bodies": ["Body1"], "rule": "Nope"});
    let error = |level: ParamCheck| boxed(level).execute("plastic.assign_rule", &p).err().map(|e| e.to_string());
    let off = error(ParamCheck::Off);
    assert!(off.is_some(), "{p} should fail today");
    assert_eq!(error(ParamCheck::Strict), off, "{p}");
}

/// #12313: keys of the wrong type that the handlers ignore today (the call then works, without
/// them) are refused by the contracts: in Warn the call still works, with a warning.
#[test]
fn the_contracts_refuse_what_the_handlers_ignore() {
    for (id, p, key) in [
        ("plastic.manage_rules", json!({"name": "Mine", "active": "yes"}), "active"),
        ("plastic.manage_rules", json!({"name": "Mine", "delete": 1}), "delete"),
        ("plastic.manage_rules", json!({"name": "Mine", "thickness": true}), "thickness"),
        ("plastic.manage_rules", json!({"name": "Mine", "from": 5}), "from"),
        ("plastic.manage_rules", json!({"name": "Mine", "material": ["ABS"]}), "material"),
        ("plastic.assign_rule", json!({"bodies": ["Body1"], "rule": 5}), "rule"),
    ] {
        assert!(boxed(ParamCheck::Off).execute(id, &p).is_ok(), "{p} works today");
        let warned = boxed(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = boxed(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{p}: {error}");
    }
}

/// #12313: the schemas take what the handlers read: a rule's name as text, its values as
/// numbers or expressions, `active` and `delete` as true or false; bodies as a name or a list of
/// one or more names (not `null`), and the rule as text or `null`.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let manage = input_schema_for::<ManageRulesParams>();
    let manage = manage.as_value();
    for (p, takes) in [
        (json!({"name": "ABS (1.5mm)"}), true),
        (json!({"name": "Mine", "from": "ABS (1.5mm)", "material": "ABS", "thickness": "2 mm", "draft": 3, "active": true}), true),
        (json!({"name": "Mine", "delete": true}), true),
        (json!({}), false),
        (json!({"name": 5}), false),
        (json!({"name": ""}), false),
        (json!({"name": "Mine", "thickness": true}), false),
        (json!({"name": "Mine", "delete": "yes"}), false),
        (json!({"name": "Mine", "angle": 3}), false),
    ] {
        assert_eq!(conforms(manage, manage, &p), takes, "{p}");
    }
    let assign = input_schema_for::<AssignRuleParams>();
    let assign = assign.as_value();
    for (p, takes) in [
        (json!({"bodies": ["Body1"], "rule": "ABS (1.5mm)"}), true),
        (json!({"bodies": "Body1"}), true),
        (json!({"bodies": ["Body1"], "rule": null}), true),
        (json!({}), false),
        (json!({"bodies": [5]}), false),
        (json!({"bodies": null}), false),
        (json!({"bodies": []}), false),
        (json!({"bodies": ["Body1"], "rule": 5}), false),
    ] {
        assert_eq!(conforms(assign, assign, &p), takes, "{p}");
    }
}

/// #12313: making, editing, listing, assigning and deleting plastic rules reply as the output
/// schemas say, in Warn without warnings: a new active rule from a library one, its thickness
/// from a design parameter, the list (with that rule's values not evaluating once the parameter
/// is out of range), the rule assigned to the box and taken away again, and the rule deleted.
#[test]
fn plastic_rules_reply_with_their_schemas() {
    let mut s = boxed(ParamCheck::Warn);
    let run = |s: &mut Session, id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run(
        &mut s,
        "plastic.manage_rules",
        json!({"name": "Mine", "from": "ABS (1.5mm)", "material": "ABS", "active": true}),
        output_schema_for::<ManageRulesResult>(),
    );
    assert_eq!((&r["rule"]["name"], &r["active"]), (&json!("Mine"), &json!("Mine")), "{r}");
    s.execute("parameters.add", &json!({"name": "wall", "expression": "2 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let r = run(
        &mut s,
        "plastic.manage_rules",
        json!({"name": "Mine", "thickness": "wall", "draft": "1.5 deg"}),
        output_schema_for::<ManageRulesResult>(),
    );
    assert_eq!(r["rule"]["values"]["thickness"], 2.0, "{r}");
    assert_eq!(r["rule"]["expressions"]["thickness"], "wall", "{r}");
    let r = run(&mut s, "plastic.rules", json!({}), output_schema_for::<RulesResult>());
    assert!(r["rules"].as_array().is_some_and(|rules| rules.iter().any(|x| x["name"] == "Mine" && x["library"] == false)), "{r}");
    // A thickness of 0 doesn't evaluate as a rule's: the list says why, in place of the values.
    s.execute("parameters.change", &json!({"name": "wall", "expression": "0 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let r = run(&mut s, "plastic.rules", json!({}), output_schema_for::<RulesResult>());
    let mine = r["rules"].as_array().and_then(|rules| rules.iter().find(|x| x["name"] == "Mine")).cloned().unwrap_or_default();
    assert!(mine["values"]["error"].is_string(), "{mine}");
    let r = run(&mut s, "plastic.assign_rule", json!({"bodies": ["Body1"], "rule": "ABS (1.5mm)"}), output_schema_for::<AssignRuleResult>());
    assert_eq!((&r["bodies"], &r["rule"]), (&json!(["Body1"]), &json!("ABS (1.5mm)")), "{r}");
    let r = run(&mut s, "plastic.rules", json!({}), output_schema_for::<RulesResult>());
    assert_eq!(r["assigned"], json!({"Body1": "ABS (1.5mm)"}), "{r}");
    let r = run(&mut s, "plastic.assign_rule", json!({"bodies": "Body1", "rule": ""}), output_schema_for::<AssignRuleResult>());
    assert_eq!(r["rule"], json!(null), "{r}");
    let r = run(&mut s, "plastic.manage_rules", json!({"name": "Mine", "delete": true}), output_schema_for::<ManageRulesResult>());
    assert_eq!(r["deleted"], "Mine", "{r}");
}

// ---- Plastic features (#12622) ----

/// A 40 × 40 × 4 mm plate, `Body1`, whose top face (z = 4) the features stand on.
fn plate(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 40, "height": 4})).unwrap_or_else(|e| panic!("{e}"));
    s
}

/// An 80 × 50 × 30 mm box shelled 2 mm thick with its top open: the rim face is at z = 30.
fn shelled(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in
        [("solid.box", json!({"length": 80, "width": 50, "height": 30})), ("solid.shell", json!({"faces": [[40, 25, 30]], "thickness": 2}))]
    {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s
}

/// `base` with `key` set to `value`.
fn with(base: &Value, key: &str, value: Value) -> Value {
    let mut p = base.clone();
    p[key] = value;
    p
}

fn boss() -> Value {
    json!({"position": [20, 20, 4], "diameter": 6, "height": 5})
}

fn lip() -> Value {
    json!({"face": [1, 25, 30], "width": 1, "height": 2})
}

fn snap_fit() -> Value {
    json!({"position": [20, 20, 4], "hook": [1, 0, 0], "length": 15, "thickness": 1.5, "width": 6, "catch_depth": 1, "catch_length": 3})
}

fn rest() -> Value {
    json!({"position": [20, 20, 4], "width": 6, "height": 2})
}

#[test]
fn the_feature_schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BossParams>(),
        &[
            "body",
            "body_name",
            "body_names",
            "diameter",
            "direction",
            "draft",
            "fillet",
            "height",
            "hole_depth",
            "hole_diameter",
            "name",
            "participants",
            "position",
            "rib_length",
            "rib_offset",
            "rib_thickness",
            "ribs",
            "touching",
        ],
    );
    assert_keys(
        input_schema_for::<LipParams>(),
        &["body", "body_name", "body_names", "face", "gap", "height", "name", "outside", "participants", "side", "touching", "type", "width"],
    );
    assert_keys(
        input_schema_for::<SnapFitParams>(),
        &[
            "base_height",
            "body",
            "body_name",
            "body_names",
            "catch_depth",
            "catch_length",
            "direction",
            "hook",
            "length",
            "name",
            "new_body",
            "participants",
            "position",
            "thickness",
            "touching",
            "width",
        ],
    );
    assert_keys(
        input_schema_for::<RestParams>(),
        &[
            "along",
            "body",
            "body_name",
            "body_names",
            "direction",
            "draft",
            "height",
            "length",
            "name",
            "participants",
            "position",
            "thickness",
            "touching",
            "width",
        ],
    );
}

/// #12622: calls to the plastic features that fail today get the same message in strict. They
/// fail before the feature is built, so the shared helper's sketch, which has no body, does for
/// them. A required key left out is the exception the helper allows: strict says it is required.
#[test]
fn strict_keeps_the_features_errors() {
    let up = |p: Value| with(&p, "direction", json!([0, 0, 1]));
    assert_strict_keeps_todays_errors(&[
        ("plastic.boss", with(&boss(), "position", json!(null))),
        ("plastic.boss", with(&boss(), "position", json!([20, 20]))),
        ("plastic.boss", with(&boss(), "position", json!("top"))),
        ("plastic.boss", with(&boss(), "direction", json!([0, 0, 0]))),
        ("plastic.boss", with(&boss(), "direction", json!(null))),
        ("plastic.boss", with(&boss(), "direction", json!("up"))),
        // No direction, and no planar face at the point to stand out of.
        ("plastic.boss", boss()),
        ("plastic.boss", with(&up(boss()), "diameter", json!(null))),
        ("plastic.boss", with(&up(boss()), "diameter", json!(true))),
        ("plastic.boss", with(&up(boss()), "diameter", json!(" "))),
        ("plastic.boss", with(&up(boss()), "diameter", json!(1e9))),
        ("plastic.boss", with(&up(boss()), "diameter", json!("1".repeat(4097)))),
        ("plastic.boss", with(&up(boss()), "height", json!("nope + 1"))),
        ("plastic.boss", with(&up(boss()), "hole_diameter", json!("nope"))),
        ("plastic.boss", with(&up(boss()), "ribs", json!("2 mm"))),
        ("plastic.boss", with(&up(boss()), "draft", json!("2 mm"))),
        ("plastic.lip", with(&lip(), "face", json!(null))),
        ("plastic.lip", with(&lip(), "face", json!([1, 25]))),
        ("plastic.lip", with(&lip(), "width", json!(null))),
        ("plastic.lip", with(&lip(), "height", json!([]))),
        ("plastic.lip", with(&lip(), "type", json!("tongue"))),
        ("plastic.lip", with(&lip(), "type", json!("TONGUE"))),
        ("plastic.lip", with(&lip(), "side", json!("left"))),
        ("plastic.snap_fit", with(&up(snap_fit()), "hook", json!(null))),
        ("plastic.snap_fit", with(&up(snap_fit()), "hook", json!([0, 0, 0]))),
        ("plastic.snap_fit", with(&up(snap_fit()), "hook", json!("x"))),
        // A hook along the arm.
        ("plastic.snap_fit", with(&up(snap_fit()), "hook", json!([0, 0, 1]))),
        ("plastic.snap_fit", with(&up(snap_fit()), "catch_length", json!(true))),
        ("plastic.rest", with(&up(rest()), "along", json!(null))),
        ("plastic.rest", with(&up(rest()), "along", json!([0, 0, 0]))),
        // `along` along the rest's direction, not across the face.
        ("plastic.rest", with(&up(rest()), "along", json!([0, 0, 2]))),
        ("plastic.rest", with(&up(rest()), "width", json!(false))),
    ]);
    // `add_feature` refuses an unknown `touching` without the command's name, as it does for
    // every feature; strict gives the same words after the command's name.
    let p = with(&up(boss()), "touching", json!("glued"));
    let error = |level: ParamCheck| Session { param_check: level, ..Session::default() }.execute("plastic.boss", &p).err().map(|e| e.to_string());
    let off = error(ParamCheck::Off).unwrap_or_default();
    assert_eq!(off, "`touching` must be separate or one_body");
    assert_eq!(error(ParamCheck::Strict), Some(format!("invalid parameters for `plastic.boss`: {off}")));
}

/// #12622: optional keys of the wrong type that the features ignore today (the feature is then
/// built without them) are refused by the contracts: in Warn the call still works, with a
/// warning naming the key.
#[test]
fn the_feature_contracts_refuse_what_the_handlers_ignore() {
    for (id, p, key) in [
        ("plastic.boss", with(&boss(), "hole_diameter", json!(true)), "hole_diameter"),
        ("plastic.boss", with(&boss(), "draft", json!("")), "draft"),
        ("plastic.boss", with(&boss(), "ribs", json!([])), "ribs"),
        ("plastic.boss", with(&boss(), "fillet", json!(1e9)), "fillet"),
        ("plastic.boss", with(&boss(), "body", json!(5)), "body"),
        ("plastic.lip", with(&lip(), "type", json!(5)), "type"),
        ("plastic.lip", with(&lip(), "side", json!(true)), "side"),
        ("plastic.lip", with(&lip(), "outside", json!("yes")), "outside"),
        ("plastic.lip", with(&lip(), "gap", json!({})), "gap"),
        ("plastic.snap_fit", with(&snap_fit(), "new_body", json!("no")), "new_body"),
        ("plastic.snap_fit", with(&snap_fit(), "base_height", json!(false)), "base_height"),
        ("plastic.rest", with(&rest(), "thickness", json!(true)), "thickness"),
        ("plastic.rest", with(&rest(), "length", json!("")), "length"),
    ] {
        let fixture: fn(ParamCheck) -> Session = if id == "plastic.lip" { shelled } else { plate };
        assert!(fixture(ParamCheck::Off).execute(id, &p).is_ok(), "{p} works today");
        let warned = fixture(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = fixture(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{p}: {error}");
    }
}

/// #12622: the feature schemas take what the handlers take. Each call is checked against both
/// Boss's schema and its handler on the plate, with the check off, so the two can't drift apart.
/// Lip's, Snap Fit's and Rest's schemas are checked on their own keys.
#[test]
fn the_feature_schemas_take_what_the_handlers_take() {
    let schema = input_schema_for::<BossParams>();
    let schema = schema.as_value();
    for p in [
        boss(),
        with(&boss(), "position", json!({"x": 20, "y": 20, "z": 4})),
        with(&boss(), "position", json!([20, 20])),
        with(&boss(), "direction", json!([0, 0, 1])),
        with(&boss(), "diameter", json!("6 mm")),
        with(&boss(), "diameter", json!("")),
        with(&boss(), "diameter", json!(1e9)),
        with(&boss(), "hole_diameter", json!(2)),
        with(&boss(), "ribs", json!(4)),
        with(&boss(), "ribs", json!("4")),
        with(&boss(), "draft", json!("1 deg")),
        with(&boss(), "touching", json!("one_body")),
        with(&boss(), "touching", json!("glued")),
        with(&boss(), "body", json!("Body1")),
        with(&boss(), "name", json!("Post")),
    ] {
        let takes = plate(ParamCheck::Off).execute("plastic.boss", &p).is_ok();
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
    for (p, takes) in [
        (lip(), true),
        (with(&lip(), "type", json!("groove")), true),
        (with(&lip(), "side", json!("outside")), true),
        (with(&lip(), "outside", json!(true)), true),
        (with(&lip(), "type", json!("tongue")), false),
        (with(&lip(), "outside", json!("yes")), false),
    ] {
        let schema = input_schema_for::<LipParams>();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{p}");
    }
    for (p, takes) in [
        (snap_fit(), true),
        (with(&snap_fit(), "new_body", json!(true)), true),
        (with(&snap_fit(), "hook", json!(null)), false),
        (with(&snap_fit(), "base_height", json!(true)), false),
    ] {
        let schema = input_schema_for::<SnapFitParams>();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{p}");
    }
    for (p, takes) in [
        (rest(), true),
        (with(&with(&rest(), "length", json!(10)), "along", json!([1, 0, 0])), true),
        (with(&rest(), "along", json!(null)), false),
        (with(&rest(), "thickness", json!(true)), false),
    ] {
        let schema = input_schema_for::<RestParams>();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{p}");
    }
}

/// #12622: each plastic feature replies as a feature command does, in Warn without warnings: a
/// boss with a hole and ribs, a lip and a groove (its type in capitals, as the handler reads it),
/// a snap fit as a body of its own, and a rectangular hollow rest. A misspelt key is ignored
/// today: Warn says so, and strict refuses it.
#[test]
fn plastic_features_reply_with_their_schemas() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let check = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let mut s = plate(ParamCheck::Warn);
    let r =
        check(&mut s, "plastic.boss", json!({"position": [10, 10, 4], "diameter": 6, "height": 5, "hole_diameter": 2, "ribs": 4, "name": "Post"}));
    assert_eq!((&r["name"], &r["changed_bodies"]), (&json!("Post"), &json!(["Body1"])), "{r}");
    let r = check(&mut s, "plastic.snap_fit", with(&with(&snap_fit(), "position", json!([30, 30, 4])), "new_body", json!(true)));
    assert_eq!(r["created_bodies"].as_array().map(Vec::len), Some(1), "a body of its own: {r}");
    check(&mut s, "plastic.rest", json!({"position": [30, 10, 4], "width": 6, "length": 8, "along": [1, 0, 0], "height": 2, "thickness": 1}));
    let mut s = shelled(ParamCheck::Warn);
    check(&mut s, "plastic.lip", lip());
    let mut s = shelled(ParamCheck::Warn);
    check(&mut s, "plastic.lip", with(&with(&lip(), "type", json!("GROOVE")), "side", json!("Outside")));
    // A misspelt key.
    let p = with(&boss(), "hole_dia", json!(2));
    let r = plate(ParamCheck::Warn).execute("plastic.boss", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("hole_dia"), "{r}");
    let error = plate(ParamCheck::Strict).execute("plastic.boss", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`hole_dia`"), "{error}");
}
