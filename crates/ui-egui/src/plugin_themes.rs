//! Colour themes from plugins (`themes@0.1`), offered as `plugin:<plugin-id>/<theme-id>` after the built-in ones.
//! Read again only when a plugin is installed, removed or switched, never per frame. When the
//! chosen theme is no longer offered, the default theme is turned on with one status line.

use solidcraft_ui_text::{format, tr};

use crate::SolveApp;

/// What the offer was read from: the switches and the installed packages. A change to either reads
/// the offer again.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) signature: Option<String>,
}

/// Reads the plugin themes again if the plugins changed since the last call.
pub(crate) fn sync(app: &mut SolveApp) {
    let dir = if cfg!(target_arch = "wasm32") { None } else { app.session.plugin_state.plugins_dir() };
    let signature = format!("{dir:?}|{:?}|{:?}", app.preferences.disabled_plugins, app.session.plugin_state.packages);
    if app.plugin_themes.signature.as_deref() == Some(signature.as_str()) {
        return;
    }
    app.plugin_themes.signature = Some(signature);
    let (plugins, _) = crate::plugins::inventory(dir.as_deref(), app.session.plugin_state.packages.values());
    let mut offers = Vec::new();
    let mut failure = None;
    for plugin in plugins.iter().filter(|plugin| crate::plugins::enabled(app, &plugin.id)) {
        for (key, _, source) in &plugin.themes {
            match read(&plugin.path, source) {
                Ok(text) => offers.push((key.clone(), text)),
                Err(error) => failure = failure.or(Some(format!("{key}: {error}"))),
            }
        }
    }
    let errors = crate::theme_files::set_plugin_themes(offers);
    if let Some(error) = failure.or_else(|| errors.into_iter().next()) {
        app.set_status(error, true);
    }
    if let Some(id) = app.preferences.theme.strip_prefix("plugin:").map(str::to_owned)
        && crate::theme_files::find(&app.preferences.theme, &app.preferences.custom_themes).is_none()
    {
        let theme = id.rsplit('/').next().unwrap_or_default().to_owned();
        app.preferences.theme = "dark".into();
        app.ui.dark = true;
        app.set_status(format("prefs.plugin_theme_unavailable", &[("name", theme.as_str())]), true);
    }
}

/// A theme file's text, read whole if it is within its size limit.
fn read(folder: &str, source: &str) -> Result<String, String> {
    let path = std::path::Path::new(folder).join(source);
    let size = std::fs::metadata(&path).map_err(|e| format!("{source}: {e}"))?.len();
    if size > solidcraft_engine::plugin::theme::MAX_THEME_BYTES as u64 {
        return Err(format!("{source}: larger than {} KiB", solidcraft_engine::plugin::theme::MAX_THEME_BYTES / 1024));
    }
    std::fs::read_to_string(&path).map_err(|e| format!("{source}: {e}"))
}

/// A plugin's theme name, read from its file, or its id when the file doesn't say.
pub(crate) fn name(folder: &str, source: &str, id: &str) -> String {
    read(folder, source)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("name").and_then(serde_json::Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned))
        .unwrap_or_else(|| id.to_owned())
}

/// The card's line naming a plugin's themes.
pub(crate) fn hint(names: &[&str]) -> String {
    tr("prefs.plugin_themes").replace("{names}", &names.join(", "))
}
