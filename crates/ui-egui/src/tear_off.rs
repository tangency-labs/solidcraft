//! Moving a tool card out of the app into a window of its own, and back (desktop only).
//!
//! A card leaves the app when the user drags its title past the app's border: [`crosses_border`]
//! decides from the position the card would have, not the one it is clamped to. The new window
//! opens hidden and follows the pointer while the main window still holds the press
//! ([`TearOff`], [`follow_pointer`]); it is shown at the point the title was grabbed. Dragging the
//! window's title back over the app and pausing puts the card back ([`NativeDockDrag`]).
//! The Parameters window is the one card that uses this (#5097).

use egui::{Context, Id, Pos2, Rect, Vec2};

/// Watches a separate window's frame for a title drag back over the app. The system moves the
/// window itself, so no pointer events reach egui: only the window's place changes.
#[derive(Clone, Default)]
pub(crate) struct NativeDockDrag {
    outer: Option<Rect>,
    parent: Option<Rect>,
    last_motion: f64,
    /// The window moved recently; keep repainting to see where it stops.
    pub(crate) moving: bool,
    armed: bool,
}

impl NativeDockDrag {
    /// Feed the window's outer and inner rectangles, the main window's inner rectangle and the
    /// time; true when the window should go back into the app. Opening jitter, resizing the window
    /// and moving the main window never count: only a deliberate move that ends over the app.
    pub(crate) fn update(&mut self, outer: Rect, inner: Rect, parent: Rect, now: f64) -> bool {
        if ![outer, inner, parent].iter().all(|r| r.is_finite() && r.is_positive()) || !now.is_finite() {
            *self = Self::default();
            return false;
        }
        let previous = self.outer.replace(outer);
        let old_parent = self.parent.replace(parent);
        let Some(previous) = previous else {
            self.last_motion = now;
            return false;
        };
        let translated = (outer.min - previous.min).length() > 0.5;
        let resized = (outer.size() - previous.size()).length() > 1.0;
        if resized || (!translated && old_parent != Some(parent)) {
            self.moving = false;
            self.armed = false;
            self.last_motion = now;
            return false;
        }
        // A useful part of the title bar must be over the app, not a single overlapping pixel.
        let target = parent.shrink(solidcraft_theme::Tokens::get().space_16);
        let title_y = outer.top() + (inner.top() - outer.top()).max(crate::ui_kit::header_height()) * 0.5;
        let overlap = (outer.right().min(target.right()) - outer.left().max(target.left())).max(0.0);
        let over_app = target.y_range().contains(title_y) && overlap >= outer.width().min(160.0);
        if translated {
            // A new drag after the window settled, or a trip outside the app, arms docking, so a
            // window that has just left the app never snaps straight back.
            self.armed |= !over_app || now - self.last_motion >= 0.4;
            self.moving = true;
            self.last_motion = now;
        }
        if self.moving && (!over_app || !self.armed) && now - self.last_motion >= 0.4 {
            self.moving = false;
        }
        self.moving && self.armed && over_app && now - self.last_motion >= 0.18
    }
}

/// A card being handed to a window of its own while the main window still holds the press.
/// The window keeps the point where the title was grabbed under the pointer. Starting a second
/// system drag after the focus changes can use another anchor and make the window jump.
#[derive(Clone)]
pub(crate) struct TearOff {
    inner_origin: Pos2,
    grab: Vec2,
    tracking: bool,
    shown: bool,
}

impl TearOff {
    /// `inner_origin` is the new window's content corner on the screen; `grab` is where the title
    /// was grabbed, from the card's corner; `tracking` is whether the button is still down.
    pub(crate) fn new(inner_origin: Pos2, grab: Vec2, tracking: bool) -> Self {
        Self { inner_origin, grab, tracking, shown: false }
    }

    /// Open the window hidden and inactive until its first placement.
    pub(crate) fn prepare(&self, builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
        if self.shown { builder } else { builder.with_active(false).with_visible(false) }
    }

    /// The window's outer position for this frame, whether to show it now, and whether the
    /// hand-off has ended (the button is up).
    fn placement(&mut self, pointer: Option<Pos2>, parent_origin: Pos2, down: bool, frame_offset: Vec2) -> (Pos2, bool, bool) {
        if self.tracking
            && let Some(pointer) = pointer.filter(|p| p.is_finite())
        {
            self.inner_origin = parent_origin + pointer.to_vec2() - self.grab;
        }
        let first = !self.shown;
        self.shown = true;
        (self.inner_origin - frame_offset, first, !self.tracking || !down)
    }
}

/// Move the window being handed off to follow the main window's pointer. Call it inside that
/// window's viewport with its outer and inner rectangles; returns where it was put, or `None`
/// when no hand-off is running. Ends the hand-off when the button is released.
pub(crate) fn follow_pointer(ctx: &Context, handoff: &mut Option<TearOff>, outer: Option<Rect>, inner: Option<Rect>) -> Option<Pos2> {
    let drag = handoff.as_mut()?;
    let (origin, pointer, down) = ctx.input_for(egui::ViewportId::ROOT, |i| {
        (i.viewport().inner_rect.map_or(Pos2::ZERO, |r| r.min), i.pointer.interact_pos(), i.pointer.primary_down())
    });
    let offset = outer.zip(inner).map_or(Vec2::ZERO, |(o, n)| n.min - o.min);
    let (position, show, finished) = drag.placement(pointer, origin, down, offset);
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
    if show {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }
    if finished {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        *handoff = None;
        end_handoff(ctx);
    }
    ctx.request_repaint_of(egui::ViewportId::ROOT);
    Some(position)
}

/// Whether a title drag has taken the card `desired` (where the pointer would put it, unclamped)
/// past the app's border `parent`. On the sides and the bottom, a useful part of the card must be
/// outside and the pointer near the edge, so a small nudge keeps the card in the app. The top
/// hands off while the pointer is still inside, before the system stops it at the app's edge.
pub(crate) fn crosses_border(parent: Rect, desired: Rect, pointer: Pos2, grab: Vec2) -> bool {
    if !parent.is_finite() || !desired.is_finite() || !pointer.is_finite() {
        return false;
    }
    let horizontal = (desired.width() * 0.18).clamp(48.0, 96.0);
    let bottom = (desired.height() * 0.18).clamp(48.0, 96.0);
    (desired.left() < parent.left() - horizontal && pointer.x < parent.left() + 24.0)
        || (desired.right() > parent.right() + horizontal && pointer.x > parent.right() - 24.0)
        || (desired.bottom() > parent.bottom() + bottom && pointer.y > parent.bottom() - 24.0)
        || (desired.top() < parent.top() && pointer.y < parent.top() + 8.0 && grab.y > 0.0)
}

fn handoff_id() -> Id {
    Id::unique("sc_tear_off_handoff")
}

/// A card has just been handed to a window of its own while the button is still down.
pub(crate) fn mark_handoff(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(handoff_id(), true));
}

pub(crate) fn end_handoff(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(handoff_id(), false));
}

/// Whether the pointer is still carrying a card into its own window: the main window must not
/// treat that press as a drag on the model.
pub(crate) fn handoff_dragging(ctx: &Context) -> bool {
    if !ctx.input(|i| i.pointer.primary_down()) {
        end_handoff(ctx);
        return false;
    }
    ctx.data(|d| d.get_temp::<bool>(handoff_id())).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    /// A window's content area under a 24-point system title bar.
    fn inner(outer: Rect) -> Rect {
        Rect::from_min_max(outer.min + vec2(0.0, 24.0), outer.max)
    }

    #[test]
    fn a_window_dragged_back_over_the_app_from_any_side_docks_after_a_pause() {
        // The main window on the main screen, and on a screen to the left (negative coordinates).
        for origin in [pos2(100.0, 50.0), pos2(-2000.0, 50.0)] {
            let parent = Rect::from_min_size(origin, vec2(1600.0, 1000.0));
            let inside = Rect::from_min_size(origin + vec2(250.0, 180.0), vec2(700.0, 500.0));
            for offset in [vec2(-1000.0, 0.0), vec2(1800.0, 0.0), vec2(0.0, -800.0), vec2(0.0, 1200.0)] {
                let outside = inside.translate(offset);
                let mut drag = NativeDockDrag::default();
                assert!(!drag.update(outside, inner(outside), parent, 0.0));
                let moved = outside.translate(vec2(10.0, 10.0));
                assert!(!drag.update(moved, inner(moved), parent, 0.1));
                assert!(!drag.update(inside, inner(inside), parent, 0.2));
                assert!(!drag.update(inside, inner(inside), parent, 0.3), "it docks only after a pause");
                assert!(drag.update(inside, inner(inside), parent, 0.41), "{origin:?} {offset:?}");
            }
        }
    }

    #[test]
    fn opening_resizing_or_moving_the_main_window_does_not_dock_a_window() {
        let parent = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let outer = Rect::from_min_size(pos2(250.0, 180.0), vec2(700.0, 500.0));
        let mut drag = NativeDockDrag::default();
        assert!(!drag.update(outer, inner(outer), parent, 0.0));
        let jitter = outer.translate(vec2(2.0, 0.0));
        assert!(!drag.update(jitter, inner(jitter), parent, 0.1));
        assert!(!drag.update(jitter, inner(jitter), parent, 0.6));
        assert!(!drag.moving, "the system's placement jitter must stop the repaints");
        let resized = Rect::from_min_max(jitter.min - vec2(40.0, 0.0), jitter.max);
        assert!(!drag.update(resized, inner(resized), parent, 1.0));
        assert!(!drag.update(resized, inner(resized), parent, 1.3));
        let moved_parent = parent.translate(vec2(50.0, 50.0));
        assert!(!drag.update(resized, inner(resized), moved_parent, 1.5));
        let moved = resized.translate(vec2(50.0, 30.0));
        assert!(!drag.update(moved, inner(moved), moved_parent, 2.0));
        assert!(drag.update(moved, inner(moved), moved_parent, 2.2), "a later deliberate title drag over the app does dock it");
    }

    #[test]
    fn a_small_nudge_stays_in_the_app_and_a_pull_past_any_edge_leaves_it() {
        let parent = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1200.0));
        let card = Rect::from_min_size(pos2(600.0, 300.0), vec2(500.0, 400.0));
        let grab = vec2(100.0, 12.0);
        let at = |pointer: Pos2| Rect::from_min_size(pointer - grab, card.size());
        // Pointer near an edge, but the card hardly outside.
        assert!(!crosses_border(parent, at(pos2(1590.0, 312.0)).translate(vec2(-400.0, 0.0)), pos2(1590.0, 312.0), grab));
        assert!(!crosses_border(parent, at(pos2(800.0, 312.0)), pos2(800.0, 312.0), grab));
        for pointer in [pos2(8.0, 312.0), pos2(1592.0, 312.0), pos2(800.0, 1192.0), pos2(800.0, 4.0)] {
            assert!(crosses_border(parent, at(pointer), pointer, grab), "{pointer:?}");
        }
        assert!(!crosses_border(parent, at(pos2(f32::NAN, 0.0)), pos2(f32::NAN, 0.0), grab));
    }

    #[test]
    fn a_handed_off_window_opens_hidden_then_follows_the_grab_point_until_release() {
        let mut drag = TearOff::new(pos2(500.0, 400.0), vec2(30.0, 10.0), true);
        let builder = drag.prepare(egui::ViewportBuilder::default());
        assert_eq!((builder.visible, builder.active), (Some(false), Some(false)));
        let frame = vec2(0.0, 28.0);
        let (position, show, finished) = drag.placement(Some(pos2(700.0, 50.0)), pos2(100.0, 100.0), true, frame);
        assert_eq!(position, pos2(770.0, 112.0));
        assert!(show && !finished);
        assert_eq!(drag.prepare(egui::ViewportBuilder::default()).visible, None, "only the first frame hides it");
        let (_, show, finished) = drag.placement(Some(pos2(710.0, 50.0)), pos2(100.0, 100.0), false, frame);
        assert!(!show && finished);
    }
}
