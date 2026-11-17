//! The Cutting Plan panel: a form for `cutlist.layout` and a read-only preview of its sheets.
use crate::SolveApp;
use serde_json::{Value, json};
use solidcraft_engine::schema::cutting_plan::{Layout, StockChoice};
use solidcraft_ui_text::{format, tr};

pub struct CuttingPlanPanel {
    document: String,
    focus_pending: bool,
    library: String,
    kerf: String,
    trim: String,
    selected: Vec<String>,
    plan: Option<Layout>,
    revision: u64,
    error: Option<String>,
    load: bool,
    initial: bool,
    choices: Vec<StockChoice>,
}

impl Default for CuttingPlanPanel {
    fn default() -> Self {
        Self {
            document: String::new(),
            focus_pending: true,
            library: String::new(),
            kerf: "3.2".into(),
            trim: "10".into(),
            selected: vec![],
            plan: None,
            revision: 0,
            error: None,
            load: true,
            initial: true,
            choices: vec![],
        }
    }
}

pub fn open(app: &mut SolveApp) {
    app.cutting_plan = Some(CuttingPlanPanel { document: app.session.document_id.clone(), ..CuttingPlanPanel::default() });
}

pub(crate) fn available(app: &SolveApp) -> bool {
    app.cutting_plan.as_ref().is_some_and(|panel| panel.document == app.session.document_id)
}

pub(crate) fn focus_pending(app: &SolveApp) -> bool {
    app.cutting_plan.as_ref().is_some_and(|panel| panel.document == app.session.document_id && panel.focus_pending)
}

pub(crate) fn set_focus_pending(app: &mut SolveApp, pending: bool) {
    if let Some(panel) = &mut app.cutting_plan
        && panel.document == app.session.document_id
    {
        panel.focus_pending = pending;
    }
}

fn params(panel: &CuttingPlanPanel) -> Result<Value, String> {
    let kerf = crate::ui_kit::parse_number(&panel.kerf).ok_or_else(|| tr("cutting_plan.valid_lengths").to_string())?;
    let trim = crate::ui_kit::parse_number(&panel.trim).ok_or_else(|| tr("cutting_plan.valid_lengths").to_string())?;
    let mut p = json!({"kerf":kerf,"trim":trim,"mode":"guillotine"});
    if !panel.library.trim().is_empty() {
        p["library"] = json!(panel.library.trim());
    }
    if !panel.load {
        p["stock"] = json!(panel.selected);
    }
    Ok(p)
}

pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut panel) = app.cutting_plan.take().filter(|panel| panel.document == app.session.document_id) else { return };
    if panel.revision != app.session.revision {
        panel.plan = None;
    }
    let mut run = panel.initial;
    panel.initial = false;
    let mut export = None;
    let mut browse = false;
    let top = ui.cursor().top();
    let report = egui::ScrollArea::vertical().id_salt("cutting-plan-report").min_scrolled_height(0.0).max_height(720.0).show(ui, |ui| {
        ui.label(tr("cutting_plan.units"));
        if crate::ui_kit::text_row(ui, tr("cutting_plan.library"), &mut panel.library, tr("cutting_plan.preset")).1 {
            panel.load = true;
            panel.plan = None;
        }
        if crate::ui_kit::button(ui, tr("cutting_plan.browse"), crate::ui_kit::Button::Normal).clicked() {
            browse = true;
        }
        let changed = crate::ui_kit::text_row(ui, tr("cutting_plan.kerf"), &mut panel.kerf, "mm").1
            | crate::ui_kit::text_row(ui, tr("cutting_plan.trim"), &mut panel.trim, "mm").1;
        if changed {
            panel.plan = None;
        }
        if crate::ui_kit::button(ui, tr("cutting_plan.run"), crate::ui_kit::Button::Normal).clicked() {
            run = true;
        }
        if let Some(error) = &panel.error {
            ui.label(error);
        }
        {
            ui.label(tr("cutting_plan.stock"));
            let mut changed = false;
            egui::ScrollArea::vertical().id_salt("sc_stock_choices").max_height(140.0).show(ui, |ui| {
                for stock in &panel.choices {
                    let mut selected = panel.selected.contains(&stock.id);
                    if crate::ui_kit::checkbox(ui, &stock.name, &mut selected).changed() {
                        if selected {
                            panel.selected.push(stock.id.clone());
                        } else {
                            panel.selected.retain(|id| id != &stock.id);
                        }
                        changed = true;
                    }
                }
            });
            if changed {
                panel.plan = None;
            }
        }
        if let Some(plan) = &panel.plan {
            ui.horizontal_wrapped(|ui| {
                for (label, ext) in [
                    ("cutting_plan.svg", "svg"),
                    ("cutting_plan.dxf", "dxf"),
                    ("cutlist.json", "json"),
                    ("cutlist.csv", "csv"),
                    ("cutting_plan.labels", "labels"),
                ] {
                    if crate::ui_kit::button(ui, tr(label), crate::ui_kit::Button::Normal).clicked() {
                        export = Some(ext);
                    }
                }
            });
            ui.vertical(|ui| {
                for purchase in &plan.to_buy {
                    let material = if purchase.material.is_empty() { tr("cutlist.unassigned") } else { &purchase.material };
                    ui.label(format(
                        "cutting_plan.buy",
                        &[("stock", &purchase.stock), ("material", material), ("count", &purchase.count.to_string())],
                    ));
                }
                for (i, sheet) in plan.sheets.iter().enumerate() {
                    ui.label(format(
                        "cutting_plan.sheet",
                        &[("number", &(i + 1).to_string()), ("stock", &sheet.stock), ("yield", &std::format!("{:.1}", sheet.yield_ratio * 100.0))],
                    ));
                    let t = solidcraft_theme::Tokens::get();
                    let scale = ((ui.available_width().min(620.0) - 2.0) / sheet.length as f32).min(1.0);
                    let (bounds, _) =
                        ui.allocate_exact_size(egui::vec2(sheet.length as f32 * scale, sheet.width as f32 * scale), egui::Sense::hover());
                    let rect = |x: f64, y: f64, l: f64, w: f64| {
                        egui::Rect::from_min_size(bounds.min + egui::vec2(x as f32, y as f32) * scale, egui::vec2(l as f32, w as f32) * scale)
                    };
                    let painter = ui.painter();
                    painter.rect_filled(bounds, 0.0, t.panel);
                    painter.rect_stroke(bounds, 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                    for offcut in &sheet.offcuts {
                        painter.rect_filled(rect(offcut.x, offcut.y, offcut.length, offcut.width), 0.0, t.accent_soft);
                    }
                    for p in &sheet.placements {
                        let r = rect(p.x, p.y, p.length, p.width);
                        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                        painter.text(r.center(), egui::Align2::CENTER_CENTER, &p.mark, egui::FontId::proportional(t.font_body), t.text);
                    }
                    for (n, cut) in sheet.cuts.iter().enumerate() {
                        let r = rect(cut.x, cut.y, cut.length, cut.width);
                        painter.rect_filled(r, 0.0, t.text_dim);
                        painter.text(
                            r.min + egui::vec2(3.0, 3.0),
                            egui::Align2::LEFT_TOP,
                            (n + 1).to_string(),
                            egui::FontId::proportional(t.font_micro),
                            t.text,
                        );
                    }
                    for placement in &sheet.placements {
                        ui.label(format(
                            "cutting_plan.piece",
                            &[("mark", &placement.mark), ("length", &placement.length.to_string()), ("width", &placement.width.to_string())],
                        ));
                    }
                    for offcut in &sheet.offcuts {
                        ui.label(format("cutting_plan.offcut", &[("length", &offcut.length.to_string()), ("width", &offcut.width.to_string())]));
                    }
                }
                for p in &plan.unplaced {
                    ui.label(format("cutting_plan.unplaced", &[("mark", &p.mark), ("reason", &p.reason)]));
                }
            });
        } else {
            ui.label(tr("cutting_plan.regenerate"));
        }
    });
    if run {
        match params(&panel)
            .and_then(|p| app.session.execute("cutlist.layout", &p).map_err(|e| e.to_string()))
            .and_then(|v| serde_json::from_value::<Layout>(v).map_err(|e| e.to_string()))
        {
            Ok(plan) => {
                if panel.load {
                    panel.selected = plan.available_stock.iter().map(|s| s.id.clone()).collect();
                }
                panel.choices = plan.available_stock.clone();
                panel.plan = Some(plan);
                panel.revision = app.session.revision;
                panel.error = None;
                panel.load = false;
            }
            // The stock choices are filled by the first run that succeeds: until then every size stays selected.
            Err(e) => panel.error = Some(e),
        }
    }
    if let Some(ext) = export {
        match params(&panel) {
            Ok(p) => {
                if ext == "labels" {
                    app.pick_save("workshop-labels.svg", &["svg"], move |app, path| {
                        if let Some(path) = path {
                            let mut p = p;
                            p["labels_path"] = json!(path);
                            let _ = app.run("cutlist.layout", p);
                        }
                    });
                } else {
                    app.pick_save_command(&std::format!("cutting-plan.{ext}"), &[ext], "cutlist.layout", p);
                }
            }
            Err(e) => panel.error = Some(e),
        }
    }
    let height = report.content_size.y.min(720.0);
    crate::panel_runtime::report_content_extent(ui, top, height, false);
    app.cutting_plan = Some(panel);
    if browse {
        app.pick_open_for(&["json"], |app, path| {
            if let (Some(panel), Some(path)) = (app.cutting_plan.as_mut(), path) {
                panel.library = path;
                panel.plan = None;
                panel.load = true;
            }
        });
    }
}

#[cfg(test)]
#[path = "cutting_plan_panel_tests.rs"]
mod tests;
