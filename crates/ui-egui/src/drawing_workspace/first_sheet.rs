//! Drawing's entry choices before a sheet is open (D19).

use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::ui_kit::{self, BackingTooltipExt as _, Button, Container, Surface};

const SIZES: &[&str] = &["A4", "A3", "A2", "A1", "A0"];

pub(super) struct BlankSheet {
    size: usize,
    title_block: bool,
}

impl BlankSheet {
    fn new(app: &SolveApp) -> Self {
        Self { size: if solidcraft_engine::doc::drawing::template::is_assembly(&app.session.doc) { 2 } else { 1 }, title_block: true }
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
    let width = room.width().min(750.0);
    let columns = width >= 590.0;
    let top = room.top() + if columns { (room.height() - 300.0).max(0.0) * 0.5 } else { 0.0 };
    let bounds = Rect::from_min_max(pos2(room.center().x - width * 0.5, top), pos2(room.center().x + width * 0.5, room.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bounds), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(bounds));
        let container = Container::new(ui.ctx(), Surface::Panel);
        let frame = container.edge_frame(t.panel).inner_margin(t.space_16);
        container.show_frame(ui, frame, bounds, |ui| {
            egui::ScrollArea::vertical().id_salt("drawing_first_sheet").min_scrolled_height(0.0).show(ui, |ui| {
                ui.set_width((width - t.space_16 * 2.0).max(0.0));
                let title = solidcraft_ui_text::format("ui.drawing_first.title", &[("name", &app.session.doc.name)]);
                ui.vertical_centered(|ui| ui.heading(title));
                if app.session.doc.features.is_empty() {
                    ui.vertical_centered(|ui| ui.label(tr("ui.drawing_first.empty_model")));
                }
                ui.add_space(t.space_12);
                let enabled = can_create(app);
                let picker = app.services.pick_open.is_some();
                let options = [Choice::Standard, Choice::Blank, Choice::Template];
                let chosen = if columns {
                    ui.columns(3, |uis| options.into_iter().zip(uis).fold(None, |chosen, (option, ui)| card(ui, option, enabled, picker).or(chosen)))
                } else {
                    options.into_iter().fold(None, |chosen, option| card(ui, option, enabled, picker).or(chosen))
                };
                match chosen {
                    Some(Choice::Blank) => app.drawing_workspace.first_sheet = Some(BlankSheet::new(app)),
                    Some(Choice::Template) => choose_template(app),
                    _ => {}
                }
            });
        });
    });
}

fn card(ui: &mut egui::Ui, choice: Choice, enabled: bool, picker: bool) -> Option<Choice> {
    let t = Tokens::get();
    let (title, description, action, enabled, target) = match choice {
        Choice::Standard => ("ui.drawing_first.standard", "ui.drawing_first.standard_description", "ui.drawing_first.make", false, "standard"),
        Choice::Blank => ("ui.drawing_first.blank", "ui.drawing_first.blank_description", "ui.drawing_first.choose_sheet", enabled, "blank"),
        Choice::Template => {
            ("ui.drawing_first.template", "ui.drawing_first.template_description", "ui.drawing_first.choose_template", enabled && picker, "template")
        }
    };
    let container = Container::new(ui.ctx(), Surface::Panel);
    let frame = container.edge_frame(t.field).inner_margin(t.space_8);
    let bounds = ui.available_rect_before_wrap();
    container
        .show_frame(ui, frame, bounds, |ui| {
            thumbnail(ui, &choice);
            ui.heading(tr(title));
            ui.label(tr(description));
            let button = ui_kit::button(ui, tr(action), Button::Enabled(enabled));
            crate::scenario::publish_target(&format!("drawing:first:{target}"), button.rect);
            button.backing_on_disabled_hover_text(tr("ui.drawing_first.unavailable")).clicked().then_some(choice)
        })
        .inner
}

fn thumbnail(ui: &mut egui::Ui, choice: &Choice) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_24 * 3.0), Sense::hover());
    let width = (rect.width() - t.space_16).clamp(0.0, t.space_24 * 4.0);
    if width <= 0.0 {
        return;
    }
    let paper = Rect::from_center_size(rect.center(), vec2(width, width * 0.7));
    let line = Stroke::new(1.0, t.paper_ink);
    ui.painter().rect(paper, 0.0, t.paper_white, line, egui::StrokeKind::Inside);
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
        .fields()
        .actions(tr("cmd.drawing_create"), tr("ui.cancel"), None)
        .show(ctx, can_create(app), |ui| {
            ui_kit::choice_row(ui, tr("ui.drawing_panel.size"), "drawing_first_size", &mut form.size, SIZES);
            ui_kit::row(ui, tr("ui.drawing_panel.title_block"), |ui| {
                ui_kit::checkbox(ui, tr("ui.drawing_panel.built_in"), &mut form.title_block);
            });
        });
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    if action == ui_kit::Action::Ok
        && can_create(app)
        && let Some(size) = SIZES.get(form.size)
    {
        let kind = if solidcraft_engine::doc::drawing::template::is_assembly(&app.session.doc) { "assembly" } else { "part" };
        let template = format!("iso-{}-{kind}", size.to_ascii_lowercase());
        if create(app, json!({"template":template,"omit_first_sheet_title_block":!form.title_block})) {
            return;
        }
    }
    app.drawing_workspace.first_sheet = Some(form);
}

fn create(app: &mut SolveApp, params: Value) -> bool {
    match app.run("drawing.create", params) {
        Ok(_) => {
            crate::drawing_sheet_page::fit(app);
            true
        }
        Err(error) => {
            app.set_status(error, true);
            false
        }
    }
}
