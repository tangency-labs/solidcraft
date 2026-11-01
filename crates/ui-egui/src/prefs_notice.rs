//! The notice for settings put back at start because they would keep SolidCraft from starting
//! on this computer (`SolveApp::settings_reset`), shown until dismissed; and the question about
//! graphics settings on trial (`prefs::Graphics`, #4015).

use crate::SolveApp;
use crate::ui_kit;

/// Seconds the question about graphics settings on trial waits before going back.
pub const TRIAL_SECONDS: f64 = 15.0;
/// The most one frame adds to that wait, so a window that drew nothing for a while (hidden)
/// doesn't give up the moment it draws again, while a hung GPU's frames, a second apart, count.
const TRIAL_FRAME_SECONDS: f64 = 1.5;

/// Draw the notice while any setting was reset. OK dismisses it; "Open Preferences" dismisses it
/// and opens the Graphics page.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    graphics_trial(app, ctx);
    if app.settings_reset.is_empty() {
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_settings_reset"), "settings", app.tr("settings.reset_title"))
        .actions(app.tr("ui.open_preferences"), app.tr("ui.heading_ok"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, app.tr("settings.reset_intro"), ui_kit::Text::Report("intro", false));
            for (index, reset) in app.settings_reset.iter().enumerate() {
                ui_kit::text(ui, &format!("• {}", reset.message()), ui_kit::Text::Report(&format!("reset:{index}"), false));
            }
        });
    if closed || action != ui_kit::Action::None {
        app.settings_reset.clear();
    }
    if action == ui_kit::Action::Ok {
        crate::prefs::open_at(app, crate::prefs::PAGE_GRAPHICS);
    }
}

/// While this start tries graphics settings: Keep saves them; Go back (or the window's close)
/// returns to the saved ones and starts again. Unanswered for [`TRIAL_SECONDS`], it gives up
/// and closes at once (unsaved changes stay for recovery, as after a crash), for a window the
/// new settings left white or black can't show the question or ask about unsaved designs. The
/// next start finds the trial unkept and goes back, saying so.
pub fn graphics_trial(app: &mut SolveApp, ctx: &egui::Context) {
    // A restart whose quit the user cancelled (Cancel at "save changes?") is off.
    if app.graphics.restart && app.docs.quitting.is_none() && !app.quit_requested {
        app.graphics.restart = false;
    }
    let now = ctx.input(|i| i.time);
    let Some(trial) = &mut app.graphics.trial else { return };
    if let Some(last) = trial.last_frame {
        trial.shown += (now - last).clamp(0.0, TRIAL_FRAME_SECONDS);
    }
    trial.last_frame = Some(now);
    let left = TRIAL_SECONDS - trial.shown;
    if left <= 0.0 {
        log::warn!("graphics: the settings on trial were not kept in {TRIAL_SECONDS} s; starting again with the saved ones");
        eprintln!("SolidCraft graphics: the settings on trial weren't kept in {TRIAL_SECONDS} s; starting again");
        app.graphics.trial = None;
        app.graphics.restart = true;
        app.quit_requested = true;
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    let previous = trial.previous.clone();
    let current = crate::prefs::selected_graphics(app);
    let body = crate::i18n::format(
        "settings.graphics_trial_body",
        &[("settings", &current.describe(&previous)), ("previous", &previous.describe(&current)), ("seconds", &format!("{}", left.ceil()))],
    );
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_graphics_trial"), "settings", app.tr("settings.graphics_trial_title"))
        .actions(app.tr("settings.graphics_trial_keep"), app.tr("settings.graphics_trial_revert"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &body, ui_kit::Text::Report("graphics_trial", false));
        });
    match action {
        ui_kit::Action::Ok => crate::prefs::keep_graphics(app),
        ui_kit::Action::Cancel => crate::prefs::revert_graphics(app),
        _ if closed => crate::prefs::revert_graphics(app),
        _ => {}
    }
}
