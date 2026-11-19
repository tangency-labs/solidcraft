//! The Sheet Metal Rules manager's standard library and rule packs (#18304).

use serde_json::{Value, json};

use crate::dialogs::Kind;
use crate::dialogs_sheet::Sm;
use crate::scenario::Harness;

fn manager() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.start("sheet.manage_rules");
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:section:sheet.library.title"}})).unwrap();
    h.frames(2);
    h
}

fn ok_commands(h: &Harness) -> Vec<(String, Value)> {
    crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()
}

fn choose(h: &mut Harness, key: &str, option: &str) {
    h.step(&json!({"click":{"handle":format!("dialog:{key}")}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:{key}:{option}")}})).unwrap();
    h.frames(2);
}

fn status(h: &Harness) -> String {
    h.app.status.as_ref().map(|(text, _, _)| text.clone()).unwrap_or_default()
}

fn folder(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("solidcraft-sheet-library-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn picking_a_material_and_gauge_fills_the_manager_and_ok_makes_that_rule() {
    let mut h = manager();
    choose(&mut h, "sheet.library.material", "Aluminium 6061-T6");
    // A millimetre design lists metric thicknesses first; the gauges are a pick away.
    choose(&mut h, "sheet.library.series", "Gauges");
    choose(&mut h, "sheet.library.gauge", "14 ga (Brown & Sharpe)  0.0641 in");
    let commands = ok_commands(&h);
    let (command, p) = &commands[0];
    assert_eq!(command, "sheet.manage_rules");
    assert_eq!(p["thickness"], "0.0641 in", "{p}");
    // The working radius stays the rule's (the tooling's), and the K-factor is the air-bending
    // estimate at that R/T = 1: the soft column's 0.40.
    assert_eq!(p["bend_radius"], "Thickness", "{p}");
    assert_eq!(p["k_factor"], "0.40", "{p}");
    assert_eq!(p["material"], "Aluminium 6061-T6", "{p}");
    // The estimate is said, and so is a radius under 6061-T6's published 3T at 14 ga.
    assert!(crate::scenario::handle_at("dialog:report:sheet_library_note_0").is_some());
    assert!(crate::scenario::handle_at("dialog:report:sheet_library_note_1").is_some());
    // Bottoming estimates again from the same R/T.
    choose(&mut h, "sheet.library.method", "Bottoming");
    assert_eq!(ok_commands(&h)[0].1["k_factor"], "0.46");
    // Another material clears the pick and leaves the thickness alone.
    choose(&mut h, "sheet.library.material", "Stainless steel 304");
    assert_eq!(ok_commands(&h)[0].1["thickness"], "0.0641 in");
    // Nothing changed in the design before OK.
    assert!(h.app.session.doc.sheet.rules.is_empty());
    for (command, p) in ok_commands(&h) {
        h.app.run(&command, p).unwrap();
    }
    let rule = h.app.session.doc.sheet_rule(None);
    assert_eq!((rule.name.as_str(), rule.material.as_str()), ("Aluminium 6061-T6, 14 ga", "Aluminium 6061-T6"));
}

#[test]
fn a_metric_thickness_is_listed_first_in_a_millimetre_design_and_picked_as_a_gauge_is() {
    let mut h = manager();
    choose(&mut h, "sheet.library.material", "Mild steel (cold-rolled)");
    choose(&mut h, "sheet.library.gauge", "1.5 mm");
    let commands = ok_commands(&h);
    let p = &commands[0].1;
    // Air bending mild steel at the default R = T: the medium column's 0.43.
    assert_eq!(
        (p["name"].as_str(), p["thickness"].as_str(), p["k_factor"].as_str()),
        (Some("Mild steel (cold-rolled), 1.5 mm"), Some("1.5 mm"), Some("0.43")),
        "{p}"
    );
}

#[test]
fn a_rule_pack_imports_into_the_personal_rules_and_they_export_as_one() {
    let dir = folder("roundtrip");
    let pack = dir.join("shop.csv");
    std::fs::write(&pack, "name,material,thickness,k_factor,bend_radius,press\nShop CRS 16,CRS,1.52 mm,0.42,0.8 mm,B1\nBad,,2,0.4,2 mm,\n").unwrap();
    let mut h = manager();
    let chosen = pack.to_string_lossy().into_owned();
    h.app.services.pick_file = Some(Box::new(move |_, extensions| {
        assert_eq!(extensions, ["json", "csv"]);
        let result = Some(chosen.clone());
        Box::pin(async move { result })
    }));
    h.step(&json!({"click":{"handle":"dialog:sheet.library:import"}})).unwrap();
    h.frames(4);
    // The bad row (a thickness with no unit) refuses the whole pack, naming it, and adds nothing.
    assert!(h.app.preferences.personal_sheet_rules.is_empty());
    assert!(status(&h).contains("rule 2 of the pack (`Bad`)") && status(&h).contains("no unit"), "{}", status(&h));
    std::fs::write(&pack, "name,material,thickness,k_factor,bend_radius,press\nShop CRS 16,CRS,1.52 mm,0.42,0.8 mm,B1\n").unwrap();
    h.step(&json!({"click":{"handle":"dialog:sheet.library:import"}})).unwrap();
    h.frames(4);
    let (rules, _) = h.app.preferences.personal_sheet_rules.sheet_rules();
    assert_eq!(rules.len(), 1, "{}", status(&h));
    assert_eq!((rules[0].name.as_str(), rules[0].material.as_str(), rules[0].bend_radius.as_str()), ("Shop CRS 16", "CRS", "0.8 mm"));
    assert!(status(&h).contains("press"), "the skipped column is named: {}", status(&h));
    // A second import keeps the personal rule of that name as it is.
    h.step(&json!({"click":{"handle":"dialog:sheet.library:import"}})).unwrap();
    h.frames(4);
    assert_eq!(h.app.preferences.personal_sheet_rules.sheet_rules().0.len(), 1);
    assert!(status(&h).contains("Shop CRS 16"), "{}", status(&h));
    let out = dir.join("mine.json");
    let chosen = out.to_string_lossy().into_owned();
    h.app.services.pick_save = Some(Box::new(move |_, _| {
        let result = Some(chosen.clone());
        Box::pin(async move { result })
    }));
    h.step(&json!({"click":{"handle":"dialog:sheet.library:export"}})).unwrap();
    h.frames(4);
    let written = std::fs::read(&out).unwrap();
    let back = solidcraft_engine::doc::sheet_library::read(&written).unwrap();
    assert_eq!(back.pack.rules.iter().map(|r| r.rule.clone()).collect::<Vec<_>>(), rules);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_manager_keeps_the_rules_material_and_ok_saves_an_edit_of_it() {
    let mut h = manager();
    h.app.run("sheet.manage_rules", json!({"name":"Galv","material":"Galvanneal","thickness":"1.6 mm","active":true})).unwrap();
    h.app.start("sheet.manage_rules");
    h.frames(4);
    let Some(Kind::Sheet(Sm::Rules { material, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("no manager") };
    assert_eq!(material, "Galvanneal");
    *material = "Galvanneal A60".into();
    let commands = ok_commands(&h);
    assert_eq!(commands[0].1["material"], "Galvanneal A60");
}

#[test]
fn a_new_rule_with_its_material_cleared_saves_none_rather_than_the_active_rules() {
    let mut h = manager();
    h.app.run("sheet.manage_rules", json!({"name":"Galv","material":"Galvanneal","thickness":"1.6 mm","active":true})).unwrap();
    h.app.start("sheet.manage_rules");
    h.frames(4);
    if let Some(Kind::Sheet(Sm::Rules { pick, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *pick = 1;
    }
    h.frames(2);
    let Some(Kind::Sheet(Sm::Rules { material, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("no manager") };
    material.clear();
    let commands = ok_commands(&h);
    assert_eq!(commands[0].1["material"], "", "{}", commands[0].1);
}
