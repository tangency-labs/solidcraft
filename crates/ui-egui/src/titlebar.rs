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
    use crate::window_chrome::{BUTTON_WIDTH, CAPTIONS, caption, caption_id};
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
        /// The native window it is shown in, as the desktop integration tells on Windows.
        window: Option<u64>,
        /// Lay out the whole window (`SolveApp::ui`) rather than only the bar and the zones.
        full: bool,
    }

    impl Win {
        fn new(width: f32) -> Self {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(Session::default(), Default::default());
            app.custom_titlebar = true;
            Self { ctx, app, width, time: 0.0, maximized: false, window: None, full: false }
        }

        /// A window the system answers for, as the Windows desktop app's are: shown in a native
        /// window of its own (the list of them is one per process) whose title bar and frame are
        /// the system's.
        fn systems(width: f32) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x5C7B_0000);
            let mut w = Self::new(width);
            let window = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            caption::hooked(window);
            w.window = Some(window);
            w
        }

        /// The native window of a window made by [`Self::systems`] (0 for any other).
        fn native(&self) -> u64 {
            self.window.unwrap_or_default()
        }

        /// The client pixel of `p`, as the window procedure names points.
        fn px(&self, p: Pos2) -> (i32, i32) {
            let ppp = self.ctx.pixels_per_point();
            ((p.x * ppp) as i32, (p.y * ppp) as i32)
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
                v.native_window = self.window;
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

    /// Home follows the preceding name or document button with the stack's one shared gap.
    #[test]
    fn home_follows_solidcrafts_name_with_the_uniform_gap() {
        let mut w = Win::new(1600.0);
        w.app.home.open = false;
        w.frame(vec![]);
        let out = w.frame(vec![]);
        let name = texts(&out).into_iter().find(|(text, _)| text == "SolidCraft").map(|(_, rect)| rect).expect("the name is drawn");
        let home = crate::scenario::rect_of("titlebar:home").expect("Home is drawn");
        let previous = crate::scenario::rect_of("library:toggle").unwrap_or(name);
        assert!((home.left() - previous.right() - solidcraft_ui_kit::app_bar_layout::application::GAP).abs() <= 0.5);
        assert!(name.right() < home.left());
    }

    /// Report a bug and the link pills are not in this list: they are flat brand-coloured capsules
    /// whose hover is a slightly lighter fill of their own colour, not the bar's translucent chrome.
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
    fn the_link_pills_leave_the_caption_buttons_and_the_controls_alone() {
        for width in [1280.0, 1600.0] {
            let mut w = Win::new(width);
            w.app.home.open = false;
            w.frame(vec![]);
            let out = w.frame(vec![]);
            let [min, ..] = CAPTIONS.map(|c| w.caption(c));
            let rect = |name: &str| crate::scenario::rect_of(name).unwrap_or_else(|| panic!("{name} at {width}"));
            let (donate, discord, report) = (rect("titlebar:kofi"), rect("titlebar:discord"), rect("titlebar:report"));
            assert_eq!(donate.height().max(discord.height()), solidcraft_ui_kit::app_bar::PILL_HEIGHT, "{width}");
            assert!(
                donate.right() < discord.left() && discord.right() < report.left() && report.right() <= min.left(),
                "{width}: {donate:?} {discord:?} {report:?} {min:?}"
            );
            for hide in ["titlebar:kofi:hide", "titlebar:discord:hide"] {
                let x = rect(hide);
                assert!(x.right() <= report.left() && x.left() >= donate.left(), "{width}: {hide} inside its pill");
            }
            // The pills sit left of the assistant and the other controls, which keep their places.
            if let Some(a) = crate::scenario::rect_of("titlebar:assistant") {
                assert!(discord.right() <= a.left(), "{width}: the pills overlap the assistant: {discord:?} {a:?}");
            }
            for (t, r) in &texts(&out) {
                assert!(r.right() <= min.left() || r.top() >= min.bottom(), "`{t}` runs under the caption buttons at {width}");
            }
            let tabs = crate::scenario::rect_of("tab:design:0");
            if let Some(tabs) = tabs {
                assert!(tabs.right() <= donate.left(), "{width}: tabs {tabs:?} run under the pills {donate:?}");
            }
            // Donate's label follows the width allocated by the uniform layout, not a
            // window-width breakpoint: the same item can be placed in either stack.
            let named = texts(&out).iter().any(|(t, r)| t == "Donate" && donate.contains_rect(*r));
            assert_eq!(named, donate.width() > solidcraft_ui_kit::app_bar_layout::application::PILL_MIN, "{width}");
        }
    }

    /// On a narrow bar the pills give way to the document tabs: they are left out, and where they
    /// stay they clear the tabs and the caption buttons.
    #[test]
    fn narrow_bars_leave_the_pills_out_before_they_crowd_the_tabs() {
        for width in [700.0, 900.0, 960.0, 1100.0] {
            let mut w = Win::new(width);
            w.app.home.open = false;
            w.frame(vec![]);
            w.frame(vec![]);
            let [min, ..] = CAPTIONS.map(|c| w.caption(c));
            let tabs = crate::scenario::rect_of("tab:design:0").expect("a tab");
            for name in ["titlebar:kofi", "titlebar:discord"] {
                if let Some(r) = crate::scenario::rect_of(name) {
                    assert!(r.left() >= tabs.right() && r.right() <= min.left(), "{width}: {name} {r:?} crowds the tab {tabs:?}");
                }
            }
            assert!(crate::scenario::rect_of("titlebar:report").is_some(), "{width}: Report a bug stays");
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

    /// Window control buttons off (#20385, for a tiling window manager): no caption buttons, the
    /// bar's contents reach the window's right edge, and empty bar space still moves the window.
    #[test]
    fn window_control_buttons_can_be_turned_off_and_the_bar_still_drags() {
        let mut w = Win::new(1600.0);
        w.frame(vec![]);
        w.frame(vec![]);
        let shown = crate::scenario::rect_of("titlebar:report").expect("Report a bug");
        assert!(shown.right() <= w.caption(Caption::Minimize).left(), "on by default: {shown:?}");
        w.app.preferences.show_window_controls = false;
        w.frame(vec![]);
        w.frame(vec![]);
        for c in CAPTIONS {
            assert_eq!(w.caption(c), Rect::NOTHING, "{c:?} is left out");
        }
        let hidden = crate::scenario::rect_of("titlebar:report").expect("Report a bug");
        assert!(hidden.right() > shown.right() + 100.0, "the contents take the buttons' room: {shown:?} -> {hidden:?}");
        let empty = pos2(900.0, 17.0);
        let cmds = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        assert_eq!(cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count(), 1, "{cmds:?}");
        let corner = pos2(1590.0, 17.0);
        assert!(!w.click(corner).contains(&ViewportCommand::Close), "nothing closes the window from the corner");
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

    /// A drag that starts on a tab or an application bar button belongs to that control, in the
    /// custom title bar (Windows, Linux) and the integrated one (macOS) alike: it never asks the OS
    /// to move the window, whatever its direction and however many tabs there are. Empty bar space
    /// still does.
    #[test]
    fn drags_from_tabs_and_bar_items_never_move_the_window() {
        for integrated in [false, true] {
            for tabs in [1, 3] {
                let mut w = Win::new(1600.0);
                w.app.custom_titlebar = !integrated;
                w.app.integrated_titlebar = integrated;
                w.app.home.open = false;
                for _ in 1..tabs {
                    assert!(crate::documents::open_tab(&mut w.app, Session::default()).is_ok());
                }
                w.frame(vec![]);
                w.frame(vec![]);
                let starts = |cmds: &[ViewportCommand]| cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count();
                let mut names = vec!["tab:design:0", "appbar:file", "appbar:undo", "titlebar:home"];
                if tabs > 1 {
                    names.push("tab:design:2");
                }
                for name in names {
                    let from = crate::scenario::handle_at(name).unwrap_or_else(|| panic!("{name}"));
                    for by in [vec2(40.0, 0.0), vec2(-30.0, 0.0), vec2(4.0, 30.0), vec2(30.0, -2.0)] {
                        w.time += 1.0;
                        let cmds = w.gesture(&[from, from + by * 0.25, from + by * 0.5, from + by]);
                        assert_eq!(starts(&cmds), 0, "integrated={integrated} tabs={tabs}: a drag from {name} by {by:?} moved the window: {cmds:?}");
                    }
                }
                w.time += 1.0;
                let empty = pos2(900.0, 17.0);
                let cmds = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
                assert_eq!(starts(&cmds), 1, "integrated={integrated} tabs={tabs}: empty bar space moves the window: {cmds:?}");
            }
        }
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
        // A drag on the design's tab belongs to the tab: it never moves the window.
        let tab = crate::scenario::handle_at("tab:design:0").expect("the design's tab");
        for round in 1..=2 {
            let cmds = w.gesture(&[tab, tab + vec2(4.0, 12.0), tab + vec2(8.0, 30.0)]); // no OS move starts, so the release arrives
            assert_eq!(starts(&cmds), 0, "tab drag {round}: {cmds:?}");
            w.time += 1.0;
        }
        let cmds = w.gesture_os_keeps_release(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        assert_eq!(starts(&cmds), 1, "bar drag after the tab drags: {cmds:?}");
        w.time += 1.0;
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

    /// Bare bar in a 1600 pt wide window, as the drag test above uses it.
    const BARE: Pos2 = pos2(900.0, 17.0);

    /// The whole window, a design open, 1600 pt wide.
    fn whole(mut w: Win) -> Win {
        w.full = true;
        w.app.home.open = false;
        w.frame(vec![]);
        w.frame(vec![]);
        w
    }

    /// Where Windows answers for the window (#13996), every frame hands it the bar: the bare bar
    /// is Windows' caption (it drags the window, and snaps it), the maximize button is Windows'
    /// own (snap layouts), and the bar's controls stay the UI's.
    #[test]
    fn the_app_bar_is_the_systems_caption_where_the_system_answers_for_the_window() {
        use caption::CaptionHit;
        let mut w = whole(Win::systems(1600.0));
        let window = w.native();
        for _ in 0..3 {
            w.frame(vec![]);
            let hit = |p: Pos2| {
                let (x, y) = w.px(p);
                caption::hit(window, x, y)
            };
            assert_eq!(hit(BARE), CaptionHit::Drag, "the bare bar");
            assert_eq!(hit(w.caption(Caption::Maximize).center()), CaptionHit::MaximizeButton);
            assert_eq!(hit(w.caption(Caption::Minimize).center()), CaptionHit::Client);
            assert_eq!(hit(w.caption(Caption::Close).center()), CaptionHit::Client);
            assert_eq!(hit(pos2(BARE.x, 400.0)), CaptionHit::Outside, "below the bar");
        }
        // The system's frame resizes the window: no zones of the UI's own, whoever draws it.
        w.app.native_resize_border = false;
        let p = pos2(1599.0, 350.0);
        let cmds = w.gesture(&[p, p + vec2(6.0, 0.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::BeginResize(_))), "{cmds:?}");
        caption::closed(window);
    }

    /// Where nothing answers for the window (other systems, the web), nothing is handed over and
    /// the bar stays egui's: a drag on the bare bar moves the window through egui, once past its
    /// drag threshold.
    #[test]
    fn without_a_system_that_answers_the_title_bar_stays_eguis() {
        let starts = |cmds: &[ViewportCommand]| cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count();
        let mut w = whole(Win::new(1600.0));
        assert_eq!(starts(&w.gesture(&[BARE, BARE + vec2(2.0, 0.0)])), 0);
        w.time += 1.0;
        let cmds = w.gesture(&[BARE, BARE + vec2(12.0, 4.0), BARE + vec2(30.0, 8.0)]);
        assert_eq!(starts(&cmds), 1, "{cmds:?}");
    }

    /// A right-click on the bare bar opens the application bar's own menu. Where Windows answers
    /// for the window that click is Windows', and comes back from the window procedure to open
    /// the same menu; a press Windows takes on the bare bar comes back too and closes it, as a
    /// press on a bar of the UI's own does.
    #[test]
    fn a_right_click_on_the_systems_caption_opens_the_application_bars_menu_as_before() {
        let app_bar_menu = |w: &Win| w.app.viewport.context_menu.is_some() && matches!(w.app.menu.target, crate::context_menu::Target::AppBar { .. });
        // The bar is the UI's own: egui sees the click.
        let mut own = whole(Win::new(1600.0));
        let button = |pressed| Event::PointerButton { pos: BARE, button: PointerButton::Secondary, pressed, modifiers: Default::default() };
        own.frame(vec![Event::PointerMoved(BARE), button(true), button(false)]);
        own.frame(vec![]);
        assert!(app_bar_menu(&own), "the application bar's menu");
        // The system answers for the window: the window procedure hands the click back.
        let mut w = whole(Win::systems(1600.0));
        assert!(!app_bar_menu(&w));
        let (x, y) = w.px(BARE);
        caption::secondary_click(w.native(), x, y);
        w.frame(vec![]);
        w.frame(vec![]);
        assert!(app_bar_menu(&w), "the same menu");
        // The menu opened at the click and lies over the bar there: beside it the bar is still the
        // system's caption. A press the system takes there is a press on the bar to the UI, as on
        // a bar of its own: the menu closes by the code that closes it on any press elsewhere.
        let (x, y) = w.px(BARE - vec2(60.0, 0.0));
        assert_eq!(caption::hit(w.native(), x, y), caption::CaptionHit::Drag, "beside the menu");
        caption::pressed(w.native(), x, y);
        w.frame(vec![]);
        w.frame(vec![]);
        assert!(w.app.viewport.context_menu.is_none(), "a press on the caption closes it");
        caption::closed(w.native());
    }
}
