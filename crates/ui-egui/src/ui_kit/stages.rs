//! Numbered stage navigation in a two-row workspace card. The caller owns commands and state.
use crate::theme::{TabAccent, Tokens};
use crate::ui_kit::BackingTooltipExt as _;
use egui::{Rect, Sense, Ui, pos2, vec2};

pub(crate) struct StageAction<'a> {
    pub icon: Option<&'a str>,
    pub label: &'a str,
    pub enabled: bool,
    pub primary: bool,
    pub reason: Option<&'a str>,
}

pub(crate) struct StageResponse {
    pub stage: Option<usize>,
    pub action: Option<usize>,
    pub chips: Vec<Rect>,
    pub actions: Vec<Rect>,
}

pub(crate) struct StageBar;
impl StageBar {
    pub(crate) const HEIGHT: f32 = 88.0;
    pub(crate) const HEADER_HEIGHT: f32 = 36.0;

    /// Shared compact workspace shell; independent of modelling ribbon label preferences.
    pub(crate) fn cards(available: Rect) -> crate::shell::Cards {
        let gap = Tokens::get().space_8;
        let min = pos2(available.left() + gap, (available.top() + gap).min(available.bottom()));
        let right = (available.right() - gap).max(min.x);
        let bottom = (min.y + Self::HEIGHT).min(available.bottom()).max(min.y);
        let workspace = Rect::from_min_max(min, pos2((min.x + 104.0).min(right), bottom));
        let ribbon = Rect::from_min_max(pos2((workspace.right() + gap).min(right), min.y), pos2(right, bottom));
        crate::shell::Cards { workspace, ribbon, bottom: (bottom + gap).min(available.bottom()) }
    }

    /// Workspace switcher content uses the same accent as the numbered stage chips.
    pub(crate) fn workspace(ui: &Ui, rect: Rect, icon: &str, label: &str, caption: &str) {
        let t = Tokens::get();
        let accent = TabAccent::current(ui.ctx());
        let center = rect.center();
        let icon_side = t.control_height + t.space_2;
        crate::icons::paint_icon(ui.painter(), Rect::from_center_size(center - vec2(0.0, t.space_16), vec2(icon_side, icon_side)), icon, 1.0);
        super::text::paint_text(
            ui,
            Rect::from_center_size(
                center + vec2(0.0, t.space_8),
                vec2(ui.painter().layout_no_wrap(label.into(), crate::fonts::semibold(t.font_button), accent.ink).size().x, t.space_16),
            ),
            label,
            crate::fonts::semibold(t.font_button),
            accent.ink,
            false,
        );
        let width = ui.painter().layout_no_wrap(caption.into(), egui::FontId::proportional(t.font_small), t.text_dim).size().x;
        let caption_rect = Rect::from_center_size(center + vec2(-t.space_4, t.space_24), vec2(width, t.space_16));
        super::text::paint_text(ui, caption_rect, caption, egui::FontId::proportional(t.font_small), t.text_dim, false);
        super::dropdown_indicator(
            ui.painter(),
            Rect::from_center_size(pos2(caption_rect.right() + t.space_6, caption_rect.center().y), vec2(t.space_8, t.space_8)),
            t.text_dim,
        );
    }

    pub(crate) fn separator(ui: &mut Ui) {
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(1.0, t.space_16 + t.space_4), Sense::hover());
        ui.painter().vline(rect.center().x, rect.y_range(), t.control_border_stroke(t.dialog_rule));
    }

    pub(crate) fn icon_action(ui: &mut Ui, icon: &str, label: &str) -> egui::Response {
        let side = Tokens::get().control_height - Tokens::get().space_2;
        let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
        super::icon_button_at(ui, rect, icon, label)
    }

    pub(crate) fn tool_gap(ui: &mut Ui) {
        ui.add_space(Tokens::get().space_4);
    }

    pub(crate) fn caption(ui: &mut Ui, label: &str) {
        let t = Tokens::get();
        let label = label.to_uppercase();
        let font = crate::fonts::semibold(t.font_small);
        let width = ui.painter().layout_no_wrap(label.clone(), font.clone(), t.text_dim).size().x
            + t.font_small * t.font_section_tracking * label.chars().count() as f32
            + t.space_8;
        let (rect, _) = ui.allocate_exact_size(vec2(width, t.control_height), Sense::hover());
        super::text::paint_text(ui, rect.shrink2(vec2(t.space_4, 0.0)), &label, font, t.text_dim, true);
    }

    /// Quiet information chip within the tools row, using the shared surface and ink tokens.
    pub(crate) fn note(ui: &mut Ui, label: &str) -> egui::Response {
        let t = Tokens::get();
        let width = ui.painter().layout_no_wrap(label.into(), egui::FontId::proportional(t.font_label), t.text_dim).size().x;
        let (rect, response) =
            ui.allocate_exact_size(vec2(width + t.space_14 + t.space_6 + t.space_8 * 2.0, t.control_height - t.space_2), Sense::hover());
        ui.painter().rect_filled(rect, t.control_radius, t.dialog_footer);
        let icon = Rect::from_center_size(pos2(rect.left() + t.space_8 + t.space_14 / 2.0, rect.center().y), vec2(t.space_14, t.space_14));
        crate::icons::paint_ink(ui.painter(), icon, "help", t.text_dim);
        super::text::paint_text(
            ui,
            Rect::from_min_max(pos2(icon.right() + t.space_6, rect.top()), rect.max),
            label,
            egui::FontId::proportional(t.font_label),
            t.text_dim,
            false,
        );
        response
    }

    /// Fixed 36 pt header; the remaining height is the stage's tool row. Both rows scroll
    /// horizontally on small windows and clip to the enclosing workspace card.
    pub(crate) fn row<R>(ui: &mut Ui, card: Rect, tools: bool, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let t = Tokens::get();
        let split = (card.top() + Self::HEADER_HEIGHT).min(card.bottom());
        let rect =
            if tools { Rect::from_min_max(pos2(card.left(), split), card.max) } else { Rect::from_min_max(card.min, pos2(card.right(), split)) };
        if !rect.is_finite() || rect.height() < t.control_height || rect.width() < t.control_height {
            return None;
        }
        if tools {
            t.secondary_hline(ui.painter(), rect.x_range(), rect.top() + 0.5, t.dialog_rule);
        }
        let inner = rect.shrink2(vec2(if tools { t.space_8 } else { t.space_6 }, 0.0));
        Some(
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner).id_salt(("stage_row", tools)), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(rect));
                ui.spacing_mut().item_spacing = vec2(if tools { t.space_8 } else { t.space_4 }, 0.0);
                egui::ScrollArea::horizontal()
                    .id_salt("stage_scroll")
                    .animated(false)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(inner.width());
                        ui.set_height(inner.height());
                        super::scope(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(if tools { t.space_8 } else { t.space_4 }, 0.0);
                            ui.horizontal_centered(add).inner
                        })
                    })
                    .inner
            })
            .inner,
        )
    }

    /// Stage chips, primary/cancel actions, then the existing command-search editor. No done
    /// badges are drawn: a selected stage is navigation, not evidence that work is complete.
    pub(crate) fn header(ui: &mut Ui, selected: usize, labels: &[&str], actions: &[StageAction<'_>]) -> StageResponse {
        let t = Tokens::get();
        let accent = TabAccent::current(ui.ctx());
        let mut result = StageResponse { stage: None, action: None, chips: Vec::new(), actions: Vec::new() };
        for (index, label) in labels.iter().enumerate() {
            if index > 0 {
                let (line, _) = ui.allocate_exact_size(vec2(t.space_16 + t.space_2, t.control_height), Sense::hover());
                t.secondary_hline(ui.painter(), line.x_range(), line.center().y, t.control_border);
            }
            let font = egui::FontId::proportional(t.font_button);
            let width = ui.painter().layout_no_wrap((*label).into(), crate::fonts::semibold(t.font_button), t.text).size().x.ceil()
                + t.space_8
                + t.space_14
                + t.space_6
                + 1.0
                + t.space_16
                + t.space_2;
            let (chip, response) = ui.allocate_exact_size(vec2(width, t.control_height + t.space_2), Sense::click());
            let on = index == selected;
            if on || response.hovered() {
                ui.painter().rect_filled(chip, chip.height() / 2.0, if on { accent.soft } else { t.hover });
            }
            let badge = Rect::from_center_size(
                pos2(chip.left() + t.space_8 + (t.space_16 + t.space_2) / 2.0, chip.center().y),
                vec2(t.space_16 + t.space_2, t.space_16 + t.space_2),
            );
            ui.painter().circle_filled(badge.center(), badge.width() / 2.0, if on { accent.color } else { t.control_fill });
            if !on {
                ui.painter().circle_stroke(badge.center(), badge.width() / 2.0, t.control_border_stroke(t.control_border));
            }
            super::text::paint_text(
                ui,
                {
                    let number = (index + 1).to_string();
                    let width = ui.painter().layout_no_wrap(number, crate::fonts::semibold(t.font_small), t.text).size().x;
                    Rect::from_center_size(badge.center(), vec2(width, badge.height()))
                },
                &(index + 1).to_string(),
                crate::fonts::semibold(t.font_small),
                if on { t.on_accent } else { t.text_dim },
                false,
            );
            let text = Rect::from_min_max(pos2(badge.right() + t.space_6 + 1.0, chip.top()), chip.right_bottom() - vec2(t.space_14, 0.0));
            super::text::paint_text(
                ui,
                text,
                label,
                if on { crate::fonts::semibold(t.font_button) } else { font },
                if on { accent.ink } else { t.text },
                false,
            );
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, *label));
            result.chips.push(chip.intersect(ui.clip_rect()));
            if response.clicked() {
                result.stage = Some(index);
            }
        }
        let search_width = super::CommandSearch::closed_width(ui.ctx()).max(188.0);
        let action_width = actions.iter().map(|a| super::actions::icon_label_width(ui, a.label, a.icon, a.primary)).sum::<f32>();
        ui.add_space((ui.available_width() - search_width - action_width - t.space_4 * (actions.len() + 2) as f32).max(t.space_8));
        for (index, item) in actions.iter().enumerate() {
            let response = super::actions::icon_label_action(ui, item.label, item.icon, item.enabled, item.primary);
            result.actions.push(response.rect.intersect(ui.clip_rect()));
            if response.clicked() {
                result.action = Some(index);
            }
            if let Some(reason) = item.reason {
                response.backing_on_disabled_hover_text(reason);
            }
        }
        ui.add_space((ui.available_width() - search_width).max(0.0));
        let (search, _) = ui.allocate_exact_size(vec2(search_width, t.control_height), Sense::hover());
        // CommandSearch owns the actual field, focus, S shortcut and registered command dispatch.
        let search = search.intersect(ui.clip_rect());
        ui.ctx().data_mut(|d| {
            let id = egui::Id::new("sc_ribbon_search");
            if search.is_positive() {
                d.insert_temp(id, search);
            } else {
                d.remove::<Rect>(id);
            }
        });
        result
    }
}
