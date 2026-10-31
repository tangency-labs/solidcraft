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
        "msaa",
        "window_system",
        "default_units",
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
fn typing_a_profile_name_keeps_focus_out_of_preferences_search() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.prefs", json!({"open": "General"}));
    h.frames(3);
    h.step(&json!({"text": "profiles"})).expect("find Settings profiles");
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Name"}})).expect("focus the profile name");
    h.frames(2);
    let name_focus = h.ctx.memory(|memory| memory.focused()).expect("the name field has focus");
    for text in ["Lap", "top"] {
        h.step(&json!({"text": text})).expect("type a profile name");
        h.frames(2);
        assert_eq!(h.app.prefs_window.filter, "profiles", "profile-name text went to Preferences search");
        assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(name_focus), "the name field keeps focus");
    }
    assert!(h.widget("Laptop").is_some(), "the name field contains the typed name");
    h.step(&json!({"widget": "Save"})).expect("save the profile");
    h.frames(2);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Laptop"));
    assert_eq!(h.app.settings_profiles.names(), ["Laptop"]);
    assert_eq!(h.app.prefs_window.filter, "profiles");
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
    h.step(&json!({"click": {"handle": "dialog:Active profile"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:list:Large monitor"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Large monitor"));
    assert!(h.app.preferences.zoom_reverse);
}
