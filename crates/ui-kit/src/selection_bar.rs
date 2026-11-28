//! Compact actions for a selection, inside the owning table or list card.
use egui::{Response, Ui, vec2};
use solidcraft_theme::Tokens;

pub struct SelectionAction<'a> {
    pub label: &'a str,
    pub icon: &'a str,
    pub enabled: bool,
}

pub struct SelectionBarResponse {
    pub actions: Vec<Response>,
    pub clear: Response,
}

/// Count at the left, labelled 24 pt actions and a divided clear control at the right.
/// Tokens: selected_bg, form_primary/ink, control_fill/border/radius, radius_lg, space_*.
pub struct SelectionBar<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub clear_label: &'a str,
}

impl SelectionBar<'_> {
    pub fn show(self, ui: &mut Ui, actions: &[SelectionAction<'_>]) -> SelectionBarResponse {
        let t = Tokens::get();
        ui.push_id(self.id, |ui| {
            let frame = egui::Frame::NONE
                .outer_margin(egui::Margin { left: t.space_12 as i8, right: t.space_12 as i8, top: 0, bottom: t.space_8 as i8 })
                .inner_margin(egui::Margin { left: t.space_12 as i8, right: t.space_8 as i8, top: t.space_6 as i8, bottom: t.space_6 as i8 })
                .corner_radius(t.radius_lg)
                .fill(t.selected_bg)
                .stroke(egui::Stroke::new(t.control_border_stroke(t.form_primary).width, t.form_primary.gamma_multiply(0.33)));
            let width = (ui.available_width() - frame.total_margin().sum().x).max(0.0);
            let shown = frame.show(ui, |ui| {
                ui.set_min_width(width);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.space_8;
                    let font = crate::type_face::semibold(t.font_label);
                    let label_width = ui.painter().layout_no_wrap(self.label.to_owned(), font.clone(), t.form_primary).size().x;
                    let (label, _) = ui.allocate_exact_size(vec2(label_width, t.space_24), egui::Sense::hover());
                    super::text::paint_text(ui, label, self.label, font, t.form_primary, false);
                    let action_width: f32 = actions.iter().map(|a| super::icon_label_width(ui, a.label, Some(a.icon), false) + t.space_8).sum();
                    let remaining = ui.available_width() - action_width - t.space_24 - t.space_2 - t.space_8;
                    ui.add_space(remaining.max(0.0));
                    let actions = actions
                        .iter()
                        .enumerate()
                        .map(|(index, action)| {
                            let response = ui
                                .add_enabled_ui(action.enabled, |ui| {
                                    let width = super::icon_label_width(ui, action.label, Some(action.icon), false);
                                    let (rect, response) = ui.allocate_exact_size(vec2(width, t.space_24), egui::Sense::click());
                                    ui.painter().rect(
                                        rect,
                                        t.control_radius,
                                        if response.hovered() { t.hover } else { t.control_fill },
                                        t.control_border_stroke(t.control_border),
                                        egui::StrokeKind::Inside,
                                    );
                                    let icon = egui::Rect::from_min_size(
                                        rect.min + vec2(t.space_10, (t.space_24 - t.space_14) / 2.0),
                                        vec2(t.space_14, t.space_14),
                                    );
                                    crate::icons::paint_ink(ui.painter(), icon, action.icon, t.form_ink);
                                    super::text::paint_text(
                                        ui,
                                        egui::Rect::from_min_max(egui::pos2(icon.right() + t.space_6, rect.top()), rect.max),
                                        action.label,
                                        egui::FontId::proportional(t.font_label),
                                        t.form_ink,
                                        false,
                                    );
                                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), action.label));
                                    if response.has_focus() {
                                        ui.painter().rect_stroke(
                                            rect,
                                            t.control_radius,
                                            t.control_border_stroke(t.form_primary),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    response
                                })
                                .inner;
                            solidcraft_ui_registry::publish_target(&format!("{}:action:{index}", self.id), response.rect);
                            response
                        })
                        .collect();
                    let (divider, _) = ui.allocate_exact_size(vec2(t.space_2, t.space_16), egui::Sense::hover());
                    t.secondary_vline(ui.painter(), divider.center().x, divider.y_range(), t.control_border);
                    let (rect, _) = ui.allocate_exact_size(vec2(t.space_24, t.space_24), egui::Sense::hover());
                    let clear = super::icon_button_at(ui, rect, "close", self.clear_label);
                    solidcraft_ui_registry::publish_target(&format!("{}:clear", self.id), clear.rect);
                    SelectionBarResponse { actions, clear }
                })
                .inner
            });
            shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Toolbar, ui.is_enabled(), self.label));
            solidcraft_ui_registry::publish_rect(self.id, shown.response.rect);
            shown.inner
        })
        .inner
    }
}
