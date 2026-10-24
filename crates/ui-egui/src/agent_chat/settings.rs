//! Settings → AI: the master switch, the agents (found and added, each with Test, Default and,
//! for added ones, Remove), permissions (global and per agent, and the "always allow" answers),
//! and the panel. Plus the Add agent dialog: an installed ACP agent, a custom command, or an
//! endpoint (OpenAI-compatible or Anthropic-compatible, whose key goes to the system keychain).

use super::agents::{self, AgentConfig, Kind, Mode, Protocol};
use super::backend::TestError;
use super::*;
use crate::ui_kit;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddKind {
    #[default]
    Installed,
    Custom,
    Api,
}

/// The Add agent dialog's fields.
#[derive(Default)]
pub struct AddState {
    pub kind: AddKind,
    pub name: String,
    pub command: String,
    pub installed: usize,
    pub preset: usize,
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    pub key: String,
    /// Seconds one model request may take.
    pub request_timeout_s: u64,
    pub error: Option<String>,
    test: Option<Test>,
}

/// A connection test running or done.
pub enum Test {
    Running(std::sync::mpsc::Receiver<Result<String, TestError>>),
    Done(Result<String, TestError>),
}

impl Test {
    fn poll(&mut self) -> Option<bool> {
        if let Test::Running(rx) = self
            && let Ok(r) = rx.try_recv()
        {
            let login = match &r {
                Ok(_) => Some(true),
                Err(TestError::AuthRequired) => Some(false),
                Err(TestError::Other(_)) => None,
            };
            *self = Test::Done(r);
            return login;
        }
        None
    }
}

pub(super) fn poll_tests(chat: &mut AgentChat) {
    for (id, test) in &mut chat.settings.tests {
        if let Some(signed_in) = test.poll() {
            chat.login_results.insert(id.clone(), signed_in);
        }
    }
}

/// Settings state that isn't preferences: the dialog and the tests.
#[derive(Default)]
pub struct State {
    pub add: Option<AddState>,
    pub tests: std::collections::HashMap<String, Test>,
    #[cfg(not(target_arch = "wasm32"))]
    capabilities: std::collections::HashMap<(String, String), Option<bool>>,
    #[cfg(not(target_arch = "wasm32"))]
    checking: Option<CapabilityCheck>,
}

#[cfg(not(target_arch = "wasm32"))]
struct CapabilityCheck {
    key: (String, String),
    rx: std::sync::mpsc::Receiver<Option<bool>>,
}

/// Poll one bounded background probe at a time. Failures are cached too.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn poll_capabilities(app: &mut SolveApp, ctx: &egui::Context) {
    use std::sync::mpsc::TryRecvError;
    let state = &mut app.agent_chat.settings;
    if let Some(check) = &state.checking {
        let result = match check.rx.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Disconnected) => Some(None),
            Err(TryRecvError::Empty) => None,
        };
        if let Some(value) = result {
            if state.capabilities.len() >= 128 {
                state.capabilities.clear();
            }
            state.capabilities.insert(check.key.clone(), value);
            state.checking = None;
        }
    }
    if state.checking.is_some() {
        return;
    }
    let Some((key, known)) = capability_selection(app) else { return };
    if app.agent_chat.settings.capabilities.contains_key(&key) {
        return;
    }
    let Some(http) = app.services.agent_http.clone() else { return };
    if app.agent_chat.settings.capabilities.len() >= 128 {
        app.agent_chat.settings.capabilities.clear();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let query = key.clone();
    let wake = ctx.clone();
    if std::thread::Builder::new()
        .name("model-capabilities".into())
        .spawn(move || {
            let _ = tx.send(discover::ollama_tools(&http, &query.0, &query.1, known));
            wake.request_repaint();
        })
        .is_ok()
    {
        app.agent_chat.settings.checking = Some(CapabilityCheck { key, rx });
    } else {
        app.agent_chat.settings.capabilities.insert(key, None);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn capability_selection(app: &SolveApp) -> Option<((String, String), bool)> {
    let entry = agents::current(app)?;
    if entry.kind != Kind::Api {
        return None;
    }
    let model = if app.agent_chat.backend.is_none() || app.agent_chat.model.is_empty() { entry.model } else { app.agent_chat.model.clone() };
    if model.is_empty() || model.len() > 4096 || entry.base_url.len() > 4096 {
        return None;
    }
    let known = app.preferences.agents.iter().any(|a| a.id == entry.id && (a.ollama || a.name.eq_ignore_ascii_case("Ollama")));
    Some(((entry.base_url, model), known))
}

/// The same advisory text is used by the panel and the control channel.
pub(super) fn model_warning(app: &SolveApp) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((key, _)) = capability_selection(app) {
        return capability_warning(&key.1, app.agent_chat.settings.capabilities.get(&key).copied().flatten());
    }
    let _ = app;
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn capability_warning(model: &str, tools: Option<bool>) -> Option<String> {
    (tools == Some(false)).then(|| trf("ui.agent_chat_model_no_tools", &[("model", model)]))
}

/// Open the Add agent dialog.
pub fn open_add(app: &mut SolveApp) {
    let preset = agents::OPENAI_PRESETS.first();
    app.agent_chat.settings.add = Some(AddState {
        base_url: preset.map(|p| p.base_url.to_string()).unwrap_or_default(),
        name: preset.map(|p| p.name.to_string()).unwrap_or_default(),
        request_timeout_s: agents::DEFAULT_REQUEST_TIMEOUT_S,
        ..Default::default()
    });
}

/// Take `index` from the protocol's presets, or make the URL empty for a custom endpoint.
pub(super) fn choose_preset(s: &mut AddState, protocol: Protocol, index: usize) {
    s.protocol = protocol;
    s.preset = index;
    if let Some(preset) = agents::presets(protocol).get(index) {
        s.base_url = preset.base_url.to_string();
        s.name = preset.name.to_string();
    } else {
        s.base_url.clear();
    }
    s.error = None;
    s.test = None;
}

/// Run a connection test in the background (it starts the agent briefly, or asks the endpoint
/// for its models).
#[cfg(not(target_arch = "wasm32"))]
fn start_test(app: &SolveApp, e: &agents::Entry, key: Option<String>) -> Test {
    let (tx, rx) = std::sync::mpsc::channel();
    match e.kind {
        Kind::Acp => {
            let argv = e.argv.clone();
            std::thread::spawn(move || {
                let _ = tx.send(super::acp_client::test_agent(&argv, &working_dir()));
            });
        }
        Kind::Api => {
            let Some(http) = app.services.agent_http.clone() else { return Test::Done(Err(TestError::Other(tr("ui.ai_no_http").to_string()))) };
            let (url, model) = (e.base_url.clone(), e.model.clone());
            std::thread::spawn(move || {
                let _ = tx.send(super::api_client::test_endpoint(&http, &url, key.as_deref(), &model).map_err(TestError::Other));
            });
        }
        Kind::Anthropic => {
            let Some(http) = app.services.agent_http.clone() else { return Test::Done(Err(TestError::Other(tr("ui.ai_no_http").to_string()))) };
            let (url, model) = (e.base_url.clone(), e.model.clone());
            std::thread::spawn(move || {
                let _ = tx.send(super::api_client::test_anthropic_endpoint(&http, &url, key.as_deref(), &model).map_err(TestError::Other));
            });
        }
    }
    Test::Running(rx)
}

/// An endpoint's request timeout field: seconds, 30 s to an hour.
fn timeout_field(ui: &mut egui::Ui, secs: &mut u64, handle: &str) -> bool {
    let r = ui_kit::number_field(ui, secs, agents::MIN_REQUEST_TIMEOUT_S..=agents::MAX_REQUEST_TIMEOUT_S, 5.0, " s")
        .on_hover_text(tr("ui.ai_request_timeout_hint"));
    crate::scenario::publish_target(handle, r.rect);
    r.changed()
}

fn test_line(ui: &mut egui::Ui, test: &Option<&Test>) {
    match test {
        Some(Test::Running(_)) => {
            ui_kit::text(ui, tr("ui.ai_testing"), ui_kit::Text::Note("testing"));
        }
        Some(Test::Done(Ok(message))) => {
            ui_kit::text(ui, message, ui_kit::Text::Report("test_ok", false));
        }
        Some(Test::Done(Err(error))) => {
            ui_kit::text(ui, error.message(), ui_kit::Text::Report("test_error", true));
        }
        None => {}
    }
}

/// The Assistant page of Settings. Copy stays close to its control; full security details
/// remain accessible on hover, and high-risk permissions always retain a visible warning.
pub fn page(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_heading(ui, tr("prefs.assistant"), tr("ai.prefs.intro"));
    ui_kit::settings_section(ui, "assistant_availability", tr("ai.prefs.availability"), |ui| {
        let response =
            ui_kit::checkbox(ui, tr("ui.ai_show_features"), &mut app.preferences.ai_features).on_hover_text(tr("ui.ai_show_features_note"));
        crate::scenario::publish_handle("prefs:ai_features", response.rect.center());
        if !cfg!(target_arch = "wasm32") {
            if app.preferences.ai_features {
                let response = ui_kit::checkbox(ui, tr("ui.ai_toolbar_button"), &mut app.preferences.ai_toolbar_button)
                    .on_hover_text(tr("ui.ai_toolbar_button_note"));
                crate::scenario::publish_handle("prefs:ai_toolbar_button", response.rect.center());
                let response = ui_kit::checkbox(ui, tr("ai.prefs.connect_at_start"), &mut app.preferences.agent_connect_at_start)
                    .on_hover_text(tr("ai.prefs.connect_at_start_note"));
                crate::scenario::publish_handle("prefs:agent_connect_at_start", response.rect.center());
                let response = ui_kit::checkbox(ui, tr("ai.prefs.open_at_start"), &mut app.preferences.agent_chat_open_at_start)
                    .on_hover_text(tr("ai.prefs.open_at_start_note"));
                crate::scenario::publish_handle("prefs:agent_chat_open_at_start", response.rect.center());
            }
            let response = ui_kit::checkbox(ui, tr("ai.prefs.background_commands"), &mut app.preferences.agent_off_thread)
                .on_hover_text(tr("ui.agent_off_thread_note"));
            crate::scenario::publish_handle("prefs:agent_off_thread", response.rect.center());
            let response = ui_kit::checkbox(ui, tr("ai.prefs.allow_control"), &mut app.preferences.agent_control)
                .on_hover_text(tr("ai.prefs.allow_control_note"));
            crate::scenario::publish_handle("prefs:agent_control", response.rect.center());
            let response = ui_kit::checkbox(ui, tr("ai.prefs.connect_apps"), &mut app.preferences.connect_ai_apps)
                .on_hover_text(tr("ai.prefs.connect_apps_note"));
            crate::scenario::publish_handle("prefs:connect_ai_apps", response.rect.center());
        }
        if cfg!(target_arch = "wasm32") {
            ui_kit::text(ui, tr("ui.agent_chat_desktop_only"), ui_kit::Text::Hint("desktop"));
        }
    });
    if !cfg!(target_arch = "wasm32") && app.preferences.ai_features {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let browsing = !crate::prefs_search::querying(ui) || app.prefs_window.section == crate::prefs::PAGE_AI;
            if browsing && app.agent_chat.detection.is_none() && app.agent_chat.detecting.is_none() {
                refresh(app, &ui.ctx().clone());
            }
            agents_section(app, ui);
        }
        permissions_section(app, ui);
        panel_section(app, ui);
    }
    pointer_section(app, ui);
}

fn permissions_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "permissions", tr("ui.ai_permissions"), |ui| {
        let mut selected = Mode::ALL.iter().position(|mode| *mode == app.preferences.ai_permissions).unwrap_or(0);
        let choices = [
            ui_kit::SettingsChoice {
                key: "prefs:ai_mode:ask",
                title: tr("ai.prefs.permission_ask"),
                summary: tr("ai.prefs.permission_ask_summary"),
                detail: tr("ui.ai_mode_ask_note"),
            },
            ui_kit::SettingsChoice {
                key: "prefs:ai_mode:edits",
                title: tr("ai.prefs.permission_edits"),
                summary: tr("ai.prefs.permission_edits_summary"),
                detail: tr("ui.ai_mode_edits_note"),
            },
            ui_kit::SettingsChoice {
                key: "prefs:ai_mode:all",
                title: tr("ai.prefs.permission_all"),
                summary: tr("ai.prefs.permission_all_summary"),
                detail: tr("ui.ai_mode_all_note"),
            },
        ];
        if ui_kit::settings_choices(ui, &choices, &mut selected)
            && let Some(mode) = Mode::ALL.get(selected)
        {
            app.preferences.ai_permissions = *mode;
        }
        if app.preferences.ai_permissions == Mode::All {
            ui_kit::text(ui, tr("ui.ai_mode_all_warning"), ui_kit::Text::Report("permissions_warning", true));
        }
        ui_kit::text(ui, tr("ai.prefs.permission_scope"), ui_kit::Text::Hint("scope")).on_hover_text(tr("ui.ai_modes_scope"));
        let remembered: Vec<_> = app
            .preferences
            .agent_always_allow
            .iter()
            .filter(|(_, tools)| !tools.is_empty())
            .map(|(agent, tools)| (agent.clone(), tools.clone()))
            .collect();
        ui_kit::section_count(ui, "always_allowed", tr("ui.ai_always_allowed"), false, Some(remembered.len()), |ui| {
            if remembered.is_empty() {
                ui_kit::text(ui, tr("ai.prefs.no_saved_approvals"), ui_kit::Text::Hint("none")).on_hover_text(tr("ui.ai_always_allowed_none"));
            }
            let names: std::collections::HashMap<_, _> = agents::entries(app).into_iter().map(|entry| (entry.id, entry.name)).collect();
            for (agent, tools) in remembered {
                let t = crate::theme::Tokens::get();
                ui_kit::settings_block(ui, names.get(&agent).unwrap_or(&agent), t.space_24 * 6.0 + t.space_6, |ui| {
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, tr("ui.ai_reset"), ui_kit::Button::Small).clicked() {
                            app.preferences.agent_always_allow.remove(&agent);
                        }
                        ui_kit::settings_short_note(ui, &tools.join(", "));
                    });
                });
            }
        });
    });
}

fn panel_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    ui_kit::settings_section(ui, "panel", tr("ui.ai_panel"), |ui| {
        ui_kit::settings_form(ui, t.space_24 * 6.0 + t.space_6, |ui| {
            ui_kit::row(ui, tr("ui.agent_chat_side"), |ui| {
                let p = &mut app.preferences;
                let mut selected = if p.agent_chat_popped { 2 } else { usize::from(p.agent_chat_side != "left") };
                let options = [tr("ui.agent_chat_left"), tr("ui.agent_chat_right"), tr("ui.agent_chat_own_window")];
                if ui_kit::segmented(ui, &mut selected, &options, ui_kit::Segments { intrinsic: true, ..Default::default() }) {
                    p.agent_chat_popped = selected == 2;
                    if selected < 2 {
                        p.agent_chat_side = if selected == 0 { "left" } else { "right" }.into();
                    }
                }
            });
            ui_kit::row(ui, tr("ui.agent_chat_turn_limit"), |ui| {
                let p = &mut app.preferences;
                let mut unlimited = p.agent_chat_turn_limit == 0;
                if ui_kit::checkbox(ui, tr("ui.agent_chat_turn_unlimited"), &mut unlimited).changed() {
                    p.agent_chat_turn_limit = if unlimited { 0 } else { gate::SEEDED_TURN_LIMIT };
                }
                if !unlimited {
                    ui_kit::settings_number(ui, &mut p.agent_chat_turn_limit, 1..=200, 1.0).on_hover_text(tr("ai.prefs.turn_limit_hint"));
                }
            });
        });
        ui_kit::text(ui, tr("ai.prefs.privacy_note"), ui_kit::Text::Hint("privacy"));
    });
}

fn pointer_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "agent_pointer", tr("ui.agent_cursor"), |ui| {
        ui_kit::settings_pair(
            ui,
            |ui| {
                ui_kit::checkbox(ui, tr("ai.prefs.show_pointer"), &mut app.ui.agent_cursor.show)
                    .on_hover_text(crate::i18n::tr_label("Show the pointer an AI agent moves (#35)"));
                ui.add_enabled_ui(app.ui.agent_cursor.show, |ui| {
                    let speed_label_width = ui_kit::ContentWidth::label(ui, tr("ai.prefs.pointer_speed")) + crate::theme::Tokens::get().space_4;
                    ui_kit::settings_block(ui, tr("ai.prefs.pointer_speed"), speed_label_width, |ui| {
                        crate::agent_cursor::speed_slider(ui, &mut app.ui.agent_cursor.speed);
                    });
                    ui_kit::checkbox(ui, tr("ai.prefs.follow_camera"), &mut app.ui.agent_cursor.follow_camera)
                        .on_hover_text(crate::i18n::tr_label("Follow the camera (Look At sketches)"));
                });
                (app.ui.agent_cursor.show, app.ui.agent_cursor.speed.label(), app.ui.agent_cursor.follow_camera)
            },
            |ui, (visible, speed, follow)| ui_kit::settings_pointer_preview(ui, visible, crate::i18n::tr_label(&speed), follow),
        );
    });
}

/// The agents: found and added, with Test, Default, a permission override and Remove.
#[cfg(not(target_arch = "wasm32"))]
fn agents_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    let all = agents::entries(app);
    let d = &app.preferences.agent_default;
    let default = if all.iter().any(|e| &e.id == d) { Some(d.clone()) } else { all.first().map(|e| e.id.clone()) };
    let mut remove: Option<String> = None;
    let mut test: Option<agents::Entry> = None;
    let mut retimed: Option<(String, u64)> = None;
    let t = crate::theme::Tokens::get();
    ui_kit::settings_section(ui, "agents", tr("ui.ai_agents"), |ui| {
        if app.agent_chat.detecting.is_some() {
            ui_kit::text(ui, tr("ui.agent_chat_detecting"), ui_kit::Text::Hint("detecting"));
        } else if all.is_empty() {
            if std::env::var_os("FLATPAK_ID").is_some() {
                super::panel::no_agents(ui);
            } else {
                ui_kit::empty_box(ui, "assistant_agents", tr("ai.prefs.no_agents"));
            }
        }
        for (index, entry) in all.iter().enumerate() {
            ui.push_id(&entry.id, |ui| {
                if index > 0 {
                    ui_kit::divider(ui, "agent_divider");
                }
                ui_kit::settings_block(ui, &entry.name, t.space_24 * 6.0 + t.space_6, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let is_default = default.as_deref() == Some(entry.id.as_str());
                        let choice = ui_kit::button(
                            ui,
                            tr(if is_default { "ui.ai_default" } else { "ai.prefs.make_default" }),
                            if is_default { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small },
                        );
                        crate::scenario::publish_target(&format!("dialog:list:{}", entry.id), choice.rect);
                        if choice.clicked() {
                            app.preferences.agent_default = entry.id.clone();
                        }
                        let test_button = ui_kit::button(ui, tr("ui.ai_test"), ui_kit::Button::Small);
                        crate::scenario::publish_handle(&format!("prefs:ai_test:{}", entry.id), test_button.rect.center());
                        let tip = match entry.kind {
                            Kind::Acp => "ui.ai_test_acp_tip",
                            Kind::Api => "ui.ai_test_api_tip",
                            Kind::Anthropic => "ui.ai_test_anthropic_tip",
                        };
                        if test_button.on_hover_text(tr(tip)).clicked() {
                            test = Some(entry.clone());
                        }
                        if !entry.found && ui_kit::button(ui, tr("ui.ai_remove"), ui_kit::Button::Ghost).clicked() {
                            remove = Some(entry.id.clone());
                        }
                        match entry.login {
                            "login_found" => ui_kit::settings_badge(ui, tr("ui.agent_chat_login_found"), false),
                            "signed_in" => ui_kit::settings_badge(ui, tr("ui.agent_chat_signed_in"), false),
                            "key" => ui_kit::settings_badge(ui, tr("ui.agent_chat_key_in_env"), false),
                            "signed_out" => ui_kit::settings_badge(ui, tr("ui.agent_chat_not_signed_in"), true),
                            _ => {}
                        }
                        if !entry.found {
                            ui_kit::settings_badge(ui, tr("ui.ai_added"), false);
                        }
                    });
                    ui_kit::settings_short_note(ui, &entry.detail());
                    // Wide enough for an endpoint's "Request timeout" too, so both rows line up.
                    let label_w = (t.form_label_width + t.space_16).max(ui_kit::ContentWidth::label(ui, tr("ui.ai_request_timeout")) + t.space_16);
                    ui_kit::settings_form(ui, label_w, |ui| {
                        ui_kit::row(ui, tr("ui.ai_permissions"), |ui| {
                            let current = app.preferences.agent_permissions.get(&entry.id).copied();
                            let shown = current.map_or_else(|| tr("ui.ai_mode_global").to_owned(), |mode| mode.label().to_owned());
                            let mut pick: Option<Option<Mode>> = None;
                            ui_kit::dropdown_menu(ui, ("sc_ai_mode", &entry.id), &shown, |ui| {
                                if ui_kit::list_picker(ui, "global", tr("ui.ai_mode_global"), current.is_none()).clicked() {
                                    pick = Some(None);
                                }
                                for mode in Mode::ALL {
                                    if ui_kit::list_picker(ui, mode.id(), mode.label(), current == Some(mode)).clicked() {
                                        pick = Some(Some(mode));
                                    }
                                }
                            });
                            match pick {
                                Some(Some(mode)) => {
                                    app.preferences.agent_permissions.insert(entry.id.clone(), mode);
                                }
                                Some(None) => {
                                    app.preferences.agent_permissions.remove(&entry.id);
                                }
                                None => {}
                            }
                        });
                    });
                    if entry.kind == Kind::Api
                        && let Some(c) = app.preferences.agents.iter_mut().find(|c| c.id == entry.id)
                    {
                        ui_kit::settings_form(ui, label_w, |ui| {
                            ui_kit::row(ui, tr("ui.ai_request_timeout"), |ui| {
                                let mut secs = entry.request_timeout_s;
                                if timeout_field(ui, &mut secs, &format!("prefs:ai_timeout:{}", entry.id)) {
                                    c.request_timeout_s = secs;
                                    retimed = Some((entry.id.clone(), secs));
                                }
                            });
                        });
                    }
                    if app.preferences.agent_permissions.get(&entry.id) == Some(&Mode::All) {
                        ui_kit::text(ui, tr("ai.prefs.override_all_warning"), ui_kit::Text::Report("agent_permissions_warning", true))
                            .on_hover_text(tr("ui.ai_mode_all_warning"));
                    }
                    test_line(ui, &app.agent_chat.settings.tests.get(&entry.id));
                });
            });
        }
        ui.horizontal_wrapped(|ui| {
            let button = ui_kit::icon_label_button(ui, tr("ui.ai_add_agent"), "plus", true, false);
            crate::scenario::publish_handle("prefs:ai_add", button.rect.center());
            if button.clicked() {
                open_add(app);
            }
            if ui_kit::button(ui, tr("ui.agent_chat_refresh"), ui_kit::Button::Enabled(app.agent_chat.detecting.is_none())).clicked() {
                refresh(app, &ui.ctx().clone());
            }
        });
    });
    if let Some(e) = test {
        let key = if e.key { app.services.secrets.as_ref().and_then(|s| s.get(&e.id).ok().flatten()) } else { None };
        let started = start_test(app, &e, key);
        app.agent_chat.settings.tests.insert(e.id.clone(), started);
    }
    // The running conversation uses a new timeout from its next request on.
    if let Some((id, secs)) = retimed
        && app.agent_chat.running_id == id
        && let Some(b) = app.agent_chat.backend.as_mut()
    {
        b.set_request_timeout(secs);
    }
    if let Some(id) = remove {
        app.agent_chat.login_results.remove(&id);
        app.agent_chat.settings.tests.remove(&id);
        app.preferences.agents.retain(|a| a.id != id);
        app.preferences.agent_permissions.remove(&id);
        app.preferences.agent_always_allow.remove(&id);
        if let Some(s) = &app.services.secrets {
            let _ = s.delete(&id);
        }
        if app.agent_chat.agent_id.as_deref() == Some(id.as_str()) {
            app.agent_chat.agent_id = None;
        }
        // A removed agent is not missed at the next start: the default connects instead.
        if app.preferences.agent_last == id {
            app.preferences.agent_last.clear();
        }
    }
    if app.agent_chat.settings.tests.values().any(|t| matches!(t, Test::Running(_))) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
}

/// The Add agent dialog (drawn every frame while it is open).
pub fn dialog(app: &mut SolveApp, ctx: &egui::Context) {
    if app.agent_chat.settings.add.is_none() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let found: Vec<agents::Entry> = agents::entries(app).into_iter().filter(|e| e.found).collect();
    #[cfg(target_arch = "wasm32")]
    let found: Vec<agents::Entry> = Vec::new();
    let has_keychain = app.services.secrets.is_some();
    let mut open = true;
    let mut add = false;
    let mut test = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_ai_add"), "settings", tr("ui.ai_add_agent_title"))
        .cards(2.0)
        .fields()
        .actions(tr("ui.ai_add"), "Cancel", None)
        .show(ctx, true, |ui| {
            let Some(s) = app.agent_chat.settings.add.as_mut() else { return };
            if let Some(test) = s.test.as_mut() {
                let _ = test.poll();
            }
            let kinds = [(AddKind::Installed, "ui.ai_add_installed"), (AddKind::Custom, "ui.ai_add_custom"), (AddKind::Api, "ui.ai_add_endpoint")];
            ui_kit::row(ui, tr("ui.ai_kind"), |ui| {
                let mut selected = kinds.iter().position(|(k, _)| *k == s.kind).unwrap_or(0);
                let labels = kinds.map(|(_, key)| tr(key));
                if ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments { compact: true, ..Default::default() })
                    && let Some((kind, _)) = kinds.get(selected)
                {
                    s.kind = *kind;
                    s.error = None;
                    s.test = None;
                }
                for (_, key) in kinds {
                    if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{}", tr("ui.ai_kind"), tr(key))) {
                        crate::scenario::publish_handle(&format!("ai_add:{key}"), at);
                    }
                }
            });
            match s.kind {
                AddKind::Installed => {
                    ui_kit::row(ui, tr("ui.agent_chat_pick"), |ui| {
                        if found.is_empty() {
                            ui_kit::text(ui, tr("ui.agent_chat_none_found_short"), ui_kit::Text::Inline);
                        } else {
                            let sel = found.get(s.installed).map(|e| e.name.clone()).unwrap_or_default();
                            ui_kit::dropdown_menu(ui, "sc_ai_add_installed", &sel, |ui| {
                                for (i, e) in found.iter().enumerate() {
                                    if ui_kit::list_picker(ui, &e.id, &format!("{}  {}", e.name, e.detail()), s.installed == i).clicked() {
                                        s.installed = i;
                                        s.command = e.argv.iter().map(|a| super::panel::quote(a)).collect::<Vec<_>>().join(" ");
                                        s.name = format!("{} ({})", e.name, tr("ui.ai_copy_suffix"));
                                    }
                                }
                            });
                        }
                    });
                    if s.command.is_empty()
                        && let Some(e) = found.get(s.installed)
                    {
                        s.command = e.argv.iter().map(|a| super::panel::quote(a)).collect::<Vec<_>>().join(" ");
                        s.name = format!("{} ({})", e.name, tr("ui.ai_copy_suffix"));
                    }
                    ui_kit::row(ui, tr("ui.ai_name"), |ui| {
                        ui_kit::text_field(ui, &mut s.name, "");
                    });
                    ui_kit::row(ui, tr("ui.agent_chat_command"), |ui| {
                        ui_kit::text_field(ui, &mut s.command, "");
                    });
                    ui_kit::text(ui, tr("ui.ai_add_installed_note"), ui_kit::Text::Note("installed"));
                }
                AddKind::Custom => {
                    ui_kit::row(ui, tr("ui.ai_name"), |ui| {
                        ui_kit::text_field(ui, &mut s.name, tr("ui.ai_name_hint"));
                    });
                    ui_kit::row(ui, tr("ui.agent_chat_command"), |ui| {
                        ui_kit::text_field(ui, &mut s.command, tr("ui.ai_command_hint"));
                    });
                    ui_kit::text(ui, tr("ui.agent_chat_custom_hint"), ui_kit::Text::Note("custom"));
                }
                AddKind::Api => {
                    ui_kit::row(ui, tr("ui.ai_protocol"), |ui| {
                        let mut selected = if s.protocol == Protocol::Anthropic { 1 } else { 0 };
                        let labels = [tr("ui.ai_add_api"), tr("ui.ai_add_anthropic")];
                        if ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments { compact: true, ..Default::default() })
                            && let Some(protocol) = [Protocol::OpenAi, Protocol::Anthropic].get(selected)
                        {
                            choose_preset(s, *protocol, 0);
                        }
                        for (name, key) in [("openai", "ui.ai_add_api"), ("anthropic", "ui.ai_add_anthropic")] {
                            if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{}", tr("ui.ai_protocol"), tr(key))) {
                                crate::scenario::publish_handle(&format!("ai_add:protocol:{name}"), at);
                            }
                        }
                    });
                    let choices: Vec<&str> =
                        agents::presets(s.protocol).iter().map(|p| p.name).chain(std::iter::once(tr("ui.ai_custom_url"))).collect();
                    ui_kit::row(ui, tr("ui.ai_preset"), |ui| {
                        let picker = ui_kit::dropdown_menu(ui, "sc_ai_add_preset", choices.get(s.preset).copied().unwrap_or_default(), |ui| {
                            for (index, preset) in choices.iter().enumerate() {
                                let response = ui_kit::list_picker(ui, preset, preset, s.preset == index);
                                crate::scenario::publish_handle(&format!("ai_add:preset:{preset}"), response.rect.center());
                                if response.clicked() {
                                    choose_preset(s, s.protocol, index);
                                }
                            }
                        });
                        crate::scenario::publish_handle("ai_add:preset", picker.rect.center());
                    });
                    ui_kit::row(ui, tr("ui.ai_name"), |ui| {
                        ui_kit::text_field(ui, &mut s.name, "");
                    });
                    ui_kit::row(ui, tr("ui.ai_base_url"), |ui| {
                        ui_kit::text_field(ui, &mut s.base_url, tr("ui.ai_url_hint"));
                    });
                    ui_kit::row(ui, tr("ui.ai_model"), |ui| {
                        ui_kit::text_field(ui, &mut s.model, tr("ui.ai_model_hint"));
                    });
                    ui_kit::row(ui, tr("ui.ai_request_timeout"), |ui| {
                        timeout_field(ui, &mut s.request_timeout_s, "ai_add:request_timeout");
                    });
                    ui_kit::row(ui, tr("ui.ai_key"), |ui| {
                        ui.add_enabled_ui(has_keychain, |ui| {
                            let r = ui_kit::password_field(ui, egui::Id::new("sc_ai_add_key"), &mut s.key, tr("ui.ai_key_hint"));
                            crate::scenario::publish_target("ai_add:key", r.rect);
                        });
                    });
                    ui_kit::text(ui, tr(if has_keychain { "ui.ai_key_note" } else { "ui.ai_no_keychain" }), ui_kit::Text::Note("keychain"));
                    if ui_kit::button(ui, tr("ui.ai_test"), ui_kit::Button::Normal).clicked() {
                        test = true;
                    }
                    test_line(ui, &s.test.as_ref());
                }
            }
            if let Some(e) = &s.error {
                ui_kit::text(ui, e, ui_kit::Text::Report("add_error", true));
            }
        });
    if let Some(rect) = crate::scenario::rect_of("dialog:ok") {
        crate::scenario::publish_handle("ai_add:add", rect.center());
    }
    add |= action == ui_kit::Action::Ok;
    open &= !closed && action != ui_kit::Action::Cancel;
    if test {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(s) = app.agent_chat.settings.add.as_ref() {
            let url = agents::normalize_url(&s.base_url);
            let e = agents::Entry {
                id: String::new(),
                name: s.name.clone(),
                kind: s.protocol.kind(),
                argv: Vec::new(),
                version: None,
                login: "",
                found: false,
                base_url: url.clone().unwrap_or_default(),
                model: s.model.trim().to_string(),
                key: false,
                request_timeout_s: s.request_timeout_s,
            };
            let key = Some(s.key.trim().to_string()).filter(|k| !k.is_empty());
            let result = match url {
                Ok(_) => start_test(app, &e, key),
                Err(err) => Test::Done(Err(TestError::Other(err))),
            };
            if let Some(s) = app.agent_chat.settings.add.as_mut() {
                s.test = Some(result);
            }
        }
    }
    if add {
        match add_agent(app) {
            Ok(()) => open = false,
            Err(e) => {
                if let Some(s) = app.agent_chat.settings.add.as_mut() {
                    s.error = Some(e);
                }
            }
        }
    }
    if !open {
        app.agent_chat.settings.add = None;
    }
    if app.agent_chat.settings.add.as_ref().is_some_and(|s| matches!(s.test, Some(Test::Running(_)))) {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

/// Add what the dialog holds; its key goes to the keychain.
fn add_agent(app: &mut SolveApp) -> Result<(), String> {
    let Some(s) = app.agent_chat.settings.add.as_ref() else { return Ok(()) };
    let name = s.name.trim().chars().take(80).collect::<String>();
    let mut c = AgentConfig { name, ..Default::default() };
    let mut key: Option<String> = None;
    match s.kind {
        AddKind::Installed | AddKind::Custom => {
            let cmd = s.command.trim();
            if acp::split_command(cmd).is_empty() {
                return Err(tr("ui.ai_need_command").to_string());
            }
            c.kind = Kind::Acp;
            c.command = cmd.chars().take(4096).collect();
            c.id = agents::new_id(&app.preferences, "custom");
        }
        AddKind::Api => {
            c.kind = s.protocol.kind();
            c.ollama = s.protocol == Protocol::OpenAi && s.preset == 0;
            c.base_url = agents::normalize_url(&s.base_url)?;
            c.model = s.model.trim().to_string();
            c.request_timeout_s = s.request_timeout_s.clamp(agents::MIN_REQUEST_TIMEOUT_S, agents::MAX_REQUEST_TIMEOUT_S);
            if c.model.is_empty() {
                return Err(tr("ui.ai_need_model").to_string());
            }
            key = Some(s.key.trim().to_string()).filter(|k| !k.is_empty());
            if key.is_some() && !agents::key_safe_for(&c.base_url) {
                return Err(tr("ui.ai_key_needs_https").to_string());
            }
            c.id = agents::new_id(&app.preferences, c.kind.id());
        }
    }
    if c.name.is_empty() {
        c.name = c.id.clone();
    }
    if let Some(k) = key {
        let Some(store) = &app.services.secrets else { return Err(tr("ui.ai_no_keychain").to_string()) };
        store.set(&c.id, &k).map_err(|e| trf("ui.ai_keychain_error", &[("error", &e)]))?;
        c.key = true;
    }
    app.preferences.agents.push(c);
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod card_tests {
    use super::*;
    use crate::scenario::{self, Harness};
    use serde_json::json;

    #[test]
    fn cached_model_warning_follows_endpoint_model_and_agent_selection() {
        let mut h = Harness::new();
        h.app.agent_chat.detection = Some(Default::default());
        h.app.preferences.agents.push(AgentConfig {
            id: "local".into(),
            kind: Kind::Api,
            base_url: "http://localhost:11434/v1".into(),
            model: "chat:4b".into(),
            ..Default::default()
        });
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::{Duration, Instant};
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let ui_thread = std::thread::current().id();
        h.app.services.agent_http = Some(Arc::new(move |request| {
            assert_ne!(std::thread::current().id(), ui_thread);
            count.fetch_add(1, Ordering::SeqCst);
            Ok((200, if request.method == "GET" { br#"{"version":"0.0"}"#.to_vec() } else { br#"{"capabilities":["completion"]}"#.to_vec() }))
        }));
        poll_capabilities(&mut h.app, &h.ctx);
        let deadline = Instant::now() + Duration::from_secs(2);
        while h.app.agent_chat.settings.checking.is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
            poll_capabilities(&mut h.app, &h.ctx);
        }
        assert!(h.app.agent_chat.settings.checking.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        poll_capabilities(&mut h.app, &h.ctx);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "cached selections do not probe again");
        assert_eq!(status(&h.app)["modelWarning"], capability_warning("chat:4b", Some(false)).unwrap());
        h.app.preferences.agents[0].model = "tools:8b".into();
        assert!(status(&h.app)["modelWarning"].is_null());
        h.app.preferences.agents[0].model = "chat:4b".into();
        h.app.preferences.agents[0].base_url = "http://localhost:1234/v1".into();
        assert!(status(&h.app)["modelWarning"].is_null());
        h.app.preferences.agents[0].base_url = "http://localhost:11434/v1".into();
        h.app.preferences.agents[0].kind = Kind::Acp;
        assert!(status(&h.app)["modelWarning"].is_null());
    }

    #[test]
    fn model_warning_only_for_explicitly_missing_tools() {
        assert_eq!(
            capability_warning("local:4b", Some(false)).as_deref(),
            Some("local:4b can't use SolidCraft's tools, so it can only chat. Pick a model with tool support.")
        );
        assert_eq!(capability_warning("local:4b", Some(true)), None);
        assert_eq!(capability_warning("local:4b", None), None);
    }

    #[test]
    fn add_agent_card_retains_kind_handles_draft_and_validation() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.ai_features = true;
        h.app.agent_chat.detection = Some(Default::default());
        open_add(&mut h.app);
        h.frames(4);
        h.step(&json!({"click":{"handle":"ai_add:ui.ai_add_custom"}})).unwrap();
        assert_eq!(h.app.agent_chat.settings.add.as_ref().unwrap().kind, AddKind::Custom);
        h.step(&json!({"click":{"handle":"ai_add:add"}})).unwrap();
        assert!(h.app.agent_chat.settings.add.as_ref().unwrap().error.is_some());
        let add = h.app.agent_chat.settings.add.as_mut().unwrap();
        add.name = "Local fixture".into();
        add.command = "solidcraft-fixture-agent --acp".into();
        h.frames(3);
        h.step(&json!({"click":{"handle":"ai_add:add"}})).unwrap();
        assert!(h.app.agent_chat.settings.add.is_none());
        assert!(h.app.preferences.agents.iter().any(|e| e.name == "Local fixture"));
        open_add(&mut h.app);
        h.frames(3);
        h.step(&json!({"click":{"handle":"ai_add:ui.ai_add_endpoint"}})).unwrap();
        assert!(scenario::handle_at("ai_add:key").is_some());
        assert!(h.app.services.secrets.is_none(), "this fixture has no keychain");
        h.step(&json!({"click":{"handle":"ai_add:protocol:anthropic"}})).unwrap();
        {
            let add = h.app.agent_chat.settings.add.as_ref().unwrap();
            assert_eq!(add.protocol, Protocol::Anthropic);
            assert_eq!(add.base_url, "https://api.anthropic.com/v1");
            assert_eq!(add.name, "Anthropic");
        }
        h.app.agent_chat.settings.add.as_mut().unwrap().model = "claude-opus-5-5".into();
        h.frames(3);
        h.step(&json!({"click":{"handle":"ai_add:add"}})).unwrap();
        assert!(h.app.agent_chat.settings.add.is_none());
        let saved = h.app.preferences.agents.last().unwrap();
        assert_eq!(saved.kind, Kind::Anthropic);
        assert_eq!(saved.base_url, "https://api.anthropic.com/v1");
        assert_eq!(saved.model, "claude-opus-5-5");
    }
}
