//! Actions elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use super::metrics::*;
use crate::{fonts::RichTextExt as _, scenario, theme::Tokens};
use egui::{Response, RichText, Ui, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Action {
    #[default]
    None,
    Ok,
    Cancel,
    Apply,
}

/// Reorder actions are directional navigation, not disclosures.
pub(crate) const MOVE_UP: &str = "▲";
pub(crate) const MOVE_DOWN: &str = "▼";

/// Every enabled button shows the pointing hand on hover, as the application's other clickable
/// chrome does. A disabled button stays inert and keeps the arrow.
fn pointer_on_hover(ui: &Ui, response: &Response) {
    if response.enabled() && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// Icon action inside an existing rectangle. Tokens: `form_icon_inset, text, text_dim; icon palette`.
pub(crate) fn icon_button_at(ui: &mut Ui, rect: egui::Rect, icon: &str, label: &str) -> Response {
    icon_button_colored(ui, rect, icon, label, None)
}

/// Quiet secondary action: a small, muted glyph with the normal full-size hit target.
pub(crate) fn quiet_icon_button_at(ui: &mut Ui, rect: egui::Rect, icon: &str, label: &str) -> Response {
    icon_button_presentation(ui, rect, icon, label, None, true)
}

pub(super) fn icon_button_colored(ui: &mut Ui, rect: egui::Rect, icon: &str, label: &str, ink: Option<egui::Color32>) -> Response {
    icon_button_presentation(ui, rect, icon, label, ink, false)
}

fn icon_button_presentation(ui: &mut Ui, rect: egui::Rect, icon: &str, label: &str, ink: Option<egui::Color32>, quiet: bool) -> Response {
    let t = Tokens::get();
    let response = ui.interact(rect, ui.make_persistent_id(("icon_button", icon, label)), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    crate::icons::paint_ink(
        ui.painter(),
        if quiet { egui::Rect::from_center_size(rect.center(), vec2(t.space_12, t.space_12)) } else { rect.shrink(t.form_icon_inset) },
        icon,
        ink.unwrap_or(if response.hovered() || response.has_focus() {
            t.form_ink
        } else if quiet {
            t.form_dim.lerp_to_gamma(t.control_fill, 0.35)
        } else {
            t.form_dim
        }),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), crate::i18n::tr_label(label)));
    pointer_on_hover(ui, &response);
    response.on_hover_text(crate::i18n::tr_label(label))
}

/// Esc cancels; unowned Enter submits only when valid. Tokens: `none`.
pub(crate) fn keyboard(ctx: &egui::Context, ok_enabled: bool) -> Action {
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        Action::Cancel
    } else if ok_enabled && ctx.memory(|m| m.focused().is_none()) && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        Action::Ok
    } else {
        Action::None
    }
}

/// Pinned actions with optional Apply, compute status and keyboard selection mark.
#[derive(Default)]
pub(crate) struct Footer<'a> {
    pub apply: Option<&'a str>,
    pub status: Option<&'a str>,
    pub selected: Option<Action>,
    pub danger: bool,
    pub confirm: bool,
    /// Parent surface corners; the footer inherits only its two bottom corners.
    pub corners: Option<egui::CornerRadius>,
    pub secondary_left: bool,
    /// The band's fill when not the usual footer colour (a translucent card's).
    pub fill: Option<egui::Color32>,
}

pub(crate) fn footer(ui: &mut Ui, ok_label: &str, cancel_label: &str, ok_enabled: bool, options: Footer<'_>) -> Action {
    footer_with_left(ui, ok_label, cancel_label, ok_enabled, options, |_| {})
}

pub(super) fn footer_with_left(
    ui: &mut Ui,
    ok_label: &str,
    cancel_label: &str,
    ok_enabled: bool,
    options: Footer<'_>,
    left: impl FnOnce(&mut Ui),
) -> Action {
    let Footer { apply: apply_label, status, selected, danger, confirm, corners, secondary_left, fill } = options;
    let t = Tokens::get();
    let ok_enabled = ok_enabled && !ok_label.is_empty();
    // The footer is a full-width band (the card has no margin of its own): its own fill, a rule
    // on top, and the buttons right-aligned with 10 pt above/below and 12 pt at the sides.
    let (band, _) = ui.allocate_exact_size(
        vec2(dimension(ui.available_width()), t.form_footer_height + if confirm { t.space_4 } else { 0.0 }),
        egui::Sense::hover(),
    );
    let corners = corners.unwrap_or(egui::CornerRadius::same(t.radius_xl));
    ui.painter().rect_filled(
        band,
        egui::CornerRadius { nw: 0, ne: 0, sw: corners.sw.saturating_sub(1), se: corners.se.saturating_sub(1) },
        fill.unwrap_or(t.dialog_footer),
    );
    t.secondary_hline(ui.painter(), band.x_range(), band.top(), t.dialog_rule);
    scenario::publish_rect("dialog:footer", band);
    scenario::publish_count("dialog:ok:enabled", f64::from(ok_enabled && ui.is_enabled()));
    let inner = band.shrink2(vec2(t.space_12, t.space_10));
    scenario::publish_rect("dialog:footer:actions", inner);
    let mut action = if ui.is_enabled() { keyboard(ui.ctx(), ok_enabled) } else { Action::None };
    let primary_fill = if t.borders.bevel.is_some() && !danger {
        t.control_fill
    } else if danger {
        t.form_error
    } else {
        t.form_primary
    };
    let primary_ink = if t.borders.bevel.is_some() && !danger {
        t.form_ink
    } else if danger && crate::theme::contrast_ratio(t.on_accent, primary_fill) < 4.5 {
        t.tab_on_fill_dark
    } else {
        t.on_accent
    };
    let mut actions_left = inner.right();
    let row = ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_6;
            for (label, kind) in [(ok_label, Action::Ok), (apply_label.unwrap_or(""), Action::Apply), (cancel_label, Action::Cancel)] {
                if (kind == Action::Apply && (apply_label.is_none() || confirm)) || (kind == Action::Ok && ok_label.is_empty()) || label.is_empty() {
                    continue;
                }
                let marked = selected == Some(kind);
                let primary = kind == Action::Ok && selected.is_none_or(|s| s == Action::Ok);
                let live = kind == Action::Cancel || ok_enabled;
                ui.spacing_mut().button_padding = vec2(if primary { t.space_16 } else { t.space_12 }, 0.0);
                let text = RichText::new(crate::i18n::tr_label(label)).size(t.font_label).color(if primary && ok_enabled {
                    primary_ink
                } else if primary {
                    t.form_dim
                } else {
                    t.form_ink
                });
                let response = ui.add_enabled(
                    live,
                    egui::Button::new(if primary && ok_enabled { text.bold_face() } else { text })
                        .selected(marked && !primary)
                        .fill(if primary && ok_enabled {
                            primary_fill
                        } else if marked {
                            t.selected_bg
                        } else {
                            t.control_fill
                        })
                        .stroke(if primary && ok_enabled {
                            egui::Stroke::NONE
                        } else if marked && !primary {
                            egui::Stroke::new(1.0, t.form_primary)
                        } else {
                            t.control_border_stroke(t.control_border)
                        })
                        .wrap_mode(egui::TextWrapMode::Extend)
                        .corner_radius(t.control_radius)
                        .min_size(vec2(if primary { t.space_24 * 3.0 } else { 0.0 }, t.form_button_height)),
                );
                t.button_bevel(ui, &response, false);
                pointer_on_hover(ui, &response);
                actions_left = actions_left.min(response.rect.left());
                let key = match kind {
                    Action::Ok => "dialog:ok",
                    Action::Apply => "dialog:apply",
                    _ => "dialog:cancel",
                };
                scenario::publish_target(key, response.rect);
                scenario::publish_handle(&format!("dialog:{label}"), response.rect.center());
                if response.clicked() {
                    action = kind;
                }
            }
        });
    });
    let left_rect = egui::Rect::from_min_max(inner.min, egui::pos2((actions_left - t.space_8).max(inner.left()), inner.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(left_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        if let Some(label) = apply_label.filter(|_| confirm) {
            let marked = selected == Some(Action::Apply);
            let mut button =
                egui::Button::new(crate::i18n::tr_label(label)).frame(secondary_left).selected(marked).min_size(vec2(0.0, t.form_button_height));
            if secondary_left {
                ui.spacing_mut().button_padding = vec2(t.space_12, 0.0);
                button = button
                    .fill(if marked { t.selected_bg } else { t.control_fill })
                    .stroke(egui::Stroke::new(1.0, if marked { t.form_primary } else { t.control_border }))
                    .corner_radius(t.control_radius);
            }
            let response = ui.add(button);
            pointer_on_hover(ui, &response);
            scenario::publish_target("dialog:apply", response.rect);
            scenario::publish_handle(&format!("dialog:{label}"), response.rect.center());
            if response.clicked() {
                action = Action::Apply;
            }
        }
        left(ui);
    });
    if let Some(status) = status {
        let left = (row.response.rect.left() + t.space_16 + t.space_4).min(actions_left);
        let right = (actions_left - t.space_8).max(left);
        let rect = egui::Rect::from_min_max(egui::pos2(left, inner.top()), egui::pos2(right, inner.bottom()));
        // Added after the actions, so status changes cannot change the button IDs.
        ui.put(rect, egui::Label::new(RichText::new(status).medium().color(t.form_dim)).truncate());
    }
    action
}

/// One equal-width secondary action, with optional existing icon master.
pub(crate) struct PairButton<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub icon: Option<&'a str>,
}

/// Equal-width secondary actions with optional icons and selected state. Tokens: `form_button_height, control_*, selected_bg, hover, font_label, text, icon/fill, accent, sketch_footer_icon_size, space_4/6/10`.
pub(crate) fn button_pair(ui: &mut egui::Ui, key: &str, states: [bool; 2], buttons: [PairButton<'_>; 2]) -> (bool, bool) {
    let t = Tokens::get();
    t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top(), t.dialog_rule);
    let mut clicked = [false; 2];
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(t.space_10 as i8, t.space_6 as i8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_6;
            let width = (ui.available_width() - t.space_6) / 2.0;
            for (index, button) in buttons.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(width, t.form_button_height), egui::Sense::click());
                let on = states.get(index).copied().unwrap_or(false);
                ui.painter().rect(
                    rect,
                    t.control_radius,
                    if on {
                        t.selected_bg
                    } else if response.hovered() || response.has_focus() {
                        t.hover
                    } else {
                        t.control_fill
                    },
                    t.control_border_stroke(t.control_border),
                    egui::StrokeKind::Inside,
                );
                let label = crate::i18n::tr_label(button.label);
                let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(t.font_label), t.form_ink);
                let icon_width = if button.icon.is_some() { t.sketch_footer_icon_size + t.space_4 } else { 0.0 };
                let left = rect.center().x - (galley.size().x + icon_width) / 2.0;
                let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
                if let Some(icon) = button.icon {
                    crate::icons::paint_icon(
                        &painter,
                        egui::Rect::from_center_size(
                            egui::pos2(left + t.sketch_footer_icon_size / 2.0, rect.center().y),
                            vec2(t.sketch_footer_icon_size, t.sketch_footer_icon_size),
                        ),
                        icon,
                        f32::from(t.icon.a()) / 255.0,
                    );
                }
                painter.galley(egui::pos2(left + icon_width, rect.center().y - galley.size().y / 2.0), galley, t.form_ink);
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, label));
                t.button_bevel(ui, &response, on);
                pointer_on_hover(ui, &response);
                crate::scenario::publish_target(&format!("{key}:{}", button.key), rect);
                if let Some(value) = clicked.get_mut(index) {
                    *value = response.on_hover_text(label).clicked();
                }
            }
        });
    });
    (clicked.first().copied().unwrap_or(false), clicked.get(1).copied().unwrap_or(false))
}

/// Shared button variants preserve native sizing and enabled-button translation.
#[derive(Clone, Copy)]
pub enum Button {
    Normal,
    Primary,
    Small,
    Enabled(bool),
    Ghost,
    SmallSelected,
}

fn button_widget(label: egui::WidgetText, options: Button) -> egui::Button<'static> {
    let t = Tokens::get();
    if t.borders.bevel.is_some() && !matches!(options, Button::Ghost) {
        let button = egui::Button::new(label.color(t.form_ink)).fill(t.control_fill).stroke(t.control_border_stroke(t.control_border));
        return if matches!(options, Button::Small | Button::SmallSelected) { button.small() } else { button };
    }
    match options {
        Button::Small => egui::Button::new(label).small(),
        Button::Primary => {
            let ink = if crate::theme::contrast_ratio(t.on_accent, t.form_primary) >= 4.5 { t.on_accent } else { t.tab_on_fill_dark };
            egui::Button::new(label.color(ink)).fill(t.form_primary)
        }
        Button::Ghost => egui::Button::new(label).frame(false),
        Button::SmallSelected => {
            egui::Button::new(label.color(t.selected_ink)).small().fill(t.selected_bg).stroke(egui::Stroke::new(1.0, t.form_primary))
        }
        _ => egui::Button::new(label),
    }
}

pub fn button(ui: &mut Ui, label: impl Into<egui::WidgetText>, options: Button) -> Response {
    let label = label.into();
    let key = label.text().to_owned();
    let response = match options {
        Button::Enabled(enabled) => ui.add_enabled_ui(enabled, |ui| ui.add(button_widget(crate::i18n::tr_label(&key).into(), options))).inner,
        _ => ui.add(button_widget(label, options)),
    };
    if !matches!(options, Button::Ghost) {
        Tokens::get().button_bevel(ui, &response, matches!(options, Button::SmallSelected));
    }
    pointer_on_hover(ui, &response);
    let key = if matches!(options, Button::Enabled(_)) { crate::i18n::tr_label(&key) } else { &key };
    publish_button(ui, &response, key);
    response
}

/// The same button at an existing rectangle, with stable identity and centred native text.
/// Tokens: shared button colours; caller supplies its token-sized rectangle and optional typography.
pub(crate) fn button_at(ui: &mut Ui, id: &str, rect: egui::Rect, label: impl Into<egui::WidgetText>, options: Button) -> Response {
    let label = label.into();
    let key = label.text().to_owned();
    let response = ui
        .scope_builder(
            egui::UiBuilder::new().id_salt(id).max_rect(rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
            |ui| {
                super::scope(ui, |ui| {
                    let draw = |ui: &mut Ui, label| ui.add_sized(rect.size(), button_widget(label, options));
                    match options {
                        Button::Enabled(enabled) => ui.add_enabled_ui(enabled, |ui| draw(ui, crate::i18n::tr_label(&key).into())).inner,
                        _ => draw(ui, label),
                    }
                })
            },
        )
        .inner;
    if !matches!(options, Button::Ghost) {
        Tokens::get().button_bevel(ui, &response, matches!(options, Button::SmallSelected));
    }
    pointer_on_hover(ui, &response);
    publish_button(ui, &response, &key);
    response
}

/// Accent action outside the footer, using the same shared button component.
pub(crate) fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let response = button(ui, crate::i18n::tr_label(label), Button::Primary);
    scenario::publish_target(&format!("dialog:button:{label}"), response.rect);
    response
}

/// Accent action across its container inside `button_pair`'s inset, label centred. Tokens:
/// `form_primary, on_accent, form_button_height, font_label, control_radius, space_6/10`.
pub(crate) fn wide_primary_button(ui: &mut Ui, label: &str) -> Response {
    let t = Tokens::get();
    let bevel = t.borders.bevel.is_some();
    let text = RichText::new(crate::i18n::tr_label(label)).size(t.font_label).color(if bevel { t.form_ink } else { t.on_accent });
    let response = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(t.space_10 as i8, t.space_6 as i8))
        .show(ui, |ui| {
            let button = egui::Button::new(crate::fonts::RichTextExt::bold_face(text))
                .fill(if bevel { t.control_fill } else { t.form_primary })
                .stroke(egui::Stroke::NONE)
                .corner_radius(t.control_radius);
            ui.add_sized(vec2(dimension(ui.available_width()), t.form_button_height), button)
        })
        .inner;
    t.button_bevel(ui, &response, false);
    pointer_on_hover(ui, &response);
    response
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;

// Additional identities must never replace an existing field handle in compound rows.
fn publish_button(ui: &Ui, response: &Response, label: &str) {
    let key = format!("dialog:button:{}:{label}", super::layout::label(ui));
    scenario::publish_target(&key, response.rect);
}

/// Nested action menu in a dense table. Tokens: scope control/font tokens.
pub(crate) fn action_menu<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
    let response = super::Menu::button(ui, crate::i18n::tr_label(label), |ui| super::scope(ui, add));
    scenario::publish_target(&format!("dialog:menu:{label}"), response.response.rect);
    response
}

/// Borderless compact table actions. Existing star glyphs keep their identities; arrows
/// use the kit chevron rather than a font-dependent triangle. Tokens: form_close_size,
/// space_4, control_radius, form_primary/dim/ink, selected_bg, hover, font_label.
pub(crate) fn compact_button(ui: &mut Ui, glyph: &str, tooltip: &str, enabled: bool, selected: bool) -> Response {
    let t = Tokens::get();
    let arrow = matches!(glyph, MOVE_UP | MOVE_DOWN);
    let width = if arrow { t.form_close_size - t.space_4 } else { t.form_close_size };
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(width, t.form_close_size), egui::Sense::click());
            if response.hovered() || selected {
                ui.painter().rect_filled(rect, t.control_radius, if selected { t.selected_bg } else { t.hover });
            }
            let ink = if !enabled {
                t.form_dim.gamma_multiply(0.35)
            } else if selected {
                t.form_primary
            } else {
                t.form_dim
            };
            if arrow {
                let icon = rect.shrink(t.form_icon_inset);
                if glyph == MOVE_UP {
                    crate::icons::paint_ink_flipped(ui.painter(), icon, "chevron", ink);
                } else {
                    crate::icons::paint_ink(ui.painter(), icon, "chevron", ink);
                }
            } else {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(t.font_label), ink);
            }
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), glyph));
            response
        })
        .inner;
    publish_button(ui, &response, glyph);
    pointer_on_hover(ui, &response);
    response.on_hover_text(crate::i18n::tr_label(tooltip))
}

/// Secondary action with an existing icon, optionally using the danger ink. Keeps button handles.
pub(crate) fn icon_label_button(ui: &mut Ui, label: &str, icon: &str, enabled: bool, danger: bool) -> Response {
    let t = Tokens::get();
    let label = crate::i18n::tr_label(label);
    let text_width = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(t.font_label), t.form_ink).size().x;
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(vec2(text_width + t.space_10 * 2.0 + t.space_14 + t.space_6, t.control_height), egui::Sense::click());
            let ink = if danger { t.form_error } else { t.form_ink };
            ui.painter().rect(
                rect,
                t.control_radius,
                if response.hovered() { t.hover } else { t.control_fill },
                t.control_border_stroke(t.control_border),
                egui::StrokeKind::Inside,
            );
            let icon_rect =
                egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_10 + t.space_14 / 2.0, rect.center().y), vec2(t.space_14, t.space_14));
            crate::icons::paint_ink(ui.painter(), icon_rect, icon, ink);
            super::text::paint_text(
                ui,
                egui::Rect::from_min_max(egui::pos2(icon_rect.right() + t.space_6, rect.top()), rect.max),
                label,
                egui::FontId::proportional(t.font_label),
                ink,
                false,
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
            response
        })
        .inner;
    pointer_on_hover(ui, &response);
    let key = format!("dialog:button:{}:{label}", super::layout::label(ui));
    scenario::publish_handle(&key, response.rect.center());
    scenario::publish_rect(&key, response.rect);
    response
}

/// Ribbon overflow action on the shared menu row, retaining shortcut, accessibility and enabled state.
/// Tokens: MenuItem's typography, spacing and semantic icon palette.
pub(crate) fn command_menu_button(ui: &mut Ui, id: &str, label: &str, icon: &str, shortcut: &str, enabled: bool) -> Response {
    let mut item = super::MenuItem::new(id, label, icon);
    item.shortcut = shortcut;
    item.enabled = enabled;
    item.show(ui)
}
