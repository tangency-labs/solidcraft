use serde_json::{Value, json};

use crate::scenario::Harness;
use crate::settings_profiles::{FORMAT, MAX_PROFILES, capture, keys, sanitize};

fn run(h: &mut Harness, id: &str, params: Value) -> Value {
    let out = h.call("engine.execute", json!({ "command": id, "params": params }));
    assert!(out["error"].is_null(), "{id}: {out}");
    out["result"].clone()
}

fn error(h: &mut Harness, id: &str, params: Value) -> String {
    let out = h.call("engine.execute", json!({ "command": id, "params": params }));
    out["error"].as_str().map(str::to_string).unwrap_or_else(|| panic!("{id} should fail: {out}"))
}

#[test]
fn a_profile_holds_input_and_interface_settings_but_not_the_machine() {
    let keys: Vec<&str> = keys().collect();
    for key in [
        "nav",
        "mouse_gestures",
        "radial_context_menu",
        "zoom_reverse",
        "theme",
        "ui_scale",
        "ui_font",
        "navigator_style",
        "language",
        "suppress_sketch_snap",
    ] {
        assert!(keys.contains(&key), "{key} belongs in a profile");
    }
    for key in [
        "space_mouse_calibrations",
        "space_mouse_driver",
        "space_mouse_speed",
        "dark_parts_shading",
        "msaa",
        "window_system",
        "default_units",
        "extrude_view_isometric",
        "extrude_view_return",
        "update",
        "agents",
        "custom_themes",
        "orbit_cursor",
    ] {
        assert!(!keys.contains(&key), "{key} stays out of a profile");
    }
    let h = Harness::new();
    let doc = capture(&h.app, "Laptop");
    assert_eq!(doc["format"], FORMAT);
    assert_eq!(sanitize(&doc).unwrap(), doc, "a captured profile passes its own checks unchanged");
}

#[test]
fn switching_profiles_applies_every_setting_at_once() {
    let mut h = Harness::new();
    h.app.preferences.ui_scale = 1.0;
    h.app.preferences.nav = "fusion".into();
    assert_eq!(run(&mut h, "settings.profile.save", json!({"name": "Laptop"})), json!({"name": "Laptop"}));

    h.app.preferences.ui_scale = 1.5;
    h.app.preferences.nav = "solidworks".into();
    h.app.preferences.mouse_gestures.enabled = true;
    h.app.preferences.radial_context_menu = true;
    h.app.keymap.custom.insert("solid.extrude".into(), vec!["E".into(), "Shift+E".into()]);
    h.app.sbox.pinned = vec!["solid.fillet".into()];
    h.app.toolbar_custom.panels.insert("DESIGN/SOLID/CREATE".into(), vec!["solid.box".into()]);
    run(&mut h, "settings.profile.save", json!({"name": "Large monitor"}));
    assert_eq!(run(&mut h, "settings.profile.list", json!({})), json!({"profiles": ["Laptop", "Large monitor"], "active": "Large monitor"}));

    let out = run(&mut h, "settings.profile.apply", json!({"name": "Laptop"}));
    assert_eq!((out["name"].as_str(), out["skipped"].as_array().map(Vec::len)), (Some("Laptop"), Some(0)), "{out}");
    assert_eq!((h.app.preferences.ui_scale, h.app.preferences.nav.as_str()), (1.0, "fusion"));
    assert!(!h.app.preferences.mouse_gestures.enabled && !h.app.preferences.radial_context_menu);
    assert!(h.app.keymap.custom.is_empty() && h.app.sbox.pinned.is_empty() && h.app.toolbar_custom.panels.is_empty());
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Laptop"));

    run(&mut h, "settings.profile.apply", json!({"name": "Large monitor"}));
    assert_eq!((h.app.preferences.ui_scale, h.app.preferences.nav.as_str()), (1.5, "solidworks"));
    assert!(h.app.preferences.mouse_gestures.enabled && h.app.preferences.radial_context_menu);
    assert_eq!(h.app.keymap.custom.get("solid.extrude").map(Vec::as_slice), Some(&["E".to_string(), "Shift+E".to_string()][..]));
    assert_eq!(h.app.sbox.pinned, ["solid.fillet"]);
    assert_eq!(h.app.toolbar_custom.panels.get("DESIGN/SOLID/CREATE"), Some(&vec!["solid.box".to_string()]));

    assert!(error(&mut h, "settings.profile.apply", json!({"name": "Desk"})).contains("no settings profile"));
}

#[test]
fn rename_and_delete_follow_the_active_profile() {
    let mut h = Harness::new();
    run(&mut h, "settings.profile.save", json!({"name": "A"}));
    run(&mut h, "settings.profile.save", json!({"name": "B"}));
    assert!(error(&mut h, "settings.profile.rename", json!({"name": "B", "to": "A"})).contains("another settings profile"));
    run(&mut h, "settings.profile.rename", json!({"name": "B", "to": "Desk"}));
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    assert_eq!(run(&mut h, "settings.profile.delete", json!({"name": "Desk"})), json!({"deleted": "Desk"}));
    assert_eq!(run(&mut h, "settings.profile.list", json!({})), json!({"profiles": ["A"], "active": null}));
}

#[test]
fn profiles_and_the_active_one_are_kept_between_runs() {
    let mut h = Harness::new();
    h.app.preferences.ui_scale = 1.25;
    run(&mut h, "settings.profile.save", json!({"name": "Laptop"}));
    let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    other.load_prefs(&h.app.prefs());
    assert_eq!(other.settings_profiles.names(), ["Laptop"]);
    assert_eq!(other.settings_profiles.active.as_deref(), Some("Laptop"));
    assert_eq!(other.settings_profiles.list, h.app.settings_profiles.list);
}

#[test]
fn gruvbox_legacy_profiles_keep_their_selected_contrast_with_empty_option_maps() {
    let mut h = Harness::new();
    for (old, family, contrast) in crate::theme_files::GRUVBOX_ALIASES {
        let profile = json!({"format":FORMAT,"name":old,"preferences":{"theme":old,"theme_options":{}}});
        run(&mut h, "settings.profile.import", json!({"profile":profile}));
        let out = run(&mut h, "settings.profile.apply", json!({"name":old}));
        assert_eq!(out["skipped"], json!([]));
        assert_eq!(h.app.preferences.theme, family);
        assert_eq!(h.app.preferences.theme_options[family]["contrast"], contrast);
    }
    h.app.preferences.theme_options.insert("unrelated".into(), [("future".into(), json!({"keep":true}))].into());
    let profile = json!({"format":FORMAT,"name":"Legacy without options","preferences":{"theme":"gruvbox-light-soft"}});
    run(&mut h, "settings.profile.import", json!({"profile":profile}));
    let out = run(&mut h, "settings.profile.apply", json!({"name":"Legacy without options"}));
    assert_eq!(out["skipped"], json!([]));
    assert_eq!(h.app.preferences.theme_options["gruvbox-light"]["contrast"], "soft");
    assert_eq!(h.app.preferences.theme_options["unrelated"]["future"], json!({"keep":true}));
    // A profile's own theme with a retired name is imported under a custom id,
    // rather than being mistaken for a built-in contrast selection.
    let profile = json!({"format":FORMAT,"name":"Named theme","preferences":{"theme":"gruvbox-dark-hard",
        "theme_options":{"gruvbox-dark-hard":{"contrast":"personal"}}},
        "themes":[{"id":"gruvbox-dark-hard","format":"solidcraft-theme/1","name":"Gruvbox Dark Hard","base":"light",
            "options":[{"id":"contrast","type":"choice","default":"personal","values":[{"id":"personal","label":{"en":"Personal"}},{"id":"other","label":{"en":"Other"}}]}]}]});
    run(&mut h, "settings.profile.import", json!({"profile":profile}));
    let out = run(&mut h, "settings.profile.apply", json!({"name":"Named theme"}));
    assert_eq!(out["skipped"], json!([]));
    assert_eq!(h.app.preferences.theme, "custom-gruvbox-dark-hard");
    assert_eq!(h.app.preferences.theme_options["custom-gruvbox-dark-hard"]["contrast"], "personal");
}

#[test]
fn gruvbox_profile_migration_keeps_malformed_theme_fields_for_skipped_reporting() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"theme":"gruvbox-light","theme_options":{"gruvbox-light":{"contrast":"soft"}}}));
    for bad in [json!({"theme":42,"theme_options":"bad"}), json!({"theme":false,"theme_options":{"gruvbox-dark":"bad"}})] {
        let before_theme = h.app.preferences.theme.clone();
        let before_options = h.app.preferences.theme_options.clone();
        let profile = json!({"format":FORMAT,"name":"Malformed","preferences":bad});
        // Keep the per-key apply guard effective even for malformed in-memory documents.
        h.app.settings_profiles.list = vec![profile];
        let out = run(&mut h, "settings.profile.apply", json!({"name":"Malformed"}));
        assert_eq!(out["skipped"], json!(["theme", "theme_options"]));
        assert_eq!(h.app.preferences.theme, before_theme);
        assert_eq!(h.app.preferences.theme_options, before_options);
    }
}

#[test]
fn theme_options_profiles_restore_replace_and_preserve_stale_values() {
    let mut h = Harness::new();
    let file = json!({
        "format":"solidcraft-theme/1", "name":"Profile Options", "base":"dark",
        "options":[{"id":"enabled","type":"bool","default":false}],
        "overlays":[{"when":{"opt":"enabled","eq":true},"colors":{"panel":"#abcdef"}}]
    });
    let out = h.call("ui.prefs", json!({"theme_import":file,"theme_options":{"profile-options":{"enabled":true}}}));
    assert!(out["error"].is_null(), "{out}");
    h.app.preferences.theme_options.entry("profile-options".into()).or_default().insert("future".into(), json!({"unrecognized":true}));
    h.app.preferences.theme_options.insert("unknown-theme".into(), [("saved".into(), json!([1, 2]))].into());
    run(&mut h, "settings.profile.save", json!({"name":"Options"}));
    let exported = run(&mut h, "settings.profile.export", json!({"name":"Options"}))["profile"].clone();
    let options = exported["preferences"]["theme_options"].clone();
    assert_eq!(exported["themes"][0]["options"][0]["default"], false);
    h.call("ui.prefs", json!({"open":"Interface","button":"defaults"}));
    run(&mut h, "settings.profile.save", json!({"name":"Empty"}));
    run(&mut h, "settings.profile.import", json!({"profile":exported,"name":"Imported"}));
    let out = run(&mut h, "settings.profile.apply", json!({"name":"Imported"}));
    assert_eq!(out["skipped"], json!([]));
    assert_eq!(serde_json::to_value(&h.app.preferences.theme_options).unwrap(), options);
    h.frames(2);
    assert_eq!(crate::theme::Tokens::get().panel, egui::Color32::from_rgb(0xab, 0xcd, 0xef));
    run(&mut h, "settings.profile.apply", json!({"name":"Empty"}));
    assert!(h.app.preferences.theme_options.is_empty());
    run(&mut h, "settings.profile.apply", json!({"name":"Options"}));
    assert_eq!(serde_json::to_value(&h.app.preferences.theme_options).unwrap(), options);
}

#[test]
fn export_and_import_go_through_one_file_and_import_never_switches() {
    let dir = std::env::temp_dir().join(format!("solidcraft-profile-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("laptop.json");
    let path = path.to_str().unwrap();
    let mut h = Harness::new();
    h.app.preferences.zoom_reverse = true;
    run(&mut h, "settings.profile.save", json!({"name": "Laptop"}));
    let out = run(&mut h, "settings.profile.export", json!({"name": "Laptop", "path": path}));
    assert_eq!(out["profile"]["format"], FORMAT);
    let written: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(written, out["profile"]);

    h.app.preferences.zoom_reverse = false;
    run(&mut h, "settings.profile.save", json!({"name": "Desk"}));
    assert_eq!(run(&mut h, "settings.profile.import", json!({"path": path})), json!({"name": "Laptop 2"}));
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"), "an import doesn't switch");
    assert!(!h.app.preferences.zoom_reverse);
    run(&mut h, "settings.profile.apply", json!({"name": "Laptop 2"}));
    assert!(h.app.preferences.zoom_reverse);
    assert_eq!(run(&mut h, "settings.profile.import", json!({"profile": written, "name": "Travel"})), json!({"name": "Travel"}));

    let big = dir.join("big.json");
    std::fs::write(&big, " ".repeat(5 * 1024 * 1024)).unwrap();
    assert!(error(&mut h, "settings.profile.import", json!({"path": big.to_str().unwrap()})).contains("at most"));
    assert!(error(&mut h, "settings.profile.import", json!({"path": dir.join("missing.json").to_str().unwrap()})).contains("missing.json"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_profiles_are_refused_or_cut_down_without_panicking() {
    let mut h = Harness::new();
    let refused = [
        json!(null),
        json!("text"),
        json!({"format": "solidcraft-shortcuts/1", "name": "A"}),
        json!({"format": FORMAT}),
        json!({"format": FORMAT, "name": ""}),
        json!({"format": FORMAT, "name": "x".repeat(65)}),
        json!({"format": FORMAT, "name": "A\u{0}"}),
        json!({"format": FORMAT, "name": "A", "preferences": []}),
    ];
    for doc in refused {
        assert!(sanitize(&doc).is_err(), "{doc}");
        error(&mut h, "settings.profile.import", json!({"profile": doc}));
    }
    let shortcuts: serde_json::Map<String, Value> = (0..5000).map(|i| (format!("cmd.{i}"), json!("F1"))).collect();
    let doc = sanitize(&json!({
        "format": FORMAT,
        "name": "A",
        "unknown": 1,
        "preferences": {"ui_scale": "big", "msaa": 8, "nav": "solidworks", "space_mouse_speed": 9.0, "no_such": true, "zoom_reverse": true},
        "shortcuts": Value::Object(shortcuts),
        "favourites": (0..500).map(|i| json!(format!("f{i}"))).chain([json!(3), json!("x".repeat(300))]).collect::<Vec<_>>(),
        "themes": [{"format": "solidcraft-theme/1", "name": "Bad", "base": "grey"}],
    }))
    .unwrap();
    assert!(doc.get("unknown").is_none());
    assert_eq!(doc["preferences"], json!({"nav": "solidworks", "zoom_reverse": true}), "unknown, unreadable and machine settings are left out");
    assert_eq!(doc["shortcuts"].as_object().map(serde_json::Map::len), Some(2000));
    assert_eq!(doc["favourites"].as_array().map(Vec::len), Some(64));
    assert_eq!(doc["themes"], json!([]));

    // Readable but refused when applied: the setting keeps its value and is reported.
    let before = h.app.preferences.ui_scale;
    let doc = json!({"format": FORMAT, "name": "Odd", "preferences": {"ui_scale": 99.0, "zoom_reverse": true}});
    run(&mut h, "settings.profile.import", json!({"profile": doc}));
    let out = run(&mut h, "settings.profile.apply", json!({"name": "Odd"}));
    assert_eq!((out["applied"].as_u64(), out["skipped"].clone()), (Some(1), json!(["ui_scale"])), "{out}");
    assert_eq!(h.app.preferences.ui_scale, before);
    assert!(h.app.preferences.zoom_reverse);

    for i in h.app.settings_profiles.list.len()..MAX_PROFILES {
        run(&mut h, "settings.profile.save", json!({ "name": format!("P{i}") }));
    }
    assert!(error(&mut h, "settings.profile.save", json!({"name": "One more"})).contains("at most"));
    let one_more = capture(&h.app, "One more");
    assert!(error(&mut h, "settings.profile.import", json!({"profile": one_more})).contains("at most"));
}

#[test]
fn a_command_with_several_keys_keeps_them_all_in_a_profile() {
    let doc = sanitize(&json!({
        "format": FORMAT,
        "name": "A",
        "shortcuts": {"solid.extrude": ["E", "Shift+E"], "sketch.line": "L", "sketch.circle": "", "bad": 3, "long": ["E", "x".repeat(300)]},
    }))
    .unwrap();
    assert_eq!(doc["shortcuts"], json!({"solid.extrude": ["E", "Shift+E"], "sketch.line": "L", "sketch.circle": ""}));
    let mut h = Harness::new();
    h.app.keymap.custom.insert("solid.extrude".into(), vec!["E".into(), "Shift+E".into()]);
    let doc = capture(&h.app, "Laptop");
    assert_eq!(doc["shortcuts"]["solid.extrude"], json!(["E", "Shift+E"]));
    assert_eq!(sanitize(&doc).unwrap(), doc, "several keys pass the profile's own checks");
}

#[test]
fn a_profile_brings_its_imported_theme_along() {
    let mut h = Harness::new();
    let file = json!({"format": "solidcraft-theme/1", "name": "Paper", "base": "light", "colors": {"panel": "#fffdf0"}});
    h.call("ui.prefs", json!({"theme_import": file}));
    assert_eq!(h.app.preferences.theme, "paper");
    run(&mut h, "settings.profile.save", json!({"name": "Bright"}));
    h.call("ui.prefs", json!({"theme_remove": "paper"}));
    assert!(h.app.preferences.custom_themes.is_empty());
    run(&mut h, "settings.profile.apply", json!({"name": "Bright"}));
    assert_eq!(h.app.preferences.theme, "paper");
    assert_eq!(h.app.preferences.custom_themes.len(), 1);
    assert!(!h.app.ui.dark);
}

#[test]
fn cancel_in_preferences_takes_a_switch_back() {
    let mut h = Harness::new();
    run(&mut h, "settings.profile.save", json!({"name": "Plain"}));
    h.app.preferences.ui_scale = 1.5;
    h.app.keymap.custom.insert("solid.extrude".into(), vec!["E".into(), "Shift+E".into()]);
    h.app.sbox.pinned = vec!["solid.fillet".into()];
    run(&mut h, "settings.profile.save", json!({"name": "Big"}));
    h.call("ui.prefs", json!({"open": "General"}));
    run(&mut h, "settings.profile.apply", json!({"name": "Plain"}));
    assert_eq!(h.app.preferences.ui_scale, 1.0);
    h.call("ui.prefs", json!({"button": "cancel"}));
    assert_eq!(h.app.preferences.ui_scale, 1.5);
    assert_eq!(h.app.keymap.custom.get("solid.extrude").map(Vec::as_slice), Some(&["E".to_string(), "Shift+E".to_string()][..]));
    assert_eq!(h.app.sbox.pinned, ["solid.fillet"]);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Big"));
}

#[test]
fn starting_a_profile_command_opens_preferences_instead_of_failing() {
    let mut h = Harness::new();
    h.app.start("settings.profile.save");
    assert!(h.app.prefs_window.open);
    assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_GENERAL);
    h.frames(2);
    assert!(h.app.prefs_window.open);
}

#[test]
fn choosing_a_profile_in_preferences_applies_it() {
    let mut h = Harness::new();
    run(&mut h, "settings.profile.save", json!({"name": "Laptop"}));
    h.app.preferences.zoom_reverse = true;
    run(&mut h, "settings.profile.save", json!({"name": "Large monitor"}));
    run(&mut h, "settings.profile.apply", json!({"name": "Laptop"}));
    assert!(!h.app.preferences.zoom_reverse);
    h.call("ui.prefs", json!({"open": "General"}));
    h.frames(3);
    h.step(&json!({"click": {"handle": "prefs:profile:Large monitor:apply"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Large monitor"));
    assert!(h.app.preferences.zoom_reverse);
}

#[test]
fn general_profile_rows_save_replace_apply_and_show_real_state_in_both_themes() {
    for theme in ["light", "dark"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"language":"en", "theme":theme, "open":"General"}));
        h.frames(3);
        assert!(h.app.settings_profiles.list.is_empty());
        let body = crate::scenario::rect_of("prefs:page_body").unwrap();
        let heading = crate::scenario::rect_of("prefs:page_heading").unwrap();
        let subtitle = crate::scenario::rect_of("dialog:hint:settings_heading").unwrap();
        let profiles = crate::scenario::rect_of("prefs:section:profiles").unwrap();
        assert!(heading.contains_rect(subtitle));
        {
            let title = h
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.job.text == "PROFILES"
                        && profiles.contains(text.pos)
                    {
                        return Some(egui::Rect::from_min_size(text.pos, text.galley.size()));
                    }
                    None
                })
                .unwrap();
            let count = crate::scenario::rect_of("dialog:section:Profiles:count").unwrap();
            assert!((count.left() - title.right() - 12.0).abs() < 1.1, "{theme}: count follows the painted title");
            let t = crate::theme::Tokens::get();
            let rule = h
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::LineSegment { points, stroke } = &shape.shape
                        && stroke.color == t.dialog_rule
                        && points.iter().all(|point| (point.y - profiles.center().y).abs() < 0.5)
                    {
                        return Some(*points);
                    }
                    None
                })
                .unwrap();
            assert!((rule[0].x - count.right() - 12.0).abs() < 0.5, "{theme}: the rule begins after the count");
            assert!((rule[1].x - profiles.right()).abs() < 0.5);
            for (key, text) in [("held", "Held"), ("computer", "This computer")] {
                let label = crate::scenario::rect_of(&format!("prefs:profiles:{key}:label")).unwrap();
                let first = crate::scenario::rect_of(&format!("prefs:profiles:{key}:first_chip")).unwrap();
                let painted = h
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(galley) = &shape.shape
                            && galley.galley.job.text == text
                            && label.contains(galley.pos)
                        {
                            return Some(egui::Rect::from_min_size(galley.pos, galley.galley.size()));
                        }
                        None
                    })
                    .unwrap();
                assert!((painted.left() - body.left()).abs() < 0.75, "{theme}: {text} starts at the body's left edge");
                assert!((first.left() - body.left() - 90.0).abs() < 0.5, "{theme}: {text} reserves the full caption column and gap");
                assert!((first.height() - 24.0).abs() < 0.5);
            }
            let held = crate::scenario::rect_of("prefs:profiles:held:row").unwrap();
            let computer = crate::scenario::rect_of("prefs:profiles:computer:row").unwrap();
            assert!((computer.top() - held.bottom() - 8.0).abs() < 0.5);
        }
        assert!((profiles.top() - heading.bottom() - 38.0).abs() < 0.5, "{theme}: the first section has the same breathing room as later sections");
        for (previous, next) in [("profiles", "language"), ("language", "saving")] {
            let previous_body = crate::scenario::rect_of(&format!("prefs:section:{previous}:body")).unwrap();
            let next_header = crate::scenario::rect_of(&format!("prefs:section:{next}")).unwrap();
            assert!((next_header.top() - previous_body.bottom() - 38.0).abs() < 0.5, "{theme}: {previous} to {next}");
            assert!((next_header.height() - 30.0).abs() < 0.5);
            let next_body = crate::scenario::rect_of(&format!("prefs:section:{next}:body")).unwrap();
            assert!((next_body.top() - next_header.bottom() - 10.0).abs() < 0.5);
        }
        let name_field = crate::scenario::rect_of("dialog:widget:Profile name").unwrap();
        assert!((name_field.width() - 220.0).abs() < 0.5, "{theme}: bounded name entry");
        assert!((name_field.left() - body.left()).abs() < 0.5, "{theme}: no separate label column before name entry");
        assert!(body.contains_rect(name_field));
        h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
        let status = h.app.status.clone();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.status, status, "empty Enter cannot submit a save");
        h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
        h.step(&json!({"text":"x".repeat(65)})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.status, status, "invalid Enter cannot submit a save");
        assert!(h.app.settings_profiles.list.is_empty());
        h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
        h.step(&json!({"key":"A", "cmd":true})).unwrap();
        h.step(&json!({"text":"  Laptop  "})).unwrap();
        h.step(&json!({"click":{"handle":"prefs:profile:save"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.settings_profiles.names(), ["Laptop"]);
        assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Laptop"));
        assert_eq!(
            h.output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some("Profile name"))
                .find_map(|(_, node)| node.value()),
            Some("  Laptop  "),
            "saving keeps exactly the typed text while the command normalizes its name"
        );
        assert!(crate::scenario::rect_of("prefs:profile:Laptop:active").is_some());
        assert!(crate::scenario::rect_of("prefs:profile:Laptop:apply").is_none());
        h.app.preferences.zoom_reverse = true;
        h.frames(3);
        assert!(crate::scenario::rect_of("prefs:profile:Laptop:edited").is_some());
        assert!(crate::scenario::rect_of("prefs:profile:edited").is_some());
        // The new label must keep save's replacement semantics, not create a duplicate.
        h.step(&json!({"click":{"handle":"prefs:profile:save"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.settings_profiles.list.len(), 1);
        assert_eq!(h.app.settings_profiles.list[0]["preferences"]["zoom_reverse"], true);
        assert!(crate::scenario::rect_of("prefs:profile:Laptop:edited").is_none());
        assert!(crate::scenario::rect_of("prefs:profile:edited").is_none());
        h.app.preferences.zoom_reverse = false;
        h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
        h.step(&json!({"key":"A", "cmd":true})).unwrap();
        h.step(&json!({"text":"Desk"})).unwrap();
        h.step(&json!({"click":{"handle":"prefs:profile:save"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.settings_profiles.names(), ["Laptop", "Desk"]);
        assert!(crate::scenario::rect_of("prefs:profile:Desk:apply").is_none());
        let laptop = crate::scenario::rect_of("prefs:profile:Laptop:card").unwrap();
        let desk = crate::scenario::rect_of("prefs:profile:Desk:card").unwrap();
        assert!((laptop.height() - 42.0).abs() < 0.5 && (desk.height() - 42.0).abs() < 0.5, "{theme}: compact saved cards");
        assert!((desk.top() - laptop.bottom() - 8.0).abs() < 0.5, "{theme}: saved card gap");
        assert_eq!((laptop.left(), laptop.right()), (desk.left(), desk.right()));
        for (name, card) in [("Laptop", laptop), ("Desk", desk)] {
            assert!(crate::scenario::rect_of("prefs:page_body").unwrap().contains_rect(card));
            for action in ["export", "rename", "delete"] {
                let target = crate::scenario::rect_of(&format!("prefs:profile:{name}:{action}")).unwrap();
                assert!(card.contains_rect(target), "{theme}: {name} {action} stays inside its card");
                assert!((target.width() - 24.0).abs() < 0.5 && (target.height() - 24.0).abs() < 0.5);
            }
        }
        let active = crate::scenario::rect_of("prefs:profile:Desk:active").unwrap();
        assert!(desk.contains_rect(active));
        assert!((active.height() - 18.0).abs() < 0.5);
        let t = crate::theme::Tokens::get();
        assert!(h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect == desk && rect.fill == t.selected_bg && rect.stroke.color == t.form_primary)));
        assert!(h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if desk.contains(text.pos) && text.galley.job.text == "Desk" && text.galley.job.sections.iter().all(|section| section.format.color == t.selected_ink && section.format.font_id == crate::fonts::semibold(t.font_label)))));
        let body = crate::scenario::rect_of("prefs:page_body").unwrap();
        assert!(h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if body.contains(text.pos) && text.galley.job.text == "Appearance" && text.galley.job.sections.iter().all(|section| section.format.color == t.selected_ink && section.format.font_id == crate::fonts::semibold(t.font_caption)))));
        assert!(laptop.contains_rect(crate::scenario::rect_of("prefs:profile:Laptop:apply").unwrap()));
        h.step(&json!({"click":{"handle":"prefs:profile:Laptop:apply"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Laptop"));
        assert!(h.app.preferences.zoom_reverse);
        assert_eq!(
            h.output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some("Profile name"))
                .find_map(|(_, node)| node.value()),
            Some("Desk"),
            "applying a different row preserves the user-typed draft"
        );
        h.app.preferences.zoom_reverse = false;
        h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
        h.step(&json!({"key":"A", "cmd":true})).unwrap();
        h.step(&json!({"text":"Laptop"})).unwrap();
        h.step(&json!({"click":{"handle":"prefs:profile:save"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.settings_profiles.list.len(), 2, "typing an existing name still replaces it");
        assert_eq!(h.app.settings_profiles.list[0]["preferences"]["zoom_reverse"], false);
        assert_eq!(
            h.output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some("Profile name"))
                .find_map(|(_, node)| node.value()),
            Some("Laptop"),
            "replacement keeps the typed name too"
        );
        assert!(h.app.prefs_window.filter.is_empty(), "name entry must not type into search");
        assert!(h.app.prefs_window.open);
    }
}

#[test]
fn general_inactive_profile_rows_export_rename_import_and_delete_without_applying() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.zoom_reverse = true;
    run(&mut h, "settings.profile.save", json!({"name":"Laptop"}));
    h.app.preferences.zoom_reverse = false;
    run(&mut h, "settings.profile.save", json!({"name":"Desk"}));
    h.call("ui.prefs", json!({"language":"en", "open":"General"}));
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some(""),
        "the name entry starts empty even with an active profile"
    );
    h.step(&json!({"click":{"handle":"prefs:profile:Laptop:export"}})).unwrap();
    h.frames(6);
    let path = h.dir.join("Laptop.solidcraft-profile.json");
    let exported: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(exported["name"], "Laptop");
    assert_eq!(exported["preferences"]["zoom_reverse"], true);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    assert!(!h.app.preferences.zoom_reverse);

    h.step(&json!({"click":{"handle":"prefs:profile:Laptop:rename"}})).unwrap();
    h.frames(3);
    assert!(h.widget("Laptop").is_some(), "the shared editor is prefilled");
    h.step(&json!({"text":"Travel"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.names(), ["Travel", "Desk"]);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    assert!(!h.app.preferences.zoom_reverse);
    assert!(h.app.prefs_window.open && h.app.prefs_window.filter.is_empty());
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some(""),
        "the row rename editor does not fill the independent name entry"
    );

    h.step(&json!({"choose_file":"Laptop.solidcraft-profile.json"})).unwrap();
    h.step(&json!({"click":{"handle":"prefs:profile:import"}})).unwrap();
    h.frames(6);
    assert_eq!(h.app.settings_profiles.names(), ["Travel", "Desk", "Laptop"]);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    assert!(!h.app.preferences.zoom_reverse, "import keeps the active settings");
    h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
    h.step(&json!({"text":"Next profile"})).unwrap();
    h.step(&json!({"click":{"handle":"prefs:profile:Travel:delete"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.names(), ["Desk", "Laptop"]);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    h.step(&json!({"click":{"handle":"prefs:profile:Desk:delete"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.active, None);
    assert!(!h.app.preferences.zoom_reverse, "deleting never applies another row");
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some("Next profile"),
        "deleting either an inactive or active row retains the draft"
    );
}

#[test]
fn general_profile_row_rename_commits_or_cancels_without_closing_preferences() {
    for action in ["Enter", "Tab", "ok", "Escape", "cancel", "outside"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        run(&mut h, "settings.profile.save", json!({"name":"Laptop"}));
        run(&mut h, "settings.profile.save", json!({"name":"Desk"}));
        h.call("ui.prefs", json!({"language":"en", "open":"General"}));
        h.step(&json!({"click":{"handle":"prefs:profile:Laptop:rename"}})).unwrap();
        h.frames(3);
        let popup = crate::scenario::rect_of("prefs:profile:rename_popup").unwrap();
        let editor = crate::scenario::rect_of("rename:name").unwrap();
        assert!(popup.contains_rect(editor), "the editor is outside the page's scroll clip");
        h.step(&json!({"text":"  Travel  "})).unwrap();
        match action {
            "ok" | "cancel" => h.step(&json!({"click":{"handle":format!("prefs:profile:rename:{action}")}})).unwrap(),
            "outside" => {
                let page = crate::scenario::rect_of("prefs:page_body").unwrap();
                let at = egui::pos2(page.right() - 4.0, page.top() + 4.0);
                assert!(!popup.contains(at));
                h.step(&json!({"click":[at.x, at.y]})).unwrap();
            }
            _ => h.step(&json!({"key":action})).unwrap(),
        }
        h.frames(3);
        let committed = matches!(action, "Enter" | "Tab" | "ok");
        assert_eq!(h.app.settings_profiles.names(), [if committed { "Travel" } else { "Laptop" }, "Desk"], "{action}");
        assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
        assert!(h.app.prefs_window.open && h.app.prefs_window.filter.is_empty(), "{action}");
        assert!(crate::scenario::rect_of("prefs:profile:rename_popup").is_none());
    }
}

#[test]
fn general_profile_row_rename_keeps_validation_and_unchanged_name_noop() {
    let mut h = Harness::new();
    h.app.home.open = false;
    run(&mut h, "settings.profile.save", json!({"name":"Laptop"}));
    run(&mut h, "settings.profile.save", json!({"name":"Desk"}));
    h.call("ui.prefs", json!({"language":"en", "open":"General"}));
    for to in ["Laptop".to_string(), "Desk".to_string(), "x".repeat(65)] {
        h.step(&json!({"click":{"handle":"prefs:profile:Laptop:rename"}})).unwrap();
        h.frames(3);
        if to != "Laptop" {
            h.step(&json!({"text":to})).unwrap();
        }
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.settings_profiles.names(), ["Laptop", "Desk"]);
        assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
        assert!(h.app.prefs_window.open);
        if to == "Desk" {
            assert!(h.widget("another settings profile is called `Desk`").is_some(), "the existing act outcome surfaces duplicates");
        }
    }
}

#[test]
fn profile_keys_preserve_the_literal_pre_split_baseline() {
    let expected = [
        "browser_bodies_folders",
        "browser_collapsed",
        "browser_eye_left",
        "browser_float",
        "browser_share",
        "browser_show",
        "browser_width",
        "center_arc_follows_cursor",
        "color_vision",
        "dialog_buttons",
        "dialog_place",
        "direct_interaction",
        "free_orbit",
        "free_orbit_upright",
        "hidden_toolbar_tabs",
        "hide_constraint_glyphs_zoomed_out",
        "history_in_browser",
        "history_play_button",
        "history_sketches",
        "home_fit_size",
        "invert_navigator_arrows",
        "keep_work_on_tab_switch",
        "language",
        "layout",
        "line_drag_draws_one",
        "line_hover_tangent_arc",
        "logo_accent",
        "look_at_anim",
        "look_at_speed",
        "manipulator_free_alt",
        "middle_double_click",
        "mouse_gestures",
        "nav",
        "nav_bridge_enabled",
        "nav_bridge_port",
        "navigation_bar",
        "navigator_fit",
        "navigator_style",
        "orbit_around",
        "orbit_reverse",
        "origin_plane_names",
        "pan_reverse_x",
        "pan_reverse_y",
        "persistent_ribbon_pressed",
        "prefer_free_sketch_points",
        "project_options_dialog",
        "quick_measure",
        "radial_context_menu",
        "ribbon_click_ends_tool",
        "ribbon_icon_grid",
        "ribbon_labels",
        "ribbon_menus_skip_pinned",
        "ribbon_reveal",
        "right_drag_orbits",
        "select_other_hidden",
        "selection_shift_add_only",
        "shortcut_cycles_modes",
        "sketch_align_points",
        "sketch_auto_dimensions",
        "sketch_auto_locate",
        "sketch_conflict_colour",
        "sketch_construction_lines",
        "sketch_exit_view",
        "sketch_face_activates_part",
        "sketch_open_look_at",
        "sketch_recent_snap",
        "sketch_state_icons",
        "sketch_tab_always",
        "sketch_tracking_guides",
        "sketch_tracking_points",
        "smooth_wheel_zoom",
        "status_bar",
        "suppress_sketch_snap",
        "theme",
        "theme_follow",
        "theme_options",
        "timeline_collapsed",
        "timeline_left",
        "timeline_left_browser_collapsed",
        "timeline_left_width",
        "timeline_origin_bodies",
        "timeline_under_browser",
        "toast_place",
        "trackpad_pan",
        "trackpad_zoom_modifier",
        "ui_font",
        "ui_scale",
        "view_confirm_corner",
        "viewport_analysis_status",
        "viewport_background",
        "viewport_section_status",
        "viewport_status_details",
        "viewport_status_summary",
        "viewport_theme",
        "wheel_zoom_speed",
        "zoom_reverse",
        "zoom_stop_at_model",
        "zoom_to_cursor",
    ];
    let mut actual: Vec<_> = keys().collect();
    actual.sort_unstable();
    assert_eq!(expected.len(), 98);
    assert_eq!(actual, expected);
}

#[test]
fn moved_profile_settings_survive_saved_roundtrip_and_apply() {
    let mut h = Harness::new();
    let defaults = crate::prefs::Prefs::default();
    assert!(!defaults.keep_work_on_tab_switch && !defaults.line_hover_tangent_arc);
    for (key, page) in [("keep_work_on_tab_switch", crate::prefs::PAGE_LAYOUT), ("line_hover_tangent_arc", crate::prefs::PAGE_SKETCH)] {
        let homes: Vec<_> = crate::prefs::FIELD_PAGES.iter().filter_map(|(field, page)| (*field == key).then_some(*page)).collect();
        assert_eq!(homes, [Some(page)], "{key} keeps its new page home");
    }
    h.app.preferences.nav_bridge_enabled = true;
    h.app.preferences.nav_bridge_port = 9876;
    h.app.preferences.origin_plane_names = crate::prefs::OriginPlaneNames::TopFrontRight;
    h.app.preferences.direct_interaction = true;
    h.app.preferences.quick_measure = true;
    h.app.preferences.viewport_analysis_status = false;
    h.app.preferences.keep_work_on_tab_switch = true;
    h.app.preferences.line_hover_tangent_arc = true;
    h.app.preferences.navigator_fit = true;
    h.app.preferences.invert_navigator_arrows = true;
    h.app.preferences.home_fit_size = 0.5;
    h.app.preferences.ribbon_click_ends_tool = false;
    h.app.preferences.sketch_face_activates_part = true;
    run(&mut h, "settings.profile.save", json!({"name":"Moved"}));
    let saved = h.app.settings_profiles.prefs();
    let doc = h.app.settings_profiles.list[0].clone();
    assert_eq!(doc["preferences"]["keep_work_on_tab_switch"], true);
    assert_eq!(doc["preferences"]["line_hover_tangent_arc"], true);
    for (key, value) in [
        ("home_fit_size", json!(0.5)),
        ("invert_navigator_arrows", json!(true)),
        ("navigator_fit", json!(true)),
        ("ribbon_click_ends_tool", json!(false)),
        ("sketch_face_activates_part", json!(true)),
    ] {
        assert_eq!(doc["preferences"][key], value, "capture retains {key}");
    }
    assert!(doc["preferences"].get("dark_parts_shading").is_none());
    assert_eq!(sanitize(&doc).unwrap(), doc);
    h.app.settings_profiles.load(&serde_json::from_str::<Value>(&serde_json::to_string(&saved).unwrap()).unwrap());
    assert_eq!(h.app.settings_profiles.list[0], doc);
    let path = h.dir.join("moved.solidcraft-profile.json");
    let exported = run(&mut h, "settings.profile.export", json!({"name":"Moved", "path":path}));
    assert_eq!(exported["profile"], doc);
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written, doc);
    run(&mut h, "settings.profile.delete", json!({"name":"Moved"}));
    assert_eq!(run(&mut h, "settings.profile.import", json!({"path":path})), json!({"name":"Moved"}));
    assert_eq!(h.app.settings_profiles.list[0], doc, "file import retains the complete profile");
    h.app.preferences.nav_bridge_enabled = false;
    h.app.preferences.nav_bridge_port = 8765;
    h.app.preferences.origin_plane_names = crate::prefs::OriginPlaneNames::Xyz;
    h.app.preferences.direct_interaction = false;
    h.app.preferences.quick_measure = false;
    h.app.preferences.viewport_analysis_status = true;
    h.app.preferences.keep_work_on_tab_switch = false;
    h.app.preferences.line_hover_tangent_arc = false;
    h.app.preferences.navigator_fit = defaults.navigator_fit;
    h.app.preferences.invert_navigator_arrows = defaults.invert_navigator_arrows;
    h.app.preferences.home_fit_size = defaults.home_fit_size;
    h.app.preferences.ribbon_click_ends_tool = defaults.ribbon_click_ends_tool;
    h.app.preferences.sketch_face_activates_part = defaults.sketch_face_activates_part;
    let out = run(&mut h, "settings.profile.apply", json!({"name":"Moved"}));
    assert_eq!(out["skipped"], json!([]));
    assert!(h.app.preferences.keep_work_on_tab_switch && h.app.preferences.line_hover_tangent_arc);
    assert_eq!(capture(&h.app, "Moved"), doc);
    for value in [0.05, 1.1] {
        let mut invalid = doc.clone();
        invalid["preferences"]["home_fit_size"] = json!(value);
        let imported = run(&mut h, "settings.profile.import", json!({"profile":invalid, "name":"Invalid fit"}));
        let out = run(&mut h, "settings.profile.apply", json!({"name":imported["name"]}));
        assert_eq!(out["skipped"], json!(["home_fit_size"]));
        assert_eq!(h.app.preferences.home_fit_size, 0.5, "an out-of-range profile value is refused");
    }
}

#[test]
fn profile_sidebar_switches_saves_and_marks_edits_with_existing_commands() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.language = "en".into();
    run(&mut h, "settings.profile.save", json!({"name":"Laptop"}));
    h.app.preferences.zoom_reverse = true;
    run(&mut h, "settings.profile.save", json!({"name":"Desk"}));
    h.call("ui.prefs", json!({"open":"Tools"}));
    h.frames(3);
    assert!(crate::scenario::rect_of("prefs:profile:edited").is_none());
    let card = crate::scenario::rect_of("prefs:profile").unwrap();
    h.step(&json!({"click":[card.left() + 3.0, card.center().y]})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("dialog:list:Laptop").is_some(), "the card's padding opens its real menu");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.prefs_window.open);
    assert!(crate::scenario::rect_of("dialog:list:Laptop").is_none(), "Escape dismisses the sidebar menu itself");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("dialog:list:Laptop").is_some(), "the same trigger opens from the keyboard");
    h.step(&json!({"click":{"handle":"dialog:list:Laptop"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Laptop"));
    assert!(!h.app.preferences.zoom_reverse);
    h.step(&json!({"widget":"General"})).unwrap();
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some(""),
        "sidebar Apply does not fill the independent name entry"
    );
    h.step(&json!({"widget":"Tools"})).unwrap();
    h.app.preferences.direct_interaction = true;
    h.frames(3);
    assert!(crate::scenario::rect_of("prefs:profile:edited").is_some());
    h.step(&json!({"click":{"handle":"prefs:profile"}})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"Save"})).unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of("prefs:profile:edited").is_none());
    assert!(h.app.prefs_window.open, "saving a profile keeps Preferences open");
    let saved = run(&mut h, "settings.profile.export", json!({"name":"Laptop"}));
    assert_eq!(saved["profile"]["preferences"]["direct_interaction"], true);
    h.step(&json!({"click":{"handle":"prefs:profile"}})).unwrap();
    h.frames(2);
    let profiles_label = h.app.tr("prefs.profiles").to_string();
    h.step(&json!({"widget":profiles_label})).unwrap();
    h.frames(3);
    assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_GENERAL);
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some(""),
        "sidebar Save does not fill the independent name entry either"
    );
    h.step(&json!({"click":{"handle":"dialog:Profile name"}})).unwrap();
    h.step(&json!({"text":"Next profile"})).unwrap();
    h.step(&json!({"click":{"handle":"prefs:profile"}})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:list:Desk"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Desk"));
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some("Next profile"),
        "selecting another sidebar profile preserves typed input"
    );
    h.step(&json!({"click":{"handle":"prefs:profile"}})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"Save"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.names(), ["Laptop", "Desk"]);
    assert_eq!(
        h.output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Profile name"))
            .find_map(|(_, node)| node.value()),
        Some("Next profile"),
        "sidebar Save preserves typed input and saves the selected profile"
    );
}
