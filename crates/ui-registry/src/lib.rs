//! The widget registry: names a widget and records where it was drawn this frame, for UI tests,
//! scenarios and the control channel (which find widgets by name, not by pixel).
//!
//! Components publish handles (points to click or drag), rects and counts while they draw; the
//! frame loop calls [`clear_handles`] as each frame starts. The registry holds no app types, so
//! any UI component can publish without depending on the app. It is per thread, and lookups
//! search from the end: when a name is published more than once in a frame, the latest wins.

use std::cell::RefCell;

thread_local! {
    /// Drag handles drawn this frame (name, screen point): manipulators publish theirs so
    /// scenarios (and the control channel's `ui.at {handle}`) can grab them.
    static HANDLES: RefCell<Vec<(String, egui::Pos2)>> = const { RefCell::new(Vec::new()) };
    /// Painted parts with no widget of their own (the toolbar, the workspace switcher…): rects
    /// and counts by name, for checks.
    static RECTS: RefCell<Vec<(String, egui::Rect)>> = const { RefCell::new(Vec::new()) };
    static COUNTS: RefCell<Vec<(String, f64)>> = const { RefCell::new(Vec::new()) };
}

/// A manipulator's handle drawn this frame at `at` (screen), by name: "arrow" (a dialog's value
/// arrow), "triad_x"… Call it from the code that draws the handle, every frame it shows.
pub fn publish_handle(name: &str, at: egui::Pos2) {
    // The latest wins (lookups search from the end), so publishing is cheap for browser rows.
    HANDLES.with(|h| h.borrow_mut().push((name.to_string(), at)));
}

/// Publish a rectangle and its centered interaction handle with the same explicit identity.
pub fn publish_target(name: &str, rect: egui::Rect) {
    publish_target_as(name, name, rect);
}

/// Publish centered geometry with explicit, independently named handle and rectangle aliases.
pub fn publish_target_as(handle: &str, rect_name: &str, rect: egui::Rect) {
    publish_handle(handle, rect.center());
    publish_rect(rect_name, rect);
}

/// Where a handle was drawn last frame.
pub fn handle_at(name: &str) -> Option<egui::Pos2> {
    HANDLES.with(|h| h.borrow().iter().rfind(|(n, _)| n == name).map(|x| x.1))
}

/// Every handle drawn last frame whose name starts with `prefix`.
pub fn handles(prefix: &str) -> Vec<(String, egui::Pos2)> {
    HANDLES.with(|h| h.borrow().iter().filter(|(n, _)| n.starts_with(prefix)).cloned().collect())
}

/// A painted part's rect this frame, by name ("toolbar", "workspace_switcher").
pub fn publish_rect(name: &str, r: egui::Rect) {
    RECTS.with(|h| h.borrow_mut().push((name.to_string(), r)));
}

/// A painted part's rect, by name (the latest published).
pub fn rect_of(name: &str) -> Option<egui::Rect> {
    RECTS.with(|h| h.borrow().iter().rfind(|(n, _)| n == name).map(|x| x.1))
}

/// Every rect published this frame whose name starts with `prefix`, in publish order (for checks).
pub fn rects(prefix: &str) -> Vec<(String, egui::Rect)> {
    RECTS.with(|r| r.borrow().iter().filter(|(name, _)| name.starts_with(prefix)).cloned().collect())
}

/// A number about this frame's drawing, by name ("axis_view_highlights": the axis view's highlighted tips).
pub fn publish_count(name: &str, v: f64) {
    COUNTS.with(|h| h.borrow_mut().push((name.to_string(), v)));
}

/// A published number, by name (the latest published).
pub fn count_of(name: &str) -> Option<f64> {
    COUNTS.with(|h| h.borrow().iter().rfind(|(n, _)| n == name).map(|x| x.1))
}

/// Publish and query another viewport's widgets without exposing their local
/// coordinates to the root viewport's scenario and control-channel lookups.
pub fn isolated<R>(paint: impl FnOnce() -> R) -> R {
    struct Restore<T: 'static>(&'static std::thread::LocalKey<RefCell<Vec<T>>>, Vec<T>);
    impl<T> Drop for Restore<T> {
        fn drop(&mut self) {
            self.0.with(|items| items.replace(std::mem::take(&mut self.1)));
        }
    }
    let _handles = Restore(&HANDLES, HANDLES.with(RefCell::take));
    let _rects = Restore(&RECTS, RECTS.with(RefCell::take));
    let _counts = Restore(&COUNTS, COUNTS.with(RefCell::take));
    paint()
}

/// Forget last frame's handles, rects and counts (called as a frame starts).
pub fn clear_handles() {
    HANDLES.with(|h| h.borrow_mut().clear());
    RECTS.with(|h| h.borrow_mut().clear());
    COUNTS.with(|h| h.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2};

    fn square(x: f32) -> Rect {
        Rect::from_min_max(pos2(x, 0.0), pos2(x + 10.0, 10.0))
    }

    #[test]
    fn published_names_are_found() {
        clear_handles();
        publish_handle("arrow", pos2(1.0, 2.0));
        publish_rect("toolbar", square(0.0));
        publish_count("tips", 3.0);
        assert_eq!(handle_at("arrow"), Some(pos2(1.0, 2.0)));
        assert_eq!(rect_of("toolbar"), Some(square(0.0)));
        assert_eq!(count_of("tips"), Some(3.0));
        assert_eq!(handle_at("missing"), None);
        assert_eq!(rect_of("missing"), None);
        assert_eq!(count_of("missing"), None);
    }

    #[test]
    fn targets_publish_a_rect_and_its_centre() {
        clear_handles();
        publish_target("ok", square(20.0));
        assert_eq!(rect_of("ok"), Some(square(20.0)));
        assert_eq!(handle_at("ok"), Some(square(20.0).center()));
        publish_target_as("grip", "frame", square(40.0));
        assert_eq!(handle_at("grip"), Some(square(40.0).center()));
        assert_eq!(rect_of("frame"), Some(square(40.0)));
        assert_eq!(rect_of("grip"), None);
    }

    #[test]
    fn the_latest_publish_wins() {
        clear_handles();
        publish_handle("row", pos2(1.0, 1.0));
        publish_handle("row", pos2(2.0, 2.0));
        publish_rect("panel", square(0.0));
        publish_rect("panel", square(5.0));
        publish_count("n", 1.0);
        publish_count("n", 2.0);
        assert_eq!(handle_at("row"), Some(pos2(2.0, 2.0)));
        assert_eq!(rect_of("panel"), Some(square(5.0)));
        assert_eq!(count_of("n"), Some(2.0));
    }

    #[test]
    fn prefix_lookups_filter_by_name() {
        clear_handles();
        publish_handle("row:Bodies", pos2(1.0, 1.0));
        publish_handle("fold:Bodies", pos2(2.0, 2.0));
        publish_handle("row:Sketches", pos2(3.0, 3.0));
        publish_rect("dialog:a", square(0.0));
        publish_rect("menu:b", square(1.0));
        publish_rect("dialog:c", square(2.0));
        let rows: Vec<String> = handles("row:").into_iter().map(|(n, _)| n).collect();
        assert_eq!(rows, ["row:Bodies", "row:Sketches"]);
        assert_eq!(handles("").len(), 3);
        assert_eq!(rects("dialog:"), [("dialog:a".to_string(), square(0.0)), ("dialog:c".to_string(), square(2.0))]);
        assert!(rects("none:").is_empty());
    }

    #[test]
    fn clearing_empties_handles_rects_and_counts() {
        publish_handle("h", pos2(0.0, 0.0));
        publish_rect("r", square(0.0));
        publish_count("c", 1.0);
        clear_handles();
        assert!(handles("").is_empty());
        assert!(rects("").is_empty());
        assert_eq!(count_of("c"), None);
    }
}
