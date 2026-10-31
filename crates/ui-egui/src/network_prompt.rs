//! Files a design names on another computer: a linked design on `\\host\share`. On Windows,
//! reading such a file, or only asking whether it exists, connects to that computer and may send
//! it the user's Windows sign-in, so the engine refuses those paths until the server is allowed
//! (`vfs::design_path_refusal`). Update Links and Open Linked Design ask first, naming the
//! server: Allow once, Always allow this server (Preferences › Design), or Cancel.
//!
//! Only these user actions ask. Commands run programmatically or by an agent never prompt: they
//! get the engine's error, and `component.links` reports such a link's `found` as unknown.

use serde_json::Value;

use crate::SolveApp;
use crate::ui_kit;

/// What runs once the servers are allowed.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// Update Links (`component.update_links`, off the window thread).
    UpdateLinks,
    /// Open Linked Design: open this file.
    OpenLinked(String),
}

/// A user action waiting for its network servers to be allowed.
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkPrompt {
    /// The servers, as the design names them.
    pub hosts: Vec<String>,
    pub then: Then,
}

/// The answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Once,
    Always,
    Cancel,
}

/// The servers `component.links` reported as not allowed (each once).
pub(crate) fn blocked_hosts<'a>(links: impl IntoIterator<Item = &'a Value>) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for host in links.into_iter().filter(|l| l["found"].is_null()).filter_map(|l| l["host"].as_str()) {
        if !hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            hosts.push(host.to_string());
        }
    }
    hosts
}

/// Update Links from the toolbar: ask first when a link is on a server not allowed yet.
pub(crate) fn update_links(app: &mut SolveApp) {
    let hosts = match app.session.execute("component.links", &serde_json::json!({})) {
        Ok(links) => blocked_hosts(links["links"].as_array().into_iter().flatten()),
        Err(_) => Vec::new(),
    };
    if hosts.is_empty() {
        run(app, Vec::new(), Then::UpdateLinks);
    } else {
        app.network_prompt = Some(NetworkPrompt { hosts, then: Then::UpdateLinks });
    }
}

/// Open Linked Design for a link `component.links` reported (`link`).
pub(crate) fn open_linked(app: &mut SolveApp, link: &Value) {
    let Some(file) = link["file"].as_str() else {
        app.set_status("The linked design has no file path", true);
        return;
    };
    if link["found"].is_null() {
        match link["host"].as_str() {
            Some(host) => app.network_prompt = Some(NetworkPrompt { hosts: vec![host.to_string()], then: Then::OpenLinked(file.to_string()) }),
            None => app.set_status(format!("{file}: {}", link["reason"].as_str().unwrap_or("not a file")), true),
        }
        return;
    }
    if link["found"] != true {
        app.set_status(format!("Linked design not found: {file}"), true);
        return;
    }
    app.open_path_async(file);
}

/// Run the action with `hosts` allowed for it alone.
fn run(app: &mut SolveApp, hosts: Vec<String>, then: Then) {
    match then {
        Then::UpdateLinks => {
            let mut request = crate::file_load::Request::new("", crate::file_load::Kind::UpdateLinks);
            request.allow_hosts = hosts;
            let _ = crate::file_load::begin_request(app, request);
        }
        // Opening a file reads only it, as File › Open does; the design it opens is checked anew.
        Then::OpenLinked(file) => app.open_path_async(&file),
    }
}

/// Act on the answer to the waiting question.
pub fn answer(app: &mut SolveApp, answer: Answer) {
    let Some(NetworkPrompt { hosts, then }) = app.network_prompt.take() else { return };
    match answer {
        Answer::Cancel => {}
        Answer::Once => run(app, hosts, then),
        Answer::Always => {
            for host in &hosts {
                if !app.preferences.network_hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
                    app.preferences.network_hosts.push(host.clone());
                }
            }
            crate::prefs::apply_network(&app.preferences);
            run(app, hosts, then)
        }
    }
}

/// The question, when one is waiting.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(prompt) = app.network_prompt.as_ref() else { return };
    let _language = crate::i18n::scope(&app.preferences.language);
    let hosts = prompt.hosts.iter().map(|h| format!("\\\\{h}")).collect::<Vec<_>>().join(", ");
    let detail = crate::i18n::format("network.prompt_detail", &[("host", &hosts)]);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_network_prompt"), "status_warning", crate::i18n::tr("network.prompt_title"))
        .tone(ui_kit::Tone::Warning)
        .actions(crate::i18n::tr("network.allow_once"), crate::i18n::tr("ui.cancel"), Some(crate::i18n::tr("network.always_allow")))
        .selected(ui_kit::Action::Cancel)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Report("network_prompt", false));
        });
    let mut chosen = match action {
        ui_kit::Action::Ok => Some(Answer::Once),
        ui_kit::Action::Apply => Some(Answer::Always),
        ui_kit::Action::Cancel => Some(Answer::Cancel),
        ui_kit::Action::None if closed => Some(Answer::Cancel),
        ui_kit::Action::None => None,
    };
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        chosen = Some(Answer::Cancel);
        app.esc_handled = true;
    }
    if let Some(a) = chosen {
        answer(app, a);
    }
}
