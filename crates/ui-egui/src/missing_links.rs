//! Parts not found on open (#6892, slice 4b): under Parts and assemblies, an assembly whose linked
//! parts are found neither at their link's path nor in a library folder opens with their last
//! bodies, and one window lists them, each with Locate… to point it at its file and Replace… to
//! choose a different one (`component.relink`). After one is found, the others are looked for in
//! that file's folder. An internal component (a part stored inside the design's file) that the
//! file no longer holds is listed too; the file chosen for it is stored in the design again
//! (`component.make_internal`), so it stays internal.
//! The engine only reports (`doc.open`'s `missing_links`); this window is the app's layer.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;

/// The window's state: the parts still missing, and those found in the last located file's
/// folder, waiting to be relinked together.
#[derive(Debug, Default, PartialEq)]
pub struct MissingLinks {
    /// Each part not found: its component, name and the file looked for.
    pub rows: Vec<Row>,
    /// Parts of `rows` whose file is in the folder of the file last located: component and file.
    pub found_nearby: Vec<(u64, String)>,
}

/// One part not found.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub component: u64,
    pub name: String,
    pub path: String,
    /// An internal component: the design's file no longer holds it, and it is not unpacked.
    pub internal: bool,
}

/// After an open: the window, when the reply names missing parts.
pub(crate) fn offer(app: &mut SolveApp, reply: &Value) {
    let rows: Vec<Row> = reply["missing_links"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(Row {
                component: m["component"].as_u64()?,
                name: m["name"].as_str()?.to_string(),
                path: m["path"].as_str()?.to_string(),
                // Omitted when false.
                internal: m["internal"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    app.missing_links = (!rows.is_empty()).then(|| MissingLinks { rows, found_nearby: Vec::new() });
}

/// Point the part of `component` at `path`, as Relink Source does; then look for the other
/// missing parts in that file's folder. Locate…, Replace… and "Relink more" all end here. An
/// internal component is then stored in the design again from that file, so it stays internal;
/// when that fails the relink is kept and the status line says why.
pub(crate) fn located(app: &mut SolveApp, component: u64, path: &str) {
    let internal = app.missing_links.as_ref().and_then(|m| m.rows.iter().find(|r| r.component == component)).filter(|r| r.internal).cloned();
    // An internal component: stored again from that file in the same undo step.
    let stored = internal.as_ref().map(|_| app.run("component.make_internal", json!({"component": component, "path": path})));
    if !matches!(stored, Some(Ok(_))) {
        if let Err(e) = app.run("component.relink", json!({"component": component, "path": path})) {
            app.set_status(e, true);
            return;
        }
        if let (Some(row), Some(Err(e))) = (&internal, &stored) {
            let text = solidcraft_ui_text::Locale::for_language(&app.preferences.language)
                .format("missing_links.not_internal", &[("name", &row.name), ("path", path), ("error", e)]);
            app.set_status(text, true);
        }
    }
    let Some(missing) = app.missing_links.as_mut() else { return };
    missing.rows.retain(|r| r.component != component);
    let folder = std::path::Path::new(path).parent().map(std::path::Path::to_path_buf);
    missing.found_nearby = missing
        .rows
        .iter()
        .filter_map(|r| {
            let candidate = folder.as_ref()?.join(std::path::Path::new(&r.path).file_name()?);
            candidate.is_file().then(|| (r.component, candidate.display().to_string()))
        })
        .collect();
    if missing.rows.is_empty() {
        app.missing_links = None;
    }
}

/// Relink every part found in the last located file's folder.
pub(crate) fn relink_nearby(app: &mut SolveApp) {
    let nearby = app.missing_links.as_mut().map(|m| std::mem::take(&mut m.found_nearby)).unwrap_or_default();
    for (component, path) in nearby {
        located(app, component, &path);
    }
}

/// Ask for the file of the part of `component`. Locate… and Replace… both open this picker: the
/// engine takes any design file for a relink, so they differ in what they say, not in what they do.
fn locate(app: &mut SolveApp, component: u64) {
    let mut extensions = solidcraft_engine::io::DESIGN_EXTS.to_vec();
    extensions.extend(["scprt", "scsht", "scasm"]);
    app.pick_open_for(&extensions, move |app, path| {
        if let Some(path) = path {
            located(app, component, &path);
        }
    });
}

/// The window, while parts are missing.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(missing) = app.missing_links.as_ref() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let nearby = solidcraft_ui_text::format("missing_links.relink_nearby", &[("count", &missing.found_nearby.len().to_string())]);
    let rows = missing.rows.clone();
    let mut asked: Option<u64> = None;
    let (closed, action) =
        ui_kit::Confirm::new(egui::Id::unique("sc_missing_links"), "status_warning", solidcraft_ui_text::tr("missing_links.title"))
            .tone(ui_kit::Tone::Warning)
            .actions(if missing.found_nearby.is_empty() { "" } else { &nearby }, solidcraft_ui_text::tr("missing_links.close"), None)
            .show(ctx, true, |ui| {
                ui_kit::text(ui, solidcraft_ui_text::tr("missing_links.detail"), ui_kit::Text::Report("missing_links", false));
                if rows.iter().any(|r| r.internal) {
                    ui_kit::text(ui, solidcraft_ui_text::tr("missing_links.internal_detail"), ui_kit::Text::Report("missing_links_internal", false));
                }
                for row in &rows {
                    ui.horizontal(|ui| {
                        for (label, tip) in
                            [("missing_links.locate", "missing_links.locate_tip"), ("missing_links.replace", "missing_links.replace_tip")]
                        {
                            let button = ui_kit::text_button(ui, solidcraft_ui_text::tr(label), true);
                            if button.backing_on_hover_text(solidcraft_ui_text::tr(tip)).clicked() {
                                asked = Some(row.component);
                            }
                        }
                        // "Pick from a design" (a part taken out of another design's file) goes here
                        // once the engine has a command for it.
                        let text = if row.internal {
                            solidcraft_ui_text::format("missing_links.internal_row", &[("name", &row.name)])
                        } else {
                            format!("{}: {}", row.name, row.path)
                        };
                        ui_kit::text(ui, &text, ui_kit::Text::Note("missing_links_row"));
                    });
                }
            });
    if let Some(component) = asked {
        locate(app, component);
    }
    match action {
        ui_kit::Action::Ok => relink_nearby(app),
        ui_kit::Action::Cancel => app.missing_links = None,
        _ if closed => app.missing_links = None,
        _ => {}
    }
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        app.missing_links = None;
        app.esc_handled = true;
    }
}
