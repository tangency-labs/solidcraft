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
