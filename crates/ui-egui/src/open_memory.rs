//! A STEP file that may need more memory than the machine can spare (#15588): opening or
//! inserting it from the window asks first, naming the memory it may need and the memory there
//! is, and "Open Anyway" reads it. SolidCraft never refuses a file the person chose to open.
//!
//! Only interactive actions ask. A control-channel or script `doc.open` or `file.insert_step` is
//! refused with that memory named until it passes `force: true`.

use serde_json::json;

use crate::SolveApp;
use crate::file_load::{Kind, Request};
use crate::ui_kit;

/// An Open or Insert waiting for "this file may need more memory" to be answered.
pub struct MemoryPrompt {
    request: Request,
    /// The memory reading it may need, in bytes.
    needed: u64,
    /// The memory SolidCraft may take for it on this machine, in bytes.
    available: u64,
}

/// Before an interactive Open or Insert of a STEP file: when it may need more memory than the
/// machine can spare, hold the request and ask (`None`); otherwise hand it back to be read.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn check_open(app: &mut SolveApp, request: Request) -> Option<Request> {
    if request.reply.is_some() || !matches!(request.kind, Kind::Open | Kind::Insert) || request.options.get("force").is_some() {
        return Some(request);
    }
    let size = solidcraft_engine::io::vfs::scoped(request.scope.clone(), || solidcraft_engine::io::vfs::len(&request.path));
    // A file over the size limit is refused for that, so there is nothing to ask.
    let size = size.ok().filter(|&size| solidcraft_engine::io::limits::check(solidcraft_engine::io::limits::Kind::Step, size).is_ok());
    let Some((needed, available)) = size.and_then(|size| solidcraft_engine::io::limits::memory_short(&request.path, size)) else {
        return Some(request);
    };
    app.memory_prompts.push(MemoryPrompt { request, needed, available });
    None
}

/// The file name a prompt names.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// The question, when one is waiting.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let Some(prompt) = app.memory_prompts.first() else { return };
    let name = file_name(&prompt.request.path);
    let describe = solidcraft_engine::io::limits::describe;
    let title = solidcraft_ui_text::format("document.needs_memory", &[("name", &name)]);
    let detail = solidcraft_ui_text::format(
        "document.needs_memory_detail",
        &[("name", &name), ("needed", &describe(prompt.needed)), ("available", &describe(prompt.available))],
    );
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_needs_memory"), "status_warning", &title)
        .tone(ui_kit::Tone::Warning)
        .actions(solidcraft_ui_text::tr("document.open_anyway"), solidcraft_ui_text::tr("ui.cancel"), None)
        .selected(ui_kit::Action::Cancel)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Report("needs_memory", false));
        });
    let mut answer = match action {
        ui_kit::Action::Ok => Some(true),
        ui_kit::Action::Cancel => Some(false),
        ui_kit::Action::None if closed => Some(false),
        _ => None,
    };
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
        app.esc_handled = true;
    }
    if let Some(anyway) = answer {
        answer_prompt(app, anyway);
    }
}

/// Act on the first waiting question: `anyway` reads the file whatever memory it may need,
/// otherwise nothing is read.
pub fn answer_prompt(app: &mut SolveApp, anyway: bool) {
    if app.memory_prompts.is_empty() {
        return;
    }
    let MemoryPrompt { mut request, .. } = app.memory_prompts.remove(0);
    if !anyway {
        return;
    }
    request.options.insert("force".into(), json!(true));
    // A design open in another SolidCraft still asks about that (#5467).
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(request) = crate::open_elsewhere::check_open(app, request) {
            let _ = crate::file_load::queue(app, request);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = crate::file_load::begin_request(app, request);
    }
}
