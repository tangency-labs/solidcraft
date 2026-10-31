//! Assistant presentation primitives; protocol state and action dispatch stay with callers.
use crate::theme::Tokens;
use egui::{InnerResponse, Response, Ui, vec2};

#[derive(Clone, Copy)]
pub(crate) enum AssistantTone {
    Neutral,
    Working,
    Success,
    Warning,
    Error,
}

impl AssistantTone {
    fn colors(self, t: &Tokens) -> (egui::Color32, egui::Color32) {
        match self {
            Self::Neutral => (t.form_dim, t.dialog_footer),
            Self::Working => (t.selected_ink, t.accent_soft),
            Self::Success => (t.form_success, t.form_success_fill),
            Self::Warning => (t.warning, t.warning.gamma_multiply(0.12)),
            Self::Error => (t.form_error, t.form_error_fill),
        }
    }
}

/// A compact state pill. Tokens: font_small, space_6/12, radius_xl and semantic state colours.
pub(crate) fn assistant_badge(ui: &mut Ui, label: &str, tone: AssistantTone) -> Response {
    let t = Tokens::get();
    let (ink, fill) = tone.colors(&t);
    badge(ui, label, ink, fill)
}

/// A compact informational chip in the active workspace colours.
pub(crate) fn workspace_chip(ui: &mut Ui, label: &str) -> Response {
    let accent = crate::theme::TabAccent::current(ui.ctx());
    badge(ui, label, accent.ink, accent.soft)
}

fn badge(ui: &mut Ui, label: &str, ink: egui::Color32, fill: egui::Color32) -> Response {
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_caption);
    let width = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), ink).size().x + t.space_12;
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.form_close_size - t.space_4), egui::Sense::hover());
    ui.painter().rect_filled(rect, t.radius_xl, fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, ink);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    response
}

/// The shared 40 pt header band. The caller owns its title, state and icon actions.
pub(crate) fn assistant_header<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    let output = egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: t.space_12 as i8,
            right: t.space_8 as i8,
            top: (t.space_4 + t.space_3) as i8,
            bottom: (t.space_4 + t.space_3) as i8,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(0.0));
            ui.set_min_height(t.form_close_size);
            add(ui)
        });
    t.secondary_hline(ui.painter(), output.response.rect.x_range(), output.response.rect.bottom(), t.dialog_rule);
    output
}

/// Inset controls band beneath the header, aligned with the conversation body.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_controls<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(t.space_12 as i8, t.space_8 as i8)).show(ui, |ui| {
        ui.set_min_width(ui.available_width().max(0.0));
        add(ui)
    })
}

/// Right-aligned user message with a compact lower-right corner and 86% maximum width.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_bubble<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    let width = ui.available_width().max(0.0);
    let bubble_width = width * 0.86;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(width - bubble_width);
        egui::Frame::NONE
            .fill(t.accent_soft)
            .corner_radius(egui::CornerRadius { nw: t.radius_xl, ne: t.radius_xl, sw: t.radius_xl, se: t.radius_sm.saturating_sub(1) })
            .inner_margin(egui::Margin::symmetric((t.space_8 + t.space_3) as i8, t.space_8 as i8))
            .show(ui, |ui| {
                let content_width = (bubble_width - (t.space_8 + t.space_3) * 2.0).max(0.0);
                ui.set_width(content_width);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(content_width);
                    ui.spacing_mut().item_spacing.y = t.space_6;
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.visuals_mut().override_text_color = Some(t.form_ink);
                    add(ui)
                })
                .inner
            })
    })
    .inner
}

/// Assistant prose remains unboxed and uses the shared readable body typography.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_prose<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    ui.scope(|ui| {
        ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(t.font_label));
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.visuals_mut().override_text_color = Some(t.form_ink);
        add(ui)
    })
    .inner
}

#[cfg(any(not(target_arch = "wasm32"), test))]
#[derive(Clone, Copy)]
pub(crate) enum AssistantToolState {
    Pending,
    Running,
    Done,
    Failed,
}

/// A 15 pt status mark with distinct shape and colour for every tool state.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_tool_status(ui: &mut Ui, state: AssistantToolState) -> Response {
    let t = Tokens::get();
    let size = t.space_12 + t.space_3;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let radius = (size - t.space_4) / 2.0;
    match state {
        AssistantToolState::Pending => {
            ui.painter().circle_stroke(rect.center(), radius, egui::Stroke::new(1.0, t.form_dim));
        }
        AssistantToolState::Running => {
            let phase = ui.input(|i| i.time) as f32 * t.space_3;
            let points = (0..=24)
                .map(|step| {
                    let angle = phase + step as f32 / 24.0 * std::f32::consts::TAU * 0.75;
                    rect.center() + vec2(angle.cos(), angle.sin()) * radius
                })
                .collect();
            ui.painter().add(egui::Shape::line(points, egui::Stroke::new(t.space_2, t.form_primary)));
            ui.ctx().request_repaint();
        }
        AssistantToolState::Done => {
            ui.painter().circle_filled(rect.center(), size / 2.0, t.form_success_fill);
            super::checks::paint_check_glyph(ui.painter(), rect, false, t.form_success);
        }
        AssistantToolState::Failed => {
            ui.painter().circle_filled(rect.center(), size / 2.0, t.form_error_fill);
            crate::icons::paint_ink(ui.painter(), rect, "close", t.form_error);
        }
    }
    response
}

/// Bordered tool group; the caller retains expansion, status and automation identities.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_tool_frame<R>(ui: &mut Ui, failed: bool, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    egui::Frame::NONE
        .fill(if failed { t.form_error_fill } else { t.dialog_body })
        .stroke(t.container_border(t.dialog_rule))
        .corner_radius(t.radius_lg)
        .inner_margin(t.space_3)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(0.0));
            add(ui)
        })
}

/// Individual tool-row surface, retaining a failed row's tint inside a shared group.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_tool_row<R>(ui: &mut Ui, failed: bool, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    egui::Frame::NONE.fill(if failed { t.form_error_fill } else { t.dialog_body }).corner_radius(t.control_radius).show(ui, add)
}

/// Indented detail box using the same field surface as inline command parameters.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_tool_detail<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    egui::Frame::NONE
        .fill(t.control_fill)
        .stroke(t.container_border(t.dialog_rule))
        .corner_radius(t.control_radius)
        .inner_margin(egui::Margin::symmetric((t.space_6 + t.space_3) as i8, t.space_6 as i8))
        .show(ui, add)
}

/// Approval frame with a tinted heading. Permission choices remain entirely caller-owned.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_approval<R>(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    let output = egui::Frame::NONE.fill(t.dialog_body).stroke(egui::Stroke::new(1.0, t.form_primary)).corner_radius(t.radius_xl).show(ui, |ui| {
        ui.set_min_width(ui.available_width().max(0.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::NONE
            .fill(t.accent_soft)
            .corner_radius(egui::CornerRadius { nw: t.radius_xl.saturating_sub(1), ne: t.radius_xl.saturating_sub(1), sw: 0, se: 0 })
            .inner_margin(egui::Margin::symmetric(t.space_10 as i8, t.space_8 as i8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width().max(0.0));
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = t.space_6;
                    let (icon, _) = ui.allocate_exact_size(vec2(t.space_16, t.space_16), egui::Sense::hover());
                    crate::icons::paint_ink(ui.painter(), icon, "shield", t.selected_ink);
                    ui.add(egui::Label::new(egui::RichText::new(title).font(crate::fonts::semibold(t.font_label)).color(t.form_ink)).wrap());
                });
            });
        egui::Frame::NONE
            .inner_margin(t.space_10)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = t.space_8;
                body(ui)
            })
            .inner
    });
    crate::scenario::publish_rect("assistant:approval", output.response.rect);
    output
}

/// Unframed growing editor inside the composer. The caller retains focus ID and Enter routing.
/// Tokens: font_label, control_height, space_8 and shared text_input alignment.
#[cfg(test)]
pub(crate) fn assistant_input(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    composer_edit(ui, id, value, hint, None).response.response
}

/// The editor of `assistant_input`, with `layouter` drawing its text when given.
#[cfg(any(not(target_arch = "wasm32"), test))]
fn composer_edit<'a>(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut String,
    hint: &str,
    layouter: Option<&'a mut dyn FnMut(&Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley>>,
) -> egui::text_edit::TextEditOutput {
    let t = Tokens::get();
    // The hint is painted here, not by the TextEdit: a multiline edit wraps its hint onto
    // several rows, and the hint must stay on one row, ending in an ellipsis when it does not fit.
    let mut edit = super::fields::text_input(egui::TextEdit::multiline(value))
        .id(id)
        .frame(egui::Frame::NONE)
        .background_color(egui::Color32::TRANSPARENT)
        .desired_rows(2)
        .desired_width(f32::INFINITY)
        .min_size(vec2(0.0, t.control_height + t.space_8));
    edit = match layouter {
        Some(layouter) => edit.layouter(layouter),
        None => edit.font(egui::FontId::proportional(t.font_label)),
    };
    let mut out = edit.show(ui);
    super::text_menu::menu(ui, &mut out.response.response, value, super::text_menu::Field::LINES);
    if value.is_empty() {
        let width = (out.response.response.rect.width() - 2.0 * t.form_inset).max(0.0);
        let galley = assistant_hint_galley(ui, hint, width);
        let pos = egui::pos2(out.response.response.rect.left() + t.form_inset, out.response.response.rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, ui.visuals().weak_text_color());
    }
    out
}

/// The composer hint laid out on one row of at most `width`, cut with an ellipsis.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_hint_galley(ui: &Ui, hint: &str, width: f32) -> std::sync::Arc<egui::Galley> {
    let t = Tokens::get();
    let mut job = egui::text::LayoutJob::simple_singleline(hint.to_owned(), egui::FontId::proportional(t.font_label), ui.visuals().weak_text_color());
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The composer's editor with a caller-supplied text layout (the pills of a message are drawn by
/// it). Everything else is as `assistant_input`; the output carries the galley and the caret.
/// Tokens: as `assistant_input`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn assistant_input_laid_out(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut String,
    hint: &str,
    layouter: &mut dyn FnMut(&Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley>,
) -> egui::text_edit::TextEditOutput {
    composer_edit(ui, id, value, hint, Some(layouter))
}

/// Compact right-aligned approval actions; wrapping grows by control rows, not the remaining card height.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_actions<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), t.control_height),
        egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true),
        add,
    )
}

/// Bottom corners follow the window; the top meets the footer rule square.
#[cfg(any(not(target_arch = "wasm32"), test))]
fn composer_corners(t: &Tokens) -> egui::CornerRadius {
    let r = t.radius_xl.saturating_sub(1);
    egui::CornerRadius { nw: 0, ne: 0, sw: r, se: r }
}

/// Composer's rounded field surface with an accent border only while focused.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_composer<R>(ui: &mut Ui, focused: bool, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    egui::Frame::NONE
        .fill(t.control_fill)
        .stroke(if focused { egui::Stroke::new(1.0, t.form_primary) } else { t.control_border_stroke(t.control_border) })
        .corner_radius(composer_corners(&t))
        .inner_margin(egui::Margin::symmetric(t.space_10 as i8, t.space_8 as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(0.0));
            ui.spacing_mut().item_spacing.y = t.space_6;
            add(ui)
        })
}

/// Circular primary send or stop action. The caller retains queue and cancellation dispatch.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_send(ui: &mut Ui, enabled: bool, busy: bool, label: &str) -> Response {
    let t = Tokens::get();
    ui.add_enabled_ui(enabled, |ui| {
        let size = t.control_height + t.space_2;
        let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
        let fill = if response.hovered() { t.form_primary.gamma_multiply(0.9) } else { t.form_primary };
        ui.painter().circle_filled(rect.center(), size / 2.0, fill);
        let ink = if crate::theme::contrast_ratio(t.on_accent, fill) >= 4.5 { t.on_accent } else { t.tab_on_fill_dark };
        crate::icons::paint_ink(ui.painter(), rect.shrink(t.space_6), if busy { "stop" } else { "send" }, ink);
        if response.has_focus() {
            ui.painter().circle_stroke(rect.center(), size / 2.0 + t.space_2, egui::Stroke::new(1.0, t.form_primary));
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
        response.on_hover_text(label)
    })
    .inner
}

/// Pinned composer footer. The enclosing shell retains ownership of scrolling and sizing.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_composer_footer<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    let output = egui::Frame::NONE
        .fill(t.dialog_footer)
        .corner_radius(composer_corners(&t))
        // No margin: the composer box inside fills this footer edge to edge. The text padding
        // lives on the box itself, so there is one inset, not two.
        .inner_margin(0)
        .show(ui, add);
    t.secondary_hline(ui.painter(), output.response.rect.x_range(), output.response.rect.top(), t.dialog_rule);
    output
}

/// Centred setup copy and caller-supplied action, bounded to a readable 300 pt column.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_empty<R>(ui: &mut Ui, title: &str, body: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    let width = ui.available_width().min(t.form_card_width + t.space_24 + t.space_12).max(0.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(width);
        ui.spacing_mut().item_spacing.y = t.space_8;
        let diameter = t.control_height + t.space_16;
        let (disc, _) = ui.allocate_exact_size(vec2(diameter, diameter), egui::Sense::hover());
        ui.painter().circle_filled(disc.center(), diameter / 2.0, t.accent_soft);
        crate::icons::paint_ink(ui.painter(), disc.shrink(t.space_8), "assistant", t.selected_ink);
        ui.add(egui::Label::new(egui::RichText::new(title).font(crate::fonts::semibold(t.font_body + t.space_2)).color(t.form_ink)).wrap());
        ui.add(egui::Label::new(egui::RichText::new(body).size(t.font_label).color(t.form_dim)).wrap());
        add(ui)
    })
    .inner
}

/// Semantic warning/error/information band with caller-owned text and actions.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn assistant_banner<R>(ui: &mut Ui, tone: AssistantTone, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    banner(ui, tone, add)
}

/// Shared semantic notice band, also available to workspace inspectors on wasm.
pub(crate) fn banner<R>(ui: &mut Ui, tone: AssistantTone, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::get();
    let (_, fill) = tone.colors(&t);
    egui::Frame::NONE.fill(fill).stroke(t.container_border(t.dialog_rule)).corner_radius(t.radius_lg).inner_margin(t.space_10).show(ui, |ui| {
        ui.set_min_width(ui.available_width().max(0.0));
        ui.spacing_mut().item_spacing.y = t.space_8;
        add(ui)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_bubble_stays_right_aligned_at_narrow_and_wide_widths() {
        for width in [220.0, 280.0, 440.0, 740.0] {
            let ctx = crate::fonts::test_context();
            for _ in 0..2 {
                super::super::test_support::frame(&ctx, vec![], |ui| {
                    ui.set_width(width);
                    let left = ui.cursor().left();
                    let available = ui.available_width();
                    let bubble = assistant_bubble(ui, |ui| {
                        ui.label("A message that wraps within its bubble.");
                    });
                    assert!((bubble.response.rect.right() - (left + available)).abs() < 1.0);
                    assert!(bubble.response.rect.width() <= available * 0.86 + 1.0);
                });
            }
        }
    }

    #[test]
    fn assistant_bubble_stacks_message_and_file_summary_without_squeezing_either() {
        for width in [280.0, 440.0, 740.0] {
            let ctx = crate::fonts::test_context();
            for _ in 0..2 {
                super::super::test_support::frame(&ctx, vec![], |ui| {
                    ui.set_width(width);
                    let bubble = assistant_bubble(ui, |ui| {
                        let message = ui.label("Make the plate thicker.").rect;
                        let files = ui.label("Files: plate-dimensions.txt").rect;
                        (message, files)
                    });
                    let (message, files) = bubble.inner;
                    assert!(files.top() >= message.bottom(), "file summary must follow message vertically");
                    assert!((message.left() - files.left()).abs() < 1.0, "both lines share the content inset");
                    assert!(files.width() > Tokens::get().control_height * 3.0, "file summary must retain readable width");
                    assert!(bubble.response.rect.height() < Tokens::get().control_height * 4.0, "two short lines remain compact");
                });
            }
        }
    }

    #[test]
    fn assistant_send_only_activates_when_enabled() {
        for enabled in [false, true] {
            let ctx = crate::fonts::test_context();
            let mut position = egui::Pos2::ZERO;
            let mut clicked = false;
            super::super::test_support::frame(&ctx, vec![], |ui| {
                position = assistant_send(ui, enabled, false, "Send").rect.center();
            });
            super::super::test_support::frame(&ctx, super::super::test_support::pointer(position, true), |ui| {
                clicked |= assistant_send(ui, enabled, false, "Send").clicked();
            });
            super::super::test_support::frame(&ctx, super::super::test_support::pointer(position, false), |ui| {
                clicked |= assistant_send(ui, enabled, false, "Send").clicked();
            });
            assert_eq!(clicked, enabled);
        }
    }

    #[test]
    fn assistant_header_and_badge_share_reference_control_heights() {
        let ctx = crate::fonts::test_context();
        super::super::test_support::frame(&ctx, vec![], |ui| {
            let t = Tokens::get();
            let header = assistant_header(ui, |ui| {
                ui.allocate_exact_size(vec2(t.space_24, t.form_close_size), egui::Sense::hover());
            });
            assert!((header.response.rect.height() - t.form_header_height).abs() < 1.0);
            let badge = assistant_badge(ui, "Ready", AssistantTone::Success);
            assert!((badge.rect.height() - (t.form_close_size - t.space_4)).abs() < 1.0);
        });
    }
}
