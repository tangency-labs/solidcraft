//! Freeze rule-manager commands, reload policy and published row/control identities.
use crate::{SolveApp, dialogs::Kind, dialogs_plastic::Pl, dialogs_sheet::Sm, scenario, theme::Tokens};
use serde_json::{Value, json};

const SHEET_ROWS: &[&str] = &[
    "Rule",
    "Thickness",
    "sheet.unfold_method.label",
    "K Factor",
    "sheet.unfold_method.at_90",
    "Bend Radius",
    "Relief Width",
    "Relief Depth",
    "Corner Relief Shape",
    "Corner Relief",
    "Hem Gap",
    "Flange Gap",
    "Angle Display",
    "Active rule",
];
const PLASTIC_ROWS: &[&str] = &[
    "Rule",
    "Material",
    "Thickness",
    "Nominal Radius",
    "Clearance",
    "Knife Edge",
    "Reveal Height",
    "Thickness Variation",
    "Draft",
    "Max Thickness",
    "Min Thickness",
    "Min Draft",
    "Active rule",
];

fn frame(app: &mut SolveApp, ctx: &egui::Context, kind: &mut Kind, labels: &[&str]) {
    scenario::clear_handles();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        crate::ui_kit::scope(ui, |ui| match kind {
            Kind::Sheet(k) => assert!(!crate::dialogs_sheet::rows(app, ui, k, &mut [])),
            Kind::Plastic(k) => assert!(!crate::dialogs_plastic::rows(app, ui, k, &[])),
            _ => panic!("rule dialog"),
        });
    });
    output.textures_delta.clear();
    let rows = scenario::rects("dialog:row:");
    assert_eq!(rows.iter().map(|(name, _)| name.trim_start_matches("dialog:row:")).collect::<Vec<_>>(), labels);
    for pair in rows.windows(2) {
        assert_eq!(pair[1].1.top() - pair[0].1.top(), Tokens::get().control_height + Tokens::get().form_gap_y);
    }
    let expected: Vec<_> = labels.iter().flat_map(|label| [format!("dialog:{label}"), format!("dialog:row:{label}")]).collect();
    assert_eq!(scenario::handles("dialog:").into_iter().map(|(name, _)| name).collect::<Vec<_>>(), expected);
}

fn commands(app: &SolveApp, kind: &Kind) -> Vec<(String, Value)> {
    match kind {
        Kind::Sheet(k) => crate::dialogs_sheet::commands(app, k, &[], &Default::default()).unwrap(),
        Kind::Plastic(k) => crate::dialogs_plastic::commands(app, k, &[], &Default::default()).unwrap(),
        _ => panic!("rule dialog"),
    }
}

#[test]
fn sheet_rule_editor_contract() {
    let mut app = SolveApp::new(Default::default(), Default::default());
    let ctx = crate::fonts::test_context();
    let (mut kind, _) = crate::dialogs_sheet::start(&app, "sheet.manage_rules").unwrap();
    frame(&mut app, &ctx, &mut kind, SHEET_ROWS);
    let mut expected = json!({"name":"Steel (mm)","active":true,"corner_relief_shape":"notch","thickness":"2.5 mm","k_factor":"0.44",
        "bend_radius":"Thickness","relief_width":"Thickness","relief_depth":"Thickness * 0.5","corner_relief":"Thickness * 4.0","hem_gap":"0.01 mm","gap":"Thickness","angle_display":"bend","unfold_method":"k_factor"});
    assert_eq!(commands(&app, &kind), vec![("sheet.manage_rules".into(), expected.clone())]);
    let Kind::Sheet(Sm::Rules { pick, values, relief, .. }) = &mut kind else { panic!() };
    *pick = 1;
    values[0] = "99 mm".into();
    *relief = 3;
    let mut new_rows = SHEET_ROWS.to_vec();
    new_rows.insert(1, "Name");
    frame(&mut app, &ctx, &mut kind, &new_rows);
    expected["name"] = json!("Rule2");
    assert_eq!(commands(&app, &kind), vec![("sheet.manage_rules".into(), expected.clone())]);
    let Kind::Sheet(Sm::Rules { name, values, relief, active, .. }) = &mut kind else { panic!() };
    *name = "  Custom  ".into();
    values[0] = "3 mm".into();
    values[1] = " ".into();
    *relief = 2;
    *active = false;
    frame(&mut app, &ctx, &mut kind, &new_rows);
    expected["name"] = json!("Custom");
    expected["thickness"] = json!("3 mm");
    expected["corner_relief_shape"] = json!("rectangular");
    expected["active"] = json!(false);
    expected.as_object_mut().unwrap().remove("k_factor");
    assert_eq!(commands(&app, &kind), vec![("sheet.manage_rules".into(), expected)]);
    let Kind::Sheet(Sm::Rules { pick, .. }) = &mut kind else { panic!() };
    *pick = 0;
    frame(&mut app, &ctx, &mut kind, SHEET_ROWS);
    assert_eq!(commands(&app, &kind)[0].1["thickness"], "2.5 mm");
    assert_eq!(commands(&app, &kind)[0].1["active"], true);
}

#[test]
fn plastic_rule_editor_contract() {
    let mut app = SolveApp::new(Default::default(), Default::default());
    let ctx = crate::fonts::test_context();
    let (mut kind, _) = crate::dialogs_plastic::start(&app, "plastic.manage_rules").unwrap();
    frame(&mut app, &ctx, &mut kind, PLASTIC_ROWS);
    let mut expected = json!({"name":"ABS (1.5mm)","active":false,"material":"ABS Plastic","thickness":"1.5 mm","nominal_radius":"0.5 * Thickness",
        "clearance":"0.1 mm","knife_edge":"1 mm","reveal_height":"0.5 mm","thickness_variation":"0.15 * Thickness","draft":"2 deg",
        "max_thickness":"3.5 mm","min_thickness":"1.2 mm","min_draft":"0.5 deg"});
    assert_eq!(commands(&app, &kind), vec![("plastic.manage_rules".into(), expected.clone())]);
    app.session.execute("plastic.manage_rules", &json!({"name":"Nylon (PA6) (2.5mm)","active":true})).unwrap();
    let Kind::Plastic(Pl::Rules { pick, .. }) = &mut kind else { panic!() };
    *pick = 1;
    frame(&mut app, &ctx, &mut kind, PLASTIC_ROWS);
    expected["name"] = json!("Nylon (PA6) (2.5mm)");
    expected["active"] = json!(true);
    expected["material"] = json!("Nylon 6");
    expected["thickness"] = json!("2.5 mm");
    expected["max_thickness"] = json!("3 mm");
    expected["min_thickness"] = json!("0.8 mm");
    assert_eq!(commands(&app, &kind), vec![("plastic.manage_rules".into(), expected)]);
    let Kind::Plastic(Pl::Rules { pick, .. }) = &mut kind else { panic!() };
    *pick = app.session.doc.plastic_rules().len();
    let mut new_rows = PLASTIC_ROWS.to_vec();
    new_rows.insert(1, "Name");
    frame(&mut app, &ctx, &mut kind, &new_rows);
    let mut expected = json!({"name":"Plastic Rule1","active":false,"material":"ABS Plastic","thickness":"1.5 mm","nominal_radius":"0.5 * Thickness",
        "clearance":"0.1 mm","knife_edge":"1 mm","reveal_height":"0.5 mm","thickness_variation":"0.15 * Thickness","draft":"2 deg",
        "max_thickness":"3.5 mm","min_thickness":"1.2 mm","min_draft":"0.5 deg"});
    assert_eq!(commands(&app, &kind), vec![("plastic.manage_rules".into(), expected.clone())]);
    let Kind::Plastic(Pl::Rules { name, values, active, .. }) = &mut kind else { panic!() };
    *name = "  Custom  ".into();
    values[0] = " ".into();
    values[1] = " ".into();
    *active = true;
    frame(&mut app, &ctx, &mut kind, &new_rows);
    expected["name"] = json!("Custom");
    expected["active"] = json!(true);
    for key in ["material", "thickness"] {
        expected.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(commands(&app, &kind), vec![("plastic.manage_rules".into(), expected)]);
}

#[test]
fn sheet_rule_editor_delete_reloads_and_plastic_has_no_delete() {
    for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
        let mut h = scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.set", json!({"layout":layout}));
        h.app.session.execute("sheet.manage_rules", &json!({"name":"Custom","thickness":"3 mm","active":true})).unwrap();
        h.app.start("sheet.manage_rules");
        h.frames(4);
        assert!(h.widget("Delete rule").is_some());
        assert_eq!(crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["name"], "Custom");
        h.step(&json!({"widget":"Delete rule"})).unwrap();
        h.frames(4);
        assert!(h.app.session.doc.sheet.rules.is_empty());
        assert!(h.widget("Delete rule").is_none());
        assert_eq!(crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["name"], "Steel (mm)");
        h.app.dialog = None;
        h.app.start("plastic.manage_rules");
        h.frames(4);
        assert!(h.widget("Delete rule").is_none());
    }
}
