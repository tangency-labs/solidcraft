use std::ops::RangeInclusive;

use egui::{Color32, Rect, Response, Stroke, StrokeKind, Ui, Vec2};

use crate::{paint_glass, GlassStyle};

const CONTROL_HEIGHT: f32 = 40.0;
const THUMB_HEIGHT: f32 = 16.0;
const THUMB_ASPECT_RATIO: f32 = 0.625;

/// A horizontal slider with a refractive glass thumb and an editable value.
///
/// Uses egui's slider interaction, keyboard navigation, clamping, and accessibility.
/// Call [`crate::init`] and provide a backdrop as for other glass widgets.
///
/// ```no_run
/// # egui::__run_test_ui(|ui| {
/// let mut volume = 50.0;
/// egui_glass::GlassSlider::new(&mut volume, 0.0..=100.0)
///     .text("Volume").suffix("%").show(ui);
/// # });
/// ```
pub struct GlassSlider<'a> {
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    text: String,
    suffix: String,
    style: GlassStyle,
    width: Option<f32>,
    step: Option<f64>,
    animate: bool,
}

impl<'a> GlassSlider<'a> {
    /// Creates a linear slider over a finite range, filling the available width.
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        Self { value, range, text: String::new(), suffix: String::new(), style: GlassStyle::regular(), width: None, step: None, animate: true }
    }

    /// Sets the visible label and the accessible name.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// Appends a unit to the editable value, for example "%" or " px".
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// Sets the thumb's optical material. Its white tint and capsule shape are retained.
    pub fn style(mut self, style: GlassStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the control width in logical points (minimum 64).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the value step for pointer and keyboard interaction.
    pub fn step_by(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    /// Enables hover/press material transitions (default `true`).
    /// Uses the UI's animation time, capped at 120 ms; `false` switches instantly.
    /// The value and thumb position always update immediately.
    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    /// Draws the control. `changed()` includes both dragging and direct value edits.
    pub fn show(self, ui: &mut Ui) -> Response {
        let old_value = *self.value;
        let width = self.width.unwrap_or_else(|| ui.available_width()).max(64.0);
        let mut response = ui.vertical(|ui| {
            ui.set_width(width);
            let header = ui.horizontal(|ui| {
                let label = ui.label(&self.text);
                let value = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let span = (f64::from(*self.range.end()) - f64::from(*self.range.start())).abs();
                    ui.add(egui::DragValue::new(self.value).range(self.range.clone())
                        .speed(self.step.unwrap_or(span / 200.0)).max_decimals(3).suffix(&self.suffix))
                }).inner;
                value.labelled_by(label.id)
            }).inner;
            let mut response = ui.scope(|ui| {
                // Keep egui's input and accessibility semantics while replacing only its paint.
                ui.set_opacity(0.0);
                ui.spacing_mut().slider_width = width;
                ui.spacing_mut().interact_size.y = CONTROL_HEIGHT;
                let mut slider = egui::Slider::new(self.value, self.range.clone()).show_value(false)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: THUMB_ASPECT_RATIO });
                if let Some(step) = self.step { slider = slider.step_by(step); }
                ui.add(slider)
            }).inner;
            response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(*self.value), &self.text));
            if ui.is_rect_visible(response.rect) {
                self.paint(ui, &response);
            }
            if header.changed() { response.mark_changed(); }
            response
        }).inner;
        if *self.value != old_value { response.mark_changed(); }
        response
    }

    fn paint(&self, ui: &Ui, response: &Response) {
        let rect = response.rect;
        let dark = ui.visuals().dark_mode;
        let accent = Color32::from_rgb(0, 136, 255);
        let track_color = if dark { Color32::from_white_alpha(45) } else { Color32::from_black_alpha(26) };
        let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 6.0));
        ui.painter().rect_filled(track, 3.0, track_color);
        let span = f64::from(*self.range.end()) - f64::from(*self.range.start());
        let fraction = if span == 0.0 { 0.0 } else { ((f64::from(*self.value) - f64::from(*self.range.start())) / span).clamp(0.0, 1.0) as f32 };
        // Match egui's rectangular handle travel: height / 2.5 * aspect_ratio.
        let half_width = rect.height() / 2.5 * THUMB_ASPECT_RATIO;
        let x = egui::lerp((rect.left() + half_width)..=(rect.right() - half_width), fraction);
        let thumb = Rect::from_center_size(egui::pos2(x, rect.center().y), Vec2::new(half_width * 2.0, THUMB_HEIGHT));
        let fill = Rect::from_min_max(track.min, egui::pos2(x, track.bottom()));
        ui.painter().rect_filled(fill, 3.0, if ui.is_enabled() { accent } else { track_color });
        let style = crate::animation::interaction_style(ui, response, self.style, self.animate);
        if ui.is_enabled() {
            paint_glass(ui, thumb, &GlassStyle {
                tint: Color32::from_rgba_unmultiplied(255, 255, 255, 245),
                specular: style.specular.min(0.12), border: style.border.min(0.08),
                shadow: style.shadow.max(0.10), shadow_radius: 6.0, shadow_offset: 2.0,
                ..style.capsule()
            });
            ui.painter().rect_stroke(thumb, 255.0, Stroke::new(0.5, Color32::from_white_alpha(180)), StrokeKind::Inside);
        } else {
            ui.painter().rect_filled(thumb, 255.0, ui.visuals().widgets.noninteractive.bg_fill);
        }
        if response.has_focus() {
            ui.painter().rect_stroke(thumb.expand(2.0), 255.0, Stroke::new(2.0, accent), StrokeKind::Outside);
        }
    }
}

impl egui::Widget for GlassSlider<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui)
    }
}
