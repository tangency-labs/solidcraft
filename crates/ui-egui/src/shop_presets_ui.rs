//! Shop presets in the app (#20113): the person's own shop rules profiles, kept in the preferences
//! (their personal profile, like the Hole presets and the personal sheet metal rules, so they are
//! there in every design and on the web), handed to the session before each command
//! (`sync`, as the print face preference is), and written back after one (`keep`). The Shop Check
//! window lists them beside Generic and manages them with the commands `sheet.shop_presets`,
//! `sheet.save_shop_preset` and `sheet.delete_shop_preset`: a preset starts from the profile of the
//! shop picked (Generic's values for a new one), or from a profile file, and a preset is deleted
//! after a second click.

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::shop_presets::slug;
use solidcraft_engine::doc::shop_rules::{self, Profile};
use solidcraft_ui_text::tr;

use crate::SolveApp;
use crate::dialogs::{Kind, form_row};
use crate::dialogs_sheet::Sm;
use crate::prefs::Prefs;
use crate::ui_kit::{self, Text};

/// A shop the window can check against: Generic, or one of the presets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shop {
    pub id: String,
    pub name: String,
    pub preset: bool,
}

/// Hand the person's presets to `session`, which resolves a preset id for `sheet.check_shop`.
pub(crate) fn sync(session: &mut Session, prefs: &Prefs) {
    if session.shop_presets != prefs.shop_presets {
        session.shop_presets.clone_from(&prefs.shop_presets);
    }
}

/// Keep what a command changed in the session's presets, in the preferences, and count the change.
pub(crate) fn keep(app: &mut SolveApp) {
    if app.preferences.shop_presets != app.session.shop_presets {
        app.preferences.shop_presets.clone_from(&app.session.shop_presets);
        app.shop_presets_revision += 1;
    }
}

/// The built-in profiles, then the presets in the order they were saved.
pub(crate) fn shops(app: &SolveApp) -> Vec<Shop> {
    let built_in = shop_rules::profile_ids().into_iter().filter_map(|id| {
        let name = shop_rules::profile(id).ok()?.name.clone();
        Some(Shop { id: id.to_string(), name, preset: false })
    });
    let presets = app.preferences.shop_presets.list().iter().map(|p| Shop { id: p.id.clone(), name: p.name.clone(), preset: true });
    built_in.chain(presets).collect()
}

/// The profile of shop `id`, for the export it recommends.
pub(crate) fn profile(app: &SolveApp, id: &str) -> Option<Profile> {
    match shop_rules::profile(id) {
        Ok(p) => Some(p.clone()),
        Err(_) => app.preferences.shop_presets.get(id)?.parsed().ok(),
    }
}

/// Save the profile of shop `from` under `name` as a preset of its own: a new id from the name, never
/// the one of the profile it starts from, so Generic's values or another preset's become a new
/// preset and nothing is replaced. The id of the new preset.
pub(crate) fn save_as(app: &mut SolveApp, from: &str, name: &str) -> Result<String, String> {
    let name = name.trim();
    if slug(name).is_some_and(|id| app.preferences.shop_presets.get(&id).is_some() || shop_rules::profile(&id).is_ok()) {
        return Err(tr("ui.shop_presets_exists").to_string());
    }
    let mut profile = app.run("sheet.shop_presets", json!({"profile": from}))?.get("profile").cloned().unwrap_or(Value::Null);
    if let Some(object) = profile.as_object_mut() {
        object.remove("id");
    }
    let saved = app.run("sheet.save_shop_preset", json!({"name": name, "profile": profile}))?;
    Ok(saved["id"].as_str().unwrap_or_default().to_string())
}

/// Show shop `id` in the open Shop Check window, when it lists it.
fn select(app: &mut SolveApp, id: &str) {
    let index = shops(app).iter().position(|s| s.id == id);
    if let (Some(index), Some(d)) = (index, app.dialog.as_mut())
        && let Kind::Sheet(Sm::ShopCheck { shop, .. }) = &mut d.kind
    {
        *shop = index;
    }
}

#[derive(Clone, Default)]
struct PanelState {
    /// The name being typed for a new preset, while it is asked for.
    naming: Option<String>,
    confirming_delete: bool,
    /// Why the last save was refused.
    error: Option<String>,
    import: bool,
}

fn panel_id() -> egui::Id {
    egui::Id::unique("shop_presets_panel")
}

/// The presets section of the Shop Check window, under its Shop row: save the shop shown as a
/// preset (Generic's values for a new one), import a profile file, delete a preset.
pub(crate) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, shop: &mut usize) {
    let id = panel_id();
    let mut state = ui.ctx().data_mut(|data| data.get_temp::<PanelState>(id)).unwrap_or_default();
    let shown = shops(app).get(*shop).cloned();
    ui_kit::section(ui, "sm_shop_presets", tr("ui.shop_presets_title"), true, |ui| {
        ui_kit::text(ui, tr("ui.shop_presets_note"), Text::Report("shop_presets_note", false));
        let button = |ui: &mut egui::Ui, action: &str, label: &str, enabled: bool| {
            let response = ui_kit::button(ui, label, ui_kit::Button::Enabled(enabled));
            crate::scenario::publish_handle(&format!("dialog:shop_presets:{action}"), response.rect.center());
            response.clicked()
        };
        ui.horizontal_wrapped(|ui| {
            if button(ui, "save", tr("ui.shop_presets_save"), shown.is_some() && state.naming.is_none()) {
                state.naming = Some(String::new());
                state.error = None;
                state.confirming_delete = false;
            }
            if button(ui, "import", tr("ui.shop_presets_import"), true) {
                state.import = true;
                state.confirming_delete = false;
            }
            if button(ui, "delete", tr("ui.shop_presets_delete"), shown.as_ref().is_some_and(|s| s.preset)) {
                state.confirming_delete = true;
                state.naming = None;
            }
        });
        if let (Some(name), Some(from)) = (state.naming.as_mut(), shown.as_ref()) {
            form_row(ui, "ui.shop_presets_name", |ui| ui_kit::focused_text_field(ui, name, tr("ui.shop_presets_name_hint")));
            // A name is short; keep what the field holds bounded.
            if name.chars().count() > 200 {
                *name = name.chars().take(200).collect();
            }
            let mut done = false;
            ui.horizontal_wrapped(|ui| {
                if button(ui, "save_confirm", solidcraft_ui_text::tr_label("Save"), !name.trim().is_empty()) {
                    match save_as(app, &from.id, name) {
                        Ok(new) => {
                            let saved = solidcraft_ui_text::format("ui.shop_presets_saved", &[("name", name.trim())]);
                            // The window's own state is borrowed while it draws: set its shop here.
                            if let Some(index) = shops(app).iter().position(|s| s.id == new) {
                                *shop = index;
                            }
                            app.set_status(saved, false);
                            state.error = None;
                            done = true;
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
                if ui_kit::button(ui, tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                    state.error = None;
                    done = true;
                }
            });
            if done {
                state.naming = None;
            }
        }
        if state.confirming_delete
            && let Some(preset) = shown.as_ref().filter(|s| s.preset)
        {
            ui.label(solidcraft_ui_text::format("ui.shop_presets_delete_prompt", &[("name", &preset.name)]));
            ui.horizontal_wrapped(|ui| {
                if button(ui, "delete_confirm", tr("ui.shop_presets_delete_confirm"), true) {
                    match app.run("sheet.delete_shop_preset", json!({"id": preset.id})) {
                        Ok(_) => {
                            *shop = 0;
                            app.set_status(solidcraft_ui_text::format("ui.shop_presets_deleted", &[("name", &preset.name)]), false);
                        }
                        Err(e) => app.set_status(e, true),
                    }
                    state.confirming_delete = false;
                }
                if ui_kit::button(ui, tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                    state.confirming_delete = false;
                }
            });
        }
        if let Some(error) = &state.error {
            ui_kit::text(ui, error, Text::Report("shop_presets_error", true));
        }
    });
    if std::mem::take(&mut state.import) {
        app.pick_file(tr("ui.shop_presets_file_kind"), &["json"], |app, path| {
            let Some(path) = path else { return };
            match app.run("sheet.save_shop_preset", json!({"path": path})) {
                Ok(saved) => {
                    let name = saved["name"].as_str().unwrap_or_default().to_string();
                    select(app, saved["id"].as_str().unwrap_or_default());
                    app.set_status(solidcraft_ui_text::format("ui.shop_presets_saved", &[("name", &name)]), false);
                }
                Err(e) => app.set_status(e, true),
            }
        });
    }
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}
