//! Assistant entry point and its status mark, shared by application chrome and previews.
use crate::{icons, theme::Tokens};
use egui::{FontId, Rect, Response, Stroke, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssistantEntryState {
    Quiet,
    Ready,
    Busy,
    NeedsAuth,
    Stopped,
    Waiting(usize),
}

/// Paint the compact entry point in its caller-owned target. The caller owns state and actions.
/// Tokens: app_bar, app_bar_hover_button/strong, agent_ok/busy, warning/error, radius_sm, font_badge.
pub(crate) fn assistant_entry(ui: &egui::Ui, response: &Response, state: AssistantEntryState, open: bool, animate: bool) {
    let t = Tokens::get();
    let at = response.rect;
    let mark = state;
    // The highlight is a 26 px square, like the other icon buttons; the mark sits on the bubble's corner.
    let mut ring = t.app_bar;
    if open || response.hovered() || response.has_focus() {
        let fill = if open { t.app_bar_hover_strong } else { t.app_bar_hover_button };
        ui.painter().rect_filled(at.shrink(t.space_3), t.radius_sm, fill);
        ring = t.app_bar.blend(fill);
    }
    icons::paint_app_bar(ui.painter(), at.shrink(t.space_6 + 1.0), "assistant", 1.0);
    let c = egui::pos2(at.right() - t.space_8 - 0.5, at.top() + t.space_10 - 0.5);
    match mark {
        AssistantEntryState::Quiet => {}
        AssistantEntryState::Ready | AssistantEntryState::NeedsAuth | AssistantEntryState::Stopped => {
            let color = match mark {
                AssistantEntryState::Ready => t.agent_ok,
                AssistantEntryState::NeedsAuth => t.warning,
                _ => t.error,
            };
            ui.painter().circle_filled(c, 5.5, ring);
            ui.painter().circle_filled(c, 4.0, color);
        }
        AssistantEntryState::Busy => {
            let phase = if animate {
                ui.ctx().request_repaint();
                ui.input(|i| i.time) as f32 * 3.0
            } else {
                0.0
            };
            ui.painter().circle_filled(c, 6.0, ring);
            let points = (0..=24)
                .map(|i| {
                    let angle = phase + i as f32 / 24.0 * std::f32::consts::TAU * 0.75;
                    c + vec2(angle.cos(), angle.sin()) * 4.0
                })
                .collect();
            ui.painter().add(egui::Shape::line(points, Stroke::new(1.8, t.agent_busy)));
        }
        AssistantEntryState::Waiting(count) => {
            let text = count.to_string();
            let font = FontId::proportional(t.font_badge);
            let galley = ui.painter().layout_no_wrap(text, font, t.app_bar);
            let badge = Rect::from_center_size(c, vec2((galley.size().x + t.space_6).max(t.space_14), t.space_14));
            ui.painter().rect_filled(badge.expand(1.5), 8.5, ring);
            ui.painter().rect_filled(badge, 7.0, t.warning);
            ui.painter().galley(badge.center() - galley.size() / 2.0, galley, t.app_bar);
        }
    }
}
