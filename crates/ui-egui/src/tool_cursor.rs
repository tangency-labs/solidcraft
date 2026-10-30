//! The tool at the cursor: while an interactive tool or a command's dialog is running, its
//! toolbar icon rides above and right of the pointer in the viewport (clear of the sketch tools'
//! coordinate readout below it), so the user sees which command the
//! next click goes to without looking up at the ribbon. It shows only while the pointer is over
//! the model itself (not over a panel, a dialog or a menu) and hides while the camera moves.
//! Off by default; View settings ("Show tool at cursor") or `ui.set {"toolCursor": true}` turn
//! it on.
//! It is UI only: drawing it runs no command and changes nothing in the design.

use egui::{Pos2, Rect, Stroke, Vec2, pos2, vec2};

use crate::SolveApp;
use crate::theme::Tokens;

/// The badge's side and the icon inside it, in points.
const BADGE: f32 = 26.0;
const ICON: f32 = 20.0;
/// The badge sits above and right of the pointer's hot spot: the system arrow hangs below and
/// right of it, and so does the sketch tools' coordinate readout (`tools::preview`, drawn from
/// [`READOUT`] below right). From the hot spot to the badge's bottom-left corner:
const OFFSET: Vec2 = vec2(12.0, -8.0);
/// Where the sketch tools' coordinate readout starts, down and right of the hot spot: right of
/// the system arrow, which hangs below it about 12 points wide at the default size and about 18
/// at the next larger one, and would cover the first value (#9480).
pub(crate) const READOUT: Vec2 = vec2(26.0, 10.0);
/// The gap to the hot spot when the badge flips to the left of the pointer.
const FLIPPED_GAP: f32 = 6.0;

/// Where the coordinate readout of a sketch tool's point at `point` (screen) starts: [`READOUT`]
/// from the pointer while it is over `view`, else from the point. The point snaps away from the
/// pointer, so a readout placed from it could still slide under the arrow.
pub(crate) fn readout_at(painter: &egui::Painter, view: Rect, point: Pos2) -> Pos2 {
    painter.ctx().pointer_hover_pos().filter(|p| view.contains(*p)).unwrap_or(point) + READOUT
}

/// The command whose tool or dialog is running: the one a click in the viewport goes to.
pub(crate) fn command(app: &SolveApp) -> Option<&'static str> {
    match (&app.tool, &app.dialog) {
        (Some(tool), _) => Some(tool.cmd),
        (None, Some(dialog)) => dialog.command(),
        _ => None,
    }
}

/// The icon the badge shows: None when it is turned off, nothing runs, or the command has no icon.
pub(crate) fn icon(app: &SolveApp) -> Option<&'static str> {
    if !app.ui.tool_cursor {
        return None;
    }
    command(app).and_then(solidcraft_engine::find_command).map(|spec| spec.icon).filter(|icon| !icon.is_empty())
}

/// The badge for a pointer at `at`: above and right of it, so it never covers the coordinate
/// readout below. Near the right edge of `bounds` it moves to the left of the pointer; with no
/// room above, it drops below the readout's line (`readout_height` high), never onto it.
fn badge_rect(at: Pos2, bounds: Rect, readout_height: f32) -> Rect {
    let mut min = pos2(at.x + OFFSET.x, at.y + OFFSET.y - BADGE);
    if min.x + BADGE > bounds.right() {
        min.x = at.x - FLIPPED_GAP - BADGE;
    }
    if min.y < bounds.top() {
        min.y = at.y + READOUT.y + readout_height + FLIPPED_GAP;
    }
    Rect::from_min_size(min, vec2(BADGE, BADGE))
}

/// Draw the badge for a pointer at `at` (None: the pointer is not over the model).
pub(crate) fn show(app: &SolveApp, painter: &egui::Painter, at: Option<Pos2>) {
    let (Some(at), Some(icon)) = (at, icon(app)) else { return };
    let t = Tokens::get();
    // The readout's line height, as `tools::preview` lays it out.
    let readout = painter.layout_no_wrap("0".into(), egui::FontId::proportional(t.font_small), t.text_dim).size().y;
    let r = badge_rect(at, painter.clip_rect(), readout);
    painter.rect(r, t.radius_sm, t.panel.gamma_multiply(0.92), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    crate::icons::paint_icon(painter, Rect::from_center_size(r.center(), vec2(ICON, ICON)), icon, 1.0);
    crate::scenario::publish_rect("viewport:tool_cursor", r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, rect_of};
    use serde_json::json;

    /// Rest the pointer on the middle of the viewport; the badge drawn there, if any.
    fn hover_middle(h: &mut Harness) -> (Pos2, Option<Rect>) {
        let c = h.app.viewport.rect.map(|r| r.center()).unwrap();
        h.step(&json!({"move": [c.x, c.y]})).unwrap();
        h.frames(2);
        (c, rect_of("viewport:tool_cursor"))
    }

    fn icon_of(id: &str) -> &'static str {
        solidcraft_engine::find_command(id).unwrap().icon
    }

    #[test]
    fn navigation_preference_restores_tool_badge_on_cancel_and_defaults() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"open":"Navigation"}));
        h.frames(4);
        crate::prefs::reveal_preference(&mut h, "view:tool_cursor");
        h.step(&json!({"click":{"handle":"view:tool_cursor"}})).unwrap();
        assert!(h.app.ui.tool_cursor);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.ui.tool_cursor);
        h.app.ui.tool_cursor = true;
        h.call("ui.prefs", json!({"open":"Navigation"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.ui.tool_cursor);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.ui.tool_cursor, "Cancel also restores the value from before Defaults");
    }

    #[test]
    fn badge_shows_the_running_tool_beside_the_pointer() {
        let mut h = Harness::new();
        h.settle();
        // Off by default: a running tool shows no badge until Navigation preferences turn it on.
        assert!(!h.app.ui.tool_cursor, "the badge is on by default");
        h.step(&json!({"start": "sketch.create"})).unwrap();
        let (_, badge) = hover_middle(&mut h);
        assert!(badge.is_none(), "off by default, yet a badge is drawn at {badge:?}");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        // The board's Display menu omits this preference; Navigation keeps its checkbox.
        h.call("ui.prefs", json!({"open":"Navigation"}));
        h.frames(4);
        crate::prefs::reveal_preference(&mut h, "view:tool_cursor");
        h.step(&json!({"click": {"handle": "view:tool_cursor"}})).unwrap();
        h.call("ui.prefs", json!({"button":"ok"}));
        h.frames(2);
        assert!(h.app.ui.tool_cursor, "the Navigation preferences checkbox did not turn the badge on");
        let (_, badge) = hover_middle(&mut h);
        assert!(badge.is_none(), "no tool runs, yet a badge is drawn at {badge:?}");

        // A dialog: Create Sketch waits for a plane.
        h.step(&json!({"start": "sketch.create"})).unwrap();
        let (at, badge) = hover_middle(&mut h);
        assert_eq!(icon(&h.app), Some(icon_of("sketch.create")));
        let badge = badge.unwrap();
        assert_eq!(badge.left_bottom(), at + OFFSET, "the badge sits above and right of the pointer");

        // An interactive sketch tool.
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.settle();
        h.step(&json!({"start": "sketch.circle.center"})).unwrap();
        let (at, badge) = hover_middle(&mut h);
        assert_eq!(icon(&h.app), Some(icon_of("sketch.circle.center")));
        let badge = badge.unwrap();
        // The coordinate readout starts READOUT below and right of the pointer: no overlap.
        assert!(badge.bottom() < at.y + READOUT.y, "the badge {badge:?} covers the readout under {at:?}");

        // Turned off in Navigation preferences.
        h.app.ui.tool_cursor = false;
        let (_, badge) = hover_middle(&mut h);
        assert!(badge.is_none(), "turned off, yet a badge is drawn at {badge:?}");
        h.app.ui.tool_cursor = true;

        // Over the window's top edge, outside the viewport: nothing.
        h.step(&json!({"move": [4.0, 4.0]})).unwrap();
        h.frames(2);
        assert!(rect_of("viewport:tool_cursor").is_none(), "a badge is drawn outside the viewport");

        // Escape ends the tool.
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(h.app.tool.is_none() && h.app.dialog.is_none(), "Escape left {:?} running", command(&h.app));
        let (_, badge) = hover_middle(&mut h);
        assert!(badge.is_none(), "the tool ended, yet a badge is drawn at {badge:?}");
    }

    /// Wherever the pointer is, the badge stays inside the viewport and off the coordinate
    /// readout (the rectangle from READOUT below right of the pointer, one line high).
    #[test]
    fn badge_stays_inside_the_viewport_and_off_the_readout() {
        let bounds = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let line = 14.0;
        let middle = Pos2::new(400.0, 300.0);
        assert_eq!(badge_rect(middle, bounds, line).left_bottom(), middle + OFFSET);
        for at in [middle, pos2(795.0, 300.0), pos2(400.0, 5.0), pos2(795.0, 5.0), pos2(5.0, 595.0), pos2(795.0, 595.0)] {
            let badge = badge_rect(at, bounds, line);
            assert!(bounds.contains_rect(badge), "at {at:?}: {badge:?} leaves {bounds:?}");
            let readout = Rect::from_min_size(at + READOUT, vec2(120.0, line));
            assert!(!badge.intersects(readout), "at {at:?}: {badge:?} covers the readout {readout:?}");
            assert!(!badge.contains(at), "at {at:?}: {badge:?} covers the hot spot");
        }
    }

    /// A line's coordinate readout starts right of the system arrow, which hangs below and
    /// right of the hot spot about 12 × 19 points at the default size and 18 × 28 at the next
    /// larger one, wherever the point snaps (#9480).
    #[test]
    fn the_coordinate_readout_starts_right_of_the_pointer_arrow() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        h.step(&json!({"start": "sketch.line"})).unwrap();
        let c = h.app.viewport.rect.map(|r| r.center()).unwrap();
        // Off a grid point, so the point snaps away from the pointer.
        for at in [c, c + vec2(5.0, 3.0), c + vec2(-4.0, 6.0)] {
            h.step(&json!({"move": [at.x, at.y]})).unwrap();
            h.frames(2);
            let readout = rect_of("viewport:readout").expect("the readout is drawn");
            let arrow = Rect::from_min_size(at, vec2(19.0, 29.0));
            assert!(!readout.intersects(arrow), "at {at:?}: the readout {readout:?} is under the arrow {arrow:?}");
            assert!(readout.top() > at.y, "at {at:?}: the readout {readout:?} is not below the hot spot");
        }
        h.step(&json!({"key": "Escape"})).unwrap();
    }

    /// Every interactive tool's command has an icon to show.
    #[test]
    fn every_interactive_tool_has_a_badge_icon() {
        let missing: Vec<&str> = solidcraft_engine::command_specs()
            .iter()
            .filter_map(|spec| crate::tools::Tool::for_command(spec.id).map(|tool| tool.cmd))
            .filter(|cmd| solidcraft_engine::find_command(cmd).is_none_or(|spec| spec.icon.is_empty()))
            .collect();
        assert!(missing.is_empty(), "tools without a badge icon: {missing:?}");
    }
}
