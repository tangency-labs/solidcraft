//! Export DXF… on a sketch: a small window that asks which DXF version to write, AutoCAD 2000
//! (every curve exact) or release 12 (lines, arcs and circles), before the file picker. The
//! version used last is remembered for the next export (#5321).
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use serde_json::json;

/// Open the window for `sketch`, on the version used last. A window that is already open is
/// cancelled first, so its preview is put back.
pub fn open(app: &mut SolveApp, sketch: u64) {
    if app.dialog.is_some() {
        crate::dialogs::cancel(app);
    }
    let r12 = app.preferences.sketch_dxf_version == "r12";
    app.dialog = Some(Dialog::new(Kind::SketchDxf { sketch, r12 }, Vec::new()));
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, r12: &mut bool) {
    crate::ui_kit::text(ui, app.tr("dxf.sketch_version"), crate::ui_kit::Text::Note("version"));
    for (value, key, label) in [(false, "ac1015", "dxf.sketch_ac1015"), (true, "r12", "dxf.sketch_r12")] {
        let r = crate::ui_kit::list_picker(ui, key, app.tr(label), *r12 == value);
        crate::scenario::publish_handle(&format!("dialog:dxf:{key}"), r.rect.center());
        if r.clicked() {
            *r12 = value;
        }
    }
}

/// Remember the version, then ask where to write the file. A cancelled picker writes nothing.
pub fn run(app: &mut SolveApp, sketch: u64, r12: bool) -> Result<(), String> {
    let version = if r12 { "r12" } else { "ac1015" };
    app.preferences.sketch_dxf_version = version.into();
    let name = app.session.doc.feature(sketch).map(|f| f.name.clone()).unwrap_or_default();
    app.pick_save_command(&format!("{name}.dxf"), &["dxf"], "sketch.export_dxf", json!({"sketch": sketch, "version": version}));
    Ok(())
}
