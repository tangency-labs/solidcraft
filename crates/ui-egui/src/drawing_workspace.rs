//! Drawing's native layout anchor. The centre is the sheet page (`drawing_sheet_page`) once a
//! drawing is open; no drawing document or paper scene is fabricated here. Missing tools expose
//! only agreed options; source services keep their existing ownership.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use solidcore_record::Vec2;

use crate::drawing_sheet_page::{active_sheet, select_sheet};

use crate::SolveApp;
use crate::ui_kit::{self, Container, Edge, Surface};
use solidcraft_theme::Tokens;

pub(crate) mod browser;
pub(crate) mod dimension;
mod first_sheet;
pub(crate) mod note;
mod panels;
pub(crate) mod placement;

use panels::Tool;

#[derive(Default)]
pub struct State {
    previous_tab: Option<String>,
    sheets_override: Option<((u64, u64), bool)>,
    pub settings_open: bool,
    sheet_notice: Option<panels::SheetNotice>,
    options: Option<Tool>,
    placement: Option<placement::Placement>,
    note: Option<note::Draft>,
    view_edit: Option<panels::ViewEdit>,
    dimension: Option<dimension::Draft>,
    dimension_hover: Option<dimension::Hover>,
    dimension_edit: Option<panels::DimensionEdit>,
    /// The object whose options the panel shows because it was selected (it alone), and the one
    /// thing selected when that was last looked at: see [`follow_selection`].
    shown_for: Option<u64>,
    selection_seen: Option<u64>,
    add_sheet: Option<panels::AddSheet>,
    rename_sheet: Option<u64>,
    sheet_menu: Option<SheetMenu>,
    delete_sheet: Option<(u64, u64, u64)>,
    first_sheet: Option<first_sheet::BlankSheet>,
    strip: Option<Rect>,
    strip_sizes: [Option<egui::Vec2>; 2],
    settings: Option<Rect>,
    /// What was selected in the other workspace when Drawing was entered (Design's selection while
    /// in Drawing, the drawing's while not): each workspace gets its own back when entered again.
    selection_away: Vec<solidcraft_engine::Sel>,
    /// The session's selection is Drawing's (else Design's).
    selection_is_drawings: bool,
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
            swap_selection(app);
            sync_history(app);
            open_a_drawing(app);
        }
    } else if active(app) {
        // The drawing stays open, on its sheet and zoom, with a sheet sketch being edited still
        // being edited, for when Drawing is entered again: the model's commands work with either.
        app.ui.tab = app.drawing_workspace.previous_tab.take().unwrap_or_else(|| "MODEL".into());
        swap_selection(app);
        sync_history(app);
        app.drawing_workspace.options = None;
        app.drawing_workspace.placement = None;
        app.drawing_workspace.note = None;
        app.drawing_workspace.view_edit = None;
        app.drawing_workspace.dimension = None;
        app.drawing_workspace.dimension_hover = None;
        app.drawing_workspace.dimension_edit = None;
        app.drawing_workspace.shown_for = None;
        app.drawing_workspace.selection_seen = None;
        app.drawing_workspace.add_sheet = None;
        app.drawing_workspace.rename_sheet = None;
        app.drawing_workspace.sheet_menu = None;
        app.drawing_workspace.delete_sheet = None;
        app.drawing_workspace.first_sheet = None;
        app.drawing_workspace.strip = None;
        app.drawing_workspace.settings = None;
    }
    app.ui.ribbon_context = None;
    app.cancel_view_animation();
    Ok(())
}

/// Entering Drawing with no drawing open opens one: the design's first, fitted on its first
/// sheet. A drawing left open when Drawing was left is still open, on its sheet and zoom. A design
/// without drawings starts on the page that makes one.
fn open_a_drawing(app: &mut SolveApp) {
    if app.session.open_drawing.is_some() {
        return;
    }
    let Some(id) = app.session.doc.drawings.first().map(|d| d.id) else { return };
    match app.run("drawing.open", serde_json::json!({"drawing": solidcraft_engine::doc::ids::encode(id)})) {
        Ok(_) => crate::drawing_sheet_page::fit(app),
        Err(error) => app.set_status(error, true),
    }
}

/// Puts the selection kept for the workspace being entered back, and keeps the one of the
/// workspace being left: picking on a sheet never changes what Design has selected, and the
/// reverse. Drawing's own is kept only while its objects are still there.
pub(crate) fn swap_selection(app: &mut SolveApp) {
    if app.drawing_workspace.selection_is_drawings == active(app) {
        return;
    }
    app.drawing_workspace.selection_is_drawings = active(app);
    let mut back = std::mem::take(&mut app.drawing_workspace.selection_away);
    let session = &app.session;
    back.retain(|item| match item {
        solidcraft_engine::Sel::Drawing { drawing, object } => {
            session.open_drawing == Some(*drawing) && session.doc.drawing(*drawing).is_some_and(|d| d.has_object(*object))
        }
        _ => true,
    });
    app.drawing_workspace.selection_away = std::mem::replace(&mut app.session.selection, back);
}

/// Undo and redo take the steps of the workspace shown: the open drawing's in Drawing, the
/// model's everywhere else, a drawing open or not.
pub(crate) fn sync_history(app: &mut SolveApp) {
    app.session.model_focus = !active(app);
}

pub fn reset_document(app: &mut SolveApp) {
    if active(app) {
        app.ui.tab = "MODEL".into();
    }
    app.drawing_workspace = State::default();
}

pub(crate) fn placing_view(app: &SolveApp) -> bool {
    app.drawing_workspace.placement.is_some()
}

pub(crate) fn placing_note(app: &SolveApp) -> bool {
    app.drawing_workspace.options == Some(Tool::Note(None))
}

pub(crate) fn placing_dimension(app: &SolveApp) -> bool {
    app.drawing_workspace.options == Some(Tool::Dimension(None))
}

/// The Drawing tool that does on a sheet what the sketch tool `command` does in a sketch, if
/// there is one: it starts from that tool's key, so the key learnt in a sketch works here.
pub(crate) fn counterpart(command: &'static str) -> Option<&'static str> {
    match command {
        "sketch.dimension" => Some("drawing.dimension"),
        _ => None,
    }
}

pub fn can_open_options(id: &str) -> bool {
    Tool::for_command(id).is_some() || matches!(id, "drawing.sheet.add" | "drawing.sheet.set")
}

/// Dismiss the current Drawing inspector without touching source or drawing data.
pub fn close_panel(app: &mut SolveApp) -> bool {
    let open = app.drawing_workspace.settings_open
        || app.drawing_workspace.options.is_some()
        || app.drawing_workspace.add_sheet.is_some()
        || app.drawing_workspace.first_sheet.is_some();
    app.drawing_workspace.settings_open = false;
    app.drawing_workspace.sheet_menu = None;
    app.drawing_workspace.rename_sheet = None;
    app.drawing_workspace.options = None;
    app.drawing_workspace.placement = None;
    app.drawing_workspace.note = None;
    app.drawing_workspace.view_edit = None;
    app.drawing_workspace.dimension = None;
    app.drawing_workspace.dimension_hover = None;
    app.drawing_workspace.dimension_edit = None;
    app.drawing_workspace.shown_for = None;
    app.drawing_workspace.add_sheet = None;
    app.drawing_workspace.first_sheet = None;
    app.drawing_workspace.settings = None;
    open
}

/// Esc with the panel open: a tool's or a double-click's panel closes (true), and the selection
/// stays for the next Esc. The panel that shows the selected object's options is not a step of its
/// own: Esc clears the selection, as in a sketch, and the panel goes with it.
pub(crate) fn escape_panel(app: &mut SolveApp) -> bool {
    let state = &app.drawing_workspace;
    if state.shown_for.is_some() && state.options.and_then(Tool::object) == state.shown_for {
        return false;
    }
    close_panel(app)
}

/// The one view, dimension or note selected shows its options in the right panel, where a tool
/// shows its own, and they go when it is no longer the selection. A tool in hand, the sheet's
/// settings or a panel opened another way stays as it is; the panel closed by hand stays closed
/// until the selection changes.
fn follow_selection(app: &mut SolveApp) {
    let one = match &app.session.selection[..] {
        [solidcraft_engine::Sel::Drawing { drawing, object }] if app.session.open_drawing == Some(*drawing) => Some(*object),
        _ => None,
    };
    if one == app.drawing_workspace.selection_seen {
        return;
    }
    app.drawing_workspace.selection_seen = one;
    let state = &app.drawing_workspace;
    let ours = state.shown_for.is_some() && state.options.and_then(Tool::object) == state.shown_for;
    let free =
        !state.settings_open && state.add_sheet.is_none() && state.first_sheet.is_none() && state.placement.is_none() && state.options.is_none();
    if !ours && !free {
        app.drawing_workspace.shown_for = None;
        return;
    }
    if ours {
        close_panel(app);
    }
    let kind = one.and_then(|object| {
        let drawing = app.session.drawing("drawing.inspect").ok()?;
        if drawing.view(object).is_some() {
            Some("view")
        } else if drawing.dimension(object).is_some() {
            Some("dimension")
        } else if drawing.notes.by_id(object).is_some() {
            Some("note")
        } else {
            None
        }
    });
    if let (Some(object), Some(kind)) = (one, kind) {
        edit_object(app, kind, object);
        app.drawing_workspace.shown_for = Some(object);
    }
}

/// Toolbar clicks inspect options. Engine execution still uses command parameters directly.
pub fn start(app: &mut SolveApp, id: &str) -> bool {
    // Select All selects what is on the sheet shown.
    if id == "select.all" && active(app) && app.session.open_drawing.is_some() {
        let sheet = active_sheet(app).map(solidcraft_engine::doc::ids::encode);
        if let Err(error) = app.run("select.all", serde_json::json!({"sheet": sheet})) {
            app.set_status(error, true);
        }
        return true;
    }
    if !active(app) || !can_open_options(id) {
        return false;
    }
    close_panel(app);
    match id {
        "drawing.sheet.add" => open_add_sheet(app),
        "drawing.sheet.set" => app.drawing_workspace.settings_open = true,
        "drawing.view.base" | "drawing.view.base.planned" => {
            app.drawing_workspace.placement = placement::Placement::new(app);
            app.drawing_workspace.options = Some(Tool::Base);
        }
        _ => app.drawing_workspace.options = Tool::for_command(id),
    }
    true
}

pub(crate) fn start_projected(app: &mut SolveApp, parent: u64) -> bool {
    let Some(tool) = placement::Placement::from_parent(app, parent) else { return false };
    close_panel(app);
    app.drawing_workspace.placement = Some(tool);
    app.drawing_workspace.options = Some(Tool::Projected);
    true
}

/// Edit object `object` of the open drawing, as a double-click on it at `at` does (the menu's Edit,
/// with `at` where the menu was opened).
pub(crate) fn edit(app: &mut SolveApp, object: u64, at: egui::Pos2) {
    let Ok(drawing) = app.session.drawing("drawing.inspect") else { return };
    let kind = if drawing.view(object).is_some() {
        "view"
    } else if drawing.dimension(object).is_some() {
        "dimension"
    } else if drawing.notes.by_id(object).is_some() {
        "note"
    } else if drawing.tables.by_id(object).is_some() {
        "parts_list"
    } else {
        return;
    };
    // A dimension's or a note's text is edited where it is drawn; the menu has no pointer on it,
    // so the box opens where the menu was opened.
    // From a row of the browser there is no place on the sheet to put the box: its options open.
    let on_sheet = app.viewport.rect.is_some_and(|sheet| sheet.contains(at)) && !crate::shell::covered(app, at);
    if on_sheet && edit_in_place(app, object, at) {
        return;
    }
    edit_object(app, kind, object);
}

/// A double-click on a dimension or a one-line note edits its text where it is, in the box a
/// sketch dimension's value is typed in (true: the box opened at `at`). Anything else (a view, a
/// table, a chain dimension, a note with fields or several lines) has no single text: its options
/// open instead ([`edit_object`]).
pub(crate) fn edit_in_place(app: &mut SolveApp, object: u64, at: egui::Pos2) -> bool {
    use crate::dim_view::SheetText;
    use solidcraft_engine::geom::cell::Part;
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return false };
    let (target, title, text) = if drawing.dimension(object).is_some() {
        let Ok(read) = app.run("drawing.dimension.inspect", serde_json::json!({"dimension": solidcraft_engine::doc::ids::encode(object)})) else {
            return false;
        };
        let Some(text) = read["text"].as_str().filter(|text| !text.contains('\n')) else { return false };
        (SheetText::Dimension(object), "ui.drawing_panel.dimension", text.to_owned())
    } else if let Some(note) = drawing.notes.by_id(object) {
        let text = note.content.0.iter().map(|part| if let Part::Text(text) = part { Some(text.as_str()) } else { None }).collect::<Option<String>>();
        let Some(text) = text.filter(|text| !text.contains('\n')) else { return false };
        (SheetText::Note(object), "ui.drawing_panel.note", text)
    } else {
        return false;
    };
    // The Dimension tool stays in hand while one of its sheet's dimensions is edited, as in a
    // sketch; any other panel gives way to the box.
    if !placing_dimension(app) {
        close_panel(app);
    }
    crate::dim_view::edit_sheet_text(target, solidcraft_ui_text::tr(title), text, at);
    true
}

/// A double-click on a text cell of the sheet's title block edits what the cell shows, where it
/// is, in the box a dimension's or a note's text is typed in (true: the box opened at `at`).
/// `point` is the pointer on paper. A cell that shows a logo or the projection symbol has no text.
pub(crate) fn edit_title_cell(app: &mut SolveApp, sheet: u64, point: Vec2, at: egui::Pos2) -> bool {
    use solidcraft_engine::doc::ids::encode;
    use solidcraft_engine::geom::title_block::CellKind;
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return false };
    let Some(record) = drawing.sheets.iter().find(|record| record.id == sheet) else { return false };
    let (Some(block), Ok(Some(placed))) = (&record.title_block, record.title_block_layout()) else { return false };
    let Some(cell) = block.cells.iter().zip(&placed.cells).find(|(_, placed)| placed.rect.contains(point)).map(|(cell, _)| cell) else {
        return false;
    };
    if !matches!(cell.kind, CellKind::Text(_)) {
        return false;
    }
    let Ok(read) = app.run("drawing.inspect", serde_json::json!({})) else { return false };
    let text = read["sheets"]
        .as_array()
        .and_then(|sheets| sheets.iter().find(|s| s["id"].as_str() == Some(encode(sheet).as_str())))
        .and_then(|s| s["title_block_cells"].as_array()?.iter().find(|c| c["name"].as_str() == Some(cell.name.as_str())))
        .and_then(|c| c["text"].as_str().map(str::to_owned));
    let Some(text) = text.filter(|text| !text.contains('\n')) else { return false };
    let title = cell.label.as_ref().map_or(cell.name.as_str(), |label| label.text.as_str()).to_owned();
    close_panel(app);
    crate::dim_view::edit_sheet_text(crate::dim_view::SheetText::TitleCell { sheet, cell: cell.name.clone() }, &title, text, at);
    true
}

pub(super) fn edit_object(app: &mut SolveApp, kind: &str, object: u64) {
    let tool = match kind {
        "view" => Tool::Edit(Some(object)),
        "dimension" => Tool::Dimension(Some(object)),
        "note" => Tool::Note(Some(object)),
        "parts_list" => Tool::PartsList(Some(object)),
        _ => return,
    };
    close_panel(app);
    app.drawing_workspace.options = Some(tool);
}

pub(crate) fn covered_rects(app: &SolveApp) -> Vec<Rect> {
    if !active(app) || app.preferences.drawing_sheets_visibility != "legacy" {
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

/// The sheet page draws the open drawing's sheet; with no drawing open there is no paper, sample
/// geometry or camera, only a note.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    follow_selection(app);
    if crate::drawing_sheet_page::show(app, ui) {
        return;
    }
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    let t = Tokens::get();
    ui.painter().rect_filled(rect, 0.0, t.viewport_top);
    let bounds = crate::shell::workspace_content(app, rect);
    first_sheet::show(app, ui, bounds);
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
        && app.preferences.drawing_sheets_visibility != *key
    {
        app.drawing_workspace.sheets_override = None;
        app.preferences.drawing_sheets_visibility = (*key).into();
    }
}

fn open_add_sheet(app: &mut SolveApp) {
    if let Some(form) = panels::AddSheet::open(app) {
        close_panel(app);
        app.drawing_workspace.add_sheet = Some(form);
    }
}

fn sheet_settings_actions(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(active) = active_sheet(app) else { return };
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return };
    if drawing.sheets.len() > 1 && !sheets_visible(app) && app.preferences.drawing_sheets_visibility != "legacy" {
        let active_name = drawing.sheets.iter().find(|sheet| sheet.id == active).map_or("", |sheet| sheet.name.as_str());
        let label = app.tr("ui.drawing_sheets.active").to_string();
        ui_kit::row(ui, &label, |ui| {
            ui_kit::dropdown_menu(ui, "drawing_active_sheet", active_name, |ui| {
                for sheet in &drawing.sheets {
                    let key = format!("drawing_active_sheet:{}", sheet.id);
                    if ui_kit::list_picker(ui, &key, &sheet.name, sheet.id == active).clicked() {
                        select_sheet(app, sheet.id);
                    }
                }
            });
        });
    }
    if !sheets_visible(app) && ui_kit::button(ui, app.tr("ui.drawing_sheets.add"), ui_kit::Button::Normal).clicked() {
        open_add_sheet(app);
    }
}

fn toggle_sheet_settings(app: &mut SolveApp) {
    let open = !app.drawing_workspace.settings_open;
    close_panel(app);
    app.drawing_workspace.settings_open = open;
}

pub(crate) fn sheets_visible(app: &SolveApp) -> bool {
    if !active(app) || app.preferences.drawing_sheets_visibility == "legacy" {
        return false;
    }
    let Ok(drawing) = app.session.drawing("drawing.inspect") else { return false };
    if app.preferences.drawing_sheets_visibility == "off" {
        return false;
    }
    app.drawing_workspace
        .sheets_override
        .filter(|(id, _)| *id == (app.session.uid, drawing.id))
        .map_or_else(|| app.preferences.drawing_sheets_visibility == "always" || drawing.sheets.len() > 1, |(_, open)| open)
}

pub(crate) fn set_sheets_visible(app: &mut SolveApp, open: bool) {
    if active(app)
        && let Ok(drawing) = app.session.drawing("drawing.inspect")
        && sheets_visible(app) != open
    {
        app.drawing_workspace.sheets_override = Some(((app.session.uid, drawing.id), open));
    }
}

pub(crate) fn sheets_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return };
    let id = ("drawing_sheets", app.session.uid, drawing.id);
    let t = Tokens::get();
    let selected = active_sheet(app);
    let top = ui.next_widget_position().y;
    let limit = t.space_24 * 9.0;
    let body = egui::ScrollArea::vertical().id_salt(id).min_scrolled_height(0.0).max_height(limit).show(ui, |ui| {
        for pair in drawing.sheets.chunks(2) {
            ui.spacing_mut().item_spacing.x = t.space_4;
            ui.columns(2, |columns| {
                for (ui, sheet) in columns.iter_mut().zip(pair) {
                    let width = ui.available_width();
                    ui.push_id(sheet.id, |ui| sheet_card(app, ui, &drawing, sheet, width, selected == Some(sheet.id)));
                }
            });
        }
    });
    crate::panel_runtime::report_content_extent(ui, top, body.content_size.y.min(limit), false);
}

fn sheet_card(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    drawing: &solidcraft_engine::doc::drawing::Drawing,
    sheet: &solidcraft_engine::doc::drawing::Sheet,
    width: f32,
    selected: bool,
) {
    let t = Tokens::get();
    let accent = solidcraft_theme::TabAccent::current(ui.ctx());
    let row = ui_kit::row_height();
    let (card, _) = ui.allocate_exact_size(vec2(width, t.space_24 * 3.5 + row), Sense::hover());
    let photo = Rect::from_min_max(card.min, pos2(card.right(), card.bottom() - row));
    let rect = photo.shrink(t.space_4);
    let bottom = Rect::from_min_max(pos2(card.left(), photo.bottom()), card.max);
    let menu = Rect::from_min_max(pos2((bottom.right() - row).max(bottom.left()), bottom.top()), bottom.right_bottom());
    let name = Rect::from_min_max(pos2((bottom.left() + t.space_8).min(menu.left()), bottom.top()), menu.left_bottom());
    let key = solidcraft_engine::doc::ids::encode(sheet.id);
    let target = format!("drawing:sheets:sheet:{key}");
    let response = ui.interact(photo, ui.scope_id().with(&target), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, &sheet.name));
    crate::scenario::publish_target(&target, photo);
    ui.painter().rect_filled(
        card,
        t.radius_md,
        if selected {
            accent.soft
        } else if response.hovered() {
            t.hover
        } else {
            t.pref_well
        },
    );
    ui.painter().rect_stroke(
        rect,
        t.radius_sm,
        t.control_border_stroke(if selected { accent.ink } else { t.control_border }),
        egui::StrokeKind::Inside,
    );
    paint_sheet_thumbnail(ui, rect, app, drawing, sheet);
    let ink = if selected { accent.ink } else { t.form_ink };
    let label = egui::RichText::new(&sheet.name).size(t.font_label).color(ink);
    let label = if selected { label.strong() } else { label };
    let name_response = ui.put(name, egui::Label::new(label).truncate().sense(Sense::click()));
    if response.clicked() || name_response.clicked() {
        select_sheet(app, sheet.id);
    }

    let menu_button = ui_kit::icon_button_at(ui, menu, "dots", app.tr("ui.drawing_sheets.actions"));
    crate::scenario::publish_target(&format!("drawing:sheets:menu:{key}"), menu_button.rect);
    if menu_button.clicked() {
        let owner = (app.session.uid, drawing.id, sheet.id);
        let open = app.drawing_workspace.sheet_menu.as_ref().is_some_and(|menu| menu.owner == owner);
        if open {
            app.drawing_workspace.sheet_menu = None;
        } else {
            open_sheet_menu(app, drawing.id, sheet.id, menu);
        }
    }
}

struct SheetMenu {
    owner: (u64, u64, u64),
    anchor: Rect,
    keyboard_row: Option<usize>,
}

fn open_sheet_menu(app: &mut SolveApp, drawing: u64, sheet: u64, anchor: Rect) {
    app.drawing_workspace.sheet_menu = Some(SheetMenu { owner: (app.session.uid, drawing, sheet), anchor, keyboard_row: None });
}

fn show_sheet_menu(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut menu) = app.drawing_workspace.sheet_menu.take() else { return };
    let (document, drawing_id, sheet_id) = menu.owner;
    if !active(app) || app.session.uid != document {
        return;
    }
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return };
    if drawing.id != drawing_id {
        return;
    }
    let Some(sheet) = drawing.sheets.iter().find(|sheet| sheet.id == sheet_id) else { return };
    let popup = Container::new(ctx, Surface::Popup)
        .area(egui::Id::unique(("drawing_sheet_actions", menu.owner)))
        .fixed_pos(menu.anchor.right_bottom())
        .constrain(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui_kit::Menu::show(ui, ui_kit::LIST_W, |ui| {
                use ui_kit::Item;
                ui_kit::Menu::heading(ui, &sheet.name);
                let items = [
                    Item::action("settings", app.tr("cmd.drawing_sheet_set"), "settings"),
                    Item::action("rename", app.tr_label("Rename"), "rename"),
                    Item::action("duplicate", app.tr("ui.drawing_sheets.duplicate"), "copy")
                        .off_because(Some(app.tr("ui.drawing_first.unavailable").into())),
                    Item::action("add", app.tr("ui.drawing_sheets.add"), "plus"),
                    Item::sep(),
                    Item::action("delete", app.tr("cmd.drawing_sheet_delete"), "delete")
                        .off_because((drawing.sheets.len() <= 1).then(|| app.tr("ui.drawing_sheets.last_sheet").into())),
                ];
                let enabled = items.each_ref().map(|item| item.enabled && !item.is_sep());
                let keyboard_pick = ui_kit::Menu::navigate(ctx, &enabled, &mut menu.keyboard_row)
                    .then(|| menu.keyboard_row.and_then(|index| items.get(index)).cloned())
                    .flatten();
                items
                    .iter()
                    .enumerate()
                    .fold(keyboard_pick, |chosen, (index, item)| ui_kit::menu::list_row_lit(ui, item, menu.keyboard_row == Some(index)).or(chosen))
            })
        });
    if let Some(action) = popup.inner {
        match action.id.as_str() {
            "settings" | "rename" => {
                select_sheet(app, sheet_id);
                start(app, "drawing.sheet.set");
                if action.id == "rename" {
                    app.drawing_workspace.rename_sheet = Some(sheet_id);
                }
            }
            "add" => {
                if let Some(form) = panels::AddSheet::open_after(app, Some(sheet_id)) {
                    close_panel(app);
                    app.drawing_workspace.add_sheet = Some(form);
                }
            }
            "delete" => app.drawing_workspace.delete_sheet = Some(menu.owner),
            _ => {}
        }
        return;
    }
    let escape = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let outside = ctx.input(|input| {
        input.pointer.any_pressed()
            && input.pointer.interact_pos().is_some_and(|point| !menu.anchor.contains(point) && !popup.response.rect.contains(point))
    });
    if !escape && !outside {
        app.drawing_workspace.sheet_menu = Some(menu);
    }
}

fn paint_sheet_thumbnail(
    ui: &egui::Ui,
    rect: Rect,
    app: &SolveApp,
    drawing: &solidcraft_engine::doc::drawing::Drawing,
    sheet: &solidcraft_engine::doc::drawing::Sheet,
) {
    let t = Tokens::get();
    let inner = rect.shrink(t.space_4);
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
    // Use cached extents without requesting work for another sheet on the UI thread.
    for view in drawing.views.iter().filter(|view| view.sheet == sheet.id) {
        if let Ok(centre) = drawing.view_centre(view.id) {
            if let Some(bounds) = solidcraft_engine::drawing_scene::cached_view_boundary(&app.session, drawing.id, view.id) {
                let outline = Rect::from_two_pos(at(bounds.min + centre), at(bounds.max + centre));
                painter.rect_stroke(outline, 0.0, stroke, egui::StrokeKind::Inside);
            } else {
                painter.circle_filled(at(centre), t.space_2, t.paper_ink);
            }
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
pub fn chrome(app: &mut SolveApp, ui: &mut egui::Ui, domain: Rect) {
    app.drawing_workspace.strip = None;
    if !active(app) || app.preferences.drawing_sheets_visibility != "legacy" {
        return;
    }
    let mut available = crate::agent_chat::tool_space(app, domain);
    if let Some(settings) = settings_rect(app) {
        let gap = Tokens::get().space_8;
        if app.preferences.drawing_panel_side == "left" {
            available.min.x = available.left().max(settings.right() + gap).min(available.right());
        } else {
            available.max.x = available.right().min(settings.left() - gap).max(available.left());
        }
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
                    toggle_sheet_settings(app);
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

pub(crate) fn show_add_sheet(app: &mut SolveApp, ctx: &egui::Context) {
    first_sheet::show_picker(app, ctx);
    show_sheet_menu(app, ctx);
    show_delete_sheet(app, ctx);
    if active(app)
        && let Some(form) = app.drawing_workspace.add_sheet.take()
    {
        app.drawing_workspace.add_sheet = panels::add_window(app, ctx, form);
    }
}

fn show_delete_sheet(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending @ (document, drawing_id, sheet_id)) = app.drawing_workspace.delete_sheet.take() else { return };
    if !active(app) || app.session.uid != document {
        return;
    }
    let Ok(drawing) = app.session.drawing("drawing.inspect") else { return };
    if drawing.id != drawing_id || drawing.sheets.len() <= 1 {
        return;
    }
    let Some(sheet) = drawing.sheets.iter().find(|sheet| sheet.id == sheet_id) else { return };
    let label = app.tr("cmd.drawing_sheet_delete");
    let message = solidcraft_ui_text::format("ui.drawing_sheets.delete_confirm", &[("name", &sheet.name)]);
    let (closed, answer) = ui_kit::Confirm::new(egui::Id::unique(("drawing_delete_sheet", pending)), "delete", label)
        .tone(ui_kit::Tone::Danger)
        .actions(label, app.tr("ui.cancel"), None)
        .show(ctx, true, |ui| {
            ui.label(message);
        });
    if answer == ui_kit::Action::Ok {
        if let Err(error) =
            app.run("drawing.sheet.delete", serde_json::json!({"sheet":solidcraft_engine::doc::ids::encode(sheet_id),"contents":"delete"}))
        {
            app.set_status(error, true);
        }
    } else if !closed && answer != ui_kit::Action::Cancel {
        app.drawing_workspace.delete_sheet = Some(pending);
    }
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
                panel_body(app, ui);
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
    let panel = ui_kit::Panel::new(panel_title(app), egui::Id::unique("sc_drawing_settings_floating"), anchor, available, side, frame).width(width);
    if let Some(card) = panel.window(&ctx).fixed_size(inner).show(&ctx, |ui| {
        ui.set_min_size(inner);
        ui.set_max_size(inner);
        ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, inner)));
        ui_kit::scope(ui, |ui| {
            panel_body(app, ui);
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

fn panel_title(app: &SolveApp) -> &'static str {
    app.drawing_workspace.options.map_or_else(|| solidcraft_ui_text::tr("cmd.drawing_sheet_set"), Tool::title)
}

pub(crate) fn inspector_visible(app: &SolveApp) -> bool {
    active(app)
        && app.preferences.drawing_sheets_visibility != "legacy"
        && (app.drawing_workspace.settings_open || app.drawing_workspace.options.is_some())
}

pub(crate) fn inspector_title(app: &SolveApp) -> &str {
    panel_title(app)
}

pub(crate) fn inspector_contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    let top = ui.next_widget_position().y;
    let body = egui::ScrollArea::vertical().id_salt("drawing_inspector").min_scrolled_height(0.0).show(ui, |ui| panel_fields(app, ui));
    crate::panel_runtime::report_content_extent(ui, top, body.content_size.y, false);
}

fn panel_body(app: &mut SolveApp, ui: &mut egui::Ui) {
    let options = app.drawing_workspace.options;
    ui.horizontal(|ui| {
        ui.heading(panel_title(app));
        if icon_action(ui, "drawing:settings:close", "close", "Close drawing panel", true).clicked() {
            close_panel(app);
        }
    });
    if options.is_none() {
        ui.horizontal(|ui| {
            for (side, label) in [("left", "Dock left"), ("right", "Dock right")] {
                if ui_kit::button(ui, solidcraft_ui_text::tr_label(label), ui_kit::Button::Normal).clicked() {
                    app.preferences.drawing_panel_side = side.into();
                }
            }
        });
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| panel_fields(app, ui));
}

/// `add` inset by the panel's standard margin, without a frame of its own (the panel draws it).
pub(crate) fn inset<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let margin = Tokens::get().space_12;
    ui.add_space(margin);
    let rect = ui.available_rect_before_wrap().shrink2(vec2(margin, 0.0));
    let inner = ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add).inner;
    ui.add_space(margin);
    inner
}

fn panel_fields(app: &mut SolveApp, ui: &mut egui::Ui) {
    if let Some(placement) = &mut app.drawing_workspace.placement {
        let finished = inset(ui, |ui| {
            placement.fields(ui, &app.session.doc.name);
            panels::finish_placement(ui)
        });
        if finished {
            close_panel(app);
        }
    } else if let Some(Tool::Note(selected)) = app.drawing_workspace.options {
        note::editor(app, ui, selected);
    } else if let Some(Tool::Edit(view)) = app.drawing_workspace.options {
        if panels::view_editor(app, ui, view) {
            close_panel(app);
        }
    } else if placing_dimension(app) {
        dimension::panel(app, ui);
    } else if let Some(Tool::Dimension(Some(placed))) = app.drawing_workspace.options {
        if panels::dimension_editor(app, ui, placed) {
            close_panel(app);
        }
    } else if let Some(tool) = app.drawing_workspace.options {
        panels::tool_body(app, ui, tool);
        if panels::footer(ui, tool) {
            close_panel(app);
        }
    } else if app.drawing_workspace.settings_open {
        panels::sheet_settings(app, ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    /// The Drawing workspace is a normal workspace: it opens with default preferences.
    #[test]
    fn drawing_workspace_opens_with_default_preferences() {
        let mut h = Harness::new();
        h.app.home.open = false;
        let on =
            crate::context_menu::items(&h.app, &crate::context_menu::Target::Workspace).into_iter().find(|item| item.label == "Drawing").unwrap();
        assert!(on.enabled && on.id == "ui.workspace.drawing");
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        assert!(active(&h.app));
    }

    /// A preferences file written while Drawings was a setting still loads: the retired key is
    /// ignored and the other settings in it are kept.
    #[test]
    fn saved_preferences_with_the_retired_drawings_key_still_load() {
        let mut h = Harness::new();
        h.app.load_prefs(r#"{"preferences":{"drawings_preview":false,"default_units":"in"}}"#);
        assert_eq!(h.app.preferences.default_units, "in");
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        assert!(active(&h.app));
    }

    /// A box in Drawing, with a drawing of one base view open: its id.
    fn drawing_with_a_view(h: &mut Harness) -> Option<u64> {
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        h.app.run("drawing.view.base", serde_json::json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[150,150]})).unwrap();
        h.app.session.open_drawing
    }

    /// A design saved with a drawing and opened again shows that drawing when Drawing is entered,
    /// on its first sheet, not the page that starts a new drawing.
    #[test]
    fn a_saved_drawing_opens_again_when_drawing_is_entered() {
        let dir = std::env::temp_dir().join(format!("solidcraft-drawing-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft").to_string_lossy().into_owned();
        let mut h = Harness::new();
        let drawing = drawing_with_a_view(&mut h);
        assert!(drawing.is_some());
        h.app.run("file.save", serde_json::json!({"path": path})).unwrap();
        h.app.run("doc.open", serde_json::json!({"path": path})).unwrap();
        assert!(!active(&h.app) && h.app.session.open_drawing.is_none(), "a design opens in Design");
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        h.frames(4);
        assert_eq!(h.app.session.open_drawing, drawing, "entering Drawing opens the design's drawing");
        let first = h.app.session.drawing("test").unwrap().sheets[0].id;
        assert_eq!(active_sheet(&h.app), Some(first), "on its first sheet");
        assert!(crate::scenario::handle_at("drawing:first:standard").is_none(), "not the new-drawing page");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Leaving Drawing leaves the drawing open: the model changes in Design with it open, and coming
    /// back finds the same drawing. Undo takes the steps of the workspace shown: the drawing's in
    /// Drawing, the model's in Design and Render.
    #[test]
    fn leaving_drawing_keeps_the_drawing_open_and_each_workspace_undoes_its_own() {
        let mut h = Harness::new();
        let drawing = drawing_with_a_view(&mut h);
        crate::workspace::enter(&mut h.app, "design").unwrap();
        assert!(!active(&h.app));
        assert_eq!(h.app.session.open_drawing, drawing, "leaving Drawing keeps the drawing open");
        h.app.run("solid.box", serde_json::json!({"length":5,"width":5,"height":5})).unwrap();
        let features = h.app.session.doc.features.len();
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        assert_eq!(h.app.session.open_drawing, drawing, "the same drawing");
        let views = |h: &Harness| h.app.session.drawing("test").unwrap().views.len();
        assert_eq!(views(&h), 1);
        h.app.run("edit.undo", serde_json::json!({})).unwrap();
        assert_eq!(views(&h), 0, "undo in Drawing takes back the drawing's step");
        assert_eq!(h.app.session.doc.features.len(), features, "and not the model's");
        crate::workspace::enter(&mut h.app, "render").unwrap();
        assert_eq!(h.app.session.open_drawing, drawing, "Render leaves Drawing the same way");
        crate::workspace::enter(&mut h.app, "design").unwrap();
        h.app.run("edit.undo", serde_json::json!({})).unwrap();
        assert_eq!(h.app.session.doc.features.len(), features - 1, "undo in Design takes back the model's step");
        assert_eq!(views(&h), 0, "and leaves the drawing's alone");
    }

    /// With a drawing open, a model edit in Design (the box's length) works, and the drawing's view
    /// of it is out of date when Drawing is entered again. Drawing → Design → Drawing keeps the
    /// drawing on the same sheet and zoom, and Design's tab, selection, camera and history are as
    /// Design left them.
    #[test]
    fn workspaces_keep_their_own_state_and_model_edits_mark_the_views() {
        let mut h = Harness::new();
        let drawing = drawing_with_a_view(&mut h);
        crate::workspace::enter(&mut h.app, "design").unwrap();
        h.app.ui.tab = "SURFACE".into();
        let body = h.app.session.model.state().bodies[0].name.clone();
        h.app.run("select.set", serde_json::json!({"items":[{"type":"body","name":body}]})).unwrap();
        let (selected, cam, undo, redo) = (h.app.session.selection.clone(), h.app.cam, h.app.session.undo.len(), h.app.session.redo.len());
        assert!(!selected.is_empty());

        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        let first = h.app.session.drawing("test").unwrap().sheets[0].id;
        solidcraft_engine::drawing_scene::extract(&mut h.app.session, first).unwrap();
        while solidcraft_engine::drawing_scene::step(&mut h.app.session) {}
        let current = h.app.run("drawing.status", serde_json::json!({})).unwrap();
        assert_eq!(current["views"][0]["status"], "current", "{current}");
        h.app.run("drawing.sheet.add", serde_json::json!({"name":"Second"})).unwrap();
        h.frames(3);
        let second = h.app.session.drawing("test").unwrap().sheets[1].id;
        select_sheet(&mut h.app, second);
        h.frames(3);
        let view = h.app.session.drawing("test").unwrap().views[0].id;
        let picked = serde_json::json!({"type":"drawing","drawing":solidcraft_engine::doc::ids::encode(drawing.unwrap()),"object":solidcraft_engine::doc::ids::encode(view)});
        h.app.run("select.set", serde_json::json!({"items":[picked]})).unwrap();
        let sheet_selection = h.app.session.selection.clone();
        let page = |h: &Harness| {
            let shown = crate::drawing_sheet_page::inspect(&h.app);
            (shown["drawing"].clone(), shown["sheet"].clone(), shown["zoom"].clone(), shown["visible_mm"].clone())
        };
        let shown = page(&h);
        assert_eq!(shown.1, serde_json::json!(solidcraft_engine::doc::ids::encode(second)));
        let drawing_steps = h.app.session.undo.len();

        crate::workspace::enter(&mut h.app, "design").unwrap();
        h.frames(3);
        assert_eq!(h.app.ui.tab, "SURFACE", "Design's tab");
        assert_eq!(h.app.session.selection, selected, "Design's selection");
        assert_eq!(h.app.cam, cam, "Design's camera");
        assert_eq!(h.app.session.undo.len(), drawing_steps, "the drawing's new sheet is its own step");
        assert!(undo < drawing_steps && h.app.session.redo.len() == redo);
        let block = h.app.session.doc.features[0].id;
        h.app.run("timeline.edit", serde_json::json!({"feature": block.to_string(), "set": {"length": 60}})).unwrap();
        assert_eq!(h.app.session.open_drawing, drawing, "the model edit leaves the drawing open");
        let status = h.app.run("drawing.status", serde_json::json!({})).unwrap();
        assert_eq!(status["views"][0]["status"], "out_of_date", "the view of the edited box: {status}");

        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        h.frames(3);
        assert_eq!(h.app.session.open_drawing, drawing, "the same drawing");
        assert_eq!(active_sheet(&h.app), Some(second), "on the same sheet");
        assert_eq!(page(&h), shown, "at the same zoom and place");
        assert_eq!(h.app.session.selection, sheet_selection, "with its own selection");
    }

    /// The drawing open when Drawing was left is still open when it is entered again, not the
    /// design's first; a design opened afresh starts on its first drawing.
    #[test]
    fn the_drawing_left_last_comes_back_not_the_first() {
        let mut h = Harness::new();
        drawing_with_a_view(&mut h);
        h.app.run("drawing.create", serde_json::json!({"name":"Second"})).unwrap();
        let ids: Vec<u64> = h.app.session.doc.drawings.iter().map(|d| d.id).collect();
        let (first, later) = (ids[0], ids[1]);
        h.app.run("drawing.open", serde_json::json!({"drawing": solidcraft_engine::doc::ids::encode(later)})).unwrap();
        crate::workspace::enter(&mut h.app, "design").unwrap();
        assert_eq!(h.app.session.open_drawing, Some(later));
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        assert_eq!(h.app.session.open_drawing, Some(later), "the drawing left last");
        crate::workspace::enter(&mut h.app, "design").unwrap();
        h.app.run("drawing.close", serde_json::json!({})).unwrap();
        reset_document(&mut h.app);
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        assert_eq!(h.app.session.open_drawing, Some(first), "a fresh start opens the design's first drawing");
    }

    /// A sheet sketch being edited never holds Design: Drawing can be left with it open, the
    /// model's commands and sketch tools work on the model there, undo takes the model's steps, and
    /// back in Drawing the sheet sketch is still being edited.
    #[test]
    fn a_sheet_sketch_stays_open_in_drawing_while_design_changes_the_model() {
        let mut h = Harness::new();
        let drawing = drawing_with_a_view(&mut h);
        let sheet = h.app.session.drawing("test").unwrap().sheets[0].id;
        h.app.run("drawing.sketch.create", serde_json::json!({"owner": solidcraft_engine::doc::ids::encode(sheet)})).unwrap();
        let sketch = h.app.session.active_sheet_sketch.clone();
        crate::workspace::enter(&mut h.app, "design").unwrap();
        assert!(!active(&h.app));
        assert_eq!(h.app.session.open_drawing, drawing);
        let features = h.app.session.doc.features.len();
        h.app.run("solid.box", serde_json::json!({"length":5,"width":5,"height":5})).unwrap();
        assert_eq!(h.app.session.doc.features.len(), features + 1, "the model changes with the sheet sketch open");
        h.app.run("edit.undo", serde_json::json!({})).unwrap();
        assert_eq!(h.app.session.doc.features.len(), features, "undo in Design takes back the model's step");
        assert_eq!(h.app.session.active_sheet_sketch, sketch, "and leaves the sheet sketch being edited");
        h.app.run("sketch.create", serde_json::json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", serde_json::json!({"center": [0, 0], "radius": 3})).unwrap();
        h.app.run("sketch.finish", serde_json::json!({})).unwrap();
        assert!(h.app.session.active_sketch.is_none(), "Finish in Design finishes the model's sketch");
        assert_eq!(h.app.session.active_sheet_sketch, sketch, "and leaves the sheet sketch alone");
        crate::workspace::enter(&mut h.app, "drawing").unwrap();
        assert!(h.app.session.sheet_sketching(), "back in Drawing the sheet sketch is still being edited");
        h.app.run("sketch.finish", serde_json::json!({})).unwrap();
        assert!(h.app.session.active_sheet_sketch.is_none());
    }

    #[test]
    fn view_interior_opens_the_card_and_moves_a_view_in_one_undo_step() {
        use serde_json::json;
        use solidcraft_engine::{doc::ids::decode, drawing_scene};

        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":60,"width":35,"height":12})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", json!({})).unwrap();
        let created = h.app.run("drawing.view.base", json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[175,140]})).unwrap();
        let id = decode(created["view"].as_str().unwrap()).unwrap();
        let sheet = active_sheet(&h.app).unwrap();
        drawing_scene::extract(&mut h.app.session, sheet).unwrap();
        drawing_scene::step(&mut h.app.session);
        h.frames(6);
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        let mm_x = 175.0;
        let x = paper.left() + paper.width() * (mm_x / 420.0);
        let y = paper.bottom() - paper.height() * (140.0 / 297.0);
        h.step(&json!({"move":[x,y]})).unwrap();
        let hover = &crate::drawing_sheet_page::inspect(&h.app)["hover"];
        assert_eq!(hover["object"], created["view"]);
        assert_eq!(hover["on_edge"], false);
        // The hovered view is lit in the accent colour, as a sketch shows what the pointer is over
        // (it was outlined with a box before).
        assert_eq!(hover["lit"], true);
        h.step(&json!({"click":[x,y]})).unwrap();
        h.frames(3);
        // Selected, with its options on the right and nothing floating over the sheet: what the
        // card offered is in the right-click menu.
        assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Drawing { drawing: h.app.session.open_drawing.unwrap(), object: id }]);
        assert!(crate::scenario::handle_at("drawing:view_selection:project").is_none(), "no card over the view");
        assert_eq!(inspector_title(&h.app), "Edit View");
        h.step(&json!({"click":[x,y], "button":"right"})).unwrap();
        h.frames(3);
        let menu = crate::context_menu::describe(&h.app);
        let rows: Vec<&str> = menu["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
        assert_eq!(&rows[..3], ["Edit View", "Projected View", "Delete"], "{rows:?}");
        crate::context_menu::close(&mut h.app);
        h.frames(2);

        let before = h.app.session.undo.len();
        h.step(&json!({"drag":[[x,y],[x+60.0,y+24.0]]})).unwrap();
        h.frames(4);
        let centre = |h: &Harness| h.app.session.drawing("drawing.inspect").unwrap().view_centre(id).unwrap();
        let after = centre(&h);
        assert!(after.x > 175.0 && after.y < 140.0, "{after:?}");
        assert_eq!(h.app.session.undo.len(), before + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(centre(&h), Vec2::new(175.0, 140.0));

        // Delete is the Delete key's, with no question of the drawing's own, and Ctrl+Z undoes it.
        h.frames(3);
        h.step(&json!({"click":[x,y]})).unwrap();
        h.frames(3);
        h.step(&json!({"key":"Delete"})).unwrap();
        h.frames(4);
        assert!(h.app.session.drawing("drawing.inspect").unwrap().view(id).is_none(), "the view is deleted at once");
        h.step(&json!({"key":"Z", "cmd": true})).unwrap();
        h.settle();
        assert!(h.app.session.drawing("drawing.inspect").unwrap().view(id).is_some(), "and Ctrl+Z brings it back");
    }

    /// A blank sheet's ribbon offers every tool that has a working panel: a click opens it.
    #[test]
    fn a_blank_sheets_working_tools_open_from_the_ribbon() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        h.frames(6);
        for (tab, id, tool) in [
            ("DRAWING LAYOUT", "drawing.view.base.planned", Tool::Base),
            ("DRAWING ANNOTATE", "drawing.dimension", Tool::Dimension(None)),
            ("DRAWING ANNOTATE", "drawing.note", Tool::Note(None)),
            ("DRAWING PARTS", "drawing.parts_list", Tool::PartsList(None)),
        ] {
            close_panel(&mut h.app);
            h.app.ui.tab = tab.into();
            h.frames(4);
            assert!(crate::scenario::handle_at(&format!("button:{id}")).is_some(), "{id} is on {tab}");
            h.step(&serde_json::json!({"click":{"handle":format!("button:{id}")}})).unwrap();
            h.frames(4);
            assert_eq!(h.app.drawing_workspace.options, Some(tool), "{id} opens its panel from the ribbon");
            let spec = solidcraft_engine::find_command(id).unwrap();
            assert!(!crate::toolbar::coming_soon(&h.app, spec), "{id} is drawn as a working tool, not faded");
        }
        for id in ["drawing.update.planned", "drawing.view.section.planned", "drawing.export.planned"] {
            let spec = solidcraft_engine::find_command(id).unwrap();
            assert!(crate::toolbar::coming_soon(&h.app, spec), "{id} opens nothing yet: faded, coming soon");
        }
        assert_eq!(h.app.tr("ui.drawing_ribbon.planned"), "Coming soon");
    }

    /// A drawing's views and notes are picked, selected all, cleared and deleted with the model's
    /// own controls: click, Shift-click, Ctrl+A, Escape and Delete on the session's selection.
    #[test]
    fn drawing_objects_take_the_shared_selection_keys() {
        use serde_json::json;
        use solidcraft_engine::Sel;
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", json!({})).unwrap();
        h.app.run("drawing.view.base", json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[120,150]})).unwrap();
        h.app.run("drawing.view.base", json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[260,150],"view":"top"})).unwrap();
        let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
        let began = std::time::Instant::now();
        while began.elapsed() < std::time::Duration::from_secs(10) {
            h.frame();
            if crate::drawing_jobs::idle(&h.app)
                && solidcraft_engine::drawing_scene::extract(&mut h.app.session, sheet)
                    .is_ok_and(|v| v.objects.iter().filter(|o| o.kind == solidcraft_engine::geom::paper::ObjectKind::View).count() >= 2)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        h.frames(4);
        let drawing = h.app.session.drawing("drawing.inspect").unwrap().clone();
        let [a, b] = [drawing.views[0].id, drawing.views[1].id];
        let size = drawing.sheets[0].sheet.size_mm().unwrap();
        let screen = |h: &Harness, view: u64| {
            let paper = crate::scenario::rect_of("sheet:paper").unwrap();
            let c = h.app.session.drawing("drawing.inspect").unwrap().view_centre(view).unwrap();
            [paper.left() + (c.x / size.x) as f32 * paper.width(), paper.bottom() - (c.y / size.y) as f32 * paper.height()]
        };
        let picked = |h: &Harness| -> Vec<u64> {
            h.app.session.selection.iter().filter_map(|s| if let Sel::Drawing { object, .. } = s { Some(*object) } else { None }).collect()
        };

        h.step(&json!({"click": screen(&h, a)})).unwrap();
        h.frames(2);
        assert_eq!(picked(&h), [a], "a click selects the view under it");
        h.step(&json!({"click": screen(&h, b), "shift": true})).unwrap();
        h.frames(2);
        assert_eq!(picked(&h), [a, b], "Shift-click adds, as in the model");

        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.session.selection.is_empty(), "Escape clears the selection");

        h.step(&json!({"key":"A", "cmd": true})).unwrap();
        h.frames(2);
        assert_eq!(picked(&h), [a, b], "Ctrl+A selects everything on the sheet");

        assert!(start(&mut h.app, "drawing.note"));
        h.frames(2);
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.drawing_workspace.options, None, "Escape puts the tool down first");
        assert_eq!(picked(&h), [a, b], "and keeps the selection for the next press");

        let undo = h.app.session.undo.len();
        h.step(&json!({"click": screen(&h, a)})).unwrap();
        h.frames(2);
        h.step(&json!({"key":"Delete"})).unwrap();
        h.frames(4);
        let left = h.app.session.drawing("drawing.inspect").unwrap();
        assert!(left.view(a).is_none() && left.view(b).is_some(), "Delete removes only the selected view");
        assert_eq!(h.app.session.undo.len(), undo + 1, "in one undo step");
        assert!(h.app.session.selection.is_empty());

        h.step(&json!({"key":"Z", "cmd": true})).unwrap();
        h.frames(2);
        assert!(h.app.session.drawing("drawing.inspect").unwrap().view(a).is_some(), "Ctrl+Z brings it back");
        h.step(&json!({"key":"Y", "cmd": true})).unwrap();
        h.frames(2);
        assert!(h.app.session.drawing("drawing.inspect").unwrap().view(a).is_none(), "and Ctrl+Y deletes it again");
    }

    /// The key that starts Dimension in a sketch starts Dimension on a sheet: D, or whatever key
    /// Sketch Dimension was rebound to.
    #[test]
    fn the_sketch_dimension_key_starts_the_dimension_tool_on_a_sheet() {
        use serde_json::json;
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", json!({})).unwrap();
        h.app.run("drawing.view.base", json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[120,150]})).unwrap();
        h.frames(8);
        assert!(!placing_dimension(&h.app));
        h.step(&json!({"key":"D"})).unwrap();
        h.frames(2);
        assert!(placing_dimension(&h.app), "D starts Dimension, as in a sketch");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(!placing_dimension(&h.app), "and Esc puts it down");

        h.call("ui.shortcut", json!({"command": "sketch.dimension", "key": "M"}));
        h.step(&json!({"key":"D"})).unwrap();
        h.frames(2);
        assert!(!placing_dimension(&h.app), "the old key no longer starts it");
        h.step(&json!({"key":"M"})).unwrap();
        h.frames(2);
        assert!(placing_dimension(&h.app), "the key Sketch Dimension was rebound to does");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);

        // A chord too, and the chords the app keeps for itself stay its own.
        h.call("ui.shortcut", json!({"command": "sketch.dimension", "key": "Ctrl+M"}));
        h.step(&json!({"key":"M", "cmd": true})).unwrap();
        h.frames(2);
        assert!(placing_dimension(&h.app), "Ctrl+M starts it once Sketch Dimension is bound to it");
    }

    #[test]
    fn base_view_places_projected_views_until_escape() {
        use solidcraft_engine::doc::drawing::view::{ViewKind, ViewPlace};
        use solidcraft_engine::geom::drawing_view::ProjectedSide;

        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        h.frames(6);
        h.step(&serde_json::json!({"click":{"handle":"button:drawing.view.base.planned"}})).unwrap();
        h.frames(6);
        assert!(placing_view(&h.app));
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        let y = paper.center().y;
        let base_x = paper.left() + paper.width() * 0.35;
        let projected_x = paper.left() + paper.width() * 0.57;
        h.step(&serde_json::json!({"move":[base_x,y]})).unwrap();
        assert!(h.app.session.drawing("drawing.inspect").unwrap().views.is_empty());
        h.step(&serde_json::json!({"click":[base_x,y]})).unwrap();
        h.frames(4);
        h.step(&serde_json::json!({"click":[projected_x,y]})).unwrap();
        h.step(&serde_json::json!({"key":"Escape"})).unwrap();
        h.frames(2);

        assert!(!placing_view(&h.app));
        let drawing = h.app.session.drawing("drawing.inspect").unwrap();
        assert_eq!(drawing.views.len(), 2);
        let base = drawing.views.iter().find(|view| matches!(view.kind, ViewKind::Base { .. })).unwrap();
        let projected = drawing.views.iter().find(|view| view.kind.parent() == Some(base.id)).unwrap();
        assert!(matches!(projected.kind, ViewKind::Projected { side: ProjectedSide::Right, .. }));
        assert!(matches!(projected.place, ViewPlace::Aligned(_)));
        assert_eq!(drawing.view_centre(base.id).unwrap().y, drawing.view_centre(projected.id).unwrap().y);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn base_view_snapshot_draws_marks_while_a_drawing_is_open() {
        let mut h = Harness::new();
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        let drawing = h.app.session.open_drawing;
        let marks = placement::snapshot_marks(
            h.app.session.scratch(),
            &serde_json::json!({"view":"front","scale":"1:1","position_mm":[0,0],"scene":true,"chord_mm":0.1}),
        )
        .unwrap();
        assert!(!marks.marks.is_empty());
        assert_eq!(h.app.session.open_drawing, drawing);
    }

    #[test]
    fn a_smaller_sheet_warns_about_views_without_moving_them() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        let view = h.app.run("drawing.view.base", serde_json::json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[310,180]})).unwrap();
        assert!(start(&mut h.app, "drawing.sheet.set"));
        h.frames(4);
        h.step(&serde_json::json!({"click":{"handle":"drawing:settings:paper"}})).unwrap();
        h.frames(3);
        h.step(&serde_json::json!({"click":{"handle":"drawing:settings:paper:A4"}})).unwrap();
        h.frames(4);
        assert!(h.widget("Views outside the drawing area (1)").is_some());
        let inspected = h.app.run("drawing.view.inspect", serde_json::json!({"view":view["view"]})).unwrap();
        assert_eq!(inspected["place"], serde_json::json!({"at":[310.0,180.0]}));
    }

    #[test]
    fn blank_sheet_starts_a_drawing_from_the_workspace() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        assert!(h.app.session.open_drawing.is_none());
        h.step(&serde_json::json!({"click":{"handle":"drawing:first:blank"}})).unwrap();
        h.frames(3);
        assert!(h.app.drawing_workspace.first_sheet.is_some());
        assert!(h.app.session.open_drawing.is_none());
        h.step(&serde_json::json!({"widget":"Create Drawing"})).unwrap();
        h.frames(4);
        let drawing = h.app.session.drawing("drawing.inspect").unwrap();
        assert_eq!(drawing.sheets.len(), 1);
        assert_eq!(active_sheet(&h.app), Some(drawing.sheets[0].id));
    }

    #[test]
    fn standard_view_layout_starts_a_drawing_with_four_views() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":100,"width":60,"height":12})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.step(&serde_json::json!({"click":{"handle":"drawing:first:standard"}})).unwrap();
        h.frames(4);
        let drawing = h.app.session.drawing("drawing.inspect").unwrap();
        assert_eq!(drawing.sheets.len(), 1);
        assert_eq!(drawing.views.len(), 4);
        assert_eq!(active_sheet(&h.app), Some(drawing.sheets[0].id));
    }

    #[test]
    fn sheets_panel_lists_sheets_of_the_open_drawing() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        h.app.run("drawing.sheet.add", serde_json::json!({"name":"Inspection sheet"})).unwrap();
        h.frames(4);
        assert!(h.widget("Inspection sheet").is_some());
        let layout = h.app.panel_runtime().unwrap().layout().layout();
        for panel in ["browser", "drawing-sheets"] {
            assert!(layout.instances.iter().any(|instance| instance.id.0 == panel && instance.requested_open));
        }

        h.app.run("drawing.close", serde_json::json!({})).unwrap();
        h.frames(2);
        assert!(h.widget("Inspection sheet").is_none());
    }

    #[test]
    fn sheet_menu_deletes_the_sheet_it_belongs_to_after_confirmation() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        let added = h.app.run("drawing.sheet.add", serde_json::json!({})).unwrap();
        let second = added["sheet"].as_str().unwrap();
        h.frames(4);
        let first = active_sheet(&h.app);
        h.step(&serde_json::json!({"click":{"handle":format!("drawing:sheets:menu:{second}")}})).unwrap();
        h.frames(3);
        h.step(&serde_json::json!({"widget":"Delete Sheet"})).unwrap();
        h.frames(3);
        assert_eq!(active_sheet(&h.app), first);
        assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().sheets.len(), 2);
        h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        assert_eq!(active_sheet(&h.app), first);
        assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().sheets.len(), 1);
    }

    #[test]
    fn browser_sheet_context_menu_targets_the_clicked_sheet_when_cards_are_hidden() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.preferences.drawing_sheets_visibility = "off".into();
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", serde_json::json!({})).unwrap();
        let added = h.app.run("drawing.sheet.add", serde_json::json!({"name":"Detail"})).unwrap();
        let second = added["sheet"].as_str().unwrap();
        let first = active_sheet(&h.app);
        h.frames(8);
        h.step(&serde_json::json!({"click":{"handle":format!("drawing:browser:sheet:{second}")},"button":"right"})).unwrap();
        h.frames(3);

        assert_eq!(active_sheet(&h.app), first);
        h.step(&serde_json::json!({"widget":"Delete Sheet"})).unwrap();
        h.frames(3);
        h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);

        assert_eq!(active_sheet(&h.app), first);
        assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().sheets.len(), 1);
    }

    /// The browser's rows of what is on a sheet take the clicks the design's browser rows take: a
    /// click selects the object (Shift adds), and a right-click opens the sheet's menu on it.
    #[test]
    fn browser_rows_select_their_objects_and_open_the_sheets_menu() {
        use serde_json::json;
        use solidcraft_engine::{Sel, doc::ids::encode};
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.run("solid.box", json!({"length":40,"width":20,"height":3})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("drawing.create", json!({})).unwrap();
        let view = h.app.run("drawing.view.base", json!({"sheet":"Sheet 1","scale":"1:1","position_mm":[150,150],"view":"top"})).unwrap();
        h.app.run("drawing.dimension.add", json!({"view": view["view"], "kind": {"length": {"edge": [20, 0, 3]}}})).unwrap();
        h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [250, 220], "content": "DEBURR"})).unwrap();
        h.frames(8);
        let drawing = h.app.session.drawing("drawing.inspect").unwrap().clone();
        let sheet = encode(drawing.sheets[0].id);
        let (view, dimension, note) = (drawing.views[0].id, drawing.dimensions[0].id, drawing.notes[0].id);
        let handle = |kind: &str, object: u64| format!("drawing:browser:sheet:{sheet}:{kind}:{}", encode(object));
        let picked = |h: &Harness| -> Vec<u64> {
            h.app.session.selection.iter().filter_map(|s| if let Sel::Drawing { object, .. } = s { Some(*object) } else { None }).collect()
        };

        h.step(&json!({"click": {"handle": handle("dimension", dimension)}})).unwrap();
        h.frames(3);
        assert_eq!(picked(&h), [dimension], "a click on the row selects the dimension");
        h.step(&json!({"click": {"handle": handle("view", view)}, "shift": true})).unwrap();
        h.frames(3);
        assert_eq!(picked(&h), [dimension, view], "Shift-click adds, as in the model");

        // A right-click on a row outside the selection selects it, and the menu is about it.
        h.step(&json!({"click": {"handle": handle("note", note)}, "button": "right"})).unwrap();
        h.frames(3);
        assert_eq!(picked(&h), [note]);
        let menu = crate::context_menu::describe(&h.app);
        let rows: Vec<&str> = menu["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
        assert_eq!(&rows[..2], ["Edit Note", "Delete"], "{rows:?}");
        crate::context_menu::pick(&mut h.app, "Edit Note").unwrap();
        h.frames(3);
        assert!(!crate::dim_view::editing() && inspector_visible(&h.app), "from the browser, Edit opens the note's options");
        close_panel(&mut h.app);
        h.step(&json!({"click": {"handle": handle("note", note)}, "button": "right"})).unwrap();
        h.frames(3);
        crate::context_menu::pick(&mut h.app, "Delete").unwrap();
        h.frames(4);
        assert!(h.app.session.drawing("drawing.inspect").unwrap().notes.by_id(note).is_none(), "the menu's Delete deletes it");
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
        assert!(!sheets_visible(&h.app));

        h.app.run("drawing.sheet.add", serde_json::json!({"name":"Replacement sheet"})).unwrap();
        h.frames(4);
        assert!(h.widget("Replacement sheet").is_some());
    }

    #[test]
    fn workspace_and_options_leave_source_and_camera_unchanged() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        h.app.ui.tab = "SURFACE".into();
        h.app.ui.show_timeline = true;
        h.app.preferences.history_in_browser = false;
        h.frames(4);
        assert!(crate::scenario::rect_of("history:card").is_some());
        let doc = h.app.session.doc.clone();
        let revision = h.app.session.revision;
        let undo = h.app.session.undo.len();
        let cam = h.app.cam;
        enter(&mut h.app, true).unwrap();
        assert!(start(&mut h.app, "drawing.view.base.planned"));
        h.frames(2);
        assert!(h.widget("Place standard views").is_some());
        assert!(h.widget("Finish").is_some());
        assert!(crate::scenario::rect_of("history:card").is_none());
        assert!(h.app.ui.show_timeline);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.revision, revision);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.cam, cam);
        enter(&mut h.app, false).unwrap();
        assert_eq!(h.app.ui.tab, "SURFACE");
        h.frames(4);
        assert!(crate::scenario::rect_of("history:card").is_some());
    }

    #[test]
    fn sheet_docking_and_settings_only_change_ui_preferences() {
        for layout in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.preferences.drawing_sheets_visibility = "legacy".into();
            h.app.home.open = false;
            h.app.ui.show_browser = true;
            h.app.ui.layout = layout;
            h.call("ui.resize", serde_json::json!({"width":1400,"height":1000}));
            h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
            enter(&mut h.app, true).unwrap();
            h.app.run("drawing.create", serde_json::json!({})).unwrap();
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
            h.app.ui.show_browser = true;
            h.app.ui.layout = layout;
            h.call("ui.resize", serde_json::json!({"width":960,"height":600}));
            h.app.preferences.drawing_panel_width = 560.0;
            h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
            enter(&mut h.app, true).unwrap();
            h.app.run("drawing.create", serde_json::json!({})).unwrap();
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
