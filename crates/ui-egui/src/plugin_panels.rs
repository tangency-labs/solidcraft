//! Panel shaders from plugins (`shaders@0.2`, kind `panel`, `docs/plugin-panels.md`).
//!
//! The plugin's WGSL is checked off the window thread and handed to the panel blur
//! (`ui_kit::blur`), which then draws every see-through panel's backdrop with it. Whatever fails
//! leaves the plain blur, with one line in the status bar.

use std::sync::{Arc, PoisonError};

use solidcraft_engine::plugin::shader::{Shader, ShaderKind};

use crate::SolveApp;
use crate::plugin_shaders::Pending;

const UNREADABLE: &str = "prefs.panel_shader_failed";

struct Active {
    key: String,
    shader: Shader,
}

#[derive(Default)]
pub(crate) struct State {
    render: Option<egui_wgpu::RenderState>,
    loading: Option<(String, Pending)>,
    active: Option<Active>,
    /// The key that failed, so it is not tried every frame; another choice clears it.
    failed: Option<String>,
    /// `failed` only because its plugin is off: turning it on again clears it.
    off: bool,
}

impl State {
    /// The window's GPU. A new device starts over (the blur drops its hook on install too).
    pub(crate) fn install(&mut self, rs: &egui_wgpu::RenderState) {
        *self = Self { render: Some(rs.clone()), ..Self::default() };
    }
}

/// Before the window's frame begins: loads, swaps or drops the chosen panel shader and writes
/// this frame's uniform. Nothing chosen: the blur has no hook and panels draw as without plugins.
pub(crate) fn prepare(app: &mut SolveApp, ctx: &egui::Context) {
    if ctx.viewport_id() != egui::ViewportId::ROOT || app.plugin_panels.render.is_none() {
        return;
    }
    let key = app.preferences.panel_shader.clone();
    if !allowed(app, &key) {
        return;
    }
    let state = &mut app.plugin_panels;
    if state.failed.as_ref().is_some_and(|failed| *failed != key) {
        state.failed = None;
    }
    if state.active.as_ref().is_some_and(|a| a.key != key) || (key.is_empty() && app.blur.panel_hook_active()) {
        drop_hook(app);
    }
    let state = &mut app.plugin_panels;
    if key.is_empty() || state.failed.as_deref() == Some(key.as_str()) {
        return;
    }
    let Some(rs) = state.render.clone() else { return };
    if solidcraft_viewport_egui::window_device(&rs).lost.is_lost() {
        // The shader may be why the driver gave up: off before the app restarts.
        app.plugin_panels.failed = Some(key.clone());
        app.preferences.panel_shader.clear();
        drop_hook(app);
        log::warn!("graphics device lost while the panel shader {key} was drawn; it is turned off");
        return;
    }
    if state.active.is_none() {
        if state.loading.as_ref().is_none_or(|(k, _)| *k != key) {
            let Some(folder) = crate::plugin_shaders::folder_of(app, &key) else {
                missing(app, &key);
                return;
            };
            let pending: Pending = Arc::default();
            let (slot, repaint, k, limit) = (pending.clone(), ctx.clone(), key.clone(), rs.device.limits().max_texture_dimension_2d);
            std::thread::spawn(move || {
                let result = crate::plugin_shaders::load(folder, k, Default::default(), limit);
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
                repaint.request_repaint();
            });
            app.plugin_panels.loading = Some((key, pending));
            return;
        }
        let done = state.loading.as_ref().and_then(|(_, p)| p.lock().unwrap_or_else(PoisonError::into_inner).take());
        let Some(done) = done else { return };
        state.loading = None;
        let built = done.and_then(|loaded| {
            if loaded.kind != ShaderKind::Panel {
                return Err(format!("{key} is not a panel shader"));
            }
            let hook = crate::blur::PanelHook { wgsl: loaded.wgsl, textures: loaded.textures, blur: loaded.shader.blur_radius() };
            app.blur.set_panel_hook(Some(hook)).map(|()| Active { key: key.clone(), shader: loaded.shader })
        });
        match built {
            Ok(active) => app.plugin_panels.active = Some(active),
            Err(error) => {
                app.plugin_panels.failed = Some(key.clone());
                let message = format!("{} {key}: {error}", solidcraft_ui_text::tr(UNREADABLE));
                log::warn!("{message}");
                app.set_status(message, true);
                return;
            }
        }
    }
    let Some(active) = &app.plugin_panels.active else { return };
    let time = if active.shader.animated {
        let focused = ctx.input(|i| i.focused);
        ctx.request_repaint_after(std::time::Duration::from_millis(if focused { 33 } else { 100 }));
        (ctx.input(|i| i.time) % 3600.0) as f32
    } else {
        0.0
    };
    let ppp = ctx.pixels_per_point();
    let window = ctx.content_rect().size() * ppp;
    let size = [window.x.round().max(1.0) as u32, window.y.round().max(1.0) as u32];
    let settings = crate::plugin_shaders::settings(&app.preferences.shader_settings, &active.key, &active.shader);
    let bytes = crate::plugin_shaders::host_uniforms(size, time, ppp, &app.cam, &solidcraft_theme::Tokens::get(), (0.0, [0.0, 0.0]), settings);
    app.blur.set_panel_hook_globals(&bytes);
}

fn drop_hook(app: &mut SolveApp) {
    app.plugin_panels.active = None;
    if let Err(error) = app.blur.set_panel_hook(None) {
        log::warn!("panel shader: {error}");
    }
}

/// A plugin turned off after its shader was set, or while it loads, drops it, saying so once.
fn allowed(app: &mut SolveApp, key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let plugin = key.split_once('/').map_or(key, |(id, _)| id);
    let enabled = crate::plugins::enabled(app, plugin);
    let state = &mut app.plugin_panels;
    if state.failed.as_deref() == Some(key) {
        if !(state.off && enabled) {
            return true;
        }
        state.failed = None;
    }
    state.off = false;
    if enabled {
        return true;
    }
    forget(app, plugin);
    missing(app, key);
    app.plugin_panels.off = true;
    false
}

/// Drops `plugin`'s panel shader, set or loading, so it is read again from its folder when next chosen.
pub(crate) fn forget(app: &mut SolveApp, plugin: &str) {
    let owns = |key: &str| key.split_once('/').is_some_and(|(id, _)| id == plugin);
    if app.plugin_panels.active.as_ref().is_some_and(|a| owns(&a.key)) {
        drop_hook(app);
    }
    let state = &mut app.plugin_panels;
    if state.loading.as_ref().is_some_and(|(key, _)| owns(key)) {
        state.loading = None;
    }
}

fn missing(app: &mut SolveApp, key: &str) {
    app.plugin_panels.failed = Some(key.to_owned());
    let message = format!("{} {key}: {}", solidcraft_ui_text::tr(UNREADABLE), solidcraft_ui_text::tr("prefs.background_shader_missing"));
    log::warn!("{message}");
    app.set_status(message, true);
}

#[cfg(test)]
#[path = "plugin_panels_tests.rs"]
mod tests;
