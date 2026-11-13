//! Drawing's native layout anchor. The centre is the sheet page (`drawing_sheet_page`) once a
//! drawing is open; no drawing document or paper scene is fabricated here. Missing tools expose
//! only agreed options; source services keep their existing ownership.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use solidcore_record::Vec2;

use crate::drawing_sheet_page::{active_sheet, select_sheet};

use crate::SolveApp;
use crate::ui_kit::{self, Container, Edge, Surface};
use solidcraft_theme::Tokens;

#[derive(Default)]
pub struct State {
    previous_tab: Option<String>,
    pub settings_open: bool,
    options: Option<&'static str>,
    strip: Option<Rect>,
    strip_sizes: [Option<egui::Vec2>; 2],
    settings: Option<Rect>,
    /// The sheet page's camera, zoom and drawn marks: screen only, never saved.
    pub(crate) page: crate::drawing_sheet_page::Page,
}

pub fn active(app: &SolveApp) -> bool {
    matches!(app.ui.tab.as_str(), "DRAWING LAYOUT" | "DRAWING ANNOTATE" | "DRAWING PARTS" | "DRAWING REVIEW")
}

pub fn enter(app: &mut SolveApp, drawing: bool) -> Result<(), String> {
    if drawing {
        if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
            return Err("Finish the current operation before entering Drawing".into());
        }
        if !active(app) {
            app.drawing_workspace.previous_tab = Some(app.ui.tab.clone());
            app.ui.tab = "DRAWING LAYOUT".into();
        }
    } else if active(app) {
        app.ui.tab = app.drawing_workspace.previous_tab.take().unwrap_or_else(|| "MODEL".into());
        app.drawing_workspace.options = None;
        app.drawing_workspace.strip = None;
        app.drawing_workspace.settings = None;
    }
    app.ui.ribbon_context = None;
    app.cancel_view_animation();
    Ok(())
}

pub fn reset_document(app: &mut SolveApp) {
    if active(app) {
        app.ui.tab = "MODEL".into();
    }
    app.drawing_workspace = State::default();
}

pub fn can_open_options(id: &str) -> bool {
    matches!(id, "drawing.view.base.planned" | "drawing.view.projected.planned" | "drawing.dimension" | "drawing.note")
}

/// Dismiss the current Drawing inspector without touching source or drawing data.
pub fn close_panel(app: &mut SolveApp) -> bool {
    let open = app.drawing_workspace.settings_open || app.drawing_workspace.options.is_some();
    app.drawing_workspace.settings_open = false;
    app.drawing_workspace.options = None;
    app.drawing_workspace.settings = None;
    open
}

/// Toolbar clicks may inspect known options. This never executes a missing CAD command.
pub fn start(app: &mut SolveApp, id: &str) -> bool {
    if !active(app) || !can_open_options(id) {
        return false;
    }
    app.drawing_workspace.options = match id {
        "drawing.view.base.planned" => Some("Base View"),
        "drawing.view.projected.planned" => Some("Projected View"),
        "drawing.dimension" => Some("Dimension"),
        "drawing.note" => Some("Note"),
        _ => None,
    };
    app.drawing_workspace.settings_open = false;
    true
}

pub(crate) fn covered_rects(app: &SolveApp) -> Vec<Rect> {
    if !active(app) {
        return Vec::new();
    }
    [app.drawing_workspace.strip, app.drawing_workspace.settings].into_iter().flatten().collect()
}

pub(crate) fn settings_rect(app: &SolveApp) -> Option<Rect> {
    active(app).then_some(app.drawing_workspace.settings).flatten()
}

fn panel_width(app: &SolveApp) -> f32 {
    if app.preferences.drawing_panel_width.is_finite() { app.preferences.drawing_panel_width.clamp(240.0, 560.0) } else { 320.0 }
}

pub(crate) fn left_inset(app: &SolveApp) -> f32 {
    if active(app)
        && Container::for_layout(app.ui.layout, Surface::Panel).floating()
        && (app.drawing_workspace.settings_open || app.drawing_workspace.options.is_some())
        && app.preferences.drawing_panel_side == "left"
    {
        panel_width(app) + crate::ui_kit::SHELL_GAP
    } else {
        0.0
    }
}

/// Reuse Browser's existing content-hugging host and resize seam, with Drawing contents.
pub(crate) fn browser_contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    if let Ok(drawing) = app.session.drawing("drawing.inspect") {
        for sheet in &drawing.sheets {
            let id = ui.make_persistent_id(("drawing_browser_sheet", app.session.uid, drawing.id, sheet.id));
            let mut open = ui.ctx().data(|data| data.get_temp::<bool>(id)).unwrap_or(true);
            let row =
                ui_kit::tree_row(ui, id, &ui_kit::Row { fold: Some(open), icon: "ws_drawing", label: &sheet.name, name: true, ..Default::default() });
            if row.fold {
                open = !open;
                ui.ctx().data_mut(|data| data.insert_temp(id, open));
            }
            if open {
                let objects = drawing.on_sheet(sheet.id);
                for (kind, ids) in [
                    ("view", objects.views),
                    ("dimension", objects.dimensions),
                    ("balloon", objects.balloons),
                    ("note", objects.notes),
                    ("table", objects.tables),
                    ("image", objects.images),
                    ("revision_tag", objects.tags),
                ] {
                    for (index, object) in ids.into_iter().enumerate() {
                        let key = format!("ui.drawing_browser_{kind}");
                        let label = solidcraft_ui_text::format(&key, &[("number", &(index + 1).to_string())]);
                        ui_kit::tree_row(ui, id.with((kind, object)), &ui_kit::Row { depth: 1, label: &label, name: true, ..Default::default() });
                    }
                }
            }
        }
    } else {
        ui_kit::text(ui, solidcraft_ui_text::tr_label("No drawing objects"), ui_kit::Text::Note("drawing_objects"));
    }
    ui.add_space(Tokens::get().space_8);
    ui_kit::text(ui, solidcraft_ui_text::tr_label("Source design"), ui_kit::Text::Heading("drawing_source"));
    ui.label(&app.session.doc.name);
    if ui_kit::button(ui, solidcraft_ui_text::tr_label("Source properties"), ui_kit::Button::Normal).clicked() {
        app.start("document.metadata");
    }
}

/// The sheet page draws the open drawing's sheet; with no drawing open there is no paper, sample
/// geometry or camera, only a note.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    if crate::drawing_sheet_page::show(app, ui) {
        return;
    }
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    let t = Tokens::get();
    ui.painter().rect_filled(rect, 0.0, t.viewport_top);
    let bounds = crate::shell::workspace_content(app, rect);
    let width = (bounds.width() - t.space_24 * 2.0).clamp(0.0, 420.0);
    let centre = Rect::from_center_size(bounds.center(), vec2(width, 140.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(centre), |ui| {
        ui.vertical_centered(|ui| {
            ui.heading(solidcraft_ui_text::tr_label("Drawing workspace"));
            ui.label(solidcraft_ui_text::tr_label("No drawing sheet is available yet."));
            ui_kit::text(ui, app.tr("ui.sheet_page.no_drawing"), ui_kit::Text::Note("drawing_waiting"));
        });
    });
}

const SHEETS_VISIBILITY: [(&str, &str); 4] = [
    ("multiple", "ui.drawing_sheets.multiple"),
    ("always", "ui.drawing_sheets.always"),
    ("off", "ui.drawing_sheets.off"),
    ("legacy", "ui.drawing_sheets.legacy"),
];

pub(crate) fn valid_sheets_visibility(value: &str) -> bool {
    SHEETS_VISIBILITY.iter().any(|(key, _)| *key == value)
}

pub(crate) fn sheets_visibility_choice(app: &mut SolveApp, ui: &mut egui::Ui) {
    let choices = SHEETS_VISIBILITY.map(|(_, label)| app.tr(label).to_string());
    let choices = choices.each_ref().map(String::as_str);
    let current = SHEETS_VISIBILITY.iter().position(|(key, _)| *key == app.preferences.drawing_sheets_visibility).unwrap_or_default();
    let mut selected = current;
    let response = ui_kit::dropdown(ui, "drawing_sheets_visibility", &mut selected, &choices);
    crate::scenario::publish_target("prefs:drawing_sheets_visibility", response.rect);
    if selected != current
        && let Some((key, _)) = SHEETS_VISIBILITY.get(selected)
    {
        app.preferences.drawing_sheets_visibility = (*key).into();
    }
}

fn add_sheet(app: &mut SolveApp) {
    let result = app.run("drawing.sheet.add", serde_json::json!({}));
    match result {
        Ok(result) => {
            if let Some(sheet) = result.get("sheet").and_then(serde_json::Value::as_str).and_then(solidcraft_engine::doc::ids::decode) {
                select_sheet(app, sheet);
            }
        }
        Err(error) => app.set_status(error, true),
    }
}

fn sheet_settings_actions(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(active) = active_sheet(app) else { return };
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return };
    if drawing.sheets.len() > 1 && app.preferences.drawing_sheets_visibility == "off" {
        let active_name = drawing.sheets.iter().find(|sheet| sheet.id == active).map_or("", |sheet| sheet.name.as_str());
        let label = app.tr("ui.drawing_sheets.active").to_string();
        ui_kit::row(ui, &label, |ui| {
            ui_kit::dropdown_menu(ui, "drawing_active_sheet", active_name, |ui| {
                for sheet in &drawing.sheets {
                    if ui.selectable_label(sheet.id == active, &sheet.name).clicked() {
                        select_sheet(app, sheet.id);
                    }
                }
            });
        });
    }
    if ui_kit::button(ui, app.tr("ui.drawing_sheets.add"), ui_kit::Button::Normal).clicked() {
        add_sheet(app);
    }
}

fn toggle_sheet_settings(app: &mut SolveApp) {
    app.drawing_workspace.settings_open = !app.drawing_workspace.settings_open;
    app.drawing_workspace.options = None;
}

/// Draw the chooser in the shared Browser host, above the Browser title and tree.
pub(crate) fn sheets_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    if !active(app) || app.preferences.drawing_sheets_visibility == "legacy" {
        return;
    }
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return };
    let show = match app.preferences.drawing_sheets_visibility.as_str() {
        "always" => true,
        "off" => false,
        _ => drawing.sheets.len() > 1,
    };
    let settings_label = app.tr_label("Sheet Settings").to_string();
    if !show {
        if ui_kit::button(ui, &settings_label, ui_kit::Button::Small).clicked() {
            toggle_sheet_settings(app);
        }
        return;
    }
    let title = app.tr("ui.drawing_sheets.title").to_string();
    let add_label = app.tr("ui.drawing_sheets.add").to_string();
    let id = ("drawing_sheets", app.session.uid, drawing.id);
    let t = Tokens::get();
    let selected = active_sheet(app);
    ui_kit::section(ui, id, &title, true, |ui| {
        ui.horizontal(|ui| {
            if icon_action(ui, "drawing:sheets:add", "plus", &add_label, true).clicked() {
                add_sheet(app);
            }
            if icon_action(ui, "drawing:sheets:settings", "settings", &settings_label, true).clicked() {
                toggle_sheet_settings(app);
            }
        });
        let width = (ui.available_width() - t.space_8).max(0.0) * 0.5;
        egui::ScrollArea::vertical().id_salt(id).max_height(t.space_24 * 9.0).show(ui, |ui| {
            for pair in drawing.sheets.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.space_8;
                    for sheet in pair {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            let target = format!("drawing:sheets:sheet:{}", solidcraft_engine::doc::ids::encode(sheet.id));
                            let (rect, _) = ui.allocate_exact_size(vec2(width, t.space_24 * 2.5), Sense::hover());
                            let response = ui_kit::button_at(
                                ui,
                                &target,
                                rect,
                                "",
                                if selected == Some(sheet.id) { ui_kit::Button::SmallSelected } else { ui_kit::Button::Normal },
                            );
                            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected == Some(sheet.id), &sheet.name));
                            crate::scenario::publish_target(&target, rect);
                            paint_sheet_thumbnail(ui, rect, &drawing, sheet);
                            ui.add(egui::Label::new(&sheet.name).truncate());
                            if response.clicked() {
                                select_sheet(app, sheet.id);
                            }
                        });
                    }
                });
            }
        });
    });
}

fn paint_sheet_thumbnail(
    ui: &egui::Ui,
    rect: Rect,
    drawing: &solidcraft_engine::doc::drawing::Drawing,
    sheet: &solidcraft_engine::doc::drawing::Sheet,
) {
    let t = Tokens::get();
    let inner = rect.shrink(t.space_8);
    let Ok(size) = sheet.sheet.size_mm() else { return };
    if !inner.is_positive() || size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let scale = (f64::from(inner.width()) / size.x).min(f64::from(inner.height()) / size.y);
    let page = Rect::from_center_size(inner.center(), vec2((size.x * scale) as f32, (size.y * scale) as f32));
    let painter = ui.painter().with_clip_rect(page.intersect(ui.clip_rect()));
    painter.rect_filled(page, t.radius_sm, t.paper_white);
    let at = |mm: Vec2| pos2(page.left() + (mm.x * scale) as f32, page.bottom() - (mm.y * scale) as f32);
    let stroke = Stroke::new(t.space_2 * 0.5, t.paper_ink);
    if let Some(border) = &sheet.border
        && let Ok(layout) = border.layout(&sheet.sheet)
    {
        painter.rect_stroke(Rect::from_two_pos(at(layout.drawing_border.min), at(layout.drawing_border.max)), 0.0, stroke, egui::StrokeKind::Inside);
    }
    if let Ok(Some(block)) = sheet.title_block_outline() {
        painter.rect_stroke(Rect::from_two_pos(at(block.min), at(block.max)), 0.0, stroke, egui::StrokeKind::Inside);
    }
    // The scene provider has one active sheet. A schematic preview uses the actual view centres
    // without changing its active sheet or asking it to generate other sheets on the UI thread.
    for view in drawing.views.iter().filter(|view| view.sheet == sheet.id) {
        if let Ok(centre) = drawing.view_centre(view.id) {
            painter.circle_filled(at(centre), t.space_2, t.paper_ink);
        }
    }
}

fn dock(app: &SolveApp) -> &str {
    match app.preferences.drawing_sheet_dock.as_str() {
        "bottom" => "bottom",
        "under_browser" if app.ui.show_browser => "under_browser",
        _ => "top",
    }
}

fn strip_size(app: &SolveApp, ui: &egui::Ui, destination: &str) -> egui::Vec2 {
    let t = Tokens::get();
    let label = ui_kit::ContentWidth::button(ui, solidcraft_ui_text::tr_label("Sheet unavailable"));
    let spacing = ui.spacing().item_spacing;
    let height = ui_kit::row_height().max(t.space_24);
    let measured = if destination == "under_browser" {
        vec2(label.max(t.space_10 + 3.0 * t.space_24 + 3.0 * spacing.x) + 10.0, height * 2.0 + spacing.y + 10.0)
    } else {
        vec2(label + t.space_10 + 3.0 * t.space_24 + 4.0 * spacing.x + 10.0, height + 10.0)
    };
    // Retain measured card dimensions; the font/control metrics cover a first frame or scale change.
    app.drawing_workspace.strip_sizes.get(usize::from(destination == "under_browser")).copied().flatten().map_or(measured, |size| size.max(measured))
}

fn needs_vertical(app: &SolveApp, available: Rect, width: f32) -> bool {
    app.ui.show_browser && app.shell.browser.is_some_and(|b| available.right() - b.right() - Tokens::get().space_8 < width)
}

fn strip_position(app: &SolveApp, available: Rect, destination: &str, size: egui::Vec2) -> egui::Pos2 {
    let gap = Tokens::get().space_8;
    let beside = app.shell.browser.map_or(available.left(), |b| b.right() + gap);
    let x = beside.clamp(available.left(), (available.right() - size.x).max(available.left()));
    match destination {
        "bottom" => pos2(x, (available.bottom() - size.y).max(available.top())),
        "under_browser" => app.shell.browser.map_or(available.min, |b| pos2(b.left(), b.bottom() + gap)),
        _ => pos2(x, available.top()),
    }
}

fn strip_target(app: &SolveApp, available: Rect, at: egui::Pos2) -> Option<&'static str> {
    if !available.contains(at) {
        return None;
    }
    let edge = Tokens::get().space_24 * 2.0;
    if at.y <= available.top() + edge {
        Some("top")
    } else if at.y >= available.bottom() - edge {
        Some("bottom")
    } else if app.ui.show_browser && app.shell.browser.is_some_and(|b| at.x <= b.right() && at.y >= b.top()) {
        Some("under_browser")
    } else {
        None
    }
}

/// Same grip and highlighted docking vocabulary as History; no document state changes.
fn strip_drag(app: &mut SolveApp, ui: &mut egui::Ui, available: Rect) {
    let grip = ui_kit::panel_grip(ui);
    let drag = ui
        .interact(grip.rect, ui.scope_id().with("drawing_sheet_grip"), Sense::drag())
        .on_hover_text(solidcraft_ui_text::tr_label("Dock sheet tabs"));
    crate::scenario::publish_target("drawing:sheets:grip", drag.rect);
    if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let target = ui.ctx().pointer_latest_pos().and_then(|p| strip_target(app, available, p));
    if drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(target) = target {
            let horizontal = strip_size(app, ui, "top");
            let target = if target != "under_browser" && needs_vertical(app, available, horizontal.x) { "under_browser" } else { target };
            let size = strip_size(app, ui, target);
            let zone = Rect::from_min_size(strip_position(app, available, target, size), size);
            let t = Tokens::get();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("drawing_dock_preview")));
            painter.rect(zone, t.radius_lg, t.accent.gamma_multiply(0.18), Stroke::new(t.space_2, t.accent), egui::StrokeKind::Inside);
            crate::scenario::publish_rect("drawing:sheets:dock_preview", zone);
        }
    }
    if drag.drag_stopped()
        && let Some(target) = target
    {
        app.preferences.drawing_sheet_dock = target.into();
    }
    // Keyboard-accessible alternative to dragging, like History's layout menu.
    ui_kit::Menu::context(&drag, |ui| dock_menu(app, ui));
}

fn dock_menu(app: &mut SolveApp, ui: &mut egui::Ui) {
    for (value, label) in [("top", "Dock at top"), ("bottom", "Dock at bottom"), ("under_browser", "Dock beneath Browser")] {
        if ui_kit::button(ui, solidcraft_ui_text::tr_label(label), ui_kit::Button::Enabled(value != "under_browser" || app.ui.show_browser)).clicked()
        {
            app.preferences.drawing_sheet_dock = value.into();
            ui.close();
        }
    }
}

fn icon_action(ui: &mut egui::Ui, id: &str, icon: &str, label: &str, enabled: bool) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(Tokens::get().space_24, Tokens::get().space_24), Sense::hover());
    let r = ui.add_enabled_ui(enabled, |ui| ui_kit::icon_button_at(ui, rect, icon, label)).inner;
    crate::scenario::publish_target(id, rect);
    r
}

/// Draw after Browser, so the strip can hug its measured right or bottom edge.
pub fn chrome(app: &mut SolveApp, ui: &mut egui::Ui) {
    app.drawing_workspace.strip = None;
    if !active(app) || app.preferences.drawing_sheets_visibility != "legacy" {
        return;
    }
    let mut available = crate::agent_chat::tool_space(app, ui.available_rect_before_wrap());
    if Container::for_layout(app.ui.layout, Surface::Panel).floating()
        && app.preferences.drawing_panel_side != "left"
        && let Some(settings) = settings_rect(app)
    {
        available.max.x = available.right().min(settings.left() - Tokens::get().space_8).max(available.left());
    }
    if let Some(bottom) = app.shell.cards.iter().map(Rect::bottom).max_by(f32::total_cmp) {
        available.min.y = available.min.y.max(bottom + Tokens::get().space_8);
    }
    if !available.is_positive() {
        return;
    }
    let destination =
        if dock(app) != "under_browser" && needs_vertical(app, available, strip_size(app, ui, "top").x) { "under_browser" } else { dock(app) }
            .to_string();
    let position = strip_position(app, available, &destination, strip_size(app, ui, &destination));
    let container = Container::new(ui.ctx(), Surface::Panel);
    let frame = container.frame(ui.ctx(), Tokens::get().panel, crate::frame::Side::Left).inner_margin(4);
    let card = container.area(egui::Id::unique("sc_drawing_sheets")).fixed_pos(position).show(ui.ctx(), |ui| {
        container.show_frame(ui, frame, available, |ui| {
            let buttons = |app: &mut SolveApp, ui: &mut egui::Ui| {
                icon_action(ui, "drawing:sheets:add", "plus", "Add sheet (not implemented)", false);
                let management = icon_action(ui, "drawing:sheets:manage", "dots", "Manage sheets and docking", true);
                Container::new(ui.ctx(), Surface::Popup).menu(&management).frame(ui_kit::Menu::frame()).show(|ui| {
                    ui_kit::button(ui, solidcraft_ui_text::tr_label("Manage sheets (not implemented)"), ui_kit::Button::Enabled(false));
                    ui.separator();
                    dock_menu(app, ui);
                });
                if icon_action(ui, "drawing:sheets:settings", "settings", "Sheet Settings", true).clicked() {
                    app.drawing_workspace.settings_open = !app.drawing_workspace.settings_open;
                    app.drawing_workspace.options = None;
                }
            };
            if destination == "under_browser" {
                ui.horizontal(|ui| {
                    strip_drag(app, ui, available);
                    buttons(app, ui);
                });
                ui_kit::button(ui, solidcraft_ui_text::tr_label("Sheet unavailable"), ui_kit::Button::Enabled(false));
            } else {
                ui.horizontal(|ui| {
                    strip_drag(app, ui, available);
                    ui_kit::button(ui, solidcraft_ui_text::tr_label("Sheet unavailable"), ui_kit::Button::Enabled(false));
                    buttons(app, ui);
                });
            }
        });
    });
    app.drawing_workspace.strip = Some(card.response.rect);
    if let Some(size) = app.drawing_workspace.strip_sizes.get_mut(usize::from(destination == "under_browser")) {
        *size = Some(card.response.rect.size());
    }
    crate::scenario::publish_rect("drawing:sheets", card.response.rect);
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    app.drawing_workspace.settings = None;
    if !active(app) || (!app.drawing_workspace.settings_open && app.drawing_workspace.options.is_none()) {
        return;
    }
    let left = app.preferences.drawing_panel_side == "left";
    let width = panel_width(app);
    let container = Container::new(ui.ctx(), Surface::Panel);
    if container.floating() {
        floating_panel(app, ui, left, width);
        return;
    }
    let r = container
        .edge(if left { "sc_drawing_settings_left" } else { "sc_drawing_settings_right" }, if left { Edge::Left } else { Edge::Right })
        .default_size(width)
        .size_range(240.0..=560.0)
        .resizable(true)
        .frame(container.edge_frame(Tokens::get().dialog_body).inner_margin(10))
        .show(ui, |ui| {
            ui_kit::scope(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| panel_body(app, ui));
            });
        });
    app.drawing_workspace.settings = Some(r.response.rect);
    app.preferences.drawing_panel_width = r.response.rect.width().clamp(240.0, 560.0);
    crate::scenario::publish_rect("drawing:settings", r.response.rect);
    let rect = r.response.rect;
    let rail = Rect::from_center_size(if left { rect.right_center() } else { rect.left_center() }, vec2(Tokens::get().space_4, rect.height()));
    crate::scenario::publish_target("drawing:settings:resize", rail);
}

/// Assistant's floating panel host and bounded resize rail; no independent window chrome.
fn floating_panel(app: &mut SolveApp, ui: &mut egui::Ui, left: bool, width: f32) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get();
    let available = crate::agent_chat::tool_space(app, ui.available_rect_before_wrap());
    let mut bounds = available.shrink(crate::ui_kit::SHELL_GAP);
    bounds.min.y = available.top();
    let width = width.min(bounds.width().max(0.0));
    let position = pos2(if left { bounds.left() } else { bounds.right() - width }, bounds.top());
    bounds = Rect::from_min_size(position, vec2(width, bounds.height().max(0.0)));
    if !bounds.is_positive() {
        return;
    }
    let side = if left { crate::frame::Side::Left } else { crate::frame::Side::Right };
    let frame = ui_kit::Panel::feature_frame(&ctx, app.ui.layout, side);
    let inner = (bounds.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
    let anchor = if left { bounds.left_top() } else { bounds.right_top() };
    let panel = ui_kit::Panel::new(
        solidcraft_ui_text::tr_label("Sheet Settings"),
        egui::Id::unique("sc_drawing_settings_floating"),
        anchor,
        available,
        side,
        frame,
    )
    .width(width);
    if let Some(card) = panel.window(&ctx).fixed_size(inner).show(&ctx, |ui| {
        ui.set_min_size(inner);
        ui.set_max_size(inner);
        ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, inner)));
        ui_kit::scope(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| panel_body(app, ui));
        });
        let rect = ui.max_rect();
        let rail = if left {
            Rect::from_x_y_ranges(rect.right() - t.space_4..=rect.right(), rect.y_range())
        } else {
            Rect::from_x_y_ranges(rect.left()..=rect.left() + t.space_4, rect.y_range())
        };
        let response =
            ui.interact(rail, egui::Id::unique("sc_drawing_settings_resize"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        crate::scenario::publish_target("drawing:settings:resize", rail);
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta().x) * if left { 1.0 } else { -1.0 };
            if delta.is_finite() {
                app.preferences.drawing_panel_width = (width + delta).clamp(240.0, 560.0);
            }
        }
    }) {
        app.drawing_workspace.settings = Some(card.response.rect);
        crate::scenario::publish_rect("drawing:settings", card.response.rect);
    }
}

fn unavailable_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui_kit::row(ui, solidcraft_ui_text::tr_label(label), |ui| {
        ui.add_enabled_ui(false, |ui| {
            if matches!(
                label,
                "Source"
                    | "Orientation"
                    | "Direction"
                    | "Parent view"
                    | "Text style"
                    | "Profile & units"
                    | "Size & orientation"
                    | "Border & template"
            ) {
                ui_kit::dropdown_menu(ui, label, solidcraft_ui_text::tr_label(value), |_| {});
            } else {
                let mut text = solidcraft_ui_text::tr_label(value).to_string();
                ui_kit::cell_editor(ui, ui.scope_id().with(label), &mut text, "", false);
            }
        });
    });
}

fn panel_body(app: &mut SolveApp, ui: &mut egui::Ui) {
    let options = app.drawing_workspace.options;
    ui.horizontal(|ui| {
        ui.heading(solidcraft_ui_text::tr_label(options.unwrap_or("Sheet Settings")));
        if icon_action(ui, "drawing:settings:close", "close", "Close drawing panel", true).clicked() {
            app.drawing_workspace.settings_open = false;
            app.drawing_workspace.options = None;
        }
    });
    ui.horizontal(|ui| {
        for (side, label) in [("left", "Dock left"), ("right", "Dock right")] {
            if ui_kit::button(ui, solidcraft_ui_text::tr_label(label), ui_kit::Button::Normal).clicked() {
                app.preferences.drawing_panel_side = side.into();
            }
        }
    });
    ui.separator();
    ui_kit::text(
        ui,
        solidcraft_ui_text::tr_label("Not implemented. These options describe the planned interface; no drawing data is changed."),
        ui_kit::Text::Note("drawing_options_pending"),
    );
    if let Some(kind) = options {
        match kind {
            "Base View" => {
                unavailable_row(ui, "Source", "Source design");
                unavailable_row(ui, "Orientation", "Not configured");
                unavailable_row(ui, "Scale", "Not configured");
            }
            "Projected View" => {
                unavailable_row(ui, "Parent view", "No drawing view available");
                unavailable_row(ui, "Direction", "Not configured");
                unavailable_row(ui, "Scale", "Inherited from parent view");
            }
            "Dimension" => {
                unavailable_row(ui, "References", "No drawing geometry available");
                unavailable_row(ui, "Units & precision", "Drawing defaults (not available)");
            }
            "Note" => {
                unavailable_row(ui, "Text", "Not configured");
                unavailable_row(ui, "Text style", "Drawing defaults (not available)");
            }
            _ => {}
        }
        ui.separator();
        ui_kit::button(ui, solidcraft_ui_text::tr_label("Apply (not implemented)"), ui_kit::Button::Enabled(false));
    } else {
        ui.heading(solidcraft_ui_text::tr_label("Drawing & title block"));
        unavailable_row(ui, "Title", "No drawing document");
        unavailable_row(ui, "Title block", "Not available");
        ui.heading(solidcraft_ui_text::tr_label("Paper & layout"));
        sheet_settings_actions(app, ui);
        unavailable_row(ui, "Size & orientation", "No sheet available");
        unavailable_row(ui, "Border & template", "Not available");
        ui.heading(solidcraft_ui_text::tr_label("Drafting defaults"));
        unavailable_row(ui, "Profile & units", "No drawing document");
        ui.separator();
        ui.heading(solidcraft_ui_text::tr_label("Screen appearance"));
        let label = app.tr("ui.sheet_page.colour").to_owned();
        ui_kit::row(ui, &label, |ui| crate::drawing_sheet_page::colour_choice(app, ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    #[test]
    fn browser_lists_sheets_of_the_open_drawing() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        h.app.run("drawing.sheet.add", serde_json::json!({"name":"Inspection sheet"})).unwrap();
        h.frames(4);
        assert!(h.widget("Inspection sheet").is_some());

        h.app.run("drawing.close", serde_json::json!({})).unwrap();
        h.frames(2);
        assert!(h.widget("Inspection sheet").is_none());
    }

    #[test]
    fn sheet_chooser_switches_canvas_and_falls_back_after_delete() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        let first = h.app.run("drawing.create", serde_json::json!({})).unwrap()["sheets"][0]["id"].as_str().unwrap().to_string();
        h.frames(4);
        assert!(crate::scenario::handle_at(&format!("drawing:sheets:sheet:{first}")).is_none());

        let second = h.app.run("drawing.sheet.add", serde_json::json!({"name":"Inspection sheet"})).unwrap()["sheet"].as_str().unwrap().to_string();
        h.frames(4);
        let revision = h.app.session.revision;
        let undo = h.app.session.undo.len();
        h.step(&serde_json::json!({"click":{"handle":format!("drawing:sheets:sheet:{second}")}})).unwrap();
        h.frames(3);
        assert_eq!(crate::drawing_sheet_page::inspect(&h.app)["sheet"], second);
        assert_eq!((h.app.session.revision, h.app.session.undo.len()), (revision, undo));

        h.app.run("drawing.sheet.delete", serde_json::json!({"sheet":second})).unwrap();
        h.frames(3);
        assert_eq!(crate::drawing_sheet_page::inspect(&h.app)["sheet"], first);
    }

    #[test]
    fn workspace_and_options_leave_source_and_camera_unchanged() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        h.app.ui.tab = "SURFACE".into();
        let doc = h.app.session.doc.clone();
        let revision = h.app.session.revision;
        let undo = h.app.session.undo.len();
        let cam = h.app.cam;
        enter(&mut h.app, true).unwrap();
        assert!(start(&mut h.app, "drawing.view.base.planned"));
        h.frames(2);
        assert!(h.widget("Apply (not implemented)").is_some());
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.revision, revision);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.cam, cam);
        enter(&mut h.app, false).unwrap();
        assert_eq!(h.app.ui.tab, "SURFACE");
    }

    #[test]
    fn sheet_docking_and_settings_only_change_ui_preferences() {
        for layout in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.preferences.drawing_sheets_visibility = "legacy".into();
            h.app.home.open = false;
            h.app.ui.layout = layout;
            h.call("ui.resize", serde_json::json!({"width":1400,"height":1000}));
            enter(&mut h.app, true).unwrap();
            let revision = h.app.session.revision;
            let doc = h.app.session.doc.clone();
            let cam = h.app.cam;
            h.frames(4);
            let strip = crate::scenario::rect_of("drawing:sheets").unwrap();
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            assert!(strip.width() < 340.0 && strip.height() < 50.0, "single sheet strip should hug its contents: {strip:?}");
            assert!(strip.left() >= browser.right(), "sheet strip belongs beside Browser");
            for (destination, at) in [
                ("bottom", pos2(700.0, 980.0)),
                ("under_browser", pos2(browser.center().x, browser.bottom() + 80.0)),
                ("top", pos2(700.0, strip.center().y)),
            ] {
                let grip = crate::scenario::handle_at("drawing:sheets:grip").unwrap();
                h.app.synthetic.push(egui::Event::PointerMoved(grip));
                h.frame();
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: grip,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frame();
                h.app.synthetic.push(egui::Event::PointerMoved(at));
                h.frames(2);
                assert!(crate::scenario::rect_of("drawing:sheets:dock_preview").is_some(), "{layout:?} {destination}: highlighted docking preview");
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frames(4);
                assert_eq!(h.app.preferences.drawing_sheet_dock, destination);
            }
            h.step(&serde_json::json!({"click":{"handle":"drawing:sheets:settings"}})).unwrap();
            h.frames(3);
            for (side, label) in [("left", "Dock left"), ("right", "Dock right")] {
                h.step(&serde_json::json!({"widget":label})).unwrap();
                h.frames(3);
                assert_eq!(h.app.preferences.drawing_panel_side, side);
                let panel = crate::scenario::rect_of("drawing:settings").unwrap();
                assert_eq!(panel.center().x < 700.0, side == "left", "{layout:?} {side}: {panel:?}");
                let rail = crate::scenario::handle_at("drawing:settings:resize").unwrap();
                h.call(
                    "ui.drag",
                    serde_json::json!({"x0":rail.x,"y0":rail.y,"x1":rail.x + if side == "left" {50.0} else {-50.0},"y1":rail.y,"steps":6}),
                );
                h.frames(3);
                assert!(app_width(&h) > panel.width() + 20.0, "{layout:?} {side}: properties resize");
            }
            assert_eq!(h.app.session.revision, revision);
            assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
            assert_eq!(h.app.cam, cam);
        }
        assert!(!can_open_options("drawing.export.planned"));
        assert!(!can_open_options("drawing.update.planned"));
    }

    fn app_width(h: &Harness) -> f32 {
        h.app.preferences.drawing_panel_width
    }

    #[test]
    fn crowded_sheet_strip_moves_beneath_browser_without_changing_its_dock_preference() {
        for layout in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.preferences.drawing_sheets_visibility = "legacy".into();
            h.app.home.open = false;
            h.app.ui.layout = layout;
            h.call("ui.resize", serde_json::json!({"width":960,"height":600}));
            h.app.preferences.drawing_panel_width = 560.0;
            enter(&mut h.app, true).unwrap();
            h.app.drawing_workspace.settings_open = true;
            h.frames(4);
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            let strip = crate::scenario::rect_of("drawing:sheets").unwrap();
            assert!(strip.top() >= browser.bottom(), "{layout:?}: crowded sheet strip must not cover Browser: {strip:?} {browser:?}");
            assert_eq!(h.app.preferences.drawing_sheet_dock, "top");
            h.step(&serde_json::json!({"click":{"handle":"drawing:settings:close"}})).unwrap();
            h.frames(4);
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            let strip = crate::scenario::rect_of("drawing:sheets").unwrap();
            assert!(
                strip.left() >= browser.right() && strip.height() < 50.0,
                "{layout:?}: preferred horizontal strip returns when room is available"
            );
            assert_eq!(h.app.preferences.drawing_sheet_dock, "top");
        }
    }
}
