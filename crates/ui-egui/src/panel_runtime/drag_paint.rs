use std::collections::BTreeMap;

use egui::{
    Context, LayerId, Rect, Ui,
    epaint::{ClippedShape, Shape},
    layers::ShapeIdx,
};
use solidcraft_engine::panels::{Destination, InstanceId};
use solidcraft_theme::Tokens;

use super::{HostLayout, HostPreview, egui_rect};

/// The draw range of a real panel from this egui pass. Widget input and the
/// runtime's published geometry retain their committed identities.
pub(super) struct CardPaint {
    pub instance: InstanceId,
    pub layer: LayerId,
    pub bounds: Rect,
    pub committed_bounds: Rect,
    pub start: usize,
    pub end: usize,
}

pub(super) fn shape_count(ctx: &Context, layer: LayerId) -> usize {
    ctx.graphics(|graphics| graphics.get(layer).map_or(0, |list| list.next_idx().0))
}

#[derive(Default)]
pub(super) struct DragPaint {
    slides: BTreeMap<InstanceId, Slide>,
}

struct Slide {
    from: Rect,
    to: Rect,
    started: f64,
}

impl Slide {
    fn current(&self, now: f64, duration: f32) -> (Rect, bool) {
        if duration <= 0.0 || self.from == self.to {
            return (self.to, false);
        }
        let time = ((now - self.started) as f32 / duration).clamp(0.0, 1.0);
        let progress = solidcraft_theme::panel_motion_ease(time);
        (Rect::from_min_max(self.from.min.lerp(self.to.min, progress), self.from.max.lerp(self.to.max, progress)), time < 1.0)
    }
}

impl DragPaint {
    pub fn active(&self) -> bool {
        !self.slides.is_empty()
    }

    pub fn clear(&mut self) {
        self.slides.clear();
    }

    /// Reuse the shapes painted by the retained panels. The proposed layout
    /// cannot run a second body, move focus, publish input, or write preferences.
    pub fn show(
        &mut self,
        ui: &Ui,
        cards: &[CardPaint],
        layout: &HostLayout,
        source: Option<&InstanceId>,
        landing: Option<&HostPreview>,
        reduced_motion: bool,
    ) -> Option<Rect> {
        let now = ui.input(|input| input.time);
        let duration = solidcraft_theme::panel_motion_duration(ui.ctx(), reduced_motion);
        let clip = ui.clip_rect().intersect(egui_rect(layout.capacity));
        let mut moving = false;
        let mut highlight_bounds = None;
        let receiver = landing.and_then(|preview| match &preview.destination {
            Some(Destination::JoinTab { section, .. }) => layout.sections.iter().find(|card| card.section == *section).map(|card| &card.instance),
            _ => None,
        });

        for card in cards {
            let preview = source == Some(&card.instance) && receiver != Some(&card.instance);
            let target = (!preview || landing.is_some())
                .then(|| layout.sections.iter().find(|section| section.instance == card.instance).map(|section| egui_rect(section.outer)))
                .flatten();
            let Some(target) = target else {
                if preview {
                    self.slides.remove(&card.instance);
                }
                replace_shapes(ui.ctx(), card, None, clip, 1.0);
                continue;
            };
            // The title follows the pointer between destinations. Once there is
            // a landing, reveal the preview there instead of flashing the old dock.
            let initial = if preview { target } else { card.committed_bounds };
            let slide = self.slides.entry(card.instance.clone()).or_insert(Slide { from: initial, to: initial, started: now });
            let (current, _) = slide.current(now, duration);
            if slide.to != target {
                *slide = Slide { from: current, to: target, started: now };
            }
            let (bounds, animated) = slide.current(now, duration);
            moving |= animated;
            if receiver.or(source) == Some(&card.instance) {
                highlight_bounds = Some(bounds);
            }
            if bounds != card.bounds || preview {
                replace_shapes(ui.ctx(), card, Some(bounds), clip, if preview { 0.68 } else { 1.0 });
            }
        }
        if moving {
            ui.ctx().request_repaint();
        } else if source.is_none() {
            self.clear();
        }
        highlight_bounds
    }
}

fn replace_shapes(ctx: &Context, card: &CardPaint, bounds: Option<Rect>, clip: Rect, opacity: f32) {
    ctx.graphics_mut(|graphics| {
        let Some(list) = graphics.get_mut(card.layer) else { return };
        let shapes: Vec<_> =
            if bounds.is_some() { list.all_entries().skip(card.start).take(card.end - card.start).cloned().collect() } else { Vec::new() };
        for index in card.start..card.end {
            list.set(ShapeIdx(index), Rect::NOTHING, Shape::Noop);
        }
        let Some(bounds) = bounds else { return };
        let visible = bounds.intersect(clip);
        if !visible.is_positive() {
            return;
        }
        let offset = bounds.min - card.bounds.min;
        for ClippedShape { clip_rect, mut shape, .. } in shapes {
            let old = clip_rect.intersect(card.bounds);
            let translated = if relocate_shape(&mut shape, card.bounds, bounds) { visible } else { old.translate(offset).intersect(visible) };
            if !translated.is_positive() {
                continue;
            }
            if opacity < 1.0 {
                egui::epaint::shape_transform::adjust_colors(&mut shape, move |color| {
                    if *color != egui::Color32::PLACEHOLDER {
                        *color = color.gamma_multiply(opacity);
                    }
                });
            }
            list.add(translated, shape);
        }
    });
}

// The captured frame already carries the correct fill, stroke, radius
// and transparency. Resize that rectangle instead of painting a second frame.
fn relocate_shape(shape: &mut Shape, original: Rect, bounds: Rect) -> bool {
    if let Shape::Vec(shapes) = shape {
        return shapes.iter_mut().fold(false, |resized, shape| relocate_shape(shape, original, bounds) | resized);
    }
    let inset = Tokens::get().space_4;
    if let Shape::Rect(frame) = shape
        && (frame.rect.min - original.min).abs().max_elem() <= inset
        && (frame.rect.max - original.max).abs().max_elem() <= inset
    {
        frame.rect = Rect::from_min_max(bounds.min + (frame.rect.min - original.min), bounds.max + (frame.rect.max - original.max));
        return true;
    }
    shape.translate(bounds.min - original.min);
    false
}

pub(super) fn paint_unmounted(ui: &Ui, layout: &HostLayout, section: &super::HostSection, title: &str, dragged: bool) {
    let bounds = egui_rect(section.outer);
    let clip = bounds.intersect(egui_rect(layout.capacity)).intersect(ui.clip_rect());
    if !clip.is_positive() {
        return;
    }
    let mut painter = ui.painter().with_clip_rect(clip);
    if dragged {
        painter.multiply_opacity(0.68);
    }
    let tokens = Tokens::get();
    let frame = crate::ui_kit::panel_host_frame(ui.ctx(), section.zone == solidcraft_engine::panels::Zone::Floating);
    crate::ui_kit::Container::paint_frame(&painter, frame, bounds);
    let header = egui_rect(section.header);
    let title = painter.layout_no_wrap(title.to_owned(), egui::FontId::proportional(tokens.font_heading), tokens.form_ink);
    let x = if layout.direction == solidcraft_engine::panels::Direction::Rtl {
        header.right() - tokens.space_8 - title.size().x
    } else {
        header.left() + tokens.space_8
    };
    painter.with_clip_rect(header.intersect(clip)).galley(egui::pos2(x, header.center().y - title.size().y / 2.0), title, tokens.form_ink);
}
