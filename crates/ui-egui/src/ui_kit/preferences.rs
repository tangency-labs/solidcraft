//! Shared preference compositions and explanatory figures. Figures are cheap painter content,
//! never additional application viewports; every colour and control metric comes from tokens.
use super::{metrics::dimension, text::paint_text};
use crate::{
    scenario,
    theme::{ColorVision, Tokens},
};
use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

pub(super) const PAGE_SCROLL_GUTTER: &str = "sc_page_scroll_gutter";

/// Presentation data is resolved by the caller; controls never distinguish standard/custom ids.
pub(crate) struct ThemeOptionRow<'a> {
    pub definition: &'a crate::theme_files::OptionDef,
    pub label: String,
    pub help: String,
    pub choices: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Copy)]
struct ThemeOptionMetrics {
    label: f32,
    readout: f32,
    reason: f32,
    minimum: f32,
}

fn theme_option_slider_minimum(t: &Tokens) -> f32 {
    t.space_24 * 2.0 + t.space_8
}

/// Measure the same fonts, gutters and control sizes used by `theme_option_control`.
fn theme_option_metrics(ui: &Ui, option: &ThemeOptionRow<'_>, value: Option<&crate::theme_files::OptionValue>) -> ThemeOptionMetrics {
    use crate::theme_files::{OptionKind, OptionValue};
    let t = Tokens::get();
    let label = super::ContentWidth::label(ui, &option.label) + t.space_4;
    let mut readout = 0.0;
    let control = match &option.definition.kind {
        OptionKind::Bool { .. } => t.space_24 + t.space_8,
        OptionKind::Range { default, min, max, step, unit } => {
            let current = match value {
                Some(OptionValue::Num(n)) => *n,
                _ => *default,
            };
            // Endpoints and adjacent steps reserve fractional readouts before the first edit.
            readout = [*min, *max, *default, current, (*min + *step).min(*max), (*max - *step).max(*min)]
                .into_iter()
                .map(|n| super::ContentWidth::label(ui, &format!("{n}{}", unit.as_deref().unwrap_or(""))))
                .fold(0.0, f32::max);
            theme_option_slider_minimum(&t) + t.space_8 + readout
        }
        OptionKind::Choice { values, .. } => {
            if values.len() > 5 {
                let labels: Vec<_> = option.choices.iter().map(String::as_str).collect();
                super::ContentWidth::dropdown(ui, &labels).min(t.form_label_width + t.space_24)
            } else {
                // Compact segments use equal widths based on the widest semibold label.
                let widest = option.choices.iter().map(|s| super::ContentWidth::name(ui, s)).fold(0.0, f32::max);
                (widest + t.space_16) * values.len() as f32
            }
        }
    };
    let reason = option
        .reason
        .as_ref()
        .map_or(0.0, |reason| ui.painter().layout_no_wrap(reason.clone(), FontId::proportional(t.font_body), t.form_dim).size().x.ceil() + t.space_8);
    // settings_form/row use the measured label plus a fixed space_14 gutter.
    ThemeOptionMetrics { label, readout, reason, minimum: label + t.space_14 + control + reason }
}

/// Ordered two-column list, with composite children sharing their parent's cell and hit row.
/// Returns Reset; edited values are returned through `values` for the caller's preference merge.
pub(crate) fn theme_options_list(
    ui: &mut Ui,
    theme_id: &str,
    name: &str,
    options: &[ThemeOptionRow<'_>],
    values: &mut std::collections::BTreeMap<String, crate::theme_files::OptionValue>,
) -> bool {
    use super::preferences_layout::{Entry, Width, choice_width, layout};
    use crate::theme_files::OptionKind;
    // Label/help/choice text is already resolved, including the theme's English fallback.
    let _labels = crate::i18n::resolved_labels();
    if options.is_empty() {
        return false;
    }
    let t = Tokens::get();
    let mut cells: Vec<Vec<usize>> = Vec::new();
    for (i, option) in options.iter().enumerate() {
        if option.definition.parent.as_ref().is_some_and(|id| options.iter().any(|o| &o.definition.id == id)) {
            continue;
        }
        let mut cell = vec![i];
        let mut parent = option.definition.id.as_str();
        for _ in 0..crate::theme_files::MAX_OPTIONS {
            let Some((j, child)) = options.iter().enumerate().find(|(_, o)| o.definition.parent.as_deref() == Some(parent)) else { break };
            if cell.contains(&j) {
                break;
            }
            cell.push(j);
            parent = &child.definition.id;
        }
        cells.push(cell);
    }
    let metrics: Vec<_> = options.iter().map(|option| theme_option_metrics(ui, option, values.get(&option.definition.id))).collect();
    let entries: Vec<_> = cells
        .iter()
        .map(|cell| {
            let mut width = if cell.len() > 1 { Width::Composite } else { Width::Narrow };
            let mut minimum = t.space_8 * cell.len().saturating_sub(1) as f32;
            let mut advanced = false;
            for &index in cell {
                let Some(option) = options.get(index) else { continue };
                advanced |= option.definition.advanced;
                if let OptionKind::Choice { values, .. } = &option.definition.kind
                    && choice_width(values.len()) == Width::Wide
                {
                    width = Width::Wide;
                }
                minimum += metrics.get(index).map_or(0.0, |m| m.minimum);
            }
            Entry { width, advanced, minimum }
        })
        .collect();
    let plan = layout(&entries, dimension(ui.available_width()) - t.space_16);
    let mut reset = false;
    let header = ui.horizontal_wrapped(|ui| {
        super::text(ui, name, super::Text::Inline);
        super::settings_inline_note(
            ui,
            &crate::i18n::format(
                if options.len() == 1 { "prefs.appearance.option_count_one" } else { "prefs.appearance.option_count" },
                &[("count", &options.len().to_string())],
            ),
        );
        if options.iter().any(|option| values.get(&option.definition.id).is_some_and(|v| *v != option.definition.default_value())) {
            let response = super::text_button(ui, crate::i18n::tr("prefs.theme_options_reset"), true);
            scenario::publish_target("prefs:theme_options_reset", response.rect);
            reset = response.clicked();
        }
    });
    scenario::publish_rect("prefs:theme_options_header", header.response.rect);
    let mut draw_rows = |ui: &mut Ui, rows: &[Vec<usize>]| {
        let mut section = None;
        for row in rows {
            // Labels are theme-authored metadata, shown only above the six-cell threshold.
            let label =
                row.iter().filter_map(|i| cells.get(*i)).flatten().filter_map(|i| options.get(*i)).find_map(|o| o.definition.section.as_deref());
            if plan.section_labels && label != section {
                if let Some(label) = label {
                    super::text(ui, label, super::Text::Hint("theme_option_section"));
                }
                section = label;
            }
            let width = dimension(ui.available_width());
            let cell_width = (width - t.space_16) / 2.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = t.space_16;
                for &index in row {
                    let Some(cell) = cells.get(index) else { continue };
                    let full = entries.get(index).is_some_and(|e| e.width == Width::Wide || e.minimum > cell_width);
                    let width = if full { width } else { cell_width };
                    ui.allocate_ui_with_layout(vec2(width.max(0.0), t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(width.max(0.0));
                        let stack = entries.get(index).is_some_and(|e| e.minimum > width);
                        let direction =
                            if stack { egui::Layout::top_down(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Center) };
                        let response = ui.with_layout(direction, |ui| {
                            ui.spacing_mut().item_spacing.x = t.space_8;
                            let total: f32 = cell.iter().filter_map(|i| metrics.get(*i)).map(|m| m.minimum).sum();
                            let usable = (width - t.space_8 * cell.len().saturating_sub(1) as f32).max(0.0);
                            for &i in cell {
                                if let (Some(option), Some(metrics)) = (options.get(i), metrics.get(i)) {
                                    let option_width = if stack { width } else { usable * metrics.minimum / total.max(1.0) };
                                    ui.allocate_ui_with_layout(
                                        vec2(option_width.max(0.0), t.control_height),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            theme_option_control(ui, option, options, values, metrics, cell.len() == 1);
                                        },
                                    );
                                }
                            }
                        });
                        if let Some(option) = cell.first().and_then(|i| options.get(*i)) {
                            scenario::publish_rect(&format!("prefs:theme_option_cell:{}", option.definition.id), response.response.rect);
                        }
                    });
                }
            });
        }
    };
    ui.push_id(("theme_options", theme_id), |ui| {
        draw_rows(ui, &plan.rows);
        if plan.more_count > 0 {
            let title = crate::i18n::format("prefs.theme_options_more", &[("count", &plan.more_count.to_string())]);
            super::section(ui, "more", &title, false, |ui| draw_rows(ui, &plan.more));
            if let Some(rect) = scenario::rect_of(&format!("dialog:section:{title}")) {
                scenario::publish_target("prefs:theme_options_more", rect);
            }
        }
    });
    reset
}

fn theme_option_control(
    ui: &mut Ui,
    option: &ThemeOptionRow<'_>,
    options: &[ThemeOptionRow<'_>],
    values: &mut std::collections::BTreeMap<String, crate::theme_files::OptionValue>,
    metrics: &ThemeOptionMetrics,
    fill_range: bool,
) {
    use crate::theme_files::{OptionKind, OptionValue};
    let t = Tokens::get();
    let def = option.definition;
    let mut active = option.reason.is_none();
    let mut parent = def.parent.as_deref();
    for _ in 0..crate::theme_files::MAX_OPTIONS {
        let Some(id) = parent else { break };
        active &= values.get(id) == Some(&OptionValue::Bool(true));
        let ancestor = options.iter().find(|o| o.definition.id == id);
        active &= ancestor.is_some_and(|o| o.reason.is_none());
        parent = ancestor.and_then(|o| o.definition.parent.as_deref());
    }
    let mut value = values.get(&def.id).cloned().unwrap_or_else(|| def.default_value());
    let before = value.clone();
    let control_width = (ui.available_width() - metrics.reason).max(0.0);
    let response = ui.allocate_ui_with_layout(vec2(control_width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.push_id(&def.id, |ui| {
            ui.add_enabled_ui(active, |ui| {
                // `row` owns the accessible control label and the shared control geometry.
                super::settings_form(ui, metrics.label, |ui| {
                    super::row(ui, &option.label, |ui| {
                        match (&def.kind, &mut value) {
                            (OptionKind::Bool { .. }, OptionValue::Bool(on)) => {
                                theme_option_switch(ui, &option.label, on);
                            }
                            (OptionKind::Range { min, max, step, unit, .. }, OptionValue::Num(n)) => {
                                let available = dimension(ui.available_width() - metrics.readout - t.space_8);
                                // Standalone ranges fill their cell; composites retain their compact control alignment.
                                let width =
                                    if fill_range { available } else { available.min(t.form_label_width.max(theme_option_slider_minimum(&t))) };
                                ui.allocate_ui_with_layout(vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    super::slider(
                                        ui,
                                        n,
                                        *min..=*max,
                                        super::Slider { step: Some(f64::from(*step)), show_value: false, ..Default::default() },
                                    );
                                });
                                // Keep fractional theme values visible, including sliders with fractional steps.
                                let readout = ui.allocate_ui_with_layout(
                                    vec2(metrics.readout, t.control_height),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        super::text(ui, &format!("{}{}", *n, unit.as_deref().unwrap_or("")), super::Text::Inline);
                                    },
                                );
                                scenario::publish_rect(&format!("prefs:theme_option_readout:{}", def.id), readout.response.rect);
                            }
                            (OptionKind::Choice { values, .. }, OptionValue::Choice(selected)) => {
                                let labels: Vec<_> = option.choices.iter().map(String::as_str).collect();
                                let mut index = values.iter().position(|v| &v.id == selected).unwrap_or(0);
                                let changed = if values.len() > 5 {
                                    super::dropdown_picked(ui, &def.id, &mut index, &labels)
                                } else {
                                    super::segmented(ui, &mut index, &labels, super::Segments { compact: true, ..Default::default() })
                                };
                                if changed && let Some(choice) = values.get(index) {
                                    *selected = choice.id.clone();
                                }
                            }
                            _ => {}
                        }
                    })
                });
            })
        })
        .inner
        .response
    });
    let response = response.inner.on_hover_text(&option.help);
    scenario::publish_target(&format!("prefs:theme_option:{}", def.id), response.rect);
    if let Some(reason) = &option.reason {
        let note = ui.horizontal(|ui| super::settings_inline_note(ui, reason));
        scenario::publish_rect(&format!("prefs:theme_option_reason:{}", def.id), note.response.rect);
    }
    if active && value != before {
        values.insert(def.id.clone(), value);
    }
}

fn theme_option_switch(ui: &mut Ui, label: &str, on: &mut bool) {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(t.space_24 + t.space_8, t.control_height), Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), t.space_16 + t.space_2));
    ui.painter().rect_filled(track, t.radius_xl, if *on { t.form_primary_fill } else { t.control_border });
    let x = if *on { track.right() - t.space_8 - t.space_2 } else { track.left() + t.space_8 + t.space_2 };
    ui.painter().circle_filled(pos2(x, track.center().y), t.space_6, if *on { t.on_accent } else { t.form_ink });
    if response.has_focus() {
        ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Inside);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label));
    scenario::publish_target(&format!("prefs:theme_option_switch:{label}"), response.rect);
}

/// Page scroller keeps content aligned while centring its bar in the window's right inset.
/// Nested pages (including Shortcuts) inherit the same gutter and height contract.
pub(crate) fn settings_scroll<R>(ui: &mut Ui, id: impl egui::AsIdSalt, add: impl FnOnce(&mut Ui) -> R) -> egui::scroll_area::ScrollAreaOutput<R> {
    let t = Tokens::get();
    let gutter = ui.stack().iter().find_map(|node| node.tags().get_downcast::<f32>(PAGE_SCROLL_GUTTER)).copied().unwrap_or(0.0);
    let width = dimension(ui.available_width());
    let height = if ui.available_height().is_finite() { dimension(ui.available_height()) } else { t.control_height * 16.0 };
    let bar = ui.spacing().scroll.floating_width;
    let extension = if gutter > 0.0 { gutter / 2.0 + bar / 2.0 } else { 0.0 };
    let host = Rect::from_min_size(ui.next_widget_position(), vec2(width + extension, height));
    ui.scope_builder(egui::UiBuilder::new().max_rect(host).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        if gutter > 0.0 {
            let scroll = &mut ui.style_mut().spacing.scroll;
            scroll.floating = true;
            scroll.floating_allocated_width = 0.0;
            scroll.bar_outer_margin = 0.0;
        }
        let mut scroll = egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            if gutter > 0.0 {
                // The scrollbar host extends into the gutter. Its content must neither use
                // that extension nor grow the page/window when a child requests extra width.
                let bounds = Rect::from_min_size(ui.next_widget_position(), vec2(width, ui.available_height()));
                let mut content = ui.new_child(egui::UiBuilder::new().max_rect(bounds));
                let clip = content.clip_rect();
                content.set_clip_rect(clip.intersect(Rect::from_min_max(pos2(bounds.left(), clip.top()), pos2(bounds.right(), clip.bottom()))));
                let result = add(&mut content);
                ui.allocate_exact_size(vec2(width, content.min_size().y), Sense::hover());
                result
            } else {
                add(ui)
            }
        });
        // Consumers publish the page's content well, not the extended scrollbar host.
        if gutter > 0.0 {
            scroll.inner_rect.max.x = scroll.inner_rect.left() + width;
        }
        scroll
    })
    .inner
}

pub(crate) fn settings_heading(ui: &mut Ui, title: &str, description: &str) {
    settings_heading_with_tag(ui, title, description, None);
}

pub(crate) fn settings_heading_with_tag(ui: &mut Ui, title: &str, description: &str, tag: Option<fn(&mut Ui)>) {
    if crate::prefs_search::querying(ui) {
        return;
    }
    let t = Tokens::get();
    let page = super::settings_page_active(ui);
    let heading = ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_3;
        let (rect, response) =
            ui.allocate_exact_size(vec2(dimension(ui.available_width()), if page { t.space_24 } else { t.space_16 + t.space_3 }), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), title));
        paint_text(ui, rect, title, crate::fonts::semibold(if page { t.font_page_heading } else { t.font_intro }), t.form_ink, false);
        if let Some(tag) = tag {
            let width = ui
                .painter()
                .layout_no_wrap(title.to_owned(), crate::fonts::semibold(if page { t.font_page_heading } else { t.font_intro }), t.form_ink)
                .size()
                .x;
            let start = (rect.left() + width + t.space_8).min(rect.right());
            let tag_rect = Rect::from_min_max(egui::pos2(start, rect.top()), rect.max);
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(tag_rect).layout(if page { egui::Layout::right_to_left(egui::Align::Center) } else { *ui.layout() }),
                tag,
            );
        }
        super::text(ui, description, super::Text::Hint("settings_heading"));
    });
    if page {
        scenario::publish_rect("prefs:page_heading", heading.response.rect);
    }
}

/// Settings reuse the same disclosure, uppercase typography and rule as the design system.
pub(crate) fn settings_section<R>(ui: &mut Ui, key: &str, title: &str, add: impl FnMut(&mut Ui) -> R) -> Option<R> {
    settings_section_count(ui, key, title, None, add)
}

/// A settings disclosure with a real item count, using the shared section badge.
pub(crate) fn settings_section_count<R>(ui: &mut Ui, key: &str, title: &str, count: Option<&str>, mut add: impl FnMut(&mut Ui) -> R) -> Option<R> {
    if crate::prefs_search::querying(ui) {
        crate::prefs_search::begin_section(ui, title);
        if !ui.is_visible() {
            return Some(add(ui));
        }
        if crate::prefs_search::shows_all(ui) {
            let mut child = crate::prefs_search::detached_child(ui, ("prefs-search-section", key), false);
            let result = add(&mut child);
            crate::prefs_search::place_search_child(ui, &child);
            return Some(result);
        }
        let mark = crate::prefs_search::search_layout_mark(ui);
        let before = crate::prefs_search::hits(ui);
        let (gained, missed) = {
            let mut probe = crate::prefs_search::detached_child(ui, ("prefs-search-section-probe", key), true);
            let missed = add(&mut probe);
            (crate::prefs_search::hits(ui) > before, missed)
        };
        crate::prefs_search::restore_search_layout(ui, mark);
        if !gained {
            return Some(missed);
        }
        let mut child = crate::prefs_search::detached_child(ui, ("prefs-search-section", key), false);
        let result = add(&mut child);
        crate::prefs_search::place_search_child(ui, &child);
        return Some(result);
    }
    let t = Tokens::get();
    let page = super::settings_page_active(ui);
    if page {
        // The page's 10 pt item gap plus the shared disclosure's 2 pt inset complete 38 pt.
        ui.add_space(t.space_24 + t.space_14 - t.space_10 - t.space_2);
    } else {
        ui.add_space(t.space_8);
    }
    let gap = ui.spacing().item_spacing.y;
    if page {
        ui.spacing_mut().item_spacing.y = t.space_10;
    }
    let result = super::section_count(ui, key, title, true, count, |ui| {
        if let Some(rect) = scenario::rect_of(&format!("dialog:section:{title}")) {
            scenario::publish_rect(&format!("prefs:section:{key}"), rect);
        }
        ui.spacing_mut().item_spacing.y = if page { t.space_14 } else { t.space_8 };
        let start = ui.next_widget_position();
        let result = add(ui);
        if page {
            scenario::publish_rect(&format!("prefs:section:{key}:body"), Rect::from_min_max(start, ui.min_rect().max));
        }
        result
    });
    ui.spacing_mut().item_spacing.y = gap;
    result
}

/// Full-width settings detail group. Container owns its surface; content cannot shrink the card.
pub(crate) fn settings_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let t = Tokens::get();
    let container = super::Container::new(ui.ctx(), super::Surface::Navigation);
    let frame = container
        .frame(ui.ctx(), t.dialog_footer, crate::frame::Side::Left)
        .inner_margin(egui::Margin::symmetric(t.space_12 as i8, t.space_10 as i8));
    container.show_frame(ui, frame, ui.max_rect(), |ui| {
        ui.set_min_width(dimension(ui.available_width()));
        add(ui)
    })
}

/// Controls and one explanatory specimen side by side, stacked on narrow viewports.
pub(crate) fn settings_pair<R>(ui: &mut Ui, controls: impl FnOnce(&mut Ui) -> R, preview: impl FnOnce(&mut Ui, R)) {
    if crate::prefs_search::suppress_chrome(ui) {
        let _ = controls(ui);
        return;
    }
    let t = Tokens::get();
    let preview_width = t.form_card_width - t.space_8;
    let gap = t.space_16 + t.space_4;
    let width = dimension(ui.available_width());
    if width < preview_width * 2.0 + gap {
        let state = controls(ui);
        preview(ui, state);
    } else {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let state = ui
                .allocate_ui_with_layout(vec2(width - preview_width - gap, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = t.space_6;
                    controls(ui)
                })
                .inner;
            ui.allocate_ui_with_layout(vec2(preview_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| preview(ui, state));
        });
    }
}

/// Dashed import tile, bounded by its carousel cell.
pub(crate) fn theme_import_tile(ui: &mut Ui, label: &str, note: &str) -> Response {
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[label, note]) {
        return crate::prefs_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let width = t.space_24 * 3.0 + t.space_16;
    let height = t.space_24 * 4.0;
    let (r, response) = ui.allocate_exact_size(vec2(width.min(dimension(ui.available_width())), height), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(r, t.radius_lg, t.hover);
    }
    let mut outline = rounded_perimeter(r.shrink(t.space_2 / 2.0), f32::from(t.radius_lg));
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    ui.painter().extend(egui::Shape::dashed_line(&outline, Stroke::new(t.space_3 / 2.0, t.control_border), t.space_3, t.space_3));
    let icon = Rect::from_center_size(pos2(r.center().x, r.top() + t.space_16), vec2(t.space_16, t.space_16));
    crate::icons::paint_ink(ui.painter(), icon, "import_tray", t.form_dim);
    centred_text(
        ui.painter(),
        Rect::from_min_size(pos2(r.left(), r.top() + t.space_24 + t.space_4), vec2(r.width(), t.space_16)),
        label,
        FontId::proportional(t.font_label),
        t.form_dim,
    );
    centred_text(
        ui.painter(),
        Rect::from_min_size(pos2(r.left(), r.top() + t.space_24 + t.space_4 + t.space_16), vec2(r.width(), t.space_16)),
        note,
        FontId::proportional(t.font_small),
        t.form_dim,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    scenario::publish_target(&format!("dialog:{label}"), r);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Compact informational state badge for settings; never an interactive control.
pub(crate) fn settings_badge(ui: &mut Ui, text: &str, warning: bool) {
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[text]) {
        return;
    }
    let t = Tokens::get();
    if super::settings_page_active(ui) {
        super::settings_state_badge(ui, text, if warning { super::SettingsBadgeTone::Warning } else { super::SettingsBadgeTone::Success });
        return;
    }
    let ink =
        if warning { if crate::theme::contrast_ratio(t.warning, t.dialog_body) >= 4.5 { t.warning } else { t.form_ink } } else { t.form_success };
    let fill = if warning { t.warning.gamma_multiply(0.1) } else { t.form_success_fill };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), crate::fonts::semibold(t.font_small), ink);
    let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + t.space_16, t.space_24), Sense::hover());
    ui.painter().rect_filled(r, t.radius_xl, fill);
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, ink);
}

#[derive(Clone, Copy)]
pub(crate) enum PreferenceFigure {
    Pivot(usize),
    UpAxis(bool),
}

/// Reusable illustrated radio card. Width comes from the figure, and wrapping is owned by
/// the parent; labels remain accessible even when translations need an ellipsis.
pub(crate) fn option_card(ui: &mut Ui, label: &str, note: Option<&str>, selected: bool, enabled: bool, figure: PreferenceFigure) -> Response {
    if crate::prefs_search::querying(ui) {
        let show = match note {
            Some(note) => crate::prefs_search::visible(ui, &[label, note]),
            None => crate::prefs_search::visible(ui, &[label]),
        };
        if !show {
            return crate::prefs_search::hidden_response(ui);
        }
    }
    let t = Tokens::get();
    let width = t.form_label_width + t.space_24 * 2.0 + t.space_6;
    let preview_height = t.space_24 * 3.0 + t.space_3;
    let label_height = t.space_16;
    let height = preview_height + t.space_6 * 2.0 + t.space_4 + label_height + if note.is_some() { t.space_16 } else { 0.0 };
    let enabled = enabled && ui.is_enabled();
    let (rect, response) =
        ui.allocate_exact_size(vec2(width.min(dimension(ui.available_width())), height), if enabled { Sense::click() } else { Sense::hover() });
    let hover = enabled && response.hovered();
    let ink = if !enabled {
        t.form_dim
    } else if selected {
        t.selected_ink
    } else {
        t.form_ink
    };
    ui.painter().rect(
        rect,
        t.radius_lg,
        if selected {
            t.selected_bg
        } else if hover {
            t.hover
        } else {
            t.control_fill
        },
        if selected { Stroke::new(2.0, t.form_primary) } else { t.control_border_stroke(t.control_border) },
        egui::StrokeKind::Inside,
    );
    let preview = Rect::from_min_size(rect.min + vec2(t.space_6, t.space_6), vec2((rect.width() - t.space_12).max(0.0), preview_height));
    paint_preference_figure(ui.painter(), preview, figure);
    let label_rect =
        Rect::from_min_size(pos2(rect.left() + t.space_4, preview.bottom() + t.space_4), vec2((rect.width() - t.space_8).max(0.0), label_height));
    centred_text_rows(
        ui.painter(),
        label_rect,
        label,
        if selected { crate::fonts::semibold(t.font_label) } else { FontId::proportional(t.font_label) },
        ink,
        1,
    );
    if let Some(note) = note {
        centred_text(
            ui.painter(),
            Rect::from_min_size(pos2(label_rect.left(), label_rect.bottom()), vec2(label_rect.width(), t.space_16)),
            note,
            FontId::proportional(t.font_small),
            t.form_dim,
        );
    }
    if selected {
        let centre = pos2(rect.right() - t.space_6 - t.space_8, rect.top() + t.space_6 + t.space_8);
        ui.painter().circle_filled(centre, t.space_8, t.form_primary);
        let ink = t.on_accent;
        ui.painter().line_segment([centre + vec2(-t.space_4, 0.0), centre + vec2(-t.space_2, t.space_2)], Stroke::new(2.0, ink));
        ui.painter().line_segment([centre + vec2(-t.space_2, t.space_2), centre + vec2(t.space_4, -t.space_3)], Stroke::new(2.0, ink));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, label));
    scenario::publish_target(&format!("prefs:option:{label}"), rect);
    let response = response.on_hover_text(label);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

pub(super) fn centred_text(p: &egui::Painter, rect: Rect, label: &str, font: FontId, ink: Color32) {
    centred_text_rows(p, rect, label, font, ink, 1);
}

fn centred_text_rows(p: &egui::Painter, rect: Rect, label: &str, font: FontId, ink: Color32, rows: usize) {
    let mut job = egui::text::LayoutJob::simple(label.to_owned(), font, ink, rect.width());
    job.wrap.max_rows = rows;
    job.halign = egui::Align::Center;
    let galley = p.layout_job(job);
    p.with_clip_rect(rect).galley(rect.center() - galley.rect.center().to_vec2(), galley, ink);
}

fn paint_preference_figure(p: &egui::Painter, rect: Rect, figure: PreferenceFigure) {
    if !p.clip_rect().intersects(rect) {
        return;
    }
    let t = Tokens::get();
    let p = p.with_clip_rect(rect);
    p.rect_filled(rect, t.control_radius, t.viewport_top);
    t.container_rect(&p, rect, t.control_radius, egui::Color32::TRANSPARENT, t.dialog_rule);
    match figure {
        PreferenceFigure::UpAxis(y_up) => super::preferences_figures::paint_orientation(&p, rect, y_up),
        PreferenceFigure::Pivot(pivot) => super::preferences_figures::paint_orbit(&p, rect, pivot),
    }
}

/// Clockwise rounded boundary shared by dashed outlines and clipped palette specimens.
pub(super) fn rounded_perimeter(r: Rect, radius: f32) -> Vec<Pos2> {
    let radius = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    let corners = [
        (pos2(r.right() - radius, r.top() + radius), -std::f32::consts::FRAC_PI_2),
        (pos2(r.right() - radius, r.bottom() - radius), 0.0),
        (pos2(r.left() + radius, r.bottom() - radius), std::f32::consts::FRAC_PI_2),
        (pos2(r.left() + radius, r.top() + radius), std::f32::consts::PI),
    ];
    corners
        .into_iter()
        .flat_map(|(centre, start)| {
            (0..=8).map(move |step| {
                let angle = start + std::f32::consts::FRAC_PI_2 * step as f32 / 8.0;
                centre + vec2(angle.cos(), angle.sin()) * radius
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct LayoutPreview {
    pub compact: bool,
    pub labels: bool,
    pub history: usize,
    pub translucent: bool,
    pub dialog_left: bool,
}

/// One live shell specimen follows workspace, toolbar, panel and dialog placement choices.
pub(crate) fn layout_preview(ui: &mut Ui, state: LayoutPreview) {
    super::preferences_figures::paint_layout(ui, state);
}

pub(crate) fn scale_specimen(ui: &mut Ui, scale: f32) {
    let t = Tokens::get();
    let scale = if scale.is_finite() { scale.clamp(0.5, 2.5) / ui.ctx().zoom_factor().max(0.5) } else { 1.0 };
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24 * 4.0 + t.space_16), Sense::hover());
    t.container_rect(ui.painter(), r, t.radius_lg, t.control_fill, t.dialog_rule);
    let p = ui.painter().with_clip_rect(r.shrink(t.space_8));
    let font = FontId::proportional(t.font_label * scale);
    p.text(
        r.right_top() + vec2(-t.space_8, t.space_8),
        egui::Align2::RIGHT_TOP,
        crate::i18n::tr("prefs.live_preview").to_uppercase(),
        FontId::proportional(t.font_micro),
        t.form_dim,
    );
    let origin = r.min + vec2(t.space_10, t.space_24);
    let line_h = t.control_height * scale;
    let distance = crate::i18n::tr_label("Distance");
    let label = p.layout_no_wrap(distance.to_owned(), font.clone(), t.form_dim);
    p.galley(origin + vec2(0.0, (line_h - label.size().y) / 2.0), label.clone(), t.form_dim);
    let field = Rect::from_min_size(origin + vec2(label.size().x + t.space_8 * scale, 0.0), vec2((t.form_label_width + t.space_6) * scale, line_h));
    p.rect(field, t.control_radius, t.input_fill(), t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(&p, field, t.control_radius, true, true);
    }
    centred_text(&p, field, "12.50", FontId::monospace(t.font_body * scale), t.form_ink);
    let button = Rect::from_min_size(field.right_top() + vec2(t.space_8 * scale, 0.0), vec2(t.space_24 * 1.5 * scale, line_h));
    p.rect_filled(button, t.control_radius, if t.borders.bevel.is_some() { t.control_fill } else { t.form_primary });
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(&p, button, t.control_radius, false, true);
    }
    centred_text(
        &p,
        button,
        crate::i18n::tr_label("OK"),
        crate::fonts::semibold(t.font_label * scale),
        if t.borders.bevel.is_some() { t.form_ink } else { t.on_accent },
    );
    let feature = Rect::from_min_size(origin + vec2(0.0, line_h + t.space_8 * scale), vec2(r.width() - t.space_10 * 2.0, line_h));
    p.rect_filled(feature, t.control_radius, t.selected_bg);
    let icon = Rect::from_center_size(pos2(feature.left() + t.space_16 * scale, feature.center().y), vec2(t.space_16, t.space_16) * scale);
    crate::icons::paint_icon(&p, icon, "extrude", 1.0);
    p.text(
        pos2(icon.right() + t.space_6 * scale, feature.center().y),
        egui::Align2::LEFT_CENTER,
        "Extrude1",
        crate::fonts::semibold(t.font_label * scale),
        t.selected_ink,
    );
    p.text(feature.right_center() - vec2(t.space_8, 0.0), egui::Align2::RIGHT_CENTER, "12.5 mm", font, t.selected_ink);
}

/// Font picker and auxiliary actions share one line, with the specimen below.
pub(crate) fn settings_font_row(ui: &mut Ui, label: &str, mut add: impl FnMut(&mut Ui, usize)) {
    let t = Tokens::get();
    if dimension(ui.available_width()) < t.form_label_width + t.space_24 * 14.0 {
        super::settings_form(ui, t.form_label_width, |ui| {
            let shown = super::row(ui, label, |ui| {
                add(ui, 0);
                ui.is_visible()
            });
            if shown {
                super::below_row_control(ui, label, ui.available_width(), |ui| {
                    ui.horizontal_wrapped(|ui| add(ui, 1));
                });
            }
        });
        return;
    }
    super::settings_form(ui, t.form_label_width, |ui| {
        super::row(ui, label, |ui| {
            let actions = t.space_24 * 8.0 + t.space_8;
            let picker = (dimension(ui.available_width()) - actions - t.space_8).max(t.space_24);
            ui.allocate_ui_with_layout(vec2(picker, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| add(ui, 0));
            ui.add_space(t.space_4);
            add(ui, 1);
        });
    });
}

/// Natural-width dim text, for inline suffixes and metadata without a full-width note row.
pub(crate) fn settings_inline_note(ui: &mut Ui, text: &str) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let galley = if super::settings_page_active(ui) && ui.layout().main_wrap() {
        ui.painter().layout(text.to_owned(), FontId::proportional(t.font_body), t.form_dim, dimension(ui.max_rect().width()))
    } else {
        ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(t.font_body), t.form_dim)
    };
    let (r, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    ui.painter().galley(r.min, galley, t.form_dim);
}

/// Framed font specimen, inheriting the chosen application font and its fallback glyphs.
pub(crate) fn font_specimen(ui: &mut Ui, text: &str) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.control_height + t.space_10), Sense::hover());
    t.container_rect(ui.painter(), r, t.control_radius, t.control_fill, t.dialog_rule);
    paint_text(ui, r.shrink2(vec2(t.space_12, 0.0)), text, FontId::proportional(t.font_button), t.form_ink, false);
}

/// Intrinsically sized segments, retaining the usual row label for accessibility and automation.
pub(crate) fn settings_segments(ui: &mut Ui, label: &str, selected: &mut usize, options: &[&str]) -> bool {
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[label]) && !crate::prefs_search::texts_hit(ui, options) {
        return false;
    }
    let t = Tokens::get();
    let widest = options.iter().map(|s| super::ContentWidth::name(ui, s)).fold(0.0, f32::max);
    let width = ((widest + t.space_16) * options.len() as f32).min(dimension(ui.available_width()));
    ui.allocate_ui_with_layout(vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.scope_builder(
            egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag_value(super::layout::LABEL, label.to_owned())),
            |ui| super::segmented(ui, selected, options, super::Segments { compact: true, ..Default::default() }),
        )
        .inner
    })
    .inner
}

pub(crate) fn status_palette_sample(ui: &mut Ui, mode: ColorVision) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let colours = Tokens::themed(crate::theme::is_dark(), mode);
    ui.horizontal_wrapped(|ui| {
        for (label, icon, fill) in [
            ("prefs.status_selected", "finish", colours.form_primary),
            ("prefs.status_warning", "warning", colours.warning),
            ("prefs.status_error", "close", colours.error),
            ("prefs.status_done", "finish", colours.form_success),
        ] {
            let ink = if crate::theme::contrast_ratio(t.on_accent, fill) >= 4.5 { t.on_accent } else { t.tab_on_fill_dark };
            let label = crate::i18n::tr(label);
            let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::fonts::semibold(t.font_small), ink);
            let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + t.space_24 + t.space_16, t.space_24), Sense::hover());
            scenario::publish_rect(&format!("prefs:status_sample:{label}"), r);
            ui.painter().rect_filled(r, t.radius_xl, fill);
            crate::icons::paint_ink(
                ui.painter(),
                Rect::from_center_size(pos2(r.left() + t.space_12, r.center().y), vec2(t.space_12, t.space_12)),
                icon,
                ink,
            );
            ui.painter().galley(pos2(r.left() + t.space_24, r.center().y - galley.size().y / 2.0), galley, ink);
        }
    });
}

/// The names under the Colour vision sample (message ids).
pub(crate) const COLOUR_VISION_SAMPLE: [&str; 6] =
    ["ui.colour_vision_fixed", "ui.colour_vision_projected", "ui.axes", "Bends", "ui.draft", "ui.colour_vision_failed"];

/// The Colour vision sample (#778): fixed and projected sketch lines, the X and Y axes, bends up
/// and down, Draft Analysis and a failed feature, each drawn as it is where it appears, in the
/// colours of `t` (the preference passes the mode being chosen, #7152). The six cells share one
/// responsive, clipped area.
pub(crate) fn colour_vision_sample(ui: &mut egui::Ui, t: &Tokens) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let count = COLOUR_VISION_SAMPLE.len() as f32;
    let gap = t.space_8.min(dimension(ui.available_width()) / (count - 1.0));
    let width = ((dimension(ui.available_width()) - gap * (count - 1.0)) / count).min(t.space_24 * 3.0 + t.space_12);
    let figure_height = t.space_24 + t.space_16 + t.space_4;
    let (area, _) = ui.allocate_exact_size(vec2(width * count + gap * (count - 1.0), figure_height + t.space_3 + t.space_16), Sense::hover());
    scenario::publish_rect("prefs:colour_vision_samples", area);
    let p = ui.painter_at(area);
    for (i, name) in COLOUR_VISION_SAMPLE.iter().enumerate() {
        let cell = egui::Rect::from_min_size(area.min + vec2((width + gap) * i as f32, 0.0), vec2(width, figure_height));
        scenario::publish_rect(&format!("prefs:colour_vision_sample:{i}"), cell);
        super::preferences_figure_ops::paint_colour_vision(&p, cell, i, t);
        centred_text(
            &p,
            Rect::from_min_size(cell.left_bottom() + vec2(0.0, t.space_3), vec2(cell.width(), t.space_16)),
            crate::i18n::tr(name),
            FontId::proportional(t.font_small),
            t.text_dim,
        );
    }
}
