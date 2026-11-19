//! Window-scoped presentation of completed sketch edits; orchestration owns event delivery.
use std::collections::BTreeMap;

use egui::{Rect, Ui};
use solidcraft_engine::SketchFinishEvent;
use solidcraft_views::WindowId;

use crate::ui_kit::toast::{self, NoticeLifetime};

struct Notice {
    event: SketchFinishEvent,
    lifetime: NoticeLifetime,
    link: Option<(egui::Id, egui::ViewportId, u64)>,
}

#[derive(Default)]
pub(crate) struct State {
    notices: BTreeMap<(u64, WindowId), Notice>,
}

impl State {
    /// Consume each engine event once, routing to the window captured when the sketch ended.
    pub(crate) fn enqueue(&mut self, document: u64, window: WindowId, event: SketchFinishEvent) {
        self.notices.insert((document, window), Notice { event, lifetime: NoticeLifetime::new(6.0), link: None });
    }

    /// Enter this window's last-painted notice from the canvas; focused widgets retain
    /// their ordinary Tab order. No modal, stale paint or foreign viewport can take focus.
    pub(crate) fn focus_shortcut(&self, ctx: &egui::Context, document: u64, window: WindowId) -> bool {
        let Some(notice) = self.notices.get(&(document, window)) else { return false };
        let Some((link, viewport, painted)) = notice.link else { return false };
        if viewport != ctx.viewport_id()
            || ctx.cumulative_frame_nr() > painted.saturating_add(1)
            || notice.lifetime.remaining() <= 0.0
            || !ctx.input(|input| input.focused)
            || ctx.memory(|memory| memory.focused().is_some())
            || crate::ui_kit::input_blocked()
            || crate::ui_kit::Container::modal_input_blocked(ctx)
        {
            return false;
        }
        if !ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
            return false;
        }
        ctx.memory_mut(|memory| {
            memory.move_focus(egui::FocusDirection::None);
            memory.request_focus(link);
        });
        true
    }

    /// A new actual edit dismisses the design's previous completion notice in every window.
    pub(crate) fn dismiss(&mut self, document: u64) {
        self.notices.retain(|(uid, _), _| *uid != document);
    }

    pub(crate) fn close_window(&mut self, document: u64, window: WindowId) {
        self.notices.remove(&(document, window));
    }

    pub(crate) fn has_for(&self, document: u64, window: WindowId) -> bool {
        self.notices.contains_key(&(document, window))
    }

    #[cfg(test)]
    pub(crate) fn latest_for(&self, document: u64, window: WindowId) -> Option<&SketchFinishEvent> {
        self.notices.get(&(document, window)).map(|notice| &notice.event)
    }

    /// Call once in the owning window's overlay, never from an individual tile.
    /// Returning the original event requests reopening through the existing sketch-edit path.
    pub(crate) fn show(&mut self, ui: &Ui, free: Rect, obstacles: &[Rect], document: u64, window: WindowId) -> Option<SketchFinishEvent> {
        let notice = self.notices.get_mut(&(document, window))?;
        let message = solidcraft_ui_text::format("sketch_finish.finished", &[("name", &notice.event.name)]);
        let action = solidcraft_ui_text::tr("sketch_finish.edit");
        let t = solidcraft_theme::Tokens::get();
        let size = crate::ui_kit::measure_inline_notice(ui, &message, action, free.width());
        let free = notice_band(free, size, obstacles, t.space_8);
        let id = ui.scope_id().with(("sketch_finish", document, window, notice.event.sequence));
        let shown = toast::inline_notice(ui, free, id, &message, action)?;
        notice.link = Some((id.with("link"), ui.ctx().viewport_id(), ui.ctx().cumulative_frame_nr()));
        crate::scenario::publish_rect(&format!("sketch:finished:{document}:{window}"), shown.rect);
        crate::scenario::publish_target(&format!("sketch:edit_finished:{document}:{window}"), shown.link);
        let expired = notice.lifetime.advance(ui.input(|input| input.time), shown.held);
        if shown.clicked {
            return self.notices.remove(&(document, window)).map(|notice| notice.event);
        }
        if expired {
            self.notices.remove(&(document, window));
            ui.ctx().request_repaint();
        } else if !shown.held {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(notice.lifetime.remaining().clamp(0.01, 6.0)));
        }
        None
    }
}

/// Test the compact notice itself before allowing distant panels to displace its centre.
fn notice_band(safe: Rect, size: egui::Vec2, obstacles: &[Rect], gap: f32) -> Rect {
    if !safe.is_finite() || safe.height() < size.y || size.x <= 0.0 || size.y <= 0.0 {
        return Rect::NOTHING;
    }
    let mut bottoms = vec![safe.bottom()];
    bottoms.extend(obstacles.iter().take(32).map(|obstacle| obstacle.top() - gap).filter(|bottom| bottom.is_finite() && *bottom <= safe.bottom()));
    bottoms.sort_by(|a, b| b.total_cmp(a));
    bottoms.dedup();
    for bottom in bottoms {
        if bottom - size.y < safe.top() {
            continue;
        }
        let band = Rect::from_min_max(egui::pos2(safe.left(), bottom - size.y), egui::pos2(safe.right(), bottom));
        let preferred = Rect::from_center_size(band.center(), size);
        if obstacles.iter().all(|obstacle| !obstacle.intersects(preferred)) {
            return band;
        }
        let free = crate::viewport_layout::free_rect(band, obstacles);
        if free.height() >= size.y && free.width() >= 80.0 {
            return free;
        }
    }
    Rect::NOTHING
}
