//! Appearance inheritance selector. The caller owns selection and commands.
use super::{Sphere, sphere::paint_sphere};
use crate::{scenario, theme::Tokens};
use egui::{Rect, Sense, Stroke, Ui, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookStep {
    Face,
    Body,
    Component,
    Material,
}

impl LookStep {
    pub const ALL: [Self; 4] = [Self::Face, Self::Body, Self::Component, Self::Material];

    fn name(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Body => "body",
            Self::Component => "component",
            Self::Material => "material",
        }
    }
}

/// Labels are caller supplied for translation. `None` is an empty inheritance slot.
/// An enum active value guarantees exactly one active step, including empty steps.
/// Publishes `{id}:{face|body|component|material}` hit targets and the `{id}` allocation.
pub fn look_chain(ui: &mut Ui, id: &str, labels: [&str; 4], looks: [Option<Sphere>; 4], active: LookStep) -> Option<LookStep> {
    let t = Tokens::get();
    let width = ui.available_width().clamp(1.0, t.form_card_width * 2.0);
    let side = (t.control_height + t.space_6).min(width / 4.0);
    let ring_width = t.space_2.min(side / 8.0);
    let height = side + t.space_2 + t.font_small + t.space_2;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let mut clicked = None;
    for (index, ((step, label), look)) in LookStep::ALL.into_iter().zip(labels).zip(looks).enumerate() {
        let cell_width = width / 4.0;
        let cell = Rect::from_min_size(rect.min + vec2(index as f32 * cell_width, 0.0), vec2(cell_width, height));
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(cell));
        let center = pos2(cell.center().x, cell.top() + side / 2.0 + ring_width);
        let radius = (side / 2.0 - ring_width).max(0.0);
        if index < 3 {
            ui.painter().line_segment(
                [center + vec2(side / 2.0 + t.space_8, 0.0), center + vec2(cell_width - side / 2.0 - t.space_8, 0.0)],
                Stroke::new(1.0, t.control_border),
            );
        }
        let key = format!("{id}:{}", step.name());
        let response = ui.interact(cell, ui.make_persistent_id(&key), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), active == step, label));
        if response.clicked() {
            clicked = Some(step);
        }
        if let Some(look) = look {
            paint_sphere(ui, Rect::from_center_size(center, vec2(radius * 2.0, radius * 2.0)), look);
        } else {
            // Evenly spaced analytic dashes, independent of any icon asset.
            for dash in 0..12 {
                let start = dash as f32 * std::f32::consts::TAU / 12.0;
                let points = (0..=4)
                    .map(|n| {
                        let a = start + n as f32 * std::f32::consts::TAU / 96.0;
                        center + vec2(a.cos(), a.sin()) * radius
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(1.0, t.form_dim)));
            }
            let arm = radius * 0.4;
            painter.line_segment([center - vec2(arm, 0.0), center + vec2(arm, 0.0)], Stroke::new(1.0, t.form_dim));
            painter.line_segment([center - vec2(0.0, arm), center + vec2(0.0, arm)], Stroke::new(1.0, t.form_dim));
        }
        if step == active {
            painter.circle_stroke(center, radius + ring_width, Stroke::new(ring_width, t.render_accent));
        }
        let font = if step == active { crate::fonts::semibold(t.font_small) } else { egui::FontId::proportional(t.font_small) };
        let label_width = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), t.form_ink).size().x;
        let label_left = (cell.center().x - label_width / 2.0).max(cell.left());
        let label_rect = Rect::from_min_max(pos2(label_left, cell.top() + side + t.space_2), cell.max);
        super::text::paint_text(ui, label_rect, label, font, if step == active { t.render_accent } else { t.form_dim }, false);
        scenario::publish_rect(&key, cell);
        scenario::publish_handle(&key, cell.center());
    }
    scenario::publish_rect(id, rect);
    clicked
}

#[cfg(test)]
#[path = "look_chain_tests.rs"]
mod tests;
