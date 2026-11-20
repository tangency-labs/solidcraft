//! Safe graphics mode (`SolveApp::safe_graphics`): the desktop host starts on the plain path after a
//! launch that failed to come up or lost its graphics device, or when Preferences › Graphics ›
//! Start in safe graphics mode asked for it (the host's `startup::SafeGraphics` keeps the record in
//! `graphics.json`'s `safe_graphics`). Here: the optional drawing it keeps off whatever the saved
//! settings say (they are kept for full graphics), the one line over the view that says so with a
//! Try full graphics button, and the Preferences row.

use serde_json::json;

use crate::SolveApp;

/// The record's key in `graphics.json`, shared with the desktop host.
const KEY: &str = "safe_graphics";

/// Ambient occlusion is drawn: on in the settings and not in safe graphics mode.
pub(crate) fn ambient_occlusion(app: &SolveApp) -> bool {
    app.preferences.ambient_occlusion && !app.safe_graphics
}

/// Dark parts shading (the sky fill) is drawn: on in the settings and not in safe graphics mode.
pub(crate) fn dark_parts_shading(app: &SolveApp) -> bool {
    app.preferences.dark_parts_shading && !app.safe_graphics
}

/// The studio material finish is drawn: on in the settings and not in safe graphics mode.
pub(crate) fn material_finish(app: &SolveApp) -> bool {
    app.preferences.viewport_material_finish && !app.safe_graphics
}

/// Blur behind panels gets the device ([`SolveApp::adopt_wgpu`]): not in safe graphics mode.
pub(crate) fn blur(app: &SolveApp) -> bool {
    !app.safe_graphics
}

/// The GPU path tracer's setup is offered to Render ([`SolveApp::adopt_wgpu`]): not in safe
/// graphics mode, where Render draws with its other image methods.
pub(crate) fn path_tracer(app: &SolveApp) -> bool {
    !app.safe_graphics
}

/// What the line over the view shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// "Started in safe graphics mode" with Try full graphics.
    Started,
    /// Try full graphics was clicked: the next start uses full graphics; Restart now.
    FullNext,
}

/// The line due this frame, if any.
pub(crate) fn line(app: &SolveApp) -> Option<Line> {
    if !app.safe_graphics || app.safe_graphics_dismissed {
        return None;
    }
    Some(if app.safe_graphics_full_next { Line::FullNext } else { Line::Started })
}

/// The line's text and its one button (none when the host cannot restart).
pub(crate) fn line_text(app: &SolveApp, line: Line) -> (&'static str, Option<&'static str>) {
    match line {
        Line::Started => (solidcraft_ui_text::tr("graphics.safe_mode_started"), Some(solidcraft_ui_text::tr("graphics.safe_mode_try_full"))),
        Line::FullNext => (
            solidcraft_ui_text::tr("graphics.safe_mode_full_next"),
            app.services.graphics_restart.is_some().then(|| solidcraft_ui_text::tr("graphics.safe_mode_restart")),
        ),
    }
}

/// The line's button was clicked.
pub(crate) fn act(app: &mut SolveApp, line: Line) {
    match line {
        Line::Started => match try_full_graphics(app) {
            Ok(()) => app.safe_graphics_full_next = true,
            Err(error) => app.set_status(error, true),
        },
        Line::FullNext => {
            if let Err(error) = crate::prefs::request_graphics_restart(app) {
                app.set_status(error, true);
            }
        }
    }
}

/// Try full graphics: the next start uses full graphics, whatever kept this one safe. The
/// failures before stay counted until a full launch proves itself, so one more failed full
/// launch makes safe graphics stick again.
pub fn try_full_graphics(app: &mut SolveApp) -> Result<(), String> {
    edit(app, |record| {
        record.insert("sticky".into(), json!(false));
        record.insert("next".into(), json!(false));
    })?;
    app.safe_graphics_next = false;
    Ok(())
}

/// Preferences › Graphics › Start in safe graphics mode: the next launch only.
pub fn set_next_launch(app: &mut SolveApp, on: bool) -> Result<(), String> {
    edit(app, |record| {
        record.insert("next".into(), json!(on));
    })?;
    app.safe_graphics_next = on;
    Ok(())
}

/// Change the record in `graphics.json`, keeping every other setting and field as they were.
fn edit(app: &SolveApp, change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Result<(), String> {
    let path = app.services.graphics_path.as_deref().ok_or_else(|| app.tr("prefs.graphics_api_unavailable").to_string())?;
    let path = std::path::Path::new(path);
    let mut graphics = crate::prefs::read_graphics_settings(path)?;
    let Some(object) = graphics.as_object_mut() else { return Err("graphics settings must be a JSON object".into()) };
    let mut record = object.get(KEY).and_then(serde_json::Value::as_object).cloned().unwrap_or_default();
    change(&mut record);
    object.insert(KEY.into(), serde_json::Value::Object(record));
    solidcraft_engine::io::safe_write::write_atomic(path, graphics.to_string().as_bytes(), false).map_err(|error| error.to_string())
}
