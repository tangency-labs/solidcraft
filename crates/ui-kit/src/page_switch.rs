//! Counted page navigation built on the segmented control's interaction and accessibility.
use egui::{Ui, vec2};

/// A compact page switch with optional counts and a sliding, inset active pill.
/// Uses the segmented control's labels, focus, targets and disabled-container behaviour.
pub struct PageSwitch<'a> {
    id: &'a str,
    labels: &'a [&'a str],
    segments: super::Segments<'a>,
}

impl<'a> PageSwitch<'a> {
    pub fn new(id: &'a str, labels: &'a [&'a str]) -> Self {
        Self { id, labels, segments: super::Segments { page_switch: true, intrinsic: true, compact: true, ..Default::default() } }
    }
    pub fn counts(mut self, counts: &'a [usize]) -> Self {
        self.segments.counts = counts;
        self
    }
    pub fn targets(mut self, targets: &'a [&'a str]) -> Self {
        self.segments.targets = targets;
        self
    }
    /// Also honours the host's zero animation duration (the app's reduced-motion setting).
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.segments.reduced_motion = reduced;
        self
    }
    pub fn width(&self, ui: &Ui) -> f32 {
        let t = solidcraft_theme::Tokens::get();
        let shown: Vec<_> = self.labels.iter().map(|label| solidcraft_ui_text::tr_label(label)).collect();
        super::choice::segment_widths(ui, &shown, &[], self.segments.counts, true, t.font_label).iter().sum::<f32>() + t.space_2 * 2.0
    }
    pub fn show(self, ui: &mut Ui, selected: &mut usize) -> bool {
        let Some(index) = selected_index(*selected, self.labels.len()) else { return false };
        let before = *selected;
        *selected = index;
        ui.push_id(self.id, |ui| super::segmented(ui, selected, self.labels, self.segments)).inner || before != *selected
    }
}

pub(super) fn selected_index(index: usize, count: usize) -> Option<usize> {
    count.checked_sub(1).map(|last| index.min(last))
}

#[derive(Clone)]
pub(super) struct Slide {
    from: [f32; 2],
    to: [f32; 2],
    started: f64,
}

impl Slide {
    pub fn new(target: [f32; 2], now: f64) -> Self {
        Self { from: target, to: target, started: now }
    }
    fn at(&self, now: f64, duration: f32) -> [f32; 2] {
        if !duration.is_finite() || duration <= 0.0 || !now.is_finite() {
            return self.to;
        }
        let progress = solidcraft_theme::panel_motion_ease(((now - self.started) / f64::from(duration)) as f32);
        [egui::lerp(self.from[0]..=self.to[0], progress), egui::lerp(self.from[1]..=self.to[1], progress)]
    }
    /// Retarget from the current position, so rapid reversals do not jump.
    pub fn sample(&mut self, target: [f32; 2], now: f64, duration: f32) -> [f32; 2] {
        if target != self.to {
            self.from = self.at(now, duration);
            self.to = target;
            self.started = now;
        }
        let position = self.at(now, duration);
        if duration <= 0.0 {
            self.from = target;
        }
        position
    }
}

pub(super) fn highlight(ui: &Ui, track: egui::Rect, target: egui::Rect, reduced: bool) -> egui::Shape {
    let t = solidcraft_theme::Tokens::get();
    let duration = solidcraft_theme::panel_motion_duration(ui.ctx(), reduced).min(0.14);
    let now = ui.input(|i| i.time);
    let target = [target.left() - track.left(), target.right() - track.left()];
    let key = ui.make_persistent_id("page_switch_slide");
    let mut slide = ui.data(|data| data.get_temp::<Slide>(key)).unwrap_or_else(|| Slide::new(target, now));
    let position = slide.sample(target, now, duration);
    ui.data_mut(|data| data.insert_temp(key, slide));
    if position != target {
        ui.ctx().request_repaint();
    }
    let pill = egui::Rect::from_min_max(egui::pos2(track.left() + position[0], track.top()), egui::pos2(track.left() + position[1], track.bottom()))
        .shrink2(vec2(0.0, t.space_2));
    solidcraft_ui_registry::publish_rect("page_switch:highlight", pill);
    egui::Shape::Vec(vec![
        egui::Shape::rect_filled(pill, t.control_radius, if solidcraft_theme::is_dark() { t.nav_hover } else { t.control_fill }),
        egui::Shape::rect_stroke(pill, t.control_radius, t.container_border(t.dialog_border), egui::StrokeKind::Inside),
    ])
}

pub(super) fn badge_width(ui: &Ui, count: usize) -> f32 {
    let t = solidcraft_theme::Tokens::get();
    (ui.painter().layout_no_wrap(count.to_string(), super::type_face::semibold(t.font_small), t.form_dim).size().x + t.space_10)
        .max(t.space_16 + t.space_2)
}

pub(super) fn badge(ui: &Ui, rect: egui::Rect, count: usize, active: bool) {
    let t = solidcraft_theme::Tokens::get();
    let rect = egui::Rect::from_center_size(rect.center(), vec2(rect.width(), t.space_16));
    ui.painter().rect_filled(rect, t.radius_lg, if active { t.selected_bg } else { t.dialog_rule });
    let font = super::type_face::semibold(t.font_small);
    let text = count.to_string();
    let width = ui.painter().layout_no_wrap(text.clone(), font.clone(), t.form_dim).size().x;
    let text_rect = egui::Rect::from_center_size(rect.center(), vec2(width, rect.height()));
    super::text::paint_text(ui, text_rect, &text, font, if active { t.form_primary } else { t.form_dim }, false);
}
