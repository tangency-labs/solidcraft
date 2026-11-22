//! Drawing's entry choices before a sheet is open (D19).

use egui::{Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::drawing_sheet::{DrawingBorderProfile, Orientation, PaperSize, Sheet, SheetSize};
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::ui_kit::{self, BackingTooltipExt as _, Container, Surface, type_face::RichTextExt as _};

use super::panels::{self, PaperChoice};

pub(super) struct BlankSheet {
    paper: PaperChoice,
    name: String,
    title_block: usize,
}

impl BlankSheet {
    fn new(app: &SolveApp) -> Self {
        let size = if solidcraft_engine::doc::drawing::template::is_assembly(&app.session.doc) { PaperSize::A2 } else { PaperSize::A3 };
        Self {
            paper: PaperChoice::new(Sheet { size: SheetSize::Standard(size), orientation: Orientation::Landscape }),
            name: String::new(),
            title_block: 0,
        }
    }
}

enum Choice {
    Standard,
    Blank,
    Template,
}

fn tr(key: &str) -> &str {
    solidcraft_ui_text::tr(key)
}

fn can_create(app: &SolveApp) -> bool {
    app.session.open_drawing.is_none() && !app.session.doc.features.is_empty() && !app.file_picker_busy()
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, bounds: Rect) {
    let t = Tokens::get();
    let room = bounds.shrink(t.space_16);
    if !room.is_positive() {
        return;
    }
    let width = room.width().min(830.0);
    let columns = width >= 590.0;
    let top = room.top() + if columns { (room.height() - 345.0).max(0.0) * 0.5 } else { 0.0 };
    let bounds = Rect::from_min_max(pos2(room.center().x - width * 0.5, top), pos2(room.center().x + width * 0.5, room.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bounds), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(bounds));
        let container = Container::new(ui.ctx(), Surface::Panel);
        let frame = container.edge_frame(t.dialog_body).corner_radius(t.radius_lg).stroke(Stroke::new(1.0, t.dialog_border)).inner_margin(t.space_24);
        container.show_frame(ui, frame, bounds, |ui| {
            egui::ScrollArea::vertical().id_salt("drawing_first_sheet").min_scrolled_height(0.0).show(ui, |ui| {
                ui.set_width((width - t.space_24 * 2.0).max(0.0));
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(tr("ui.drawing_first.title")).size(t.font_page_heading).semibold().color(t.form_ink));
                    let subtitle = if app.session.doc.features.is_empty() {
                        tr("ui.drawing_first.empty_model").to_owned()
                    } else {
                        solidcraft_ui_text::format("ui.drawing_first.subtitle", &[("name", &app.session.doc.name)])
                    };
                    ui.label(RichText::new(subtitle).size(t.font_heading).color(t.form_dim));
                });
                ui.add_space(t.space_16);
                let enabled = can_create(app);
                let picker = app.services.pick_open.is_some();
                let options = [Choice::Standard, Choice::Blank, Choice::Template];
                let chosen = if columns {
                    ui.columns(3, |uis| options.into_iter().zip(uis).fold(None, |chosen, (option, ui)| card(ui, option, enabled, picker).or(chosen)))
                } else {
                    options.into_iter().fold(None, |chosen, option| card(ui, option, enabled, picker).or(chosen))
                };
                let blank = BlankSheet::new(app);
                if enabled && let Some(Sheet { size: SheetSize::Standard(size), .. }) = blank.paper.selected() {
                    ui.add_space(t.space_12);
                    let paper = super::panels::size_caption(size);
                    let defaults = solidcraft_ui_text::format("ui.drawing_first.defaults", &[("paper", &paper)]);
                    ui.vertical_centered(|ui| ui.label(RichText::new(defaults).size(t.font_caption).color(t.form_dim)));
                }
                match chosen {
                    Some(Choice::Standard) => {
                        create(app, json!({"standard_views": true}));
                    }
                    Some(Choice::Blank) => app.drawing_workspace.first_sheet = Some(blank),
                    Some(Choice::Template) => choose_template(app),
                    None => {}
                }
            });
        });
    });
}

fn card(ui: &mut egui::Ui, choice: Choice, enabled: bool, picker: bool) -> Option<Choice> {
    let t = Tokens::get();
    let (title, description, action, enabled, target) = match choice {
        Choice::Standard => ("ui.drawing_first.standard", "ui.drawing_first.standard_description", "ui.drawing_first.make", enabled, "standard"),
        Choice::Blank => ("ui.drawing_first.blank", "ui.drawing_first.blank_description", "ui.drawing_first.choose_sheet", enabled, "blank"),
        Choice::Template => {
            ("ui.drawing_first.template", "ui.drawing_first.template_description", "ui.drawing_first.choose_template", enabled && picker, "template")
        }
    };
    let container = Container::new(ui.ctx(), Surface::Panel);
    let frame = container.edge_frame(t.panel).corner_radius(t.radius_md).stroke(Stroke::new(1.0, t.border)).inner_margin(t.space_12);
    let bounds = ui.available_rect_before_wrap();
    container
        .show_frame(ui, frame, bounds, |ui| {
            thumbnail(ui, &choice);
            ui.add_space(t.space_8);
            ui.label(RichText::new(tr(title)).size(t.font_subtitle).medium().color(t.form_ink));
            ui.label(RichText::new(tr(description)).size(t.font_label).color(t.form_dim));
            ui.add_space(t.space_8);
            let id = format!("drawing:first:{target}");
            let button = ui_kit::wide_form_button(ui, tr(action), matches!(choice, Choice::Standard), enabled);
            crate::scenario::publish_target(&id, button.rect);
            button.backing_on_disabled_hover_text(tr("ui.drawing_first.unavailable")).clicked().then_some(choice)
        })
        .inner
}

fn thumbnail(ui: &mut egui::Ui, choice: &Choice) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_24 * 4.0), Sense::hover());
    ui.painter().rect_filled(rect, t.radius_sm, t.pref_well);
    let width = (rect.width() - t.space_16).clamp(0.0, t.space_24 * 5.0);
    if width <= 0.0 {
        return;
    }
    let paper = Rect::from_center_size(rect.center(), vec2(width, width * 0.7));
    let line = Stroke::new(1.0, t.figure_line);
    ui.painter().rect(paper, t.radius_sm, t.paper_white, line, egui::StrokeKind::Inside);
    let content = paper.shrink(width * 0.10);
    match choice {
        Choice::Standard => {
            for (x, y) in [(0.15, 0.05), (0.15, 0.57), (0.6, 0.57)] {
                let view = Rect::from_min_size(content.min + vec2(x * content.width(), y * content.height()), content.size() * 0.32);
                ui.painter().rect_stroke(view, 0.0, line, egui::StrokeKind::Inside);
            }
        }
        Choice::Blank | Choice::Template => {
            ui.painter().rect_stroke(content, 0.0, line, egui::StrokeKind::Inside);
            if matches!(choice, Choice::Template) {
                let block = Rect::from_min_max(content.max - content.size() * 0.30, content.max);
                ui.painter().rect_stroke(block, 0.0, line, egui::StrokeKind::Inside);
            }
        }
    }
}

fn choose_template(app: &mut SolveApp) {
    let uid = app.session.uid;
    app.pick_open_for(&["json"], move |app, path| {
        if super::active(app)
            && app.session.uid == uid
            && app.session.open_drawing.is_none()
            && let Some(path) = path
        {
            create(app, json!({"template":path}));
        }
    });
}

pub(super) fn show_picker(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut form) = app.drawing_workspace.first_sheet.take() else { return };
    if !super::active(app) || app.session.open_drawing.is_some() {
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_drawing_first_sheet"), "new", tr("ui.drawing_first.choose_sheet"))
        .cards(2.5)
        .fields()
        .actions(tr("cmd.drawing_create"), tr("ui.cancel"), None)
        .show(ctx, can_create(app) && panels::valid_sheet_name(&form.name) && form.paper.selected().is_some(), |ui| {
            panels::sheet_columns(ui, &mut form.paper, |fields, paper| {
                panels::paper_preview(fields, paper, true, form.title_block == 0);
                ui_kit::text_row(fields, tr("ui.drawing_panel.name"), &mut form.name, "Sheet 1");
                panels::paper_border(fields, paper, Some(&DrawingBorderProfile::iso()));
                ui_kit::choice_row(
                    fields,
                    tr("ui.drawing_panel.title_block"),
                    "drawing_first_title_block",
                    &mut form.title_block,
                    &[tr("ui.drawing_panel.built_in"), tr("ui.drawing_panel.none")],
                );
            });
        });
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    if action == ui_kit::Action::Ok
        && can_create(app)
        && panels::valid_sheet_name(&form.name)
        && let Some(paper) = form.paper.selected()
    {
        let mut params = json!({
            "first_sheet_paper": paper,
            "omit_first_sheet_title_block": form.title_block != 0,
        });
        if !form.name.is_empty() {
            params["first_sheet_name"] = json!(form.name);
        }
        if create(app, params) {
            return;
        }
    }
    app.drawing_workspace.first_sheet = Some(form);
}

fn create(app: &mut SolveApp, params: Value) -> bool {
    match app.run("drawing.create", params) {
        Ok(_) => {
            crate::drawing_sheet_page::fit(app);
            if app.preferences.design_sub_tab_sheets {
                let owner = app
                    .session
                    .open_drawing
                    .and_then(|id| app.session.doc.drawing(id))
                    .and_then(|drawing| drawing.sheets.first().map(|sheet| (drawing.id, sheet.id)));
                if let Some((drawing, sheet)) = owner
                    && let Err(error) = crate::sheet_views::open_in(app, drawing, sheet, false)
                {
                    app.set_status(error, true);
                }
            }
            true
        }
        Err(error) => {
            app.set_status(error, true);
            false
        }
    }
}
