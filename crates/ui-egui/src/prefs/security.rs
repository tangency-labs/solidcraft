//! Security preferences: signing saved designs and checking signatures when they open.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
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
                        app.prefs_window.signing_keys = Some(signing_keys(app));
                    }
                });
            }
            if cfg!(not(target_arch = "wasm32"))
                && let Some(keys) = app.prefs_window.signing_keys.clone()
            {
                ui_kit::settings_form(ui, labels, |ui| {
                    match keys {
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
                                    app.prefs_window.signing_keys = None;
                                }
                            }
                        }
                        Err(error) => {
                            ui_kit::settings_inline_note(ui, &error);
                        }
                    }
                    if ui.button(app.tr("prefs.security.refresh_keys")).clicked() {
                        app.prefs_window.signing_keys = Some(signing_keys(app));
                    }
                });
            }
            setting::verify_opened_designs(app, ui);
        });
        ui_kit::settings_section(ui, "encryption", app.tr("prefs.security.encryption"), |ui| {
            ui_kit::settings_inline_note(ui, app.tr("prefs.security.encryption_note"));
        });
    });
}

/// The GnuPG secret keys the signing-key field can use, as `file.gpg_keys` lists them.
fn signing_keys(app: &mut SolveApp) -> Result<Vec<SigningKey>, String> {
    let reply = app.run("file.gpg_keys", serde_json::json!({}))?;
    if !reply.get("available").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        return Err(app.tr("prefs.security.gpg_missing").to_string());
    }
    let keys = reply.get("keys").and_then(serde_json::Value::as_array).ok_or_else(|| app.tr("prefs.security.gpg_missing").to_string())?;
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
