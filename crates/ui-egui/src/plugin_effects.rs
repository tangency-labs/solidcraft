//! Effects a plugin offers the host to draw (capability `effects@0.1`, `docs/plugin-effects.md`).
//!
//! A switch on its plugin's card in Preferences › Plugins chooses one, kept as `plugin-id/effect-id` in
//! the `ui_effect` preference (`""`: none, the default). The host draws the effect only while that
//! plugin is turned on and its manifest still declares an effect of a kind the host draws. Today the
//! only kind is `hue-wave`, the RGB wave over the ribbon, its button blocks and the panels.

use crate::SolveApp;
use crate::icons::{Wave, WaveSet};

/// The effect kind the host draws as the RGB wave.
const HUE_WAVE: &str = "hue-wave";

/// The wave as the ribbon and panels draw it this frame. Off unless the chosen effect's plugin is on.
pub(crate) fn wave_set(app: &mut SolveApp) -> WaveSet {
    let on = chosen_and_installed(app);
    let p = &app.preferences;
    WaveSet {
        on,
        icons: p.ui_effect_icons,
        blocks: p.ui_effect_buttons,
        panels: p.ui_effect_panels,
        wave: Wave { length: p.ui_effect_length.max(1.0), speed: p.ui_effect_speed },
    }
}

/// Whether the chosen effect is a hue wave of an installed plugin that is turned on. The
/// installed check reads the plugin's manifest, so its answer is kept for the chosen key.
fn chosen_and_installed(app: &mut SolveApp) -> bool {
    let key = app.preferences.ui_effect.clone();
    if key.is_empty() {
        return false;
    }
    let Some((plugin, _)) = key.split_once('/') else { return false };
    if !crate::plugins::enabled(app, plugin) {
        return false;
    }
    if let Some((cached, installed)) = &app.plugin_effects.checked
        && *cached == key
    {
        return *installed;
    }
    let installed = installed(app, &key);
    app.plugin_effects.checked = Some((key, installed));
    installed
}

#[cfg(not(target_arch = "wasm32"))]
fn installed(app: &SolveApp, key: &str) -> bool {
    let Some((plugin, effect)) = key.split_once('/') else { return false };
    let mut paths: Vec<String> = app.session.plugin_state.packages.get(plugin).cloned().into_iter().collect();
    if let Some(dir) = app.session.plugin_state.plugins_dir() {
        paths.push(dir.join(plugin).to_string_lossy().into_owned());
    }
    paths.into_iter().any(|path| {
        solidcraft_engine::plugin::read_package(&path)
            .is_ok_and(|p| p.manifest.id == plugin && p.manifest.effects.iter().any(|e| e.id == effect && e.kind == HUE_WAVE))
    })
}

#[cfg(target_arch = "wasm32")]
fn installed(_app: &SolveApp, _key: &str) -> bool {
    false
}

/// A plugin was removed or replaced: the next frame reads its manifest again.
pub(crate) fn forget(app: &mut SolveApp) {
    app.plugin_effects.checked = None;
}

/// What the state keeps between frames: the chosen key and whether its plugin offers it.
#[derive(Default)]
pub(crate) struct State {
    checked: Option<(String, bool)>,
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
}
