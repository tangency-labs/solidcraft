use serde_json::{Value, json};

use super::{AssignRuleParams, AssignRuleResult, ManageRulesParams, ManageRulesResult, RulesParams, RulesResult};
use crate::Session;
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
/// names, and the rule as text or `null`.
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
