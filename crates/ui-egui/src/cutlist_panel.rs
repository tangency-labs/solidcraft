//! A derived cut list, refreshed when the design changes; sizes follow display units.

use serde_json::json;
use solidcraft_engine::schema::cutlist::CutList;
use solidcraft_ui_text::{format, tr};

use crate::SolveApp;

#[derive(Default)]
pub struct CutListPanel {
    document: String,
    focus_pending: bool,
    cache: Option<(u64, CutList)>,
    error: Option<String>,
    sort: usize,
    descending: bool,
}

pub fn open(app: &mut SolveApp) {
    app.cutlist = Some(CutListPanel { document: app.session.document_id.clone(), focus_pending: true, ..Default::default() });
}

pub(crate) fn available(app: &SolveApp) -> bool {
    app.cutlist.as_ref().is_some_and(|panel| panel.document == app.session.document_id)
}

pub(crate) fn focus_pending(app: &SolveApp) -> bool {
    app.cutlist.as_ref().is_some_and(|panel| panel.document == app.session.document_id && panel.focus_pending)
}

pub(crate) fn set_focus_pending(app: &mut SolveApp, pending: bool) {
    if let Some(panel) = &mut app.cutlist
        && panel.document == app.session.document_id
    {
        panel.focus_pending = pending;
    }
}

pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut panel) = app.cutlist.take().filter(|panel| panel.document == app.session.document_id) else { return };
    let revision = app.session.revision;
    if panel.cache.as_ref().is_none_or(|(r, _)| *r != revision) {
        match app
            .session
            .execute("cutlist.parts", &json!({}))
            .and_then(|v| serde_json::from_value(v).map_err(|e| solidcraft_engine::EngineError::Other(e.to_string())))
        {
            Ok(list) => {
                panel.cache = Some((revision, list));
                panel.error = None;
            }
            Err(e) => {
                panel.cache = Some((revision, CutList::default()));
                panel.error = Some(e.to_string());
            }
        }
    }
    let mut list = panel.cache.as_ref().map(|(_, l)| l.clone()).unwrap_or_default();
    list.rows.sort_by(|a, b| {
        let order = match panel.sort {
            1 => a.count.cmp(&b.count),
            2 => a.stock.length.total_cmp(&b.stock.length),
            3 => a.stock.width.total_cmp(&b.stock.width),
            4 => a.stock.thickness.total_cmp(&b.stock.thickness),
            5 => a.material.cmp(&b.material),
            6 => a.grain.cmp(&b.grain),
            _ => a.name.cmp(&b.name),
        };
        let order = order.then_with(|| a.item.cmp(&b.item));
        if panel.descending { order.reverse() } else { order }
    });
    let units = app.session.doc.units.clone();
    let decimals = app.preferences.length_decimals;
    let mut export = None;
    let top = ui.cursor().top();
    let report = egui::ScrollArea::vertical().id_salt("cutlist-report").min_scrolled_height(0.0).max_height(360.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if crate::ui_kit::button(ui, tr("cutlist.csv"), crate::ui_kit::Button::Normal).clicked() {
                export = Some("csv");
            }
            if crate::ui_kit::button(ui, tr("cutlist.json"), crate::ui_kit::Button::Normal).clicked() {
                export = Some("json");
            }
        });
        if let Some(error) = &panel.error {
            ui.label(error);
        }
        egui::ScrollArea::both().max_height(360.0).show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::Grid::new("sc_cutlist_grid").num_columns(8).striped(true).show(ui, |ui| {
                for (i, id) in
                    ["cutlist.name", "cutlist.count", "cutlist.length", "cutlist.width", "cutlist.thickness", "cutlist.material", "cutlist.grain"]
                        .iter()
                        .enumerate()
                {
                    if crate::ui_kit::button(ui, tr(id), crate::ui_kit::Button::Normal).clicked() {
                        if panel.sort == i {
                            panel.descending = !panel.descending;
                        } else {
                            panel.sort = i;
                            panel.descending = false;
                        }
                    }
                }
                ui.label(tr("cutlist.accuracy"));
                ui.end_row();
                for row in &list.rows {
                    ui.label(&row.name);
                    ui.label(row.count.to_string());
                    for mm in [row.stock.length, row.stock.width, row.stock.thickness] {
                        ui.label(crate::prefs::format_length(mm, &units, decimals));
                    }
                    ui.label(if row.material.is_empty() { tr("cutlist.unassigned") } else { &row.material });
                    ui.label(&row.grain);
                    ui.label(tr(if row.stock.exact { "cutlist.exact" } else { "cutlist.approximate" }));
                    ui.end_row();
                }
            });
        });
        ui.separator();
        for total in &list.totals {
            let material = if total.material.is_empty() { tr("cutlist.unassigned") } else { &total.material };
            ui.label(format("cutlist.total", &[("material", material), ("count", &total.count.to_string())]));
        }
        if !list.excluded.is_empty() {
            ui.label(tr("cutlist.excluded"));
            for excluded in &list.excluded {
                let reason = exclusion_reason(&excluded.reason);
                ui.label(format("cutlist.exclusion", &[("body", &excluded.body), ("reason", reason)]));
            }
        }
    });
    if let Some(ext) = export {
        let document = panel.document.clone();
        app.pick_save(&format!("cut-list.{ext}"), &[ext], move |app, path| {
            if let Some(path) = path
                && app.session.document_id == document
            {
                let _ = app.run("cutlist.parts", json!({"format": ext, "path": path}));
            }
        });
    }
    let height = report.content_size.y.min(360.0);
    crate::panel_runtime::report_content_extent(ui, top, height, false);
    app.cutlist = Some(panel);
}

/// The reason `cutlist.parts` gives for leaving a body out, in the reader's language. The stock
/// measure's own error text is internal and untranslated; the command's JSON keeps it for scripts.
fn exclusion_reason(reason: &str) -> &'static str {
    match reason {
        "purchased" => tr("cutlist.purchased"),
        "reference" => tr("cutlist.reference"),
        _ if reason.starts_with("stock size could not be measured") => tr("cutlist.unmeasured"),
        _ => tr("cutlist.unsupported"),
    }
}

#[cfg(test)]
#[path = "cutlist_panel_tests.rs"]
mod tests;
