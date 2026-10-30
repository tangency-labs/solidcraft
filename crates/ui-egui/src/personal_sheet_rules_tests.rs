//! Personal Sheet Metal Rules (#9346): saved from one design's rule manager, offered by every
//! design's, and added to a design only by the manager's OK.

use serde_json::{Value, json};

use super::{INVALID_VALUES, Library, Rule};
use crate::dialogs::Kind;
use crate::dialogs_sheet::Sm;
use crate::scenario::Harness;

/// A design in `units` with the Sheet Metal Rules manager open and its Personal rules section
/// unfolded.
fn manager(units: &str) -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    if units != "mm" {
        h.app.run("document.units", json!({ "units": units })).unwrap();
    }
    h.app.start("sheet.manage_rules");
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:section:sheet.personal_rules.title"}})).unwrap();
    h.frames(2);
    h
}

/// Another design whose profile has `library`.
fn manager_with(units: &str, library: &Library) -> Harness {
    let mut h = manager(units);
    h.app.preferences.personal_sheet_rules = library.clone();
    h.frames(2);
    h
}

fn sm(h: &mut Harness) -> &mut Sm {
    match &mut h.app.dialog.as_mut().expect("the rule manager is open").kind {
        Kind::Sheet(sm @ Sm::Rules { .. }) => sm,
        _ => panic!("expected the Sheet Metal Rules manager"),
    }
}

/// Type the manager's values in its field order: thickness, K factor, bend radius, relief width,
/// relief depth, corner relief, hem gap and flange gap.
fn type_values(h: &mut Harness, typed: [&str; 8]) {
    let Sm::Rules { values, .. } = sm(h) else { panic!() };
    *values = typed.iter().map(|v| v.to_string()).collect();
    h.frames(2);
}

fn save_personal(h: &mut Harness, name: &str) {
    h.step(&json!({"widget":"Save new"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:sheet.personal_rules.name"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":name})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.frames(2);
}

fn choose_personal(h: &mut Harness, name: &str) {
    h.step(&json!({"click":{"handle":"dialog:sheet.personal_rules.rule"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:sheet.personal_rules.rule:{name}")}})).unwrap();
    h.frames(2);
}

/// What the manager's OK runs now.
fn ok_commands(h: &Harness) -> Vec<(String, Value)> {
    crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()
}

fn library_json(h: &Harness) -> Value {
    serde_json::to_value(&h.app.preferences.personal_sheet_rules).unwrap()
}

fn row(name: &str) -> Value {
    json!({"name":name,"thickness":"2 mm","k_factor":"0.44","bend_radius":"Thickness","relief_width":"Thickness",
        "relief_depth":"Thickness * 0.5","corner_relief":"Thickness * 4.0","corner_relief_shape":"notch","hem_gap":"0.01 mm",
        "gap":"Thickness","angle_display":"bend"})
}

#[test]
fn a_personal_rule_saved_in_one_design_is_added_to_another_by_ok() {
    let mut h = manager("mm");
    type_values(&mut h, ["1.6", "0.42", "1.6", "Thickness", "Thickness * 0.5", "Thickness * 4.0", "0.01", "Thickness"]);
    save_personal(&mut h, "CRS 1.6");
    // Bare numbers are saved with the design's unit; expressions as written.
    assert_eq!(
        library_json(&h),
        json!([{"name":"CRS 1.6","thickness":"1.6 mm","k_factor":"0.42","bend_radius":"1.6 mm","relief_width":"Thickness",
            "relief_depth":"Thickness * 0.5","corner_relief":"Thickness * 4.0","corner_relief_shape":"notch","hem_gap":"0.01 mm",
            "gap":"Thickness","angle_display":"bend"}])
    );
    assert!(h.app.session.doc.sheet.rules.is_empty(), "saving a personal rule is a preference, not a design change");

    let mut other = manager_with("mm", &h.app.preferences.personal_sheet_rules);
    let before = serde_json::to_value(&*other.app.session.doc).unwrap();
    choose_personal(&mut other, "CRS 1.6");
    assert_eq!(serde_json::to_value(&*other.app.session.doc).unwrap(), before, "picking a personal rule changes nothing in the design");
    assert_eq!(
        ok_commands(&other),
        vec![(
            "sheet.manage_rules".to_string(),
            json!({"name":"CRS 1.6","active":true,"thickness":"1.6 mm","k_factor":"0.42","bend_radius":"1.6 mm","relief_width":"Thickness",
                "relief_depth":"Thickness * 0.5","corner_relief":"Thickness * 4.0","hem_gap":"0.01 mm","gap":"Thickness",
                "corner_relief_shape":"notch","angle_display":"bend","unfold_method":"k_factor"})
        )]
    );
    other.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    other.frames(4);
    let doc = &other.app.session.doc;
    assert_eq!(doc.sheet.active.as_deref(), Some("CRS 1.6"));
    let rule = doc.sheet.rules.iter().find(|r| r.name == "CRS 1.6").expect("OK adds the rule to the design");
    assert_eq!((rule.thickness.as_str(), rule.k_factor.as_str(), rule.bend_radius.as_str()), ("1.6 mm", "0.42", "1.6 mm"));
}

#[test]
fn a_personal_rule_keeps_its_size_between_inch_and_millimetre_designs() {
    let mut h = manager("in");
    type_values(&mut h, ["0.0625", "0.44", "Thickness", "Thickness", "Thickness * 0.5", "(1 + 1) / 8", "0.01", "Thickness"]);
    save_personal(&mut h, "16 ga");
    let saved = library_json(&h);
    assert_eq!(saved[0]["thickness"], "0.0625 in");
    assert_eq!(saved[0]["corner_relief"], "((1 + 1) / 8) * 1 in", "an expression without a unit is read in the design's");
    assert_eq!(saved[0]["bend_radius"], "Thickness");

    let mut mm = manager_with("mm", &h.app.preferences.personal_sheet_rules);
    choose_personal(&mut mm, "16 ga");
    mm.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    mm.frames(4);
    let doc = &mm.app.session.doc;
    let (vals, _) = doc.param_values();
    let values = doc.rule_values(&vals, &doc.sheet_rule(Some("16 ga"))).unwrap();
    // 1/16 in and 1/4 in, whatever the design's unit.
    assert!((values.thickness - 1.5875).abs() < 1e-9, "thickness {}", values.thickness);
    assert!((values.bend_radius - 1.5875).abs() < 1e-9, "bend radius {}", values.bend_radius);
    assert!((values.corner_relief - 6.35).abs() < 1e-9, "corner relief {}", values.corner_relief);
}

#[test]
fn a_personal_rule_updates_the_design_rule_of_the_same_name() {
    let mut h = manager("mm");
    type_values(&mut h, ["3", "0.4", "4.5", "Thickness", "Thickness * 0.5", "Thickness * 4.0", "0.01", "Thickness"]);
    save_personal(&mut h, "Shop 3 mm");
    h.app.dialog = None;
    h.frames(2);
    h.app.session.execute("sheet.manage_rules", &json!({"name":"Shop 3 mm","thickness":"2 mm","k_factor":0.5,"active":true})).unwrap();
    h.app.session.execute("sheet.manage_rules", &json!({"name":"Other","thickness":"1 mm","active":true})).unwrap();
    h.app.start("sheet.manage_rules");
    h.frames(4);
    choose_personal(&mut h, "Shop 3 mm");
    let Sm::Rules { pick, active, .. } = sm(&mut h) else { panic!() };
    assert_eq!((*pick, *active), (0, false), "the design's own rule of that name, still not its active rule");
    let commands = ok_commands(&h);
    assert_eq!(
        [&commands[0].1["name"], &commands[0].1["thickness"], &commands[0].1["k_factor"], &commands[0].1["bend_radius"], &commands[0].1["active"]],
        [&json!("Shop 3 mm"), &json!("3 mm"), &json!("0.4"), &json!("4.5 mm"), &json!(false)]
    );
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.frames(4);
    let doc = &h.app.session.doc;
    assert_eq!(doc.sheet.rules.len(), 2, "updated in place, not added again");
    assert_eq!(doc.sheet_rule(Some("Shop 3 mm")).thickness, "3 mm");
    assert_eq!(doc.sheet.active.as_deref(), Some("Other"));
}

#[test]
fn the_save_icon_rename_and_delete_change_only_the_personal_library() {
    let mut h = manager("mm");
    save_personal(&mut h, "Steel 2.5");
    assert_eq!(library_json(&h)[0]["thickness"], "2.5 mm");
    let Sm::Rules { values, .. } = sm(&mut h) else { panic!() };
    values[0] = "3 mm".into();
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:sheet.personal_rules:overwrite"}})).unwrap();
    h.frames(2);
    assert_eq!(library_json(&h)[0]["thickness"], "3 mm", "the save icon overwrites the selected personal rule");
    h.step(&json!({"widget":"Rename"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:sheet.personal_rules.name"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"Steel 3"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.frames(2);
    assert_eq!(library_json(&h)[0]["name"], "Steel 3");
    h.step(&json!({"widget":"Delete"})).unwrap();
    h.frames(1);
    h.step(&json!({"widget":"Confirm delete"})).unwrap();
    h.frames(2);
    assert_eq!(library_json(&h), json!([]));
    assert!(h.app.session.doc.sheet.rules.is_empty(), "the design never changed");
}

#[test]
fn saving_refuses_a_rule_the_design_could_not_use() {
    let mut h = manager("mm");
    let doc = h.app.session.doc.clone();
    // A K factor past 1, an empty thickness, a definition and an unknown parameter.
    for (index, text) in [(1, "1.5"), (0, ""), (0, "T = 2"), (2, "Missing * 2")] {
        let Sm::Rules { values, .. } = sm(&mut h) else { panic!() };
        let kept = std::mem::replace(&mut values[index], text.into());
        assert_eq!(Rule::capture(sm(&mut h), "Bad", &doc), Err(INVALID_VALUES), "{text:?}");
        let Sm::Rules { values, .. } = sm(&mut h) else { panic!() };
        values[index] = kept;
    }
    assert!(Rule::capture(sm(&mut h), "Good", &doc).is_ok());
    let Sm::Rules { values, .. } = sm(&mut h) else { panic!() };
    values[1] = "1.5".into();
    // The manager reports the refusal a few frames later, which moves its rows: click once it has.
    for _ in 0..10 {
        if crate::scenario::handle_at("dialog:report:error").is_some() {
            break;
        }
        h.frames(1);
    }
    h.frames(1);
    save_personal(&mut h, "Bad K");
    assert_eq!(library_json(&h), json!([]), "nothing is saved while the prompt reports the error");
    assert!(personal_sheet_rules_prompt_open(&h));
}

fn personal_sheet_rules_prompt_open(h: &Harness) -> bool {
    super::name_prompt_open(&h.ctx, h.app.dialog.as_ref())
}

#[test]
fn malformed_rows_are_dropped_and_a_newer_versions_values_are_kept_but_never_recalled() {
    let mut bad_shape = row("Bad shape");
    bad_shape["corner_relief_shape"] = json!("hexagon");
    let mut empty = row("Empty");
    empty["thickness"] = json!(" ");
    let mut newer = row("Newer");
    newer["unfold"] = json!("bend table");
    let rows = json!([null, row("Shop"), bad_shape, empty, row(" padded "), row("SHOP"), {"name":"incomplete"}, newer]);
    let mut library: Library = serde_json::from_value(rows).unwrap();
    assert_eq!(library.0.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Shop", "Newer"]);
    assert_eq!(serde_json::to_value(&library).unwrap()[1]["unfold"], "bend table", "a newer version's value survives");
    let many: Vec<_> = (0..100).map(|i| row(&format!("Rule {i}"))).collect();
    assert_eq!(serde_json::from_value::<Library>(json!(many)).unwrap().0.len(), 64);
    library.rename("Newer", "Newer rule").unwrap();
    assert_eq!(serde_json::to_value(&library).unwrap()[1]["unfold"], "bend table", "renaming keeps it too");
    assert_eq!(library.rename("Newer rule", "shop"), Err("sheet.personal_rules.name_exists"));
    assert_eq!(library.rename("Newer rule", " "), Err("sheet.personal_rules.invalid_name"));

    let mut h = manager("mm");
    let doc = h.app.session.doc.clone();
    let before = format!("{:?}", sm(&mut h));
    let newer = library.0[1].clone();
    assert_eq!(newer.recall(sm(&mut h), &doc), Err("sheet.personal_rules.unsupported"));
    assert_eq!(format!("{:?}", sm(&mut h)), before, "a refused recall leaves the manager as it was");
    assert_eq!(library.overwrite("Newer rule", sm(&mut h), &doc), Err("sheet.personal_rules.unsupported"));
}

/// A rule that unfolds by a press brake chart's 90° bend deduction (#6794) is saved with its
/// method and its value, with the value's unit, and recalled with both in a design of another
/// unit. A rule shown with the K-factor keeps no 90° value, not even one left in its hidden field.
#[test]
fn a_personal_rule_keeps_its_unfold_method_and_90_degree_value() {
    use std::f64::consts::FRAC_PI_2;
    let mut h = manager("in");
    {
        let Sm::Rules { values, method, .. } = sm(&mut h) else { panic!() };
        *values = ["0.0625", "0.44", "Thickness", "Thickness", "Thickness * 0.5", "Thickness * 4.0", "0.0004", "Thickness", "0.1114"]
            .map(String::from)
            .to_vec();
        *method = 1;
    }
    h.frames(2);
    save_personal(&mut h, "CRS 16ga");
    let saved = library_json(&h);
    assert_eq!((&saved[0]["unfold_method"], &saved[0]["unfold_value"]), (&json!("deduction"), &json!("0.1114 in")), "{saved}");

    // Recalled in a millimetre design: the same rule, so the same K-factor once OK adds it.
    let mut other = manager_with("mm", &h.app.preferences.personal_sheet_rules);
    choose_personal(&mut other, "CRS 16ga");
    {
        let Sm::Rules { method, values, .. } = sm(&mut other) else { panic!() };
        assert_eq!((*method, values[8].as_str()), (1, "0.1114 in"));
    }
    let commands = ok_commands(&other);
    assert_eq!((&commands[0].1["unfold_method"], &commands[0].1["unfold_value"]), (&json!("deduction"), &json!("0.1114 in")));
    other.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    other.frames(2);
    let doc = &other.app.session.doc;
    let rule = doc.sheet_rule(None);
    assert_eq!((rule.name.as_str(), rule.unfold_method.as_str()), ("CRS 16ga", "deduction"));
    let (vals, _) = doc.param_values();
    let k = doc.rule_values(&vals, &rule).unwrap().k_factor;
    let (t, r, deduction) = (1.5875, 1.5875, 0.1114 * 25.4);
    assert!((k - ((2.0 * (r + t) - deduction) / FRAC_PI_2 - r) / t).abs() < 1e-9, "{k}");

    // Shown with the K-factor, the rule is saved without the value left in the hidden field.
    {
        let Sm::Rules { method, .. } = sm(&mut h) else { panic!() };
        *method = 0;
    }
    h.frames(2);
    save_personal(&mut h, "K only");
    let saved = library_json(&h);
    let k_only = saved.as_array().and_then(|rows| rows.iter().find(|row| row["name"] == "K only")).cloned().unwrap_or_default();
    assert!(k_only.is_object() && k_only.get("unfold_method").is_none() && k_only.get("unfold_value").is_none(), "{saved}");
}
