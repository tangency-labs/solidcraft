//! MACRO tab buttons (`macro.*`, docs/macros.md): recording is session state, so Record, Pause,
//! Resume and Stop run the commands directly. Play evals the loaded Rhai macro, asking for its
//! parameters first, or opens a `.rhai` file when nothing is loaded. Save writes the recording.

use egui::{Align2, FontId, Sense, vec2};
use serde_json::{Value, json};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit;

pub(crate) fn toolbar(app: &mut SolveApp, id: &str) {
    match id {
        "macro.record" => {
            if app.run(id, json!({})).is_ok() {
                app.set_status(app.tr("macro.recording_started").to_string(), false);
            }
        }
        "macro.pause" => {
            if app.run(id, json!({})).is_ok() {
                let steps = app.session.macros.steps();
                app.set_status(crate::i18n::format("macro.paused", &[("steps", &steps.to_string())]), false);
            }
        }
        "macro.resume" => {
            if app.run(id, json!({})).is_ok() {
                app.set_status(app.tr("macro.resumed").to_string(), false);
            }
        }
        "macro.stop" => {
            if let Ok(r) = app.run(id, json!({})) {
                let steps = r.get("steps").and_then(Value::as_u64).unwrap_or(0);
                if steps == 0 {
                    app.set_status(app.tr("macro.stopped_empty").to_string(), false);
                } else {
                    app.set_status(crate::i18n::format("macro.stopped", &[("steps", &steps.to_string())]), false);
                }
            }
        }
        "macro.play" => play_button(app),
        "macro.open" => {
            app.pick_open_for(&["rhai"], |app, path| {
                let Some(path) = path else { return };
                open_picked(app, path, false);
            });
        }
        "macro.save" => {
            let name = format!("{}-macro.rhai", design_stem(app));
            app.pick_save(&name, &["rhai"], |app, path| {
                let Some(path) = path else { return };
                match app.run("macro.save", json!({"path": path.clone()})) {
                    Ok(r) => {
                        let steps = r.get("steps").and_then(Value::as_u64).unwrap_or(0);
                        let saved = r.get("path").and_then(Value::as_str).unwrap_or(&path);
                        app.set_status(crate::i18n::format("macro.saved", &[("steps", &steps.to_string()), ("path", saved)]), false);
                    }
                    Err(e) => app.set_status(format!("{}: {e}", app.tr("cmd.macro_save")), true),
                }
            });
        }
        "macro.clear" => {
            app.macro_run = None;
            if app.run(id, json!({})).is_ok() {
                app.set_status(app.tr("macro.cleared").to_string(), false);
            }
        }
        _ => {
            let _ = app.run(id, json!({}));
        }
    }
}

fn play_button(app: &mut SolveApp) {
    if app.session.macros.playing {
        app.set_status("a macro is playing".to_string(), true);
        return;
    }
    let loaded = app.session.macros.loaded.clone();
    match loaded {
        Some(m) if !m.params.is_empty() => crate::macro_run::open(app),
        Some(m) if !m.source.is_empty() => play(app, json!({})),
        _ => {
            app.pick_open_for(&["rhai"], |app, path| {
                let Some(path) = path else { return };
                open_picked(app, path, true);
            });
        }
    }
}

fn open_picked(app: &mut SolveApp, path: String, play_after: bool) {
    match app.run("macro.open", json!({"path": path})) {
        Ok(r) => {
            let steps = r.get("steps").and_then(Value::as_u64).unwrap_or(0);
            app.set_status(crate::i18n::format("macro.opened", &[("steps", &steps.to_string())]), false);
            if play_after {
                if app.session.macros.loaded.as_ref().is_some_and(|m| !m.params.is_empty()) {
                    crate::macro_run::open(app);
                } else {
                    play(app, json!({}));
                }
            }
        }
        Err(e) => app.set_status(format!("{}: {e}", app.tr("cmd.macro_open")), true),
    }
}

pub(crate) fn play(app: &mut SolveApp, params: Value) {
    match app.run("macro.play", params) {
        Ok(r) => {
            let ran = r.get("steps").and_then(Value::as_u64).unwrap_or(0);
            app.set_status(crate::i18n::format("macro.played", &[("steps", &ran.to_string())]), false);
        }
        Err(e) => app.set_status(crate::i18n::format("macro.failed", &[("error", &e)]), true),
    }
}

pub(crate) fn chip(app: &SolveApp, ctx: &egui::Context) {
    if !app.session.macros.recording {
        return;
    }
    let steps = app.session.macros.steps();
    let paused = app.session.macros.paused;
    let id = if paused { "macro.chip_paused" } else { "macro.chip_recording" };
    let text = crate::i18n::format(id, &[("steps", &steps.to_string())]);
    let t = Tokens::get();
    ui_kit::Container::new(ctx, ui_kit::Surface::Toast)
        .area(egui::Id::new("sc_macro_recording"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 36.0))
        .interactable(false)
        .show(ctx, |ui| {
            let color = if paused { t.warning } else { t.error };
            let galley = ui.painter().layout_no_wrap(text, FontId::proportional(t.font_label), t.text);
            let size = galley.size() + vec2(22.0, 8.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, t.radius_sm, t.overlay);
            ui.painter().circle_filled(rect.left_center() + vec2(10.0, 0.0), 4.0, color);
            ui.painter().galley(rect.min + vec2(18.0, 4.0), galley, t.text);
            crate::scenario::publish_rect("macro:chip", rect);
        });
}

fn design_stem(app: &SolveApp) -> String {
    let name: String = app.session.doc.name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '-' }).collect();
    let name = name.trim_matches([' ', '-']);
    if name.is_empty() { "design".to_string() } else { name.to_string() }
}
