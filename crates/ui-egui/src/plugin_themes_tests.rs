//! Plugin themes (`themes@0.1`): the offer, the fallback, the card hint and the install review.

use crate::scenario::Harness;

const GLASS: &str = r##"{"format":"solidcraft-theme/1","name":"Night","base":"dark","colors":{"accent":"#44d62c"}}"##;

/// A plugin `tints` with one theme, in a plugins folder the app reads.
fn install(h: &mut Harness) -> std::path::PathBuf {
    let dir = h.dir.join("theme-plugins");
    let folder = dir.join("tints");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("plugin.json"),
        r#"{"id":"tints","name":"Tints","version":"1","capabilities":["themes@0.1"],"themes":[{"id":"night","source":"night.json"}]}"#,
    )
    .unwrap();
    std::fs::write(folder.join("night.json"), GLASS).unwrap();
    h.app.session.plugin_state.plugins_dir = Some(dir.clone());
    dir
}

fn offered(h: &Harness) -> bool {
    crate::theme_files::find("plugin:tints/night", &h.app.preferences.custom_themes).is_some()
}

/// The offer lists an installed plugin's theme, which applies its colours while chosen; turning
/// the plugin off, or removing it, turns the default theme on with one status line, once.
#[test]
fn a_plugin_theme_is_listed_applies_and_falls_back_once_when_its_plugin_goes() {
    let mut h = Harness::new();
    let dir = install(&mut h);
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(offered(&h), "status {:?}", h.app.status);
    assert!(crate::theme_files::plugin_theme_choices().contains(&("plugin:tints/night".into(), "Night".into())));
    h.app.preferences.theme = "plugin:tints/night".into();
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert_eq!(h.app.preferences.theme, "plugin:tints/night", "an offered theme stays chosen");
    let accent = crate::theme_files::find("plugin:tints/night", &h.app.preferences.custom_themes).unwrap();
    assert_eq!(crate::theme_files::resolve(accent).accent, crate::theme_files::parse_color("#44d62c").unwrap());

    h.app.preferences.disabled_plugins.insert("tints".into());
    h.app.status = None;
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(!offered(&h));
    assert_eq!(h.app.preferences.theme, "dark");
    assert!(h.app.ui.dark);
    let (message, _, error) = h.app.status.clone().expect("one status line");
    assert!(error && message.contains("night"), "{message}");
    h.app.status = None;
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(h.app.status.is_none(), "not retried or repeated while nothing changes");

    h.app.preferences.disabled_plugins.remove("tints");
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(offered(&h), "back on the offer once its plugin is turned on");

    std::fs::remove_dir_all(dir.join("tints")).unwrap();
    h.app.preferences.theme = "plugin:tints/night".into();
    h.app.plugin_themes.signature = None;
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(!offered(&h), "a removed plugin's theme is gone");
    assert_eq!(h.app.preferences.theme, "dark");
}

/// A theme file that is no longer a theme leaves the offer and the picker's list alone.
#[test]
fn a_plugin_theme_whose_file_is_broken_is_not_offered() {
    let mut h = Harness::new();
    let dir = install(&mut h);
    std::fs::write(dir.join("tints").join("night.json"), "{").unwrap();
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(!offered(&h));
    assert!(crate::theme_files::plugin_theme_choices().is_empty());
}

/// A theme source that names a folder is refused as not a file, before any size check.
#[test]
fn a_plugin_theme_whose_source_is_a_folder_is_not_offered() {
    let mut h = Harness::new();
    let dir = install(&mut h);
    let source = dir.join("tints").join("night.json");
    std::fs::remove_file(&source).unwrap();
    std::fs::create_dir(&source).unwrap();
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(!offered(&h));
    let (message, _, error) = h.app.status.clone().expect("one status line");
    assert!(error && message.contains("not a file"), "{message}");
}

/// The install review names a package's themes by their ids.
#[test]
fn the_install_review_lists_a_plugins_themes() {
    crate::i18n::install();
    let lines = crate::plugin_install::permission_lines(&serde_json::json!({
        "id": "tints", "themes": [{"id": "night", "source": "night.json"}]
    }));
    assert!(lines.iter().any(|line| line.contains("night")), "{lines:?}");
}

/// A plugin that does not ask for themes@0.1 has no themes, adds nothing to the picker and its install
/// review has the lines it always had.
#[test]
fn a_plugin_without_themes_changes_nothing_in_the_app() {
    crate::i18n::install();
    let mut h = Harness::new();
    let dir = h.dir.join("legacy-plugins");
    let folder = dir.join("gears");
    std::fs::create_dir_all(&folder).unwrap();
    let legacy = r#"{"id":"gears","name":"Gears","version":"1","capabilities":["commands@0.1"],"runtime":{"module":"plugin.wasm","entries":["make"],"commands":["solid.box"]}}"#;
    std::fs::write(folder.join("plugin.json"), legacy).unwrap();
    std::fs::write(folder.join("plugin.wasm"), b"\0asm\x0d\0\x01\0").unwrap();
    h.app.session.plugin_state.plugins_dir = Some(dir.clone());
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    let (plugins, skipped) = crate::plugins::inventory(Some(&dir), std::iter::empty());
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(plugins.len(), 1);
    assert!(plugins[0].themes.is_empty());
    assert_eq!(plugins[0].entries.len(), 1);
    assert!(crate::theme_files::plugin_theme_choices().is_empty());
    assert_eq!(h.app.preferences.theme, Harness::new().app.preferences.theme);

    let lines = crate::plugin_install::permission_lines(&serde_json::json!({
        "id": "gears", "permissions": {"command_groups": ["file"], "clock": true}, "commands": ["solid.box"]
    }));
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines.iter().all(|line| !line.to_lowercase().contains("theme")), "{lines:?}");
}

#[test]
fn the_card_names_a_plugins_themes() {
    crate::i18n::install();
    let line = crate::plugin_themes::hint(&["Night", "Dawn"]);
    assert!(line.contains("Night") && line.contains("Dawn"), "{line}");
}

/// A plugin theme chosen for the day or night slot goes back to the built-in default when its plugin is
/// removed, and stays while the plugin is only turned off.
#[test]
fn removing_a_plugin_resets_its_day_and_night_themes() {
    let mut h = Harness::new();
    let dir = install(&mut h);
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    h.app.preferences.theme_day = "plugin:tints/night".into();
    h.app.preferences.theme_night = "plugin:tints/night".into();

    h.app.preferences.disabled_plugins.insert("tints".into());
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert_eq!(h.app.preferences.theme_day, "plugin:tints/night", "turned off is not removed");
    h.app.preferences.disabled_plugins.remove("tints");

    std::fs::remove_dir_all(dir.join("tints")).unwrap();
    h.app.plugin_themes.signature = None;
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert_eq!((h.app.preferences.theme_day.as_str(), h.app.preferences.theme_night.as_str()), ("light", "dark"));
}

/// A slot that holds a built-in theme is left alone when a plugin goes.
#[test]
fn removing_a_plugin_leaves_other_day_and_night_themes() {
    let mut h = Harness::new();
    let dir = install(&mut h);
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    h.app.preferences.theme_day = "akari".into();
    h.app.preferences.theme_night = "plugin:tints/night".into();
    std::fs::remove_dir_all(dir.join("tints")).unwrap();
    h.app.plugin_themes.signature = None;
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert_eq!((h.app.preferences.theme_day.as_str(), h.app.preferences.theme_night.as_str()), ("akari", "dark"));
}
