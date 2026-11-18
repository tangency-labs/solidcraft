//! Effects a plugin offers the host to draw (capability `effects@0.1`, `docs/plugin-effects.md`).
//!
//! A switch on its plugin's card in Preferences › Plugins chooses one, kept as `plugin-id/effect-id` in
//! the `ui_effect` preference (`""`: none, the default). The host draws the effect only while that
//! plugin is turned on and its manifest still declares an effect of a kind the host draws: `hue-wave`,
//! the RGB wave over the ribbon, its button blocks and the panels, or `galaxy`, a sky behind the model.

use std::path::PathBuf;

use crate::SolveApp;
use crate::icons::{Wave, WaveSet};

/// The effect kind the host draws as the RGB wave.
const HUE_WAVE: &str = "hue-wave";
/// The effect kind the host draws as a galaxy behind the model.
const GALAXY: &str = "galaxy";

/// What the sky behind the model is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkyKind {
    Galaxy,
}

/// The sky a chosen effect asks for: its kind, and the picture from its plugin's package if the
/// effect names one (else the host draws its own).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sky {
    pub kind: SkyKind,
    pub picture: Option<PathBuf>,
}

/// The wave as the ribbon and panels draw it this frame. Off unless the chosen effect's plugin is on.
pub(crate) fn wave_set(app: &mut SolveApp) -> WaveSet {
    let on = chosen(app).is_some_and(|chosen| chosen.kind == HUE_WAVE);
    let p = &app.preferences;
    WaveSet {
        on,
        icons: p.ui_effect_icons,
        blocks: p.ui_effect_buttons,
        panels: p.ui_effect_panels,
        wave: Wave { length: p.ui_effect_length.max(1.0), speed: p.ui_effect_speed },
    }
}

/// The sky behind the model, if the chosen effect is one of a plugin that is on.
pub(crate) fn sky(app: &mut SolveApp) -> Option<Sky> {
    let chosen = chosen(app)?;
    let kind = match chosen.kind.as_str() {
        GALAXY => SkyKind::Galaxy,
        _ => return None,
    };
    Some(Sky { kind, picture: chosen.picture })
}

/// The chosen effect, as its plugin's manifest declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chosen {
    pub kind: String,
    pub picture: Option<PathBuf>,
}

/// The chosen effect when it is of an installed plugin that is turned on and of a kind the host
/// draws. The installed check reads the plugin's manifest, so its answer is kept for the chosen key.
pub(crate) fn chosen(app: &mut SolveApp) -> Option<Chosen> {
    let key = app.preferences.ui_effect.clone();
    if key.is_empty() {
        return None;
    }
    let (plugin, _) = key.split_once('/')?;
    if !crate::plugins::enabled(app, plugin) {
        return None;
    }
    if let Some((cached, found)) = &app.plugin_effects.checked
        && *cached == key
    {
        return found.clone();
    }
    let found = installed(app, &key);
    app.plugin_effects.checked = Some((key, found.clone()));
    found
}

#[cfg(not(target_arch = "wasm32"))]
fn installed(app: &SolveApp, key: &str) -> Option<Chosen> {
    let (plugin, effect) = key.split_once('/')?;
    let mut paths: Vec<String> = app.session.plugin_state.packages.get(plugin).cloned().into_iter().collect();
    if let Some(dir) = app.session.plugin_state.plugins_dir() {
        paths.push(dir.join(plugin).to_string_lossy().into_owned());
    }
    paths.into_iter().find_map(|path| {
        let package = solidcraft_engine::plugin::read_package(&path).ok().filter(|p| p.manifest.id == plugin)?;
        let declared = package.manifest.effects.iter().find(|e| e.id == effect && matches!(e.kind.as_str(), HUE_WAVE | GALAXY))?;
        let picture = declared
            .picture
            .as_deref()
            .filter(|picture| solidcraft_engine::plugin::picture_path_ok(picture))
            .map(|picture| PathBuf::from(&path).join(picture));
        Some(Chosen { kind: declared.kind.clone(), picture })
    })
}

#[cfg(target_arch = "wasm32")]
fn installed(_app: &SolveApp, _key: &str) -> Option<Chosen> {
    None
}

/// A plugin was removed or replaced: the next frame reads its manifest again.
pub(crate) fn forget(app: &mut SolveApp) {
    app.plugin_effects.checked = None;
}

/// What the state keeps between frames: the chosen key and what its plugin offers under it.
#[derive(Default)]
pub(crate) struct State {
    checked: Option<(String, Option<Chosen>)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn put(dir: &Path, id: &str, manifest: &str, files: &[(&str, &[u8])]) {
        let folder = dir.join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("plugin.json"), manifest).unwrap();
        for (name, bytes) in files {
            std::fs::write(folder.join(name), bytes).unwrap();
        }
    }

    fn harness() -> (crate::scenario::Harness, PathBuf) {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("sky-plugins");
        h.app.session.plugin_state.plugins_dir = Some(dir.clone());
        (h, dir)
    }

    const GALAXY: &str = r#"{"id":"sky","name":"Sky","version":"1","description":"","capabilities":["effects@0.1"],"effects":[{"id":"stars","name":"Stars","kind":"galaxy","picture":"stars.jpg"}]}"#;
    const RAIN: &str = r#"{"id":"wet","name":"Wet","version":"1","description":"","capabilities":["effects@0.1"],"effects":[{"id":"rain","name":"Rain","kind":"rain","picture":"night.png"}]}"#;
    const BARE_GALAXY: &str = r#"{"id":"sky","name":"Sky","version":"1","description":"","capabilities":["effects@0.1"],"effects":[{"id":"stars","name":"Stars","kind":"galaxy"}]}"#;
    const WAVE: &str = r#"{"id":"waves","name":"Waves","version":"1","description":"","capabilities":["effects@0.1"],"effects":[{"id":"rgb-wave","name":"RGB wave","kind":"hue-wave"}]}"#;
    const NONE: &str = r#"{"id":"old","name":"Old","version":"1","description":"","capabilities":["commands@0.1"]}"#;

    #[test]
    fn a_chosen_galaxy_is_the_sky_with_the_picture_from_its_own_package() {
        let (mut h, dir) = harness();
        put(&dir, "sky", GALAXY, &[("stars.jpg", b"x")]);
        assert!(sky(&mut h.app).is_none(), "nothing chosen: no sky");
        h.app.preferences.ui_effect = "sky/stars".into();
        let chosen = sky(&mut h.app).expect("the galaxy is chosen");
        assert_eq!(chosen.kind, SkyKind::Galaxy);
        assert_eq!(chosen.picture, Some(dir.join("sky").join("stars.jpg")));
        assert!(!wave_set(&mut h.app).on, "a galaxy is not the wave");
        h.app.preferences.disabled_plugins.insert("sky".into());
        assert!(sky(&mut h.app).is_none(), "its plugin turned off: no sky");
    }

    #[test]
    fn a_galaxy_with_no_picture_is_the_sky_with_none() {
        let (mut h, dir) = harness();
        put(&dir, "sky", BARE_GALAXY, &[]);
        h.app.preferences.ui_effect = "sky/stars".into();
        let chosen = sky(&mut h.app).expect("the galaxy is chosen");
        assert_eq!((chosen.kind, chosen.picture), (SkyKind::Galaxy, None));
    }

    #[test]
    fn a_manifest_whose_picture_leaves_the_package_is_not_read() {
        let (mut h, dir) = harness();
        put(&dir, "sky", &GALAXY.replace("stars.jpg", "../stars.jpg"), &[]);
        h.app.preferences.ui_effect = "sky/stars".into();
        assert_eq!(sky(&mut h.app), None);
    }

    /// A plugin with `effects@0.1` and a hue wave only, and one with no effects, have no sky.
    #[test]
    fn a_legacy_plugin_has_no_sky_and_draws_as_before() {
        let (mut h, dir) = harness();
        put(&dir, "waves", WAVE, &[]);
        put(&dir, "old", NONE, &[]);
        let ctx = crate::fonts::test_context();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        for key in ["", "waves/rgb-wave", "old/anything"] {
            h.app.preferences.ui_effect = key.into();
            h.app.preferences.ui_effect_icons = true;
            assert!(sky(&mut h.app).is_none(), "{key:?}: no sky");
            assert_eq!(wave_set(&mut h.app).on, key == "waves/rgb-wave", "{key:?}: the wave as before");
            let painter = ctx.layer_painter(egui::LayerId::background());
            assert!(!crate::gpu::paint_sky(&mut h.app, &painter, rect), "{key:?}: no sky layer drawn");
            assert!(!crate::gpu::sky_shown(&ctx), "{key:?}: glass sees no sky");
        }
    }

    #[test]
    fn a_chosen_rain_is_the_sky_with_its_picture_or_none() {
        let (mut h, dir) = harness();
        put(&dir, "wet", RAIN, &[("night.png", b"x")]);
        h.app.preferences.ui_effect = "wet/rain".into();
        let chosen = sky(&mut h.app).expect("the rain is chosen");
        assert_eq!((chosen.kind, chosen.picture), (SkyKind::Rain, Some(dir.join("wet").join("night.png"))));
        put(&dir, "wet", &RAIN.replace(r#","picture":"night.png""#, ""), &[]);
        forget(&mut h.app);
        assert_eq!(sky(&mut h.app).map(|s| (s.kind, s.picture)), Some((SkyKind::Rain, None)));
    }

    #[test]
    fn the_rain_amount_defaults_to_the_hosts_and_stays_between_none_and_full() {
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.app.preferences.rain_amount, 0.6);
        crate::prefs::control(&mut h.app, &serde_json::json!({"rain_amount": 0.25})).unwrap();
        assert_eq!(h.app.preferences.rain_amount, 0.25);
        assert!(crate::prefs::control(&mut h.app, &serde_json::json!({"rain_amount": 1.5})).is_err());
        assert_eq!(h.app.preferences.rain_amount, 0.25, "a refused value changes nothing");
    }

    /// A hue-wave-only plugin and one with no effects are not made rainy by the rain setting.
    #[test]
    fn a_legacy_plugin_has_no_rain_whatever_the_rain_amount() {
        let (mut h, dir) = harness();
        put(&dir, "waves", WAVE, &[]);
        put(&dir, "old", NONE, &[]);
        h.app.preferences.rain_amount = 1.0;
        let ctx = crate::fonts::test_context();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        for key in ["", "waves/rgb-wave", "old/anything"] {
            h.app.preferences.ui_effect = key.into();
            assert!(sky(&mut h.app).is_none(), "{key:?}: no sky");
            let painter = ctx.layer_painter(egui::LayerId::background());
            assert!(!crate::gpu::paint_sky(&mut h.app, &painter, rect), "{key:?}: no sky layer drawn");
            assert!(!crate::gpu::sky_shown(&ctx), "{key:?}: glass sees no sky");
        }
    }
}
