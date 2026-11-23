use serde_json::{Value, json};
use solidcraft_engine::doc::drawing::{
    Drawing, MAX_DPI, MAX_NAME_BYTES, MIN_DPI, Sheet,
    dimension::DimensionKind,
    table::{Kind, MassFormat, PartsList},
};
use solidcraft_engine::doc::ids;
use solidcraft_engine::geom::{
    drawing_sheet::{DrawingBorderProfile, Orientation, PaperSize, Sheet as Paper, SheetSize},
    drawing_view::Scale,
    table::Heading,
    title_block::{self, TitleBlock},
};
use solidcraft_engine::render::VisualStyle;
use solidcraft_theme::Tokens;
use std::sync::LazyLock;

use crate::SolveApp;
use crate::drawing_sheet_page::{active_sheet, select_sheet};
use crate::ui_kit::{self, BackingTooltipExt as _, Button};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    Base,
    Projected,
    Edit(Option<u64>),
    Dimension(Option<u64>),
    Note(Option<u64>),
    PartsList(Option<u64>),
    Export,
    Detail,
}

impl Tool {
    /// The placed object whose options these are, if they are one's.
    pub(super) fn object(self) -> Option<u64> {
        match self {
            Self::Base | Self::Projected | Self::Export | Self::Detail => None,
            Self::Edit(object) | Self::Dimension(object) | Self::Note(object) | Self::PartsList(object) => object,
        }
    }

    pub(super) fn for_command(id: &str) -> Option<Self> {
        match id {
            "drawing.view.base" | "drawing.view.base.planned" => Some(Self::Base),
            "drawing.view.projected" | "drawing.view.projected.planned" => Some(Self::Projected),
            "drawing.view.edit" => Some(Self::Edit(None)),
            "drawing.dimension" => Some(Self::Dimension(None)),
            "drawing.note" => Some(Self::Note(None)),
            "drawing.parts_list" => Some(Self::PartsList(None)),
            "drawing.export" => Some(Self::Export),
            "drawing.view.detail" | "drawing.view.detail.planned" => Some(Self::Detail),
            _ => None,
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Base => tr("ui.drawing_panel.base"),
            Self::Projected => tr("ui.drawing_panel.projected"),
            Self::Edit(_) => tr("ui.drawing_panel.edit"),
            Self::Dimension(_) => tr("ui.drawing_panel.dimension"),
            Self::Note(_) => tr("ui.drawing_panel.note"),
            Self::PartsList(_) => tr("ui.drawing_panel.parts_list"),
            Self::Export => tr("ui.drawing_export.title"),
            Self::Detail => tr("ui.drawing_detail.title"),
        }
    }
}

fn tr(id: &'static str) -> &'static str {
    solidcraft_ui_text::tr(id)
}

fn text(ui: &mut egui::Ui, label: &str, value: &str) {
    ui_kit::text_row(ui, label, &mut value.to_owned(), "");
}

fn choice(ui: &mut egui::Ui, label: &str, mut selected: usize, options: &[&str]) {
    ui_kit::choice_row(ui, label, label, &mut selected, options);
}

fn segments(ui: &mut egui::Ui, label: &str, mut selected: usize, options: &[&str]) {
    ui_kit::row(ui, label, |ui| {
        ui_kit::segmented(ui, &mut selected, options, ui_kit::Segments::default());
    });
}

pub(super) fn pending(ui: &mut egui::Ui, label: &str) {
    ui_kit::button(ui, label, ui_kit::Button::Enabled(false)).backing_on_disabled_hover_text(tr("ui.drawing_panel.planned"));
}

fn caption(ui: &mut egui::Ui, id: &'static str) {
    ui_kit::text(ui, tr(id), ui_kit::Text::Note(id));
}

pub(super) fn finish_placement(ui: &mut egui::Ui) -> bool {
    ui.separator();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui_kit::button(ui, tr("ui.drawing_panel.finish"), Button::Normal).clicked()
        })
        .inner
    })
    .inner
}

pub(super) fn footer(ui: &mut egui::Ui, tool: Tool) -> bool {
    let mut close = false;
    ui.separator();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if matches!(tool, Tool::Base | Tool::Projected) {
                pending(ui, tr("ui.drawing_panel.finish"));
            } else {
                pending(ui, tr("ui.heading_ok"));
                pending(ui, tr("ui.apply"));
                close = ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
            }
        });
    });
    close
}

pub(super) fn tool_body(app: &SolveApp, ui: &mut egui::Ui, tool: Tool) {
    let drawing = app.session.drawing("drawing.inspect").ok().map(|drawing| drawing.as_ref());
    let sheet = active_sheet(app);
    caption(ui, "ui.drawing_panel.planned");
    ui.add_space(Tokens::get().space_8);
    match tool {
        Tool::Base | Tool::Projected => base_view(ui, &app.session.doc.name, tool == Tool::Projected),
        Tool::Edit(_) => {}
        Tool::Dimension(selected) => dimension(ui, drawing, sheet, selected),
        Tool::Note(_) | Tool::Export | Tool::Detail => {}
        Tool::PartsList(table) => parts_list(ui, drawing, sheet, table, &app.session.doc.name),
    }
}

fn base_view(ui: &mut egui::Ui, source: &str, projected: bool) {
    ui_kit::section_heading(ui, tr("ui.drawing_panel.orientation"));
    ui.add_enabled_ui(false, |ui| {
        if projected {
            choice(ui, tr("ui.drawing_panel.parent"), 0, &[tr("ui.drawing_panel.none")]);
            choice(ui, tr("ui.drawing_panel.direction"), 0, &[tr("ui.right"), tr("ui.top"), tr("ui.drawing_panel.left"), tr("ui.bottom")]);
        } else {
            caption(ui, "ui.drawing_panel.face_pick");
            ui.horizontal_wrapped(|ui| {
                for (index, label) in [
                    tr("ui.front"),
                    tr("ui.top"),
                    tr("ui.right"),
                    tr("ui.drawing_panel.left"),
                    tr("ui.bottom"),
                    tr("ui.back"),
                    tr("ui.drawing_panel.iso"),
                ]
                .into_iter()
                .enumerate()
                {
                    ui_kit::button(ui, label, if index == 0 { Button::SmallSelected } else { Button::Small });
                }
            });
            choice(ui, tr("ui.drawing_panel.saved_view"), 0, &[tr("ui.drawing_panel.none")]);
        }
        ui_kit::section_heading(ui, tr("ui.drawing_panel.view"));
        choice(ui, tr("ui.drawing_panel.model"), 0, &[source]);
        scale(ui, None);
        visual_style(ui, tr("ui.drawing_panel.visual_style"), VisualStyle::VisibleHidden);
    });
    if !projected {
        pending(ui, tr("ui.drawing_panel.standard_views"));
    }
}

fn scale(ui: &mut egui::Ui, scale: Option<String>) {
    let auto = tr("ui.drawing_panel.auto_fit");
    let presets = [auto, "1:4", "1:2", "1:1", "2:1", "4:1"];
    let shown = scale.as_deref().unwrap_or(auto);
    if let Some(index) = presets.iter().position(|preset| *preset == shown) {
        choice(ui, tr("ui.scale"), index, &presets);
    } else {
        choice(ui, tr("ui.scale"), 0, &[shown]);
    }
}

fn visual_style(ui: &mut egui::Ui, label: &str, style: VisualStyle) {
    let options = [
        "ui.drawing_panel.visible",
        "ui.drawing_panel.hidden",
        "ui.drawing_panel.shaded",
        "ui.drawing_panel.shaded_visible",
        "ui.drawing_panel.shaded_hidden",
    ]
    .map(tr);
    let selected = VisualStyle::ALL.iter().position(|candidate| *candidate == style).unwrap_or_default();
    choice(ui, label, selected, &options);
}

pub(super) struct ViewEdit {
    view: u64,
    scale: String,
    style: VisualStyle,
}

pub(super) fn view_editor(app: &mut SolveApp, ui: &mut egui::Ui, selected: Option<u64>) -> bool {
    let sheet = active_sheet(app);
    let drawing = app.session.drawing("drawing.inspect").ok();
    let available: Vec<_> =
        drawing.into_iter().flat_map(|drawing| drawing.views.iter()).filter(|view| Some(view.sheet) == sheet).map(|view| view.id).collect();
    let mut index = selected
        .or_else(|| app.drawing_workspace.view_edit.as_ref().map(|edit| edit.view))
        .and_then(|id| available.iter().position(|view| *view == id))
        .unwrap_or_default();
    if selected.is_none() && available.len() > 1 {
        let labels: Vec<_> =
            (1..=available.len()).map(|number| solidcraft_ui_text::format("ui.drawing_browser_view", &[("number", &number.to_string())])).collect();
        let labels: Vec<_> = labels.iter().map(String::as_str).collect();
        ui_kit::choice_row(ui, tr("ui.drawing_panel.view"), "drawing:edit:view", &mut index, &labels);
    }
    let selected = selected.or_else(|| available.get(index).copied());
    let resolved = drawing.and_then(|drawing| {
        let view = match selected {
            Some(id) => drawing.view(id).filter(|view| Some(view.sheet) == sheet),
            None => drawing.views.iter().find(|view| Some(view.sheet) == sheet).map(|view| view.as_ref()),
        }?;
        drawing.resolve_view(view.id).ok().map(|settings| (view.id, settings.scale.value, settings.visual_style.value))
    });
    let Some((view, scale, style)) = resolved else {
        return ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
    };
    let mut edit =
        app.drawing_workspace.view_edit.take().filter(|edit| edit.view == view).unwrap_or_else(|| ViewEdit { view, scale: scale.to_string(), style });
    let styles = VisualStyle::ALL;
    let labels = [
        tr("ui.drawing_panel.visible"),
        tr("ui.drawing_panel.hidden"),
        tr("ui.drawing_panel.shaded"),
        tr("ui.drawing_panel.shaded_visible"),
        tr("ui.drawing_panel.shaded_hidden"),
    ];
    let mut index = styles.iter().position(|candidate| *candidate == edit.style).unwrap_or_default();
    let (cancel, apply, ok) = super::inset(ui, |ui| {
        ui_kit::section_heading(ui, tr("ui.drawing_panel.display"));
        let row = ui_kit::choice_row(ui, tr("ui.drawing_panel.visual_style"), "drawing:edit:style", &mut index, &labels);
        crate::scenario::publish_handle("drawing:edit:style", row.rect.right_center() - egui::vec2(12.0, 0.0));
        if let Some(value) = styles.get(index) {
            edit.style = *value;
        }
        ui_kit::section_heading(ui, tr("ui.scale"));
        ui_kit::text_row(ui, tr("ui.scale"), &mut edit.scale, "1:1");
        let valid = edit.scale.trim().parse::<Scale>().is_ok();
        ui.separator();
        ui.horizontal(|ui| {
            let cancel = ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
            let apply = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.apply"), Button::Normal)).inner.clicked();
            let ok = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.heading_ok"), Button::Normal)).inner.clicked();
            (cancel, apply, ok)
        })
        .inner
    });
    if cancel {
        return true;
    }
    let mut settings = serde_json::Map::new();
    if let Ok(value) = edit.scale.trim().parse::<Scale>()
        && value != scale
    {
        settings.insert("scale".into(), json!(value));
    }
    if edit.style != style {
        settings.insert("visual_style".into(), json!(edit.style));
    }
    app.drawing_workspace.view_edit = Some(edit);
    if (!apply && !ok) || settings.is_empty() {
        return ok;
    }
    match app.run("drawing.view.edit", json!({"view": ids::encode(view), "set": settings})) {
        Ok(_) => ok,
        Err(error) => {
            app.set_status(error, true);
            false
        }
    }
}

pub(super) struct DimensionEdit {
    dimension: u64,
    offset: String,
    decimals: usize,
    text: String,
}

/// A length typed in the Offset field: a number of millimetres, with or without its unit.
fn offset_mm(typed: &str) -> Option<f64> {
    typed.trim().trim_end_matches("mm").trim().parse::<f64>().ok().filter(|value| value.is_finite())
}

/// A placed dimension's options: how far its line sits from what it measures, how many decimals
/// its number shows, and the text shown instead of the number (none: the number). Apply and OK
/// change it through `drawing.dimension.edit`, in one undo step. Returns whether to close.
pub(super) fn dimension_editor(app: &mut SolveApp, ui: &mut egui::Ui, selected: u64) -> bool {
    let placed = app.session.drawing("drawing.inspect").ok().and_then(|drawing| {
        let item = drawing.dimension(selected)?;
        let kind = match item.kind {
            DimensionKind::Length { .. } => 1,
            DimensionKind::Angle { .. } => 2,
            _ => 0,
        };
        Some((kind, item.offset_mm, usize::from(item.decimals), item.override_text.clone().unwrap_or_default()))
    });
    let Some((kind, offset, decimals, text)) = placed else {
        return super::inset(ui, |ui| ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked());
    };
    let mut edit = app.drawing_workspace.dimension_edit.take().filter(|edit| edit.dimension == selected).unwrap_or_else(|| DimensionEdit {
        dimension: selected,
        offset: format!("{offset} mm"),
        decimals,
        text: text.clone(),
    });
    let (cancel, apply, ok) = super::inset(ui, |ui| {
        ui.add_enabled_ui(false, |ui| {
            segments(
                ui,
                tr("ui.drawing_panel.type"),
                kind,
                &[tr("ui.drawing_panel.linear"), tr("ui.drawing_panel.length"), tr("ui.drawing_panel.angle")],
            );
        });
        ui_kit::section_heading(ui, tr("ui.drawing_panel.layout"));
        ui_kit::text_row(ui, tr("ui.drawing_panel.offset"), &mut edit.offset, "mm");
        let labels = ["0", "1", "2", "3", "4", "5", "6"];
        edit.decimals = edit.decimals.min(labels.len() - 1);
        ui_kit::choice_row(ui, tr("ui.drawing_panel.decimals"), "drawing:dimension:decimals", &mut edit.decimals, &labels);
        ui_kit::text_row(ui, tr("ui.drawing_panel.text"), &mut edit.text, solidcraft_engine::doc::drawing::dimension::VALUE_TOKEN);
        let valid = offset_mm(&edit.offset).is_some();
        ui.separator();
        ui.horizontal(|ui| {
            let cancel = ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
            let apply = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.apply"), Button::Normal)).inner.clicked();
            let ok = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.heading_ok"), Button::Normal)).inner.clicked();
            (cancel, apply, ok)
        })
        .inner
    });
    if cancel {
        return true;
    }
    let mut set = serde_json::Map::new();
    if let Some(value) = offset_mm(&edit.offset)
        && (value - offset).abs() > 1e-9
    {
        set.insert("offset_mm".into(), json!(value));
    }
    if edit.decimals != decimals {
        set.insert("decimals".into(), json!(edit.decimals));
    }
    let typed = edit.text.trim();
    if typed != text {
        set.insert("override_text".into(), if typed.is_empty() { Value::Null } else { json!(typed) });
    }
    if (!apply && !ok) || set.is_empty() {
        app.drawing_workspace.dimension_edit = Some(edit);
        return ok;
    }
    match app.run("drawing.dimension.edit", json!({"dimension": ids::encode(selected), "set": set})) {
        // The fields are read again from the dimension as it is now.
        Ok(_) => ok,
        Err(error) => {
            app.drawing_workspace.dimension_edit = Some(edit);
            app.set_status(error, true);
            false
        }
    }
}

fn dimension(ui: &mut egui::Ui, drawing: Option<&Drawing>, sheet: Option<u64>, selected: Option<u64>) {
    let selected = drawing.and_then(|drawing| drawing.dimensions.iter().find(|item| Some(item.id) == selected));
    let kind = selected.map_or(0, |item| match item.kind {
        DimensionKind::Length { .. } => 1,
        DimensionKind::Angle { .. } => 2,
        _ => 0,
    });
    ui.add_enabled_ui(false, |ui| {
        segments(
            ui,
            tr("ui.drawing_panel.type"),
            kind,
            &[tr("ui.drawing_panel.linear"), tr("ui.drawing_panel.length"), tr("ui.drawing_panel.angle")],
        );
        ui_kit::section_heading(ui, tr("ui.drawing_panel.references"));
        caption(ui, "ui.drawing_panel.pick_references");
        ui_kit::section_heading(ui, tr("ui.drawing_panel.layout"));
        text(ui, tr("ui.drawing_panel.offset"), &format!("{} mm", selected.map_or(0.0, |item| item.offset_mm)));
        let decimals = [tr("ui.drawing_panel.default"), "0", "1", "2", "3", "4", "5", "6"];
        let precision = selected.map_or(0, |item| usize::from(item.decimals).saturating_add(1).min(decimals.len() - 1));
        choice(ui, tr("ui.drawing_panel.decimals"), precision, &decimals);
        text(
            ui,
            tr("ui.drawing_panel.text"),
            selected.and_then(|item| item.override_text.as_deref()).unwrap_or(solidcraft_engine::doc::drawing::dimension::VALUE_TOKEN),
        );
    });
    if let Some(drawing) = drawing {
        ui_kit::section_heading(ui, tr("ui.drawing_panel.dimensions"));
        for item in drawing.dimensions.iter().filter(|item| drawing.view(item.view).is_some_and(|view| Some(view.sheet) == sheet)) {
            if let Some(value) = item.override_text.as_deref() {
                ui.label(format!("{value} · ⓘ {}", tr("ui.drawing_panel.overridden")));
            } else if let Some(value) = item.last_good {
                ui.label(format!("{value:.precision$}", precision = usize::from(item.decimals)));
            }
        }
    }
}

fn parts_list(ui: &mut egui::Ui, drawing: Option<&Drawing>, sheet: Option<u64>, selected: Option<u64>, source: &str) {
    let stored = drawing.and_then(|drawing| {
        drawing
            .tables
            .iter()
            .find(|table| matches!(table.kind, Kind::PartsList(_)) && selected.map_or_else(|| Some(table.sheet) == sheet, |id| table.id == id))
    });
    let fallback = drawing.and_then(|drawing| {
        let of =
            drawing.sheets.iter().find(|candidate| Some(candidate.id) == sheet).and_then(|sheet| sheet.documents).or_else(|| {
                drawing.sources.first().map(|source| solidcraft_engine::doc::drawing::SheetComponent { source: source.id, component: 0 })
            })?;
        Some(
            Kind::PartsList(PartsList {
                of,
                view: Default::default(),
                group: Default::default(),
                fold: false,
                purchased_contents: false,
                mass: MassFormat::default(),
            })
            .default_table(),
        )
    });
    let table = stored.map(|stored| &stored.table).or(fallback.as_ref());
    ui.add_enabled_ui(false, |ui| {
        choice(ui, tr("ui.drawing_panel.model"), 0, &[source]);
        segments(
            ui,
            tr("ui.drawing_panel.heading"),
            usize::from(table.is_some_and(|table| table.heading == Heading::Bottom)),
            &[tr("ui.top"), tr("ui.bottom")],
        );
        text(ui, tr("ui.drawing_panel.row_height"), &format!("{} mm", table.map_or(7.0, |table| table.row_height_mm)));
        ui_kit::section_heading(ui, tr("ui.drawing_panel.columns"));
        if let Some(table) = table {
            for column in &table.columns {
                let mut width = column.width_mm.map_or_else(|| tr("ui.drawing_panel.auto").to_owned(), |value| format!("{value} mm"));
                ui_kit::row(ui, &column.heading, |ui| {
                    ui.horizontal(|ui| {
                        let mut printed = column.printed;
                        ui_kit::checkbox(ui, "", &mut printed);
                        ui_kit::text_field(ui, &mut width, "");
                    });
                });
            }
        }
    });
    ui_kit::section_heading(ui, tr("ui.drawing_panel.items"));
    pending(ui, tr("ui.drawing_panel.renumber"));
}

#[derive(Clone)]
pub(super) struct AddSheet {
    pub(super) document: u64,
    pub(super) drawing: u64,
    pub(super) window: solidcraft_views::WindowId,
    name: String,
    after: usize,
    paper: PaperChoice,
    title_block: usize,
}

const SIZES: &[PaperSize] = &[PaperSize::A4, PaperSize::A3, PaperSize::A2, PaperSize::A1, PaperSize::A0, PaperSize::Letter, PaperSize::AnsiB];
static STANDARD_TITLE_BLOCK: LazyLock<TitleBlock> = LazyLock::new(title_block::default_block);

pub(super) fn valid_sheet_name(name: &str) -> bool {
    name.is_empty() || (!name.trim().is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control))
}

fn size_name(size: PaperSize) -> String {
    match size {
        PaperSize::AnsiB => tr("ui.drawing_first.tabloid").to_owned(),
        PaperSize::Letter => tr("ui.drawing_panel.letter").to_owned(),
        PaperSize::Legal => tr("ui.drawing_panel.legal").to_owned(),
        other => format!("{other:?}"),
    }
}

pub(super) fn size_caption(size: PaperSize) -> String {
    let (short, long) = size.sides_mm();
    format!("{} · {long:.0} × {short:.0} mm", size_name(size))
}

impl AddSheet {
    pub(super) fn open(app: &SolveApp) -> Option<Self> {
        Self::open_after(app, None)
    }

    pub(super) fn open_after(app: &SolveApp, sheet: Option<u64>) -> Option<Self> {
        let drawing = app.session.drawing("drawing.inspect").ok()?;
        let after = match sheet {
            Some(id) => drawing.sheets.iter().position(|sheet| sheet.id == id)?,
            None => drawing.sheets.len().checked_sub(1)?,
        };
        let paper = PaperChoice::new(drawing.sheets.get(after)?.sheet);
        Some(Self {
            document: app.session.uid,
            drawing: drawing.id,
            window: app.view_windows.current_window(),
            name: String::new(),
            after,
            paper,
            title_block: 0,
        })
    }
}

pub(super) fn add_window(app: &mut SolveApp, ctx: &egui::Context, mut form: AddSheet) -> Option<AddSheet> {
    let drawing = app.session.drawing("drawing.inspect").ok()?.clone();
    let after = drawing.sheets.get(form.after)?;
    let default_name =
        (drawing.sheets.len() + 1..).map(|n| format!("Sheet {n}")).find(|name| drawing.sheets.iter().all(|sheet| sheet.name != *name))?;
    let ready = valid_sheet_name(&form.name) && form.paper.selected().is_some();
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_drawing_add_sheet"), "plus", tr("ui.drawing_sheets.add"))
        .cards(2.5)
        .fields()
        .actions(tr("ui.drawing_sheets.add"), tr("ui.cancel"), None)
        .show(ctx, ready, |ui| {
            sheet_columns(ui, &mut form.paper, |fields, paper| {
                let block = match form.title_block {
                    0 => after.title_block.is_some(),
                    1 => true,
                    _ => false,
                };
                paper_preview(fields, paper, after.border.is_some(), block);
                ui_kit::text_row(fields, tr("ui.drawing_panel.name"), &mut form.name, &default_name);
                paper_border(fields, paper, after.border.as_ref());
                let choices = [tr("ui.drawing_panel.inherit"), tr("ui.drawing_panel.built_in"), tr("ui.drawing_panel.none")];
                let inherited = match &after.title_block {
                    Some(block) if *block == *STANDARD_TITLE_BLOCK => choices[1],
                    Some(_) => choices[0],
                    None => choices[2],
                };
                let current = [inherited, choices[1], choices[2]].get(form.title_block).copied().unwrap_or(inherited);
                ui_kit::row(fields, tr("ui.drawing_panel.title_block"), |ui| {
                    ui_kit::dropdown_menu(ui, "drawing_add_sheet_block", current, |ui| {
                        for (index, label) in choices.into_iter().enumerate() {
                            if ui_kit::list_picker(ui, &format!("drawing_add_sheet_block_{index}"), label, form.title_block == index).clicked() {
                                form.title_block = index;
                                ui.close();
                            }
                        }
                    })
                });
                let names = drawing.sheets.iter().map(|sheet| tr("ui.drawing_first.after_named").replace("{name}", &sheet.name)).collect::<Vec<_>>();
                let names = names.iter().map(String::as_str).collect::<Vec<_>>();
                ui_kit::choice_row(fields, tr("ui.drawing_first.position"), "drawing_add_sheet_after", &mut form.after, &names);
            });
        });
    if closed || action == ui_kit::Action::Cancel {
        return None;
    }
    if action == ui_kit::Action::Ok {
        let Some(paper) = form.paper.selected() else { return Some(form) };
        if !valid_sheet_name(&form.name) {
            return Some(form);
        }
        let mut params = json!({"after": ids::encode(drawing.sheets.get(form.after)?.id), "size": paper.size, "orientation": paper.orientation});
        if !form.name.is_empty() {
            params["name"] = json!(form.name);
        }
        match form.title_block {
            1 => params["title_block"] = json!("built-in"),
            2 => params["title_block"] = Value::Null,
            _ => {}
        }
        match app.run("drawing.sheet.add", params) {
            Ok(result) => {
                if let Some(id) = result.get("sheet").and_then(Value::as_str).and_then(ids::decode) {
                    select_sheet(app, id);
                    if app.preferences.design_sub_tab_sheets
                        && let Err(error) = crate::sheet_views::open_in(app, drawing.id, id, false)
                    {
                        app.set_status(error, true);
                    }
                }
                return None;
            }
            Err(error) => app.set_status(error, true),
        }
    }
    Some(form)
}

#[derive(Clone)]
pub(super) struct PaperChoice {
    size: usize,
    orientation: usize,
    short: String,
    long: String,
}

impl PaperChoice {
    pub(super) fn new(paper: Paper) -> Self {
        let (size, short, long) = match paper.size {
            SheetSize::Standard(size) => {
                let (short, long) = size.sides_mm();
                (SIZES.iter().position(|&candidate| candidate == size).unwrap_or(SIZES.len()), short, long)
            }
            SheetSize::Custom { sides_mm: [a, b] } => (SIZES.len(), a.min(b), a.max(b)),
        };
        Self { size, orientation: usize::from(paper.orientation == Orientation::Portrait), short: short.to_string(), long: long.to_string() }
    }

    pub(super) fn selected(&self) -> Option<Paper> {
        let size = if let Some(&size) = SIZES.get(self.size) {
            SheetSize::Standard(size)
        } else {
            SheetSize::Custom { sides_mm: [self.short.trim().parse().ok()?, self.long.trim().parse().ok()?] }
        };
        let orientation = if self.orientation == 0 { Orientation::Landscape } else { Orientation::Portrait };
        let paper = Paper { size, orientation };
        paper.size_mm().ok()?;
        Some(paper)
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        ui_kit::section_heading(ui, tr("ui.drawing_panel.paper_size"));
        let mut labels = SIZES.iter().map(|&size| size_name(size)).collect::<Vec<_>>();
        labels.push(tr("ui.graphics_custom").to_owned());
        let mut ids = SIZES.iter().map(|size| format!("drawing:paper-size:{size:?}")).collect::<Vec<_>>();
        ids.push("drawing:paper-size:custom".into());
        let mut captions = Vec::with_capacity(labels.len());
        let tiles = labels
            .iter()
            .zip(&ids)
            .enumerate()
            .map(|(index, (label, id))| {
                let aspect = if let Some(&size) = SIZES.get(index) {
                    let (short, long) = size.sides_mm();
                    captions.push(match size {
                        PaperSize::Letter => "11 × 8.5 in".to_owned(),
                        PaperSize::AnsiB => "17 × 11 in".to_owned(),
                        _ => format!("{long:.0} × {short:.0}"),
                    });
                    long / short
                } else {
                    captions.push(tr("ui.drawing_first.any_size").to_owned());
                    1.0
                };
                ui_kit::AspectTile { id, label, aspect, selected: self.size == index }
            })
            .collect::<Vec<_>>();
        if let Some(index) = ui_kit::aspect_grid_with_captions(ui, &tiles, &captions, 4) {
            self.size = index;
        }
        ui.add_space(Tokens::get().space_16);
        ui_kit::section_heading(ui, tr("ui.orientation"));
        ui_kit::segmented(
            ui,
            &mut self.orientation,
            &[tr("ui.drawing_first.landscape"), tr("ui.drawing_first.portrait")],
            ui_kit::Segments {
                workspace_accent: true,
                targets: &["drawing:orientation:landscape", "drawing:orientation:portrait"],
                ..Default::default()
            },
        );
        if self.size == SIZES.len() {
            ui_kit::text_row(ui, tr("ui.drawing_first.short_side"), &mut self.short, "");
            ui_kit::text_row(ui, tr("ui.drawing_first.long_side"), &mut self.long, "");
        }
    }
}

pub(super) fn sheet_columns(ui: &mut egui::Ui, choice: &mut PaperChoice, fields: impl FnOnce(&mut egui::Ui, Option<Paper>)) {
    let t = Tokens::get();
    let width = (ui.available_width() - t.space_24).max(0.0);
    let left = width * 0.59;
    let columns = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_24;
        ui.vertical(|ui| {
            ui.set_width(left);
            choice.show(ui);
        });
        ui.vertical(|ui| {
            ui.set_width(width - left);
            fields(ui, choice.selected());
        });
    });
    let bounds = columns.response.rect;
    ui.painter().vline(bounds.left() + left + t.space_12, bounds.y_range(), egui::Stroke::new(1.0, t.control_border));
}

pub(super) fn paper_preview(ui: &mut egui::Ui, paper: Option<Paper>, has_border: bool, block: bool) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.space_24 * 8.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius_md, t.sheet_soft);
    let Some(size) = paper.and_then(|sheet| sheet.size_mm().ok()) else { return };
    let area = rect.shrink(t.space_16);
    let long = size.x.max(size.y);
    let width = size.x / long;
    let height = size.y / long;
    let scale =
        (f64::from(area.width().max(0.0)) / width.max(f64::MIN_POSITIVE)).min(f64::from(area.height().max(0.0)) / height.max(f64::MIN_POSITIVE));
    let paper = egui::Rect::from_center_size(area.center(), egui::vec2((width * scale) as f32, (height * scale) as f32));
    painter.rect_filled(paper, 0.0, t.paper_white);
    let line = egui::Stroke::new(t.space_2 * 0.5, t.paper_ink);
    let border = paper.shrink(t.space_4);
    if has_border && border.is_positive() {
        painter.rect_stroke(border, 0.0, line, egui::StrokeKind::Inside);
        if block {
            let title = egui::Rect::from_min_max(border.right_bottom() - border.size() * egui::vec2(0.42, 0.18), border.right_bottom());
            painter.rect_stroke(title, 0.0, line, egui::StrokeKind::Inside);
            for fraction in [0.25, 0.5, 0.75] {
                painter.hline(title.x_range(), title.top() + title.height() * fraction, line);
            }
            for fraction in [1.0 / 3.0, 2.0 / 3.0] {
                painter.vline(title.left() + title.width() * fraction, title.center().y..=title.bottom(), line);
            }
        }
    }
    ui.add_space(t.space_8);
}

pub(super) fn paper_border(ui: &mut egui::Ui, paper: Option<Paper>, border: Option<&DrawingBorderProfile>) {
    let label = match border {
        Some(border) if *border == DrawingBorderProfile::iso() => match paper.map(|paper| paper.size) {
            Some(SheetSize::Standard(size)) => format!("ISO 5457 · {}", size_name(size)),
            _ => "ISO 5457".to_owned(),
        },
        Some(_) => tr("ui.graphics_custom").to_owned(),
        None => tr("ui.drawing_panel.none").to_owned(),
    };
    ui.add_enabled_ui(false, |ui| choice(ui, tr("ui.drawing_panel.border"), 0, &[&label]));
}

pub(super) struct SheetNotice {
    owner: (u64, u64, u64),
    revision: u64,
    views: Vec<u64>,
}

fn change_sheet(app: &mut SolveApp, sheet: u64, key: &str, value: Value) {
    let mut params = json!({"sheet": ids::encode(sheet)});
    params[key] = value;
    match app.run("drawing.sheet.set", params) {
        Ok(reply) => {
            let views = reply["outside"].as_array().into_iter().flatten().filter_map(|view| ids::decode(view["view"].as_str()?)).collect::<Vec<_>>();
            app.drawing_workspace.sheet_notice = app.session.open_drawing.filter(|_| !views.is_empty()).map(|drawing| SheetNotice {
                owner: (app.session.uid, drawing, sheet),
                revision: app.session.revision,
                views,
            });
        }
        Err(error) => app.set_status(error, true),
    }
}

fn sheet_notice(app: &mut SolveApp, ui: &mut egui::Ui, drawing: &Drawing, sheet: &Sheet) {
    let Some(notice) = app.drawing_workspace.sheet_notice.as_ref() else { return };
    if notice.owner != (app.session.uid, drawing.id, sheet.id) || notice.revision != app.session.revision {
        return;
    }
    let t = Tokens::get();
    let message = solidcraft_ui_text::format("ui.drawing_settings.outside", &[("count", &notice.views.len().to_string())]);
    let container = ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Panel);
    let frame =
        container.edge_frame(t.pref_note_fill).stroke(t.control_border_stroke(t.pref_note)).corner_radius(t.radius_md).inner_margin(t.space_12);
    let area = ui.available_rect_before_wrap();
    let picked = container
        .show_frame(ui, frame, area, |ui| {
            ui.set_width((area.width() - 2.0 * t.space_12).max(0.0));
            ui.label(egui::RichText::new(message).strong().color(t.pref_note));
            ui.label(tr("ui.drawing_settings.unmoved"));
            ui.horizontal_wrapped(|ui| {
                let mut picked = None;
                for (index, view) in drawing.views.iter().filter(|view| view.sheet == sheet.id).enumerate() {
                    if notice.views.contains(&view.id) {
                        let label = solidcraft_ui_text::format("ui.drawing_settings.view", &[("number", &(index + 1).to_string())]);
                        if ui_kit::button(ui, label, Button::Small).clicked() {
                            picked = Some(view.id);
                        }
                    }
                }
                picked
            })
            .inner
        })
        .inner;
    if let Some(view) = picked {
        super::edit_object(app, "view", view);
    }
}

pub(super) fn sheet_settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() else { return };
    let Some(sheet) = active_sheet(app).and_then(|id| drawing.sheets.iter().find(|sheet| sheet.id == id)) else {
        caption(ui, "ui.drawing_panel.no_sheet");
        return;
    };
    let t = Tokens::get();
    super::inset(ui, |ui| {
        ui.spacing_mut().item_spacing.y = t.space_2;
        super::sheet_settings_actions(app, ui);
        ui_kit::settings_form(ui, t.form_label_width + t.space_24, |ui| {
            sheet_notice(app, ui, &drawing, sheet);
            sheet_fields(app, ui, sheet);

            next_sheet_section(ui, "ui.drawing_panel.screen");
            ui_kit::row(ui, tr("ui.sheet_page.colour"), |ui| crate::drawing_sheet_page::colour_choice(app, ui));
        });
        ui.add_space(t.space_8);
        ui.separator();
        settings_note(ui, "ui.drawing_settings.immediate");
    });
}

fn next_sheet_section(ui: &mut egui::Ui, key: &'static str) {
    ui.add_space(Tokens::get().space_12);
    ui_kit::section_heading(ui, tr(key));
}

fn settings_note(ui: &mut egui::Ui, key: &'static str) {
    let t = Tokens::get();
    ui.add(egui::Label::new(egui::RichText::new(tr(key)).size(t.font_small).color(t.form_dim)).wrap());
}

fn sheet_fields(app: &mut SolveApp, ui: &mut egui::Ui, sheet: &Sheet) {
    let context = ui.ctx().clone();
    let name_id = ui.make_persistent_id(("sheet_settings_name", app.session.uid, sheet.id));
    let mut name = context.data(|data| data.get_temp::<String>(name_id)).unwrap_or_else(|| sheet.name.clone());
    ui_kit::section_heading(ui, tr("ui.drawing_panel.sheet"));
    let (enter, response) = ui_kit::row(ui, tr("ui.drawing_panel.name"), |ui| ui_kit::text_field_reserving(ui, &mut name, "", 0.0));
    if app.drawing_workspace.rename_sheet.take() == Some(sheet.id) {
        response.request_focus();
    }
    if enter || response.lost_focus() {
        if !name.trim().is_empty() && name.len() <= MAX_NAME_BYTES && name != sheet.name {
            change_sheet(app, sheet.id, "name", json!(name));
        }
        context.data_mut(|data| data.remove::<String>(name_id));
    } else if response.changed() {
        context.data_mut(|data| data.insert_temp(name_id, name));
    }

    next_sheet_section(ui, "ui.drawing_panel.paper");
    let selected = match sheet.sheet.size {
        SheetSize::Standard(size) => size_caption(size),
        SheetSize::Custom { sides_mm: [a, b] } => format!("{a} × {b} mm"),
    };
    let mut new_size = None;
    let picker = ui_kit::row(ui, tr("ui.drawing_panel.size"), |ui| {
        ui_kit::dropdown_menu(ui, "drawing_sheet_size", &selected, |ui| {
            for size in PaperSize::ALL {
                let caption = size_caption(size);
                let option = ui_kit::list_picker(ui, &caption, &caption, sheet.sheet.size == SheetSize::Standard(size));
                crate::scenario::publish_target(&format!("drawing:settings:paper:{size:?}"), option.rect);
                if option.clicked() {
                    new_size = Some(size);
                    ui.close();
                }
            }
        })
    });
    crate::scenario::publish_target("drawing:settings:paper", picker.rect);
    if let Some(size) = new_size {
        change_sheet(app, sheet.id, "size", json!(SheetSize::Standard(size)));
    }
    let mut orientation = usize::from(sheet.sheet.orientation == Orientation::Portrait);
    let changed = ui_kit::row(ui, tr("ui.drawing_panel.orientation"), |ui| {
        ui_kit::segmented(
            ui,
            &mut orientation,
            &[tr("ui.drawing_panel.landscape"), tr("ui.drawing_panel.portrait")],
            ui_kit::Segments { workspace_accent: true, ..Default::default() },
        )
    });
    if changed {
        let value = if orientation == 0 { Orientation::Landscape } else { Orientation::Portrait };
        change_sheet(app, sheet.id, "orientation", json!(value));
    }
    let dpi_id = ui.make_persistent_id(("sheet_settings_dpi", app.session.uid, sheet.id));
    let mut dpi = context.data(|data| data.get_temp::<String>(dpi_id)).unwrap_or_else(|| sheet.dpi.to_string());
    let picture = tr("ui.drawing_settings.picture");
    let (enter, response) = ui_kit::row(ui, picture, |ui| {
        let t = Tokens::get();
        let (enter, response) = ui_kit::text_field_reserving(ui, &mut dpi, "", t.space_24);
        ui.painter().text(
            response.rect.right_center() - egui::vec2(t.space_8, 0.0),
            egui::Align2::RIGHT_CENTER,
            "dpi",
            egui::FontId::monospace(t.font_small),
            t.form_dim,
        );
        (enter, response)
    });
    ui_kit::below_row_control(ui, picture, ui.available_width(), |ui| {
        settings_note(ui, "ui.drawing_settings.picture_help");
    });
    if enter || response.lost_focus() {
        if let Ok(value) = dpi.parse::<f64>()
            && (MIN_DPI..=MAX_DPI).contains(&value)
            && value != sheet.dpi
        {
            change_sheet(app, sheet.id, "dpi", json!(value));
        }
        context.data_mut(|data| data.remove::<String>(dpi_id));
    } else if response.changed() {
        context.data_mut(|data| data.insert_temp(dpi_id, dpi));
    }
    next_sheet_section(ui, "ui.drawing_panel.border_title");
    paper_border(ui, Some(sheet.sheet), sheet.border.as_ref());
    let built_in = tr("ui.drawing_panel.built_in");
    let none = tr("ui.drawing_panel.none");
    let current = match sheet.title_block.as_ref() {
        Some(block) if *block == *STANDARD_TITLE_BLOCK => built_in,
        Some(_) => tr("ui.graphics_custom"),
        None => none,
    };
    let mut replacement = None;
    ui_kit::row(ui, tr("ui.drawing_panel.title_block"), |ui| {
        ui_kit::dropdown_menu(ui, "drawing_sheet_title_block", current, |ui| {
            for (label, value) in [(built_in, json!("built-in")), (none, Value::Null)] {
                if ui_kit::list_picker(ui, label, label, current == label).clicked() {
                    if current != label {
                        replacement = Some(value);
                    }
                    ui.close();
                }
            }
        });
    });
    if let Some(value) = replacement {
        change_sheet(app, sheet.id, "title_block", value);
    }
    let template = tr("ui.drawing_panel.template");
    ui.add_enabled_ui(false, |ui| choice(ui, template, 0, &[none]));
    ui_kit::below_row_control(ui, template, ui.available_width(), |ui| {
        settings_note(ui, "ui.drawing_settings.template_help");
    });
    next_sheet_section(ui, "ui.drawing_panel.counting");
    let mut count = !sheet.excluded.count;
    if ui_kit::checkbox(ui, tr("ui.drawing_panel.count_sheet"), &mut count).changed() {
        change_sheet(app, sheet.id, "excluded", json!({"count": !count}));
    }
    let mut print = !sheet.excluded.print;
    if ui_kit::checkbox(ui, tr("ui.drawing_panel.print_sheet"), &mut print).changed() {
        change_sheet(app, sheet.id, "excluded", json!({"print": !print}));
    }
}
