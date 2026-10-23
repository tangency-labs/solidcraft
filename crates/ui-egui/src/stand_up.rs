//! Standing up a Y-up file (#3). Many CAD programs save parts with Y up, so a STEP or IGES file
//! from them opens lying on its back in a Z-up design. After such a file opens, SolidCraft
//! offers to stand it up: a quarter turn about X (Y → Z) of the component the file came in as
//! (#1386), so an assembly's parts and instances turn together, in one step that can be undone. Preferences › Design › "Offer to stand up Y-up
//! files" turns the offer off; with Y up as the default orientation it isn't needed. When the
//! file's header names an exporter known to save Y up (SolidWorks), Preferences › Design ›
//! "Stand up Y-up files automatically" stands it up without asking, with Undo in the message.

use serde_json::json;

use crate::SolveApp;
use crate::ui_kit;

/// The bodies a just-opened file brought (`before`: body names before it opened, none for a
/// new design), and the import's component placement from the command's `result`: offer to
/// stand them up.
pub fn offer(app: &mut SolveApp, path: &str, before: &[String], result: Option<&serde_json::Value>) {
    let io = &solidcraft_engine::io::is_step_path;
    if !(io(path) || solidcraft_engine::io::is_iges_path(path)) || app.preferences.up_axis != "z" {
        return;
    }
    let text = |k: &str| result.and_then(|r| r.get(k)).and_then(serde_json::Value::as_str);
    // SolidCraft's own STEP exports already use Z up.
    if text("system") == Some("SolidCraft") || text("exporter_up") == Some("z") {
        return;
    }
    let bodies: Vec<String> = app.session.world_state().bodies.iter().map(|b| b.name.clone()).filter(|n| !before.contains(n)).collect();
    if bodies.is_empty() {
        return;
    }
    let occurrence = result.and_then(|r| r.get("occurrence")).and_then(serde_json::Value::as_u64);
    // A header that names a Y-up exporter: stand it up now, one undoable step, with Undo in the toast.
    if text("exporter_up") == Some("y") && app.preferences.auto_stand_up {
        let exporter = text("exporter").unwrap_or("a Y-up program").to_string();
        app.stand_up = Some(bodies);
        app.stand_up_occurrence = occurrence;
        match stand_up(app) {
            Ok(()) => {
                app.set_status(format!("Stood up (exported Y-up by {exporter}) \u{2014} Undo"), false);
                app.status_undo = true;
            }
            Err(e) => app.set_status(e, true),
        }
        return;
    }
    if !app.preferences.offer_stand_up {
        return;
    }
    app.stand_up = Some(bodies);
    app.stand_up_occurrence = occurrence;
}

/// Turn the offered bodies a quarter turn about X (Y up becomes Z up).
pub fn stand_up(app: &mut SolveApp) -> Result<(), String> {
    let Some(bodies) = app.stand_up.take() else { return Err("no file is waiting to be stood up".into()) };
    match app.stand_up_occurrence.take() {
        // The import's component turns about its parent's origin, contents and all.
        Some(occurrence) => app.run("occurrence.move", json!({ "occurrence": occurrence, "axis": [1, 0, 0], "angle": "90 deg" })),
        None => app.run("solid.move", json!({ "bodies": bodies, "axis": [1, 0, 0], "angle": "90 deg" })),
    }
    .map_err(|e| e.to_string())?;
    app.animate_view("home");
    Ok(())
}

/// Dismiss the offer and keep the file as it opened ("Leave it"); the preference stays on.
pub fn leave(app: &mut SolveApp) {
    app.stand_up = None;
    app.stand_up_occurrence = None;
}

/// The offer, under the toolbar.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.stand_up.is_none() {
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_stand_up"), "move", crate::i18n::tr_label("Stand this part up?"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 140.0))
        .actions("Stand it up", "Leave it", Some("Don't ask again"))
        .show(ctx, true, |ui| {
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Files from many CAD programs have Y up, so they open lying on their back here (Z up)."),
                ui_kit::Text::Report("why", false),
            );
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Stand it up: a quarter turn about X, as one Move feature you can undo."),
                ui_kit::Text::Report("how", false),
            );
            ui_kit::text(ui, crate::i18n::tr_label("Preferences › Design turns the offer back on"), ui_kit::Text::Note("never"));
        });
    for (handle, key) in [("stand_up:stand", "dialog:ok"), ("stand_up:never", "dialog:apply"), ("stand_up:leave", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    match action {
        ui_kit::Action::Ok => {
            if let Err(e) = stand_up(app) {
                app.set_status(e, true);
            }
        }
        ui_kit::Action::Cancel => leave(app),
        ui_kit::Action::Apply => {
            leave(app);
            app.preferences.offer_stand_up = false;
        }
        ui_kit::Action::None if closed => leave(app),
        ui_kit::Action::None => {}
    }
}
