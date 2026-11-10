//! SolidCraft's window title bar: the shared window chrome ([`crate::window_chrome`]) with the
//! app's own answers. Closing asks about unsaved designs first ([`window_action`]); on Windows and
//! Linux without OS decorations ([`SolveApp::custom_titlebar`]) the application bar is the title
//! bar, and on macOS it runs under the traffic lights ([`SolveApp::integrated_titlebar`]).

use crate::window_chrome::WindowAction;
pub use crate::window_chrome::{Caption, dpi_scaled_size, resize_border_px, resize_hit, resize_zones, toggle_maximize};

use crate::SolveApp;

/// Window actions for the control channel (`ui.window`): minimize, maximize, restore, toggle
/// (maximize or restore), close.
pub fn window_action(app: &mut SolveApp, ctx: &egui::Context, action: &str) -> Result<(), String> {
    run(app, ctx, WindowAction::parse(action)?);
    Ok(())
}

/// Runs a title-bar action as SolidCraft does.
pub(crate) fn run(app: &mut SolveApp, ctx: &egui::Context, action: WindowAction) {
    if action == WindowAction::Close {
        // Unsaved designs are asked about first; the window closes once they are answered.
        crate::documents::request_quit(app);
        if !app.quit_requested {
            return;
        }
    }
    action.send(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_chrome::{BUTTON_WIDTH, CAPTIONS, caption_id};
    use egui::{Event, PointerButton, Pos2, RawInput, Rect, ResizeDirection, ViewportCommand, ViewportId, pos2, vec2};
    use solidcraft_engine::Session;
    use solidcraft_theme::Tokens;

    /// A headless window `width` pt wide showing the application bar (as the title bar) and the
    /// resize zones.
    struct Win {
        ctx: egui::Context,
        app: SolveApp,
        width: f32,
        time: f64,
        maximized: bool,
        /// Lay out the whole window (`SolveApp::ui`) rather than only the bar and the zones.
        full: bool,
    }

    impl Win {
        fn new(width: f32) -> Self {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(Session::default(), Default::default());
            app.custom_titlebar = true;
            Self { ctx, app, width, time: 0.0, maximized: false, full: false }
        }

        /// One frame with `events`, the input going through `release_press_taken_by_os` first
        /// as in the desktop app.
        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 0.05;
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 700.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            if let Some(v) = input.viewports.get_mut(&ViewportId::ROOT) {
                v.maximized = Some(self.maximized);
            }
            // As the desktop app's raw-input hook does first.
            crate::window_chrome::release_press_taken_by_os(&self.ctx, &mut input);
            let (app, full) = (&mut self.app, self.full);
            let mut out = self.ctx.run_ui(input, |ui| {
                if full {
                    app.ui(ui);
                    return;
                }
                crate::toolbar::app_bar(app, ui);
                if app.custom_titlebar {
                    resize_zones(ui);
                }
            });
            out.textures_delta.clear();
            out
        }

        /// Frames for a pointer gesture: move to `path[0]`, press, move along `path`, release.
        /// The viewport commands the frames sent.
        fn gesture(&mut self, path: &[Pos2]) -> Vec<ViewportCommand> {
            let button = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() };
            let mut events = vec![vec![Event::PointerMoved(path[0])], vec![button(path[0], true)]];
            events.extend(path[1..].iter().map(|p| vec![Event::PointerMoved(*p)]));
            events.push(vec![button(*path.last().unwrap_or(&path[0]), false)]);
            events.into_iter().flat_map(|e| self.frame(e).viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()).collect()
        }

        /// A gesture whose release the OS keeps, as an X11 window manager does once a press
        /// started a window move or resize: press, move along `path`, then one quiet frame.
        fn gesture_os_keeps_release(&mut self, path: &[Pos2]) -> Vec<ViewportCommand> {
            let press = Event::PointerButton { pos: path[0], button: PointerButton::Primary, pressed: true, modifiers: Default::default() };
            let mut events = vec![vec![Event::PointerMoved(path[0])], vec![press]];
            events.extend(path[1..].iter().map(|p| vec![Event::PointerMoved(*p)]));
            events.push(vec![]);
            events.into_iter().flat_map(|e| self.frame(e).viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()).collect()
        }

        fn click(&mut self, p: Pos2) -> Vec<ViewportCommand> {
            self.gesture(&[p])
        }

        fn caption(&self, c: Caption) -> Rect {
            self.ctx.read_response(caption_id(c)).map(|r| r.rect).unwrap_or(Rect::NOTHING)
        }
    }

    fn texts(out: &egui::FullOutput) -> Vec<(String, Rect)> {
        fn walk(s: &egui::Shape, v: &mut Vec<(String, Rect)>) {
            match s {
                egui::Shape::Text(t) => v.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
                egui::Shape::Vec(s) => s.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut v = vec![];
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
        v
    }

    #[test]
    fn every_bar_control_uses_translucent_hover_chrome() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let mut w = Win::new(1600.0);
            w.app.home.open = false;
            w.frame(vec![]);
            w.frame(vec![]);
            let t = Tokens::get();
            let mut controls: Vec<Rect> = [
                "titlebar:home",
                "appbar:file",
                "titlebar:save",
                "appbar:undo",
                "appbar:redo",
                "tab:design:0",
                "tab:close:0",
                "titlebar:assistant",
                "titlebar:preferences",
                "appbar:help",
                "titlebar:report",
            ]
            .into_iter()
            .map(|name| crate::scenario::rect_of(name).unwrap_or_else(|| panic!("{name}")))
            .collect();
            controls.extend(CAPTIONS.map(|c| w.caption(c)));
            for rect in controls {
                w.frame(vec![Event::PointerMoved(rect.center())]);
                let out = w.frame(vec![]);
                let fills: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(shape) if shape.rect == rect || shape.rect == rect.shrink(3.0) => Some(shape.fill),
                        _ => None,
                    })
                    .collect();
                assert!(
                    fills.iter().any(|fill| [t.app_bar_hover, t.app_bar_hover_button, t.app_bar_hover_strong, t.app_bar_tab_active].contains(fill)),
                    "dark={dark}: {rect:?} has no application bar hover fill: {fills:?}"
                );
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn caption_buttons_sit_in_the_corner_and_nothing_runs_under_them() {
        for width in [960.0, 1600.0] {
            let mut w = Win::new(width);
            w.frame(vec![]);
            let out = w.frame(vec![]);
            let [min, max, close] = CAPTIONS.map(|c| w.caption(c));
            assert_eq!((close.right(), close.top()), (width, 0.0), "Close sits in the window's corner at {width}");
            assert!([min, max, close].iter().all(|r| r.width() == BUTTON_WIDTH));
            assert_eq!((min.right(), max.right()), (max.left(), close.left()));
            let texts = texts(&out);
            let version = texts.iter().find(|(t, _)| t.starts_with('v')).map(|(_, r)| *r);
            assert!(version.is_some_and(|r| r.right() <= min.left() - 4.0), "the version stays left of the buttons: {version:?}");
            for (t, r) in &texts {
                assert!(r.right() <= min.left() || r.top() >= min.bottom(), "`{t}` runs under the caption buttons at {width}");
            }
        }
    }

    #[test]
    fn no_caption_buttons_without_a_custom_title_bar() {
        let mut w = Win::new(1200.0);
        w.app.custom_titlebar = false;
        w.frame(vec![]);
        w.frame(vec![]);
        assert_eq!(w.caption(Caption::Close), Rect::NOTHING);
    }

    #[test]
    fn minimize_maximize_and_close() {
        let mut w = Win::new(1200.0);
        w.frame(vec![]);
        let (min, max, close) = (w.caption(Caption::Minimize).center(), w.caption(Caption::Maximize).center(), w.caption(Caption::Close).center());
        assert!(w.click(min).contains(&ViewportCommand::Minimized(true)));
        assert!(w.click(max).contains(&ViewportCommand::Maximized(true)));
        w.maximized = true;
        assert!(w.click(max).contains(&ViewportCommand::Maximized(false)), "Restore");
        assert!(w.click(close).contains(&ViewportCommand::Close));
        assert!(w.app.quit_requested);
    }

    #[test]
    fn empty_bar_space_drags_and_double_click_maximizes() {
        let mut w = Win::new(1600.0);
        w.frame(vec![]);
        let empty = pos2(900.0, 17.0);
        let cmds = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        assert_eq!(cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count(), 1, "{cmds:?}");
        let mut cmds = w.click(empty);
        cmds.extend(w.click(empty));
        assert!(cmds.contains(&ViewportCommand::Maximized(true)), "{cmds:?}");
        w.maximized = true;
        w.time += 1.0;
        let mut cmds = w.click(empty);
        cmds.extend(w.click(empty));
        assert!(cmds.contains(&ViewportCommand::Maximized(false)), "{cmds:?}");
        // The bar's buttons keep their clicks: dragging from Undo doesn't move the window.
        w.time += 1.0;
        let undo = crate::scenario::handle_at("appbar:undo").unwrap();
        let cmds = w.gesture(&[undo, undo + vec2(12.0, 4.0), undo + vec2(30.0, 8.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::StartDrag)), "{cmds:?}");
    }

    /// On X11 the window manager keeps the release of the press that started a window move or
    /// resize: every drag on the bar or the design's tab still moves the window and every edge
    /// drag still resizes, not every other one (#13334).
    #[test]
    fn a_move_whose_release_the_os_keeps_does_not_swallow_the_next_drag() {
        let starts = |cmds: &[ViewportCommand]| cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count();
        let mut w = Win::new(1600.0);
        w.frame(vec![]);
        let empty = pos2(900.0, 17.0);
        for round in 1..=3 {
            let cmds = w.gesture_os_keeps_release(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
            assert_eq!(starts(&cmds), 1, "bar drag {round}: {cmds:?}");
            w.time += 1.0;
        }
        let east = pos2(1598.0, 300.0);
        for round in 1..=2 {
            let cmds = w.gesture_os_keeps_release(&[east, east + vec2(6.0, 0.0)]);
            assert_eq!(cmds, vec![ViewportCommand::BeginResize(ResizeDirection::East)], "resize {round}");
        }
        // The only design's tab is part of the title bar: a drag on it moves the window too.
        let tab = crate::scenario::handle_at("tab:design:0").expect("the design's tab");
        for round in 1..=2 {
            let cmds = w.gesture_os_keeps_release(&[tab, tab + vec2(4.0, 12.0), tab + vec2(8.0, 30.0)]);
            assert_eq!(starts(&cmds), 1, "tab drag {round}: {cmds:?}");
            w.time += 1.0;
        }
        // A real release that does arrive later is ignored: no click, so no maximize.
        let mut cmds = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        cmds.extend(w.frame(vec![]).viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default());
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::Maximized(_))), "{cmds:?}");
    }

    #[test]
    fn edge_zones_resize_unless_maximized() {
        use ResizeDirection::*;
        let mut w = Win::new(1000.0);
        w.frame(vec![]);
        for (p, dir) in [
            (pos2(1.0, 300.0), West),
            (pos2(998.0, 300.0), East),
            (pos2(500.0, 1.0), North),
            (pos2(500.0, 698.0), South),
            (pos2(2.0, 2.0), NorthWest),
            (pos2(997.0, 3.0), NorthEast),
            (pos2(3.0, 697.0), SouthWest),
            (pos2(998.0, 698.0), SouthEast),
        ] {
            let cmds = w.gesture(&[p, p + vec2(6.0, 6.0)]);
            assert_eq!(cmds, vec![ViewportCommand::BeginResize(dir)], "at {p:?}");
        }
        w.maximized = true;
        let cmds = w.gesture(&[pos2(1.0, 300.0), pos2(8.0, 300.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::BeginResize(_))), "{cmds:?}");
    }

    #[test]
    fn edges_resize_on_the_start_page() {
        // At launch without a design the start page shows; its layout returned before the
        // resize zones were laid out, so the edges did nothing until a design opened (#1381).
        let mut w = Win::new(1200.0);
        w.full = true;
        let rev = w.app.session.revision;
        w.app.home.show(rev);
        w.frame(vec![]);
        assert!(w.app.home.open);
        let p = pos2(1199.0, 350.0);
        let cmds = w.gesture(&[p, p + vec2(6.0, 0.0)]);
        assert_eq!(cmds, vec![ViewportCommand::BeginResize(ResizeDirection::East)]);
        let p = pos2(1.0, 699.0);
        let cmds = w.gesture(&[p, p + vec2(-6.0, 6.0)]);
        assert_eq!(cmds, vec![ViewportCommand::BeginResize(ResizeDirection::SouthWest)]);
        // Where the OS hit test offers the border, egui lays out no zones of its own.
        w.app.native_resize_border = true;
        let p = pos2(1199.0, 350.0);
        let cmds = w.gesture(&[p, p + vec2(6.0, 0.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::BeginResize(_))), "{cmds:?}");
    }
}
