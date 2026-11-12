//! Security preferences: signing saved designs and checking signatures when they open.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    tick(app);
    ui_kit::settings_page(ui, |ui| {
        let t = Tokens::get();
        let labels = t.space_24 * 6.0 + t.space_6;
        ui_kit::settings_heading(ui, app.tr("prefs.security"), app.tr("prefs.security_note"));
        ui_kit::settings_section(ui, "signing", app.tr("prefs.security.signing"), |ui| {
            setting::sign_saved_designs(app, ui);
            ui_kit::settings_form(ui, labels, |ui| {
                let tip = app.tr("prefs.security.signing_key_help").to_string();
                ui_kit::settings_row_help(ui, app.tr("prefs.security.signing_key"), &tip, |ui| {
                    let hint = app.tr("prefs.security.signing_key_hint").to_string();
                    let (_, response) = ui_kit::text_field_reserving(ui, &mut app.preferences.signing_key, &hint, 0.0);
                    crate::scenario::publish_target("prefs:signing_key", response.rect);
                    response
                });
            });
            if cfg!(not(target_arch = "wasm32")) {
                ui_kit::settings_form(ui, labels, |ui| {
                    if ui.button(app.tr("prefs.security.choose_key")).clicked() {
                        list_signing_keys(app);
                    }
                });
            }
            if cfg!(not(target_arch = "wasm32")) {
                match app.prefs_window.signing_keys.clone() {
                    SigningKeys::Idle => {}
                    SigningKeys::Loading => {
                        ui_kit::settings_form(ui, labels, |ui| {
                            ui_kit::settings_inline_note(ui, app.tr("prefs.security.reading_keys"));
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                        });
                    }
                    SigningKeys::Ready(result) => {
                        ui_kit::settings_form(ui, labels, |ui| {
                            match result {
                                Ok(keys) if keys.is_empty() => {
                                    ui_kit::settings_inline_note(ui, app.tr("prefs.security.no_signing_keys"));
                                }
                                Ok(keys) => {
                                    ui.label(app.tr("prefs.security.signing_keys_heading"));
                                    for key in keys {
                                        let suffix = if key.on_card { format!(" ({})", app.tr("prefs.security.key_on_card")) } else { String::new() };
                                        let label = format!("{}{suffix} — {}", key.user_id, key.fingerprint);
                                        if ui.selectable_label(false, label).clicked() {
                                            app.preferences.signing_key = key.fingerprint.clone();
                                            app.prefs_window.signing_keys = SigningKeys::Idle;
                                        }
                                    }
                                }
                                Err(SigningKeysError::Missing) => {
                                    ui_kit::settings_inline_note(ui, app.tr("prefs.security.gpg_missing"));
                                }
                                Err(SigningKeysError::Message(error)) => {
                                    ui_kit::settings_inline_note(ui, &error);
                                }
                            }
                            if ui.button(app.tr("prefs.security.refresh_keys")).clicked() {
                                list_signing_keys(app);
                            }
                        });
                    }
                }
            }
            setting::verify_opened_designs(app, ui);
            ui_kit::settings_form(ui, labels, |ui| {
                let tip = app.tr("prefs.security.trusted_keys_help").to_string();
                ui_kit::settings_row_help(ui, app.tr("prefs.security.trusted_keys"), &tip, |ui| {
                    let hint = app.tr("prefs.security.trusted_keys_hint").to_string();
                    let (_, response) = ui_kit::text_field_reserving(ui, &mut app.preferences.trusted_signing_keys, &hint, 0.0);
                    crate::scenario::publish_target("prefs:trusted_signing_keys", response.rect);
                    response
                });
            });
        });
        ui_kit::settings_section(ui, "encryption", app.tr("prefs.security.encryption"), |ui| {
            ui_kit::settings_inline_note(ui, app.tr("prefs.security.encryption_note"));
        });
    });
}

/// Ask a worker for GnuPG's signing keys: listing them can wait on an agent or a smartcard, and
/// the window must keep drawing meanwhile.
fn list_signing_keys(app: &mut SolveApp) {
    if matches!(&app.prefs_window.signing_keys, SigningKeys::Loading) {
        return;
    }
    app.prefs_window.signing_keys = SigningKeys::Loading;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("gpg-keys".into()).spawn(move || {
            let result = solidcraft_engine::Session::default()
                .execute("file.gpg_keys", &serde_json::json!({}))
                .map_err(|error| SigningKeysError::Message(error.to_string()))
                .and_then(signing_keys_from_reply);
            let _ = tx.send(result);
        });
        if spawned.is_ok() {
            app.prefs_window.signing_keys_rx = Some(rx);
            return;
        }
    }
    app.prefs_window.signing_keys = SigningKeys::Ready(Err(SigningKeysError::Message("the browser cannot list GnuPG keys".to_string())));
}

/// Take up the worker's answer when it arrives.
fn tick(app: &mut SolveApp) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.prefs_window.signing_keys_rx.is_some() {
        let answer = match app.prefs_window.signing_keys_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            Some(answer) => answer,
            None => return,
        };
        app.prefs_window.signing_keys_rx = None;
        app.prefs_window.signing_keys = SigningKeys::Ready(answer);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// The keys in `file.gpg_keys`'s reply, or why there are none.
fn signing_keys_from_reply(reply: serde_json::Value) -> Result<Vec<SigningKey>, SigningKeysError> {
    if !reply.get("available").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        return Err(SigningKeysError::Missing);
    }
    let keys = reply.get("keys").and_then(serde_json::Value::as_array).ok_or(SigningKeysError::Missing)?;
    Ok(keys
        .iter()
        .filter_map(|key| {
            Some(SigningKey {
                fingerprint: key.get("fingerprint")?.as_str()?.to_string(),
                user_id: key.get("user_id")?.as_str()?.to_string(),
                on_card: key.get("on_card")?.as_bool().unwrap_or(false),
            })
        })
        .collect())
}
