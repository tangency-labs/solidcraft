//! Layout elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use super::{metrics::*, text::paint_text};
use crate::{scenario, theme::Tokens};
use egui::{Response, RichText, Ui, vec2};

pub(super) const FORM: &str = "sc_dense_form";

pub(super) const LABEL: &str = "sc_form_label";
pub(super) const FIXED_LABEL: &str = "sc_fixed_label_width";
pub(super) const GROW_LABEL: &str = "sc_grow_label_column";

/// A form whose card widens so its widest label fits (#10620). Each pass the form publishes,
/// under `key`, the width its label column needs beyond the design's `form_label_width`; the
/// card's owner reads that before opening the card, so the card settles one frame later.
/// Tokens: `form_label_width, font_label, dialog_label, space_4`.
pub(crate) fn grow_form<R>(ui: &mut Ui, key: egui::Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag_value(GROW_LABEL, key)), add).inner
}

/// A deliberate label grid for multi-column settings, inherited by the existing shared rows.
pub(crate) fn settings_form<R>(ui: &mut Ui, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag_value(FIXED_LABEL, dimension(label_width))), add)
        .inner
}

/// Mark a form without global state: children inherit this tag through their UI stack.
/// Inherited form styling and label-column identity. Tokens: `space_4/8, form_gap_y, control_height, font_label, accent, selected_bg, dialog_rule, control_fill/border/radius, text`.
pub(crate) fn scope<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag(FORM)), |ui| {
        let t = Tokens::get();
        ui.spacing_mut().item_spacing = vec2(t.space_4, t.form_gap_y);
        ui.spacing_mut().interact_size.y = row_height();
        ui.spacing_mut().button_padding = vec2(t.space_8, t.space_4);
        ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(t.font_label));
        ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(t.font_label));
        ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(t.font_body));
        let visuals = ui.visuals_mut();
        visuals.weak_text_color = Some(t.form_dim);
        visuals.selection.stroke = egui::Stroke::new(1.0, t.form_primary);
        visuals.selection.bg_fill = t.selected_bg;
        visuals.widgets.noninteractive.bg_stroke = t.tertiary_border(t.dialog_rule);
        for state in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
            state.bg_fill = t.control_fill;
            state.weak_bg_fill = t.control_fill;
            state.bg_stroke = t.control_border_stroke(t.control_border);
            state.corner_radius = t.control_radius.into();
            state.expansion = 0.0;
            state.fg_stroke.color = t.form_ink;
        }
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        add(ui)
    })
    .inner
}

/// Compact library cards share search, segment, chip and footer metrics.
pub(crate) fn catalog_scope<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag("sc_catalog")), |ui| {
        ui.spacing_mut().item_spacing.y = Tokens::get().space_8;
        add(ui)
    })
    .inner
}

pub(super) fn in_catalog(ui: &Ui) -> bool {
    ui.stack().iter().any(|node| node.tags().contains("sc_catalog"))
}

/// One-line library footer with the shared information glyph.
pub(crate) fn catalog_footer_height(ui: &Ui) -> f32 {
    let t = Tokens::get();
    let line = ui.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(t.font_small)));
    (line + t.space_16).max(t.control_height)
}

/// Single-line catalog note. Attach further guidance to the returned response.
pub(crate) fn catalog_footer(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), catalog_footer_height(ui)), egui::Sense::hover());
    t.secondary_hline(ui.painter(), rect.x_range(), rect.top(), t.dialog_rule);
    let icon =
        egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_6, rect.top() + t.space_8 + t.font_small * 0.5), vec2(t.space_12, t.space_12));
    ui.painter().circle_stroke(icon.center(), t.space_4, t.control_border_stroke(t.form_dim));
    ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, "i", crate::fonts::semibold(t.font_badge), t.form_dim);
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), egui::FontId::proportional(t.font_small), t.form_dim, (rect.width() - t.space_24).max(1.0));
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(rect.left_top() + vec2(t.space_24, t.space_8), galley, t.form_dim);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    crate::scenario::publish_rect("catalog:footer", rect);
    response
}

/// A short note with further guidance on its information icon and text.
pub(crate) fn catalog_footer_with_tip(ui: &mut Ui, text: &str, tip: &str) {
    catalog_footer(ui, text).on_hover_text(tip);
}

/// A filling catalog body above a measured, bottom-anchored footer. Long detail
/// cards scroll independently so the body retains its requested minimum height.
pub(crate) struct CatalogLayout {
    id: egui::Id,
    key: String,
    bounds: egui::Rect,
    footer_height: f32,
    min_body_height: f32,
}

impl CatalogLayout {
    pub(crate) fn new(ui: &Ui, id: &str, min_rows: usize) -> Self {
        let key = id.to_owned();
        let id = ui.make_persistent_id(id);
        let t = Tokens::get();
        let min_body_height = min_rows as f32 * (t.control_height + t.space_10) + min_rows.saturating_sub(1) as f32 * ui.spacing().item_spacing.y;
        Self {
            id,
            key,
            bounds: ui.available_rect_before_wrap(),
            footer_height: ui.data(|data| data.get_temp::<f32>(id)).unwrap_or(0.0),
            min_body_height,
        }
    }

    pub(crate) fn body<R>(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        let gap = ui.spacing().item_spacing.y;
        let height = (ui.available_height() - self.footer_height - gap).max(self.min_body_height).min(ui.available_height()).max(0.0);
        let scroll = egui::ScrollArea::vertical()
            .id_salt(self.id.with("body"))
            .max_height(height)
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .show(ui, add);
        scenario::publish_rect(&format!("{}:body", self.key), scroll.inner_rect);
        scroll.inner
    }

    pub(crate) fn footer(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        let scroll = egui::ScrollArea::vertical()
            .id_salt(self.id.with("footer"))
            .max_height(ui.available_height().max(0.0))
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .show(ui, add);
        let height = scroll.content_size.y;
        ui.data_mut(|data| data.insert_temp(self.id, height));
        if (height - self.footer_height).abs() > 0.5 {
            ui.ctx().request_discard("catalog footer measurement changed");
        }
        scenario::publish_rect(&format!("{}:content", self.key), self.bounds);
        scenario::publish_rect(&format!("{}:footer", self.key), scroll.inner_rect);
    }
}

/// Read the enclosing row accessibility label. Tokens: `none`.
pub(crate) fn label(ui: &Ui) -> &str {
    ui.stack().iter().find_map(|node| node.tags().get_downcast::<String>(LABEL)).map(String::as_str).unwrap_or("")
}

/// Publish accessibility and scenario geometry. Tokens: `none`.
pub(super) fn publish(ui: &Ui, response: &Response, kind: egui::WidgetType) {
    let t = Tokens::get();
    match kind {
        egui::WidgetType::TextEdit => {
            t.input_bevel(ui, response);
            if response.has_focus() {
                crate::prefs_search::note_key_sink(ui.ctx());
            }
        }
        egui::WidgetType::ComboBox | egui::WidgetType::Button => t.button_bevel(ui, response, false),
        _ => {}
    }
    let label = label(ui);
    response.widget_info(|| egui::WidgetInfo::labeled(kind, ui.is_enabled(), crate::i18n::tr_label(label)));
    publish_geometry(ui, response);
}

/// Aligned label/control row, 28 pt. Tokens: `control_height, form_label_width, font_label, dialog_label, space_8`.
pub(crate) fn row<R>(ui: &mut Ui, text: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    sized_row(ui, text, row_height(), control)
}

/// Semantic ink for a form's explanatory message.
#[derive(Clone, Copy)]
pub(crate) enum StatusTone {
    Dim,
    Warning,
    Error,
}

/// Full-width, wrapping status or refusal text. Place directly in the form, outside a `row`:
/// rows clip their controls to one line. Tokens: `font_label, text_dim, warning, error`.
pub(crate) fn status_line(ui: &mut Ui, text: &str, tone: StatusTone) -> Response {
    let t = Tokens::get();
    let ink = match tone {
        StatusTone::Dim => t.text_dim,
        StatusTone::Warning => t.warning,
        StatusTone::Error => t.error,
    };
    let response = ui.add(egui::Label::new(RichText::new(crate::i18n::tr_label(text)).size(t.font_label).color(ink)).wrap());
    scenario::publish_rect(&format!("dialog:status:{text}"), response.rect);
    response
}

/// Declarative expression row; retains the field's completion and accepted Enter. Tokens: `row, field`.
pub(crate) fn expr_row(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    row(ui, label, |ui| super::field(ui, value))
}

/// Whole-number count row with step arrows; retains completion and accepted Enter. Tokens: `row, count_field`.
pub(crate) fn count_row(ui: &mut Ui, label: &str, value: &mut String, limits: &super::Count<'_>) -> bool {
    row(ui, label, |ui| super::count_field(ui, value, limits))
}

/// Expression row with an empty-value hint. Tokens: `row, hint_field`.
pub(crate) fn hint_row(ui: &mut Ui, label: &str, value: &mut String, hint: &str) -> bool {
    row(ui, label, |ui| super::hint_field(ui, value, hint))
}

/// Plain text row; the caller retains Enter/change ownership. Tokens: `row, text_field`.
pub(crate) fn text_row(ui: &mut Ui, label: &str, value: &mut String, hint: &str) -> (bool, bool) {
    row(ui, label, |ui| super::text_field(ui, value, hint))
}

/// Indexed choice row with the caller's unchanged dropdown ID. Tokens: `row, dropdown`.
pub(crate) fn choice_row(ui: &mut Ui, label: &str, id: &str, selected: &mut usize, options: &[&str]) -> Response {
    row(ui, label, |ui| super::dropdown(ui, id, selected, options))
}

/// Internal form geometry for controls which need several lines (e.g. text areas).
pub(super) fn sized_row<R>(ui: &mut Ui, text: &str, height: f32, control: impl FnOnce(&mut Ui) -> R) -> R {
    let mut passthrough = false;
    if crate::prefs_search::querying(ui) {
        // A row with no label has no words to match; its controls supply them (#9265).
        let unlabelled = text.is_empty();
        if unlabelled {
            crate::prefs_search::set_unlabelled_control(ui);
        } else {
            crate::prefs_search::set_control(ui, text);
        }
        if !crate::prefs_search::visible(ui, &[text]) {
            if unlabelled {
                crate::prefs_search::push_unlabelled(ui);
            }
            let result = crate::prefs_search::run_hidden(ui, text, control);
            if unlabelled {
                crate::prefs_search::pop_unlabelled(ui);
            }
            crate::prefs_search::recheck(ui, &[text]);
            return result;
        }
        crate::prefs_search::push_passthrough(ui);
        passthrough = true;
    }
    let result = sized_row_body(ui, text, height, control);
    if passthrough {
        crate::prefs_search::pop_passthrough(ui);
    }
    result
}

fn sized_row_body<R>(ui: &mut Ui, text: &str, height: f32, control: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    crate::prefs_search::remember(ui, text, rect);
    crate::prefs_search::follow(ui, text, rect);
    // A nested row (`"  Of"`) lines up under the one above: the lookup uses the text without its
    // indent and the indent stays in front of the translation. The `LABEL` tag keeps the original.
    let shown = indented_label(text);
    // Measuring takes the original text and prepares it the same way, so column and row agree.
    let label_width = label_column(ui, text, width);
    let label_rect = egui::Rect::from_min_size(rect.min, vec2(label_width, rect.height()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(label_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        // Left aligned in the label column, as in the design.
        ui.add(egui::Label::new(RichText::new(shown.clone()).size(t.font_label).color(t.dialog_label)).truncate()).on_hover_text(shown);
    });
    let control_rect = egui::Rect::from_min_max(egui::pos2((label_rect.right() + label_gap(ui, width)).min(rect.right()), rect.top()), rect.max);
    let result = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(text)
                .max_rect(control_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center))
                .ui_stack_info(egui::UiStackInfo::default().with_tag_value(LABEL, text.to_string())),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(control_rect));
                ui.spacing_mut().text_edit_width = control_rect.width();
                control(ui)
            },
        )
        .inner;
    scenario::publish_rect(&format!("dialog:row:{text}"), rect);
    scenario::publish_rect(&format!("dialog:label:{text}"), label_rect);
    scenario::publish_rect(&format!("dialog:control:{text}"), control_rect);
    scenario::publish_handle(&format!("dialog:row:{text}"), rect.center());
    result
}

/// A dim one-line caption in the control column of the row labelled `aligned_to`, directly under that
/// row's control. It says in words what a colour or an icon shows, so colour is never the only cue.
/// Dim helper beneath a row control. Tokens: `form_icon_size, font_small, text_dim, space_8; label-column tokens`.
pub(crate) fn caption(ui: &mut Ui, aligned_to: &str, text: &str) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(width, t.form_icon_size), egui::Sense::hover());
    let left = (rect.left() + label_column(ui, aligned_to, width) + label_gap(ui, width)).min(rect.right());
    let text_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), rect.max);
    paint_text(ui, text_rect, text, egui::FontId::proportional(t.font_small), t.form_dim, false);
    scenario::publish_rect(&format!("dialog:caption:{aligned_to}"), text_rect);
}

/// Where a [`figure_rows`] field went (in the order given): the field's rect, and whether it is
/// hovered or has the keyboard (its part of the figure lights up).
pub(crate) struct FigureSlot {
    pub rect: egui::Rect,
    pub hot: bool,
}

/// A figure in the label column beside a stack of expression fields, each under its own caption,
/// for a picture whose parts the fields size (the Hole dialog's section): the caller paints the
/// figure where the fields went, with leaders to them. At least four rows tall; the fields keep
/// their labels' handles and accessible names. Returns whether Enter was accepted in a field.
/// Tokens: `control_height, form_label_width, font_small, form_dim, space_2/4, label gap`.
pub(crate) fn figure_rows(
    ui: &mut Ui,
    id: &str,
    fields: &mut [(&str, &mut String)],
    paint: impl FnOnce(&egui::Painter, egui::Rect, &[FigureSlot]),
) -> bool {
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let slot = t.font_small + t.space_2 + row_height();
    let stack = fields.len() as f32 * slot + fields.len().saturating_sub(1) as f32 * t.space_4;
    let height = stack.max(row_height() * 4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let figure = egui::Rect::from_min_size(rect.min, vec2((t.form_label_width * 1.4).min(width * 0.45), height));
    let left = (figure.right() + label_gap(ui, width)).min(rect.right());
    let focused = ui.ctx().memory(|m| m.focused()).and_then(|f| ui.ctx().read_response(f)).map(|r| r.rect);
    let mut enter = false;
    let mut top = rect.top() + (height - stack) / 2.0;
    let mut slots = Vec::with_capacity(fields.len());
    for (label, value) in fields.iter_mut() {
        let caption = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(rect.right(), top + t.font_small));
        paint_text(ui, caption, crate::i18n::tr_label(label), egui::FontId::proportional(t.font_small), t.form_dim, false);
        let field = egui::Rect::from_min_max(egui::pos2(left, caption.bottom() + t.space_2), egui::pos2(rect.right(), top + slot));
        enter |= ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt((id, *label))
                    .max_rect(field)
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .ui_stack_info(egui::UiStackInfo::default().with_tag_value(LABEL, label.to_string())),
                |ui| {
                    ui.spacing_mut().text_edit_width = field.width();
                    super::field(ui, value)
                },
            )
            .inner;
        let hot = ui.rect_contains_pointer(field) || focused.is_some_and(|f| field.contains_rect(f));
        slots.push(FigureSlot { rect: field, hot });
        top += slot + t.space_4;
    }
    paint(ui.painter(), figure, &slots);
    scenario::publish_rect(&format!("dialog:figure:{id}"), rect);
    scenario::publish_rect(&format!("dialog:figure_picture:{id}"), figure);
    enter
}

/// Memory belongs to the enclosing dialog ID; animation is avoided so each row keeps its fixed height.
/// Persisted disclosure with a rule and uppercase title. Tokens: `control_height, space_2, radius_sm, hover, dialog_rule, form_section_icon_size, form_gap_y, font_caption, font_section_tracking, text_dim`.
/// Static section caption for an Inspector group that stays expanded.
#[cfg(test)]
pub(crate) fn section_heading(ui: &mut Ui, title: &str) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_16), egui::Sense::hover());
    paint_text(ui, rect, &crate::i18n::tr_label(title).to_uppercase(), crate::fonts::semibold(t.font_caption), t.form_dim, true);
    scenario::publish_rect(&format!("dialog:heading:{title}"), rect);
}

pub(crate) fn section<R>(ui: &mut Ui, id: impl egui::AsIdSalt, title: &str, default_open: bool, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    section_count(ui, id, title, default_open, None::<usize>, add)
}

/// Section with a renderer/method tag. Shares disclosure, typography and badge tokens with section_count.
pub(crate) fn section_method<R>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    title: &str,
    default_open: bool,
    method: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    section_count(ui, id, title, default_open, Some(method), add)
}

/// Section heading with an optional count badge; disclosure state uses the same identity.
pub(crate) fn section_count<R>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    title: &str,
    default_open: bool,
    count: Option<impl std::fmt::Display>,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    section_with_badge(ui, id, title, default_open, (count.map(|count| count.to_string()), false, false), add)
}

/// Dense editor disclosure: 24 pt heading and 6 pt field gaps, with the same saved state and commands.
pub(crate) fn editor_section<R>(ui: &mut Ui, id: impl egui::AsIdSalt, title: &str, default_open: bool, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    section_with_badge(ui, id, title, default_open, (None, false, true), add)
}

/// Compact editor disclosure with a method badge. Unsupported content stays
/// inspectable and disabled; its translated reason remains visible when folded.
/// Uses the editor section's spacing, typography and semantic theme tokens.
pub(crate) fn method_section<R>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    title: &str,
    method: &str,
    reason: Option<&str>,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let result =
        section_with_badge(ui, id, title, false, (Some(method.to_owned()), false, true), |ui| ui.add_enabled_ui(reason.is_none(), add).inner);
    if let Some(reason) = reason {
        super::text(ui, reason, super::Text::Note("method_section_reason"));
    }
    result
}

/// A collapsed upcoming group with an inert disclosure and a trailing status chip.
#[cfg(test)]
pub(crate) fn pending_section(ui: &mut Ui, id: impl egui::AsIdSalt, title: &str, badge: &str) {
    section_with_badge(ui, id, title, false, (Some(badge.to_owned()), true, true), |_| {});
}

fn section_with_badge<R>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    title: &str,
    default_open: bool,
    badge: (Option<String>, bool, bool),
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let (count, pending, compact) = badge;
    // A search still walks the body, so a setting inside a folded section can match, without
    // leaving the folded heading on screen for every query.
    if crate::prefs_search::querying(ui) && crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[title]) {
        return Some(add(ui));
    }
    let searching = crate::prefs_search::querying(ui);
    let t = Tokens::get();
    ui.add_space(t.space_2);
    let id = ui.make_persistent_id(id);
    let mut open = !pending && (searching || ui.ctx().data_mut(|data| data.get_persisted::<bool>(id)).unwrap_or(default_open));
    let (rect, response) = ui.allocate_exact_size(
        vec2(dimension(ui.available_width()), if compact { t.control_height - t.space_4 } else { row_height() }),
        egui::Sense::click(),
    );
    crate::prefs_search::follow(ui, title, rect);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), t.radius_sm, t.hover);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + t.form_section_icon_size / 2.0, rect.center().y),
        vec2(t.form_section_icon_size, t.form_section_icon_size),
    );
    super::disclosure(ui.painter(), icon_rect.center(), open, t.form_dim);
    let text_rect = egui::Rect::from_min_max(egui::pos2((icon_rect.right() + t.form_gap_y).min(rect.right()), rect.top()), rect.max);
    let caption = crate::i18n::tr_label(title).to_uppercase();
    let badge_value = count;
    let badge_width = badge_value.as_ref().map_or(0.0, |value| label_width(ui, value) + t.space_12);
    let reserved = if badge_value.is_some() { badge_width + t.space_12 } else { 0.0 };
    let mut format = egui::TextFormat::simple(crate::fonts::semibold(t.font_caption), t.form_dim);
    format.extra_letter_spacing = t.font_caption * t.font_section_tracking;
    let title_width = ui
        .fonts_mut(|fonts| fonts.layout_job(egui::text::LayoutJob::single_section(caption.clone(), format)))
        .size()
        .x
        .ceil()
        .min((text_rect.width() - reserved).max(0.0));
    let title_rect = egui::Rect::from_min_max(text_rect.min, egui::pos2(text_rect.left() + title_width, text_rect.bottom()));
    paint_text(ui, title_rect, &caption, crate::fonts::semibold(t.font_caption), t.form_dim, true);
    let mut rule_left = title_rect.right() + t.space_12;
    let mut rule_right = rect.right();
    if let Some(value) = badge_value {
        let width = badge_width.min((rect.right() - rule_left).max(0.0));
        let left = if pending { rect.right() - width } else { rule_left.min(rect.right()) };
        let badge = egui::Rect::from_min_size(egui::pos2(left, rect.center().y - t.space_10), vec2(width, t.space_10 * 2.0));
        ui.painter().rect_filled(badge, t.radius_xl, t.dialog_footer);
        ui.painter().with_clip_rect(ui.clip_rect().intersect(badge)).text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            value,
            egui::FontId::proportional(t.font_small),
            t.form_dim,
        );
        scenario::publish_rect(&format!("dialog:section:{title}:count"), badge);
        if pending {
            rule_right = badge.left() - t.space_12;
        } else {
            rule_left = badge.right() + t.space_12;
        }
    }
    if rule_left < rule_right {
        t.secondary_hline(ui.painter(), rule_left..=rule_right, rect.center().y, t.dialog_rule);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), open, crate::i18n::tr_label(title)));
    scenario::publish_handle(&format!("dialog:section:{title}"), response.rect.center());
    scenario::publish_rect(&format!("dialog:section:{title}"), rect);
    if response.clicked() && !pending && !searching {
        open = !open;
    }
    if !searching {
        ui.ctx().data_mut(|data| data.insert_persisted(id, open));
    }
    open.then(|| {
        ui.push_id(id, |ui| {
            if compact {
                ui.spacing_mut().item_spacing.y = t.space_6;
            }
            add(ui)
        })
        .inner
    })
}

/// Shortcut matches during Preferences search. Closed until opened, and closed again when the
/// query changes, so a short word does not lay out every command.
pub(crate) fn search_fold<R>(ui: &mut Ui, title: &str, count: usize, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let t = Tokens::get();
    let id = egui::Id::new(("prefs_search_fold", title));
    let query = crate::prefs_search::query(ui);
    let mut open = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let previous = ui.data(|data| data.get_temp::<String>(id.with("query"))).unwrap_or_default();
    if previous != query {
        open = false;
    }
    crate::prefs_search::reveal(ui);
    ui.add_space(t.space_2);
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::click());
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), t.radius_sm, t.hover);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + t.form_section_icon_size / 2.0, rect.center().y),
        vec2(t.form_section_icon_size, t.form_section_icon_size),
    );
    super::disclosure(ui.painter(), icon_rect.center(), open, t.form_dim);
    let text_rect = egui::Rect::from_min_max(egui::pos2((icon_rect.right() + t.form_gap_y).min(rect.right()), rect.top()), rect.max);
    let caption = title.to_uppercase();
    let badge_value = count.to_string();
    let badge_width = label_width(ui, &badge_value) + t.space_12;
    let reserved = badge_width + t.space_12;
    let mut format = egui::TextFormat::simple(crate::fonts::semibold(t.font_caption), t.form_dim);
    format.extra_letter_spacing = t.font_caption * t.font_section_tracking;
    let title_width = ui
        .fonts_mut(|fonts| fonts.layout_job(egui::text::LayoutJob::single_section(caption.clone(), format)))
        .size()
        .x
        .ceil()
        .min((text_rect.width() - reserved).max(0.0));
    let title_rect = egui::Rect::from_min_max(text_rect.min, egui::pos2(text_rect.left() + title_width, text_rect.bottom()));
    paint_text(ui, title_rect, &caption, crate::fonts::semibold(t.font_caption), t.form_dim, true);
    let mut rule_left = title_rect.right() + t.space_12;
    let width = badge_width.min((rect.right() - rule_left).max(0.0));
    let badge = egui::Rect::from_min_size(egui::pos2(rule_left.min(rect.right()), rect.center().y - t.space_10), vec2(width, t.space_10 * 2.0));
    ui.painter().rect_filled(badge, t.radius_xl, t.dialog_footer);
    ui.painter().with_clip_rect(ui.clip_rect().intersect(badge)).text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        &badge_value,
        egui::FontId::proportional(t.font_small),
        t.form_dim,
    );
    rule_left = badge.right() + t.space_12;
    if rule_left < rect.right() {
        t.secondary_hline(ui.painter(), rule_left..=rect.right(), rect.center().y, t.dialog_rule);
    }
    let accessible = format!("{title} ({count})");
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), open, &accessible));
    scenario::publish_rect("prefs:search:shortcuts", rect);
    scenario::publish_handle("prefs:search:shortcuts", response.rect.center());
    if response.clicked() {
        open = !open;
    }
    ui.data_mut(|data| {
        data.insert_temp(id.with("query"), query);
        data.insert_temp(id, open);
    });
    open.then(|| {
        crate::prefs_search::push_force(ui);
        let result = add(ui);
        crate::prefs_search::pop_force(ui);
        result
    })
}

/// Picked-item list entry with a remove action. Returns that action's Response.
/// Tokens: control_height, space_8, font_label, text and icon-button tokens.
/// Picked-item row with a remove action. Tokens: `control_height, space_8, font_label, text; icon-button tokens`.
pub(crate) fn list_row(ui: &mut Ui, key: &str, text: &str) -> Response {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::hover());
    let remove = egui::Rect::from_min_max(egui::pos2((rect.right() - row_height()).max(rect.left()), rect.top()), rect.max);
    let label = egui::Rect::from_min_max(rect.min, egui::pos2((remove.left() - t.space_8).max(rect.left()), rect.bottom()));
    paint_text(ui, label, text, egui::FontId::proportional(t.font_label), t.form_ink, false);
    let response = ui.push_id(key, |ui| super::icon_button_at(ui, remove, "close", "Remove")).inner;
    scenario::publish_rect(&format!("dialog:list:{key}"), rect);
    scenario::publish_handle(&format!("dialog:list:{key}:remove"), remove.center());
    response
}

/// Full-width rule; Tokens: dialog_rule. Use between groups without extra padding.
/// Full-width rule without extra padding. Tokens: `dialog_rule`.
pub(crate) fn divider(ui: &mut Ui, key: &str) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), 1.0), egui::Sense::hover());
    Tokens::get().secondary_hline(ui.painter(), rect.x_range(), rect.top(), Tokens::get().dialog_rule);
    scenario::publish_rect(&format!("dialog:divider:{key}"), rect);
}

/// Give a compound row's subfield its accessible name without allocating layout space.
/// Name a subfield without allocating another row. Tokens: `none`.
pub(super) fn named_control<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().id_salt(label).ui_stack_info(egui::UiStackInfo::default().with_tag_value(LABEL, label.to_owned())), add)
        .inner
}

/// Publish geometry without replacing native accessibility. Tokens: `none`.
pub(super) fn publish_geometry(ui: &Ui, response: &Response) {
    let label = label(ui);
    scenario::publish_target_as(&format!("dialog:{label}"), &format!("dialog:widget:{label}"), response.rect);
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;

/// Large read-only licence/report with equal-height virtualized lines. Long lines elide;
/// hovering exposes the full line without introducing a horizontal scrollbar.
/// Tokens: font_small, text, control_height. Only visible lines are laid out.
pub(crate) fn text_lines(ui: &mut Ui, id: &str, text: &str) {
    let t = Tokens::get();
    let lines: Vec<_> = text.lines().collect();
    let font = egui::FontId::monospace(t.font_small);
    let height = ui.fonts_mut(|f| f.row_height(&font));
    let response = egui::ScrollArea::vertical().id_salt(id).max_height(t.control_height * 15.0).show_rows(ui, height, lines.len(), |ui, rows| {
        for line in lines.get(rows).unwrap_or_default() {
            ui.add(egui::Label::new(RichText::new(*line).font(font.clone()).color(t.form_ink)).truncate()).on_hover_text(*line);
        }
    });
    scenario::publish_rect(&format!("dialog:lines:{id}"), response.inner_rect);
}

/// Read-only text variants retain their original wrapping, allocation and scenario identities.
pub(crate) enum Text<'a> {
    Hint(&'a str),
    Note(&'a str),
    MonoNote(&'a str),
    Report(&'a str, bool),
    Error(&'a str),
    Heading(&'a str),
    Inline,
    Cell,
    NameCell(bool),
    MonoCell,
    Value(&'a str, bool),
}

fn note_galley(ui: &Ui, text: &str) -> std::sync::Arc<egui::Galley> {
    let t = Tokens::get();
    ui.painter().layout(text.to_owned(), egui::FontId::proportional(t.font_small), t.form_dim, dimension(ui.available_width()))
}

/// Reserve the actual wrapped note height when budgeting a bounded card's scroll area.
#[cfg(test)]
pub(crate) fn note_height(ui: &Ui, text: &str) -> f32 {
    note_galley(ui, text).size().y + Tokens::get().space_4 * 2.0
}

pub(crate) fn text(ui: &mut Ui, text: &str, style: Text<'_>) -> Response {
    text_impl(ui, text, style, None)
}

/// One name-cell tooltip: the action hint, with the full name only if the cell clips it.
pub(crate) fn name_cell_with_hint(ui: &mut Ui, name: &str, strong: bool, hint: &str) -> Response {
    text_impl(ui, name, Text::NameCell(strong), Some(hint))
}

fn text_impl(ui: &mut Ui, text: &str, style: Text<'_>, hover_hint: Option<&str>) -> Response {
    if crate::prefs_search::querying(ui) {
        let heading = matches!(style, Text::Heading(_));
        if heading {
            let hit = crate::prefs_search::texts_hit(ui, &[text]);
            crate::prefs_search::set_heading_open(ui, hit);
            if hit {
                crate::prefs_search::reveal(ui);
            } else if crate::prefs_search::suppress_chrome(ui) {
                return crate::prefs_search::hidden_response(ui);
            }
        } else if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[text]) {
            return crate::prefs_search::hidden_response(ui);
        } else if crate::prefs_search::texts_hit(ui, &[text]) && !crate::prefs_search::shows_all(ui) {
            crate::prefs_search::reveal(ui);
        }
    }
    let t = Tokens::get();
    let fixed = matches!(style, Text::Cell | Text::NameCell(_) | Text::MonoCell | Text::Value(_, _));
    let ink = match style {
        Text::Hint(_) | Text::Note(_) | Text::MonoNote(_) => t.form_dim,
        Text::Inline if in_catalog(ui) => t.form_dim,
        Text::Report(_, true) | Text::Value(_, true) | Text::Error(_) => t.form_error,
        _ => t.form_ink,
    };
    let font = match style {
        Text::Inline if in_catalog(ui) => crate::fonts::semibold(t.font_small),
        Text::NameCell(true) => crate::fonts::semibold(t.font_label),
        Text::NameCell(false) => crate::fonts::medium(t.font_label),
        Text::Heading(_) => crate::fonts::semibold(t.font_button),
        Text::MonoCell | Text::Value(_, _) => egui::FontId::monospace(t.font_body),
        Text::MonoNote(_) => egui::FontId::monospace(t.font_small),
        Text::Hint(_) | Text::Error(_) => egui::FontId::proportional(t.font_body),
        Text::Note(_) => egui::FontId::proportional(t.font_small),
        _ => egui::FontId::proportional(t.font_label),
    };
    let response = if fixed {
        let (rect, response) = ui.allocate_exact_size(
            vec2(dimension(ui.available_width()), row_height()),
            if matches!(style, Text::Value(_, _)) { egui::Sense::hover() } else { egui::Sense::click() },
        );
        let tooltip = hover_hint.map(|hint| {
            if ui.painter().layout_no_wrap(text.to_owned(), font.clone(), ink).size().x > rect.width() {
                format!("{text}\n{hint}")
            } else {
                hint.to_owned()
            }
        });
        paint_text(ui, rect, text, font, ink, false);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
        response.on_hover_text(tooltip.as_deref().unwrap_or(text))
    } else if matches!(style, Text::Error(_)) {
        egui::Frame::NONE
            .fill(t.form_error_fill)
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::symmetric(t.form_inset as i8, t.space_6 as i8))
            .show(ui, |ui| {
                ui.set_min_width(dimension(ui.available_width()));
                ui.add(egui::Label::new(RichText::new(text).font(font).color(ink)).wrap())
            })
            .inner
    } else if matches!(style, Text::Hint(_) | Text::Note(_) | Text::MonoNote(_)) {
        let width = dimension(ui.available_width());
        let galley = if matches!(style, Text::Note(_)) { note_galley(ui, text) } else { ui.painter().layout(text.to_owned(), font, ink, width) };
        let padding = if matches!(style, Text::Note(_) | Text::MonoNote(_)) { t.space_4 } else { 0.0 };
        let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y + padding * 2.0), egui::Sense::hover());
        ui.painter().galley(egui::pos2(rect.left(), rect.top() + padding), galley, ink);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
        response
    } else {
        let label = egui::Label::new(RichText::new(text).font(font).color(ink));
        ui.add(if matches!(style, Text::Inline) { label.truncate() } else { label.wrap() })
    };
    let response = if matches!(style, Text::Hint(_)) { response.on_hover_text(text) } else { response };
    let (kind, key) = match style {
        Text::Hint(key) => ("hint", key),
        Text::Note(key) | Text::MonoNote(key) => ("note", key),
        Text::Report(key, _) | Text::Value(key, _) | Text::Error(key) | Text::Heading(key) => ("report", key),
        Text::Inline => ("inline", text),
        Text::Cell | Text::MonoCell | Text::NameCell(_) => ("cell", text),
    };
    let id = format!("dialog:{kind}:{key}");
    scenario::publish_rect(&id, response.rect);
    if matches!(style, Text::Heading(_)) {
        crate::prefs_search::remember(ui, text, response.rect);
        crate::prefs_search::follow(ui, text, response.rect);
    }
    if !matches!(style, Text::Note(_) | Text::MonoNote(_)) {
        scenario::publish_handle(&id, response.rect.center());
    }
    response
}

/// A two-line named item beside an enable checkbox and compact actions.
/// Tokens: control_height, space_4/8/16, dialog_rule/footer, form_ink/dim, font_label/small.
pub(crate) fn summary_row(ui: &mut Ui, name: &str, summary: &str, enabled: &mut bool, actions_width: f32, actions: impl FnOnce(&mut Ui)) -> bool {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height() + t.space_16), egui::Sense::hover());
    t.container_rect(ui.painter(), rect, t.radius_lg, t.dialog_body, t.dialog_rule);
    let check = egui::Rect::from_min_size(rect.min + vec2(t.space_6, 0.0), vec2(t.space_24, rect.height()));
    let changed = ui
        .scope_builder(egui::UiBuilder::new().max_rect(check).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            super::checkbox(ui, "", enabled).on_hover_text(crate::i18n::tr_label("Enable rule")).changed()
        })
        .inner;
    let right = (rect.right() - dimension(actions_width) - t.space_6).max(check.right());
    let text = egui::Rect::from_min_max(
        egui::pos2(check.right() + t.space_4, rect.top()),
        egui::pos2((right - t.space_8).max(check.right() + t.space_4), rect.bottom()),
    );
    let title = egui::Rect::from_min_max(text.min, egui::pos2(text.right(), text.center().y));
    let detail = egui::Rect::from_min_max(egui::pos2(text.left(), text.center().y), text.max);
    paint_text(ui, title, name, crate::fonts::semibold(t.font_label), t.form_ink, false);
    paint_text(ui, detail, summary, egui::FontId::proportional(t.font_small), t.form_dim, false);
    let response = ui.interact(text, ui.id().with("summary"), egui::Sense::hover()).on_hover_text(summary);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), format!("{name}: {summary}")));
    scenario::publish_rect(&format!("dialog:inline:{name}"), title);
    scenario::publish_rect("dialog:report:rule_report", detail);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(egui::pos2(right, rect.top()), rect.max))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        actions,
    );
    changed
}
