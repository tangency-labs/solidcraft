use serde_json::{Value, json};
use solidcraft_engine::doc::drawing::{
    Drawing, MAX_DPI, MAX_NAME_BYTES, MIN_DPI, Sheet,
    dimension::DimensionKind,
    table::{Kind, MassFormat, PartsList},
    view::{ViewKind, ViewPlace},
};
use solidcraft_engine::doc::{drawing_view::TangentEdges, ids};
use solidcraft_engine::geom::{
    drawing_sheet::{Orientation, PaperSize, SheetSize},
    table::Heading,
};
use solidcraft_engine::render::VisualStyle;
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::drawing_sheet_page::{active_sheet, select_sheet};
use crate::ui_kit::{self, BackingTooltipExt as _, Button};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    Base,
    Projected,
    Edit(Option<u64>),
    Dimension(Option<u64>),
    Note,
    PartsList(Option<u64>),
}

impl Tool {
    pub(super) fn for_command(id: &str) -> Option<Self> {
        match id {
            "drawing.view.base.planned" => Some(Self::Base),
            "drawing.view.projected.planned" => Some(Self::Projected),
            "drawing.view.edit" => Some(Self::Edit(None)),
            "drawing.dimension" => Some(Self::Dimension(None)),
            "drawing.note" => Some(Self::Note),
            "drawing.parts_list" => Some(Self::PartsList(None)),
            _ => None,
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Base => tr("ui.drawing_panel.base"),
            Self::Projected => tr("ui.drawing_panel.projected"),
            Self::Edit(_) => tr("ui.drawing_panel.edit"),
            Self::Dimension(_) => tr("ui.drawing_panel.dimension"),
            Self::Note => tr("ui.drawing_panel.note"),
            Self::PartsList(_) => tr("ui.drawing_panel.parts_list"),
        }
    }
}

fn tr(id: &str) -> &'static str {
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

fn pending(ui: &mut egui::Ui, label: &str) {
    ui_kit::button(ui, label, Button::Enabled(false)).backing_on_disabled_hover_text(tr("ui.drawing_panel.planned"));
}

fn caption(ui: &mut egui::Ui, id: &'static str) {
    ui_kit::text(ui, tr(id), ui_kit::Text::Note(id));
}

pub(super) fn footer(ui: &mut egui::Ui, tool: Tool) -> bool {
    let mut close = false;
    ui_kit::Panel::editor_footer(ui, "drawing_options_footer", |ui| {
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
        Tool::Edit(view) => edit_view(ui, drawing, sheet, view),
        Tool::Dimension(dimension) => dimension(ui, drawing, sheet, dimension),
        Tool::Note => {
            ui.add_enabled_ui(false, |ui| {
                text(ui, tr("ui.drawing_panel.text"), "");
                choice(ui, tr("ui.drawing_panel.text_style"), 0, &[tr("ui.drawing_panel.default")]);
            })
            .inner
        }
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
    let options = ["visible", "hidden", "shaded", "shaded_visible", "shaded_hidden"].map(|style| tr(&format!("ui.drawing_panel.{style}")));
    let selected = VisualStyle::ALL.iter().position(|candidate| *candidate == style).unwrap_or_default();
    choice(ui, label, selected, &options);
}

fn edit_view(ui: &mut egui::Ui, drawing: Option<&Drawing>, sheet: Option<u64>, selected: Option<u64>) {
    let view = drawing.and_then(|drawing| {
        selected.and_then(|id| drawing.view(id)).or_else(|| drawing.views.iter().find(|view| Some(view.sheet) == sheet).map(|view| view.as_ref()))
    });
    let settings = drawing.zip(view).and_then(|(drawing, view)| drawing.resolve_view(view.id).ok());
    let projected = view.is_some_and(|view| matches!(view.kind, ViewKind::Projected { .. }));
    let mut reference = settings.as_ref().map_or(0.0, |settings| settings.reference_shown_percent);
    ui.add_enabled_ui(false, |ui| {
        ui_kit::section_heading(ui, tr("ui.drawing_panel.display"));
        visual_style(ui, tr("ui.drawing_panel.visual_style"), settings.as_ref().map_or(VisualStyle::Visible, |settings| settings.visual_style.value));
        visual_style(
            ui,
            tr("ui.drawing_panel.reference_style"),
            settings.as_ref().map_or(VisualStyle::VisibleHidden, |settings| settings.reference_style.value),
        );
        ui_kit::row(ui, tr("ui.drawing_panel.reference_shown"), |ui| {
            ui_kit::slider(ui, &mut reference, 0.0..=100.0, ui_kit::Slider { format: |value| format!("{value:.0} %"), ..Default::default() });
        });
        choice(
            ui,
            tr("ui.drawing_panel.hide_model"),
            usize::from(settings.as_ref().is_some_and(|settings| settings.reference_hides_model.value)),
            &[tr("ui.drawing_panel.no"), tr("ui.drawing_panel.yes")],
        );
        choice(
            ui,
            tr("ui.drawing_panel.tangent"),
            usize::from(settings.as_ref().is_some_and(|settings| settings.tangent_edges.value == TangentEdges::Show)),
            &[tr("ui.drawing_panel.hide"), tr("ui.drawing_panel.show")],
        );
        text(ui, tr("ui.drawing_panel.margin"), "0 mm");
        ui_kit::section_heading(ui, tr("ui.scale"));
        scale(ui, settings.as_ref().map(|settings| settings.scale.value.to_string()));
        ui_kit::section_heading(ui, tr("ui.drawing_panel.placement"));
        ui_kit::checkbox(ui, tr("ui.drawing_panel.link"), &mut view.is_some_and(|view| projected && view.settings.is_empty()));
        ui_kit::checkbox(ui, tr("ui.drawing_panel.aligned"), &mut view.is_some_and(|view| matches!(view.place, ViewPlace::Aligned(_))));
    });
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
        text(ui, tr("ui.drawing_panel.text"), selected.and_then(|item| item.override_text.as_deref()).unwrap_or("<dim>"));
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
    name: String,
    after: usize,
    size: usize,
    title_block: usize,
}

const SIZES: [PaperSize; 8] =
    [PaperSize::A4, PaperSize::A3, PaperSize::A2, PaperSize::A1, PaperSize::A0, PaperSize::Letter, PaperSize::Legal, PaperSize::A5];

fn size_name(size: PaperSize) -> String {
    match size {
        PaperSize::Letter => tr("ui.drawing_panel.letter").to_owned(),
        PaperSize::Legal => tr("ui.drawing_panel.legal").to_owned(),
        other => format!("{other:?}"),
    }
}

fn size_caption(size: PaperSize) -> String {
    let (short, long) = size.sides_mm();
    format!("{} · {long:.0} × {short:.0} mm", size_name(size))
}

impl AddSheet {
    pub(super) fn open(app: &SolveApp) -> Option<Self> {
        let drawing = app.session.drawing("drawing.inspect").ok()?;
        let after = drawing.sheets.len().checked_sub(1)?;
        let size = match drawing.sheets.get(after)?.sheet.size {
            SheetSize::Standard(size) => SIZES.iter().position(|candidate| *candidate == size).unwrap_or(1),
            SheetSize::Custom { .. } => 1,
        };
        Some(Self { name: String::new(), after, size, title_block: 0 })
    }
}

pub(super) fn add_window(app: &mut SolveApp, ctx: &egui::Context, mut form: AddSheet) -> Option<AddSheet> {
    let drawing = app.session.drawing("drawing.inspect").ok()?.clone();
    let after = drawing.sheets.get(form.after)?;
    let default_name =
        (drawing.sheets.len() + 1..).map(|n| format!("Sheet {n}")).find(|name| drawing.sheets.iter().all(|sheet| sheet.name != *name))?;
    let ready = form.name.len() <= MAX_NAME_BYTES && (form.name.is_empty() || !form.name.trim().is_empty());
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_drawing_add_sheet"), "plus", tr("ui.drawing_sheets.add"))
        .cards(2.5)
        .fields()
        .actions(tr("ui.drawing_sheets.add"), tr("ui.cancel"), None)
        .show(ctx, ready, |ui| {
            ui.columns(2, |columns| {
                let [sizes, fields] = columns else { return };
                ui_kit::section_heading(sizes, tr("ui.drawing_panel.paper_size"));
                for (index, size) in SIZES.into_iter().enumerate() {
                    let button = if form.size == index { Button::SmallSelected } else { Button::Small };
                    if ui_kit::button(sizes, size_caption(size), button).clicked() {
                        form.size = index;
                    }
                }
                paper_preview(fields, SIZES.get(form.size).copied().unwrap_or(PaperSize::A3), after.sheet.orientation, form.title_block != 2);
                ui_kit::text_row(fields, tr("ui.drawing_panel.name"), &mut form.name, &default_name);
                fields.add_enabled_ui(false, |ui| {
                    choice(ui, tr("ui.drawing_panel.border"), usize::from(after.border.is_none()), &["ISO 5457", tr("ui.drawing_panel.none")]);
                });
                ui_kit::choice_row(
                    fields,
                    tr("ui.drawing_panel.title_block"),
                    "drawing_add_sheet_block",
                    &mut form.title_block,
                    &[tr("ui.drawing_panel.inherit"), tr("ui.drawing_panel.built_in"), tr("ui.drawing_panel.none")],
                );
                let names = drawing.sheets.iter().map(|sheet| sheet.name.as_str()).collect::<Vec<_>>();
                ui_kit::choice_row(fields, tr("ui.drawing_panel.after_sheet"), "drawing_add_sheet_after", &mut form.after, &names);
            });
        });
    if closed || action == ui_kit::Action::Cancel {
        return None;
    }
    if action == ui_kit::Action::Ok {
        let size = *SIZES.get(form.size)?;
        let mut params = json!({"after": ids::encode(drawing.sheets.get(form.after)?.id), "size": SheetSize::Standard(size)});
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
                }
                return None;
            }
            Err(error) => app.set_status(error, true),
        }
    }
    Some(form)
}

fn paper_preview(ui: &mut egui::Ui, size: PaperSize, orientation: Orientation, block: bool) {
    let t = Tokens::get();
    let (short, long) = size.sides_mm();
    let (width, height) = if orientation == Orientation::Landscape { (long, short) } else { (short, long) };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.space_24 * 5.0), egui::Sense::hover());
    let ratio = (width / height) as f32;
    let area = rect.shrink(t.space_8);
    let width = area.width().min(area.height() * ratio).max(0.0);
    let paper = egui::Rect::from_center_size(area.center(), egui::vec2(width, width / ratio));
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius_md, t.field);
    painter.rect_filled(paper, 0.0, t.paper_white);
    let line = egui::Stroke::new(t.space_2 * 0.5, t.paper_ink);
    let border = paper.shrink(t.space_4);
    if border.is_positive() {
        painter.rect_stroke(border, 0.0, line, egui::StrokeKind::Inside);
        if block {
            let title = egui::Rect::from_min_max(border.right_bottom() - border.size() * egui::vec2(0.42, 0.18), border.right_bottom());
            painter.rect_stroke(title, 0.0, line, egui::StrokeKind::Inside);
        }
    }
}

fn change_sheet(app: &mut SolveApp, sheet: u64, key: &str, value: Value) {
    let mut params = json!({"sheet": ids::encode(sheet)});
    params[key] = value;
    if let Err(error) = app.run("drawing.sheet.set", params) {
        app.set_status(error, true);
    }
}

pub(super) fn sheet_settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    let sheet = active_sheet(app).and_then(|id| app.session.drawing("drawing.inspect").ok()?.sheets.iter().find(|sheet| sheet.id == id).cloned());
    if let Some(sheet) = sheet {
        sheet_fields(app, ui, &sheet);
    } else {
        caption(ui, "ui.drawing_panel.no_sheet");
    }
    ui_kit::section_heading(ui, tr("ui.drawing_panel.screen"));
    ui_kit::row(ui, tr("ui.sheet_page.colour"), |ui| crate::drawing_sheet_page::colour_choice(app, ui));
    ui_kit::row(ui, tr("ui.drawing_backdrop.label"), |ui| crate::drawing_sheet_page::backdrop_choice(app, ui));
}

fn sheet_fields(app: &mut SolveApp, ui: &mut egui::Ui, sheet: &Sheet) {
    let context = ui.ctx().clone();
    let name_id = ui.make_persistent_id(("sheet_settings_name", app.session.uid, sheet.id));
    let mut name = context.data(|data| data.get_temp::<String>(name_id)).unwrap_or_else(|| sheet.name.clone());
    ui_kit::section_heading(ui, tr("ui.drawing_panel.sheet"));
    let (enter, response) = ui_kit::row(ui, tr("ui.drawing_panel.name"), |ui| ui_kit::text_field_reserving(ui, &mut name, "", 0.0));
    if enter || response.lost_focus() {
        if !name.trim().is_empty() && name.len() <= MAX_NAME_BYTES && name != sheet.name {
            change_sheet(app, sheet.id, "name", json!(name));
        }
        context.data_mut(|data| data.remove::<String>(name_id));
    } else if response.changed() {
        context.data_mut(|data| data.insert_temp(name_id, name));
    }

    ui_kit::section_heading(ui, tr("ui.drawing_panel.paper"));
    let selected = match sheet.sheet.size {
        SheetSize::Standard(size) => size_caption(size),
        SheetSize::Custom { sides_mm: [a, b] } => format!("{a} × {b} mm"),
    };
    let mut new_size = None;
    ui_kit::row(ui, tr("ui.drawing_panel.size"), |ui| {
        ui_kit::dropdown_menu(ui, "drawing_sheet_size", &selected, |ui| {
            for size in PaperSize::ALL {
                if ui.selectable_label(sheet.sheet.size == SheetSize::Standard(size), size_caption(size)).clicked() {
                    new_size = Some(size);
                }
            }
        });
    });
    if let Some(size) = new_size {
        change_sheet(app, sheet.id, "size", json!(SheetSize::Standard(size)));
    }
    let mut orientation = usize::from(sheet.sheet.orientation == Orientation::Portrait);
    let changed = ui_kit::row(ui, tr("ui.drawing_panel.orientation"), |ui| {
        ui_kit::segmented(ui, &mut orientation, &[tr("ui.drawing_panel.landscape"), tr("ui.drawing_panel.portrait")], ui_kit::Segments::default())
    });
    if changed {
        let value = if orientation == 0 { Orientation::Landscape } else { Orientation::Portrait };
        change_sheet(app, sheet.id, "orientation", json!(value));
    }
    let dpi_id = ui.make_persistent_id(("sheet_settings_dpi", app.session.uid, sheet.id));
    let mut dpi = context.data(|data| data.get_temp::<String>(dpi_id)).unwrap_or_else(|| sheet.dpi.to_string());
    let (enter, response) = ui_kit::row(ui, tr("ui.drawing_panel.picture_dpi"), |ui| ui_kit::text_field_reserving(ui, &mut dpi, "", 0.0));
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
    ui_kit::section_heading(ui, tr("ui.drawing_panel.border_title"));
    ui.add_enabled_ui(false, |ui| {
        choice(
            ui,
            tr("ui.drawing_panel.border"),
            0,
            &[if sheet.border.is_some() { tr("ui.drawing_panel.existing") } else { tr("ui.drawing_panel.none") }],
        );
        choice(
            ui,
            tr("ui.drawing_panel.title_block"),
            0,
            &[if sheet.title_block.is_some() { tr("ui.drawing_panel.existing") } else { tr("ui.drawing_panel.none") }],
        );
        choice(ui, tr("ui.drawing_panel.template"), 0, &[tr("ui.drawing_panel.none")]);
    });
    ui_kit::section_heading(ui, tr("ui.drawing_panel.counting"));
    let mut count = !sheet.excluded.count;
    if ui_kit::checkbox(ui, tr("ui.drawing_panel.count_sheet"), &mut count).changed() {
        change_sheet(app, sheet.id, "excluded", json!({"count": !count}));
    }
    let mut print = !sheet.excluded.print;
    if ui_kit::checkbox(ui, tr("ui.drawing_panel.print_sheet"), &mut print).changed() {
        change_sheet(app, sheet.id, "excluded", json!({"print": !print}));
    }
}
