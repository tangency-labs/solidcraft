//! Window chrome any Craft app shares: the window's settings ([`viewport`]) and its title bar,
//! drawn from plain inputs ([`TitleBar`]) and answering with [`WindowAction`]s for the app to run.
//!
//! The OS window frame is kept. On macOS the OS title bar is hidden and the content extends under
//! it, with room left for the traffic lights ([`TRAFFIC_LIGHTS`]). On Windows and Linux an app
//! can drop the OS decorations: then empty space in the bar drags the window and a double-click
//! there maximizes or restores it, caption buttons at the far right minimize, maximize/restore and
//! close, and thin invisible zones along the window's edges resize it. Dragging uses the OS's own
//! move loop, so Windows snapping (to the screen edges and corners) works as with a native title
//! bar.
//!
//! The resize zones are as wide as Windows' own resize border at the window's scale
//! ([`resize_border_px`]), corners win where they overlap the edges ([`resize_hit`]). An app that
//! answers the system's hit test itself (SolidCraft's `apps/solidcraft/src/native_frame.rs`) uses
//! them there, so the OS shows its resize cursors and resizes from the first frame; elsewhere egui
//! zones above everything else start an OS resize ([`resize_zones`]). The web build has neither.
//!
//! Nothing here knows an app's state: closing is a [`WindowAction::Close`] the app answers (it may
//! ask about unsaved work first).
//!
//! A move or resize hands the press to the OS, which keeps the pointer until the button goes up.
//! On X11 the window manager then receives that release, not the app, so egui would keep the
//! button down and the next press would only end the old drag: the bar moved the window every
//! other time (#13334). An app calls [`release_press_taken_by_os`] first in its raw-input hook.
//!
//! Adapted from VectorCraft's `crates/ui-egui/src/titlebar.rs` (MIT OR Apache-2.0).

use crate::icons::glyphs::paint_glyph;
use crate::ui_kit::BackingTooltipExt as _;
use egui::{
    Color32, CursorIcon, Id, LayerId, Order, PointerButton, Pos2, Rect, ResizeDirection, Sense, Stroke, Ui, ViewportBuilder, ViewportCommand, pos2,
    vec2,
};
use solidcraft_theme::Tokens;

pub mod native_menu;

/// One caption button: as wide as the native Windows ones.
pub const BUTTON_WIDTH: f32 = 46.0;
/// The three caption buttons together.
pub const WIDTH: f32 = 3.0 * BUTTON_WIDTH;
/// Room for the macOS traffic lights at the bar's left end.
pub const TRAFFIC_LIGHTS: f32 = 76.0;
/// Windows' resize border at 100 % (96 DPI): `SM_CXSIZEFRAME` + `SM_CXPADDEDBORDER`, 4 + 4 px.
pub const RESIZE_BORDER_AT_96_DPI: f32 = 8.0;

/// The window's settings: `title` and `app_id` (the desktop entry's reverse-DNS id) are the
/// app's; `custom_titlebar` drops the OS decorations so the app's bar is the title bar. On macOS
/// the OS title bar is hidden and the content runs under it. The app adds its own icon.
pub fn viewport(title: &str, app_id: &str, custom_titlebar: bool) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([1600.0, 1000.0])
        .with_min_inner_size([960.0, 600.0])
        .with_drag_and_drop(true)
        .with_decorations(!custom_titlebar)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_app_id(app_id)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Caption {
    Minimize,
    Maximize,
    Close,
}

/// Left to right.
pub const CAPTIONS: [Caption; 3] = [Caption::Minimize, Caption::Maximize, Caption::Close];

/// Caption button ids (stable, so tests and agents can find them).
pub fn caption_id(c: Caption) -> Id {
    Id::unique(("titlebar-caption", c as u8))
}

/// What the window is asked to do, by a caption button or by name (an app's control channel).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowAction {
    Minimize,
    Maximize,
    Restore,
    /// Maximize or restore.
    Toggle,
    Close,
}

impl WindowAction {
    pub const ALL: [Self; 5] = [Self::Minimize, Self::Maximize, Self::Restore, Self::Toggle, Self::Close];

    pub fn name(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Restore => "restore",
            Self::Toggle => "toggle",
            Self::Close => "close",
        }
    }

    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|a| a.name() == name)
            .ok_or_else(|| format!("unknown window action `{name}` (minimize, maximize, restore, toggle, close)"))
    }

    /// Sends the action's viewport command. [`Self::Close`] closes the window at once: an app
    /// with unsaved work asks first and sends it once answered.
    pub fn send(self, ctx: &egui::Context) {
        match self {
            Self::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Self::Maximize => ctx.send_viewport_cmd(ViewportCommand::Maximized(true)),
            Self::Restore => ctx.send_viewport_cmd(ViewportCommand::Maximized(false)),
            Self::Toggle => toggle_maximize(ctx),
            Self::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }
}

pub fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

pub fn toggle_maximize(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized(ctx)));
}

/// How the window's title bar behaves: which of the OS's controls share the bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TitleBar {
    /// macOS: the OS draws its traffic lights over the bar's left end.
    pub traffic_lights: bool,
    /// No OS decorations (Windows, Linux): the bar has its own caption buttons at the right end.
    pub caption_buttons: bool,
}

/// Where a [`TitleBar`] puts things in the bar's rect.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TitleBarLayout {
    /// Empty space here moves the window; `None` under the OS's own title bar.
    pub drag: Option<Rect>,
    /// What is left for the app's contents.
    pub content: Rect,
    /// The caption buttons, flush with the window's right edge.
    pub captions: Option<Rect>,
}

impl TitleBar {
    pub fn layout(self, bar: Rect) -> TitleBarLayout {
        let left = bar.left() + if self.traffic_lights { TRAFFIC_LIGHTS } else { 0.0 };
        let right = bar.right() - if self.caption_buttons { WIDTH } else { 0.0 };
        TitleBarLayout {
            drag: (self.traffic_lights || self.caption_buttons).then_some(bar),
            content: Rect::from_min_max(pos2(left, bar.top()), pos2(right, bar.bottom())),
            captions: self.caption_buttons.then(|| Rect::from_min_max(pos2(right, bar.top()), bar.right_bottom())),
        }
    }

    /// Lays out the bar in `bar`: the drag area first (so the contents' widgets, registered
    /// later, take their clicks), `contents` in the rect left for them, then the caption buttons.
    /// The caption button clicked, as the action for the app to run.
    pub fn show(self, ui: &mut Ui, bar: Rect, contents: impl FnOnce(&mut Ui, Rect)) -> Option<WindowAction> {
        let layout = self.layout(bar);
        if let Some(r) = layout.drag {
            drag_area(ui, r);
        }
        contents(ui, layout.content);
        if self.caption_buttons { caption_buttons(ui, bar) } else { None }
    }
}

/// The bar's background: a press-and-drag on empty space moves the window, a double-click
/// toggles maximize. Call it before laying out the bar so its buttons, registered later, take
/// their clicks.
pub fn drag_area(ui: &Ui, rect: Rect) {
    let ctx = ui.ctx();
    let resp = ui.interact(rect, Id::unique("titlebar-drag"), Sense::click_and_drag());
    // egui hands a drag that starts on a click-only widget to the drag-sensing background below
    // it, so remember whether the press itself landed on bare background.
    if ctx.input(|i| i.pointer.any_pressed()) {
        let bare = ctx.viewport(|v| v.hits.click.is_some_and(|w| w.id == resp.id));
        ctx.data_mut(|d| d.insert_temp(resp.id, bare));
    }
    if resp.double_clicked() {
        toggle_maximize(ctx);
    } else if resp.drag_started_by(PointerButton::Primary) && ctx.data(|d| d.get_temp(resp.id).unwrap_or(false)) {
        hand_press_to_os(ctx, ViewportCommand::StartDrag);
    }
}

/// Where the press handed to the OS by a window move or resize was, until the next frame.
fn press_taken_by_os_id() -> Id {
    Id::unique("titlebar-press-taken-by-os")
}

/// Start an OS window move or resize (`StartDrag` or `BeginResize`) with the press egui is
/// handling, and remember that press for [`release_press_taken_by_os`].
pub fn hand_press_to_os(ctx: &egui::Context, cmd: ViewportCommand) {
    let pos = ctx.input(|i| i.pointer.interact_pos());
    ctx.send_viewport_cmd(cmd);
    if let Some(pos) = pos {
        ctx.data_mut(|d| d.insert_temp(press_taken_by_os_id(), pos));
    }
}

/// On the frame after a window move or resize started, release the primary button where it was
/// pressed, before the frame's own events. The OS's release may never reach the app (X11); where
/// it does arrive later, egui ignores it, as no button is down then and it is no click.
pub fn release_press_taken_by_os(ctx: &egui::Context, raw: &mut egui::RawInput) {
    if let Some(pos) = ctx.data_mut(|d| d.remove_temp::<Pos2>(press_taken_by_os_id())) {
        let release = egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: ctx.input(|i| i.modifiers) };
        raw.events.insert(0, release);
    }
}

/// Minimize, Maximize/Restore and Close at the right end of `bar`, full bar height, flush with
/// the window's edge so a maximized window's corner hits Close. The one clicked, as the action
/// for the app to run.
pub fn caption_buttons(ui: &mut Ui, bar: Rect) -> Option<WindowAction> {
    let t = Tokens::get();
    let max = maximized(ui.ctx());
    let mut clicked = None;
    for (i, c) in CAPTIONS.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(bar.right() - WIDTH + i as f32 * BUTTON_WIDTH, bar.top()), vec2(BUTTON_WIDTH, bar.height()));
        let resp = ui.interact(r, caption_id(c), Sense::click());
        let fill = match (resp.is_pointer_button_down_on(), resp.hovered()) {
            (true, _) => t.app_bar_hover_subtle,
            (_, true) => t.app_bar_hover_button,
            _ if resp.has_focus() => t.app_bar_hover_button,
            _ => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(r, 0.0, fill);
        paint_glyph(ui, c, max, r.center(), Stroke::new(1.0, t.app_bar_text));
        let tip = match c {
            Caption::Minimize => "Minimize",
            Caption::Maximize if max => "Restore",
            Caption::Maximize => "Maximize",
            Caption::Close => "Close",
        };
        if resp.backing_on_hover_text(tip).clicked() {
            clicked = Some(match c {
                Caption::Minimize => WindowAction::Minimize,
                Caption::Maximize => WindowAction::Toggle,
                Caption::Close => WindowAction::Close,
            });
        }
    }
    clicked
}

/// The resize border in physical pixels at `scale` (1.0 = 96 DPI): Windows' default, scaled and
/// rounded, never less than a pixel.
pub fn resize_border_px(scale: f32) -> f32 {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    (RESIZE_BORDER_AT_96_DPI * s).round().max(1.0)
}

/// The side of a corner zone for an edge `edge` wide: twice the edge, so a corner is easy to hit.
pub fn corner_size(edge: f32) -> f32 {
    2.0 * edge
}

/// The edge and corner zones of a window with rect `w`, edges `edge` wide: (zone, direction,
/// cursor). Corners come last so they win where they overlap the edges.
pub fn zones(w: Rect, edge: f32) -> [(Rect, ResizeDirection, CursorIcon); 8] {
    use ResizeDirection::*;
    let (e, c) = (edge, corner_size(edge));
    [
        (Rect::from_min_max(w.left_top(), pos2(w.right(), w.top() + e)), North, CursorIcon::ResizeVertical),
        (Rect::from_min_max(pos2(w.left(), w.bottom() - e), w.right_bottom()), South, CursorIcon::ResizeVertical),
        (Rect::from_min_max(w.left_top(), pos2(w.left() + e, w.bottom())), West, CursorIcon::ResizeHorizontal),
        (Rect::from_min_max(pos2(w.right() - e, w.top()), w.right_bottom()), East, CursorIcon::ResizeHorizontal),
        (Rect::from_min_size(w.left_top(), vec2(c, c)), NorthWest, CursorIcon::ResizeNwSe),
        (Rect::from_min_size(w.right_top() - vec2(c, 0.0), vec2(c, c)), NorthEast, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.left_bottom() - vec2(0.0, c), vec2(c, c)), SouthWest, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.right_bottom() - vec2(c, c), vec2(c, c)), SouthEast, CursorIcon::ResizeNwSe),
    ]
}

/// The resize direction at `p` in a window with rect `w` and edges `edge` wide, `None` away
/// from the edges and outside the window. A point is in the window when `min <= p < max` (pixel
/// rects: the right and bottom pixels are at `max - 1`). Corners win over edges.
pub fn resize_hit(w: Rect, p: Pos2, edge: f32) -> Option<ResizeDirection> {
    if !(w.is_positive() && edge > 0.0 && p.x >= w.left() && p.x < w.right() && p.y >= w.top() && p.y < w.bottom()) {
        return None;
    }
    zones(w, edge).into_iter().rev().find(|(r, ..)| p.x >= r.left() && p.x < r.right() && p.y >= r.top() && p.y < r.bottom()).map(|(_, d, _)| d)
}

/// The window's new physical size when it moves from `old_dpi` to `new_dpi` (Windows'
/// `WM_GETDPISCALEDSIZE`): the same logical size, rounded as winit rounds the size it then
/// applies, so the rect the OS suggests mid-move is exactly the rect the window gets.
pub fn dpi_scaled_size(size: (i32, i32), old_dpi: u32, new_dpi: u32) -> Option<(i32, i32)> {
    if old_dpi == 0 || new_dpi == 0 || size.0 <= 0 || size.1 <= 0 {
        return None;
    }
    let (old, new) = (f64::from(old_dpi) / 96.0, f64::from(new_dpi) / 96.0);
    let scale = |v: i32| (f64::from(v) / old * new).round().clamp(1.0, f64::from(i32::MAX)) as i32;
    Some((scale(size.0), scale(size.1)))
}

/// Invisible resize zones along the window's edges, above everything else; a press in one
/// starts an OS resize in that direction. None while the window is maximized or fullscreen.
pub fn resize_zones(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if maximized(&ctx) || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let w = ctx.content_rect();
    let ppp = ctx.pixels_per_point();
    let edge = resize_border_px(ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(ppp)) / ppp;
    let layer = LayerId::new(Order::Foreground, Id::unique("titlebar-resize"));
    let zui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(w));
    for (i, (r, dir, cursor)) in zones(w, edge).into_iter().enumerate() {
        let resp = zui.interact(r, Id::unique(("titlebar-resize", i)), Sense::drag()).on_hover_cursor(cursor);
        if resp.drag_started_by(PointerButton::Primary) {
            hand_press_to_os(&ctx, ViewportCommand::BeginResize(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportCommand, ViewportId, pos2, vec2};

    const BAR: f32 = 34.0;

    /// A headless window `width` pt wide that draws only a title bar, from plain inputs.
    struct Bar {
        ctx: egui::Context,
        bar: TitleBar,
        width: f32,
        time: f64,
        maximized: bool,
        /// The rect the bar handed its contents, and the action it returned, last frame.
        content: Option<Rect>,
        action: Option<WindowAction>,
    }

    impl Bar {
        fn new(bar: TitleBar, width: f32) -> Self {
            Self { ctx: crate::fonts::test_context(), bar, width, time: 0.0, maximized: false, content: None, action: None }
        }

        fn rect(&self) -> Rect {
            Rect::from_min_size(Pos2::ZERO, vec2(self.width, BAR))
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<ViewportCommand> {
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
            let (bar, rect) = (self.bar, self.rect());
            let (mut content, mut action) = (None, None);
            let mut out = self.ctx.run_ui(input, |ui| {
                action = bar.show(ui, rect, |_, r| content = Some(r));
            });
            out.textures_delta.clear();
            (self.content, self.action) = (content, action);
            out.viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()
        }

        /// Move to `path[0]`, press, move along `path`, release: the commands sent and the
        /// actions returned.
        fn gesture(&mut self, path: &[Pos2]) -> (Vec<ViewportCommand>, Vec<WindowAction>) {
            let button = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() };
            let mut events = vec![vec![Event::PointerMoved(path[0])], vec![button(path[0], true)]];
            events.extend(path[1..].iter().map(|p| vec![Event::PointerMoved(*p)]));
            events.push(vec![button(*path.last().unwrap_or(&path[0]), false)]);
            let (mut cmds, mut actions) = (vec![], vec![]);
            for e in events {
                cmds.extend(self.frame(e));
                actions.extend(self.action);
            }
            (cmds, actions)
        }
    }

    #[test]
    fn title_bar_from_plain_inputs_insets_for_the_traffic_lights_and_drags_from_empty_space() {
        let bar = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, BAR));
        // macOS: the OS draws the traffic lights over the bar's left end; no caption buttons.
        let mac = TitleBar { traffic_lights: true, caption_buttons: false };
        let layout = mac.layout(bar);
        assert_eq!(layout.drag, Some(bar));
        assert_eq!(layout.content, Rect::from_min_max(pos2(TRAFFIC_LIGHTS, 0.0), pos2(1200.0, BAR)));
        assert_eq!(layout.captions, None);
        // Windows and Linux without OS decorations: caption buttons at the right end.
        let own = TitleBar { traffic_lights: false, caption_buttons: true };
        let layout = own.layout(bar);
        assert_eq!(layout.drag, Some(bar));
        assert_eq!(layout.content, Rect::from_min_max(pos2(0.0, 0.0), pos2(1200.0 - WIDTH, BAR)));
        assert_eq!(layout.captions, Some(Rect::from_min_max(pos2(1200.0 - WIDTH, 0.0), pos2(1200.0, BAR))));
        // The OS's own title bar: the bar is plain content and moves nothing.
        let os = TitleBar { traffic_lights: false, caption_buttons: false };
        assert_eq!(os.layout(bar), TitleBarLayout { drag: None, content: bar, captions: None });

        let mut w = Bar::new(mac, 1200.0);
        w.frame(vec![]);
        assert_eq!(w.content, Some(mac.layout(bar).content), "the contents get the inset rect");
        let empty = pos2(600.0, 17.0);
        let (cmds, _) = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        assert_eq!(cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count(), 1, "{cmds:?}");
        w.time += 1.0;
        let (mut cmds, _) = w.gesture(&[empty]);
        cmds.extend(w.gesture(&[empty]).0);
        assert!(cmds.contains(&ViewportCommand::Maximized(true)), "double-click maximizes: {cmds:?}");

        let mut w = Bar::new(TitleBar { traffic_lights: false, caption_buttons: false }, 1200.0);
        w.frame(vec![]);
        let (cmds, _) = w.gesture(&[empty, empty + vec2(30.0, 8.0)]);
        assert!(cmds.is_empty(), "no drag area under the OS's own title bar: {cmds:?}");
    }

    #[test]
    fn caption_buttons_return_actions_and_leave_close_to_the_app() {
        let mut w = Bar::new(TitleBar { traffic_lights: false, caption_buttons: true }, 1200.0);
        w.frame(vec![]);
        let at = |c: Caption| w.ctx.read_response(caption_id(c)).map(|r| r.rect.center()).unwrap_or(Pos2::ZERO);
        let (min, max, close) = (at(Caption::Minimize), at(Caption::Maximize), at(Caption::Close));
        assert_eq!(close, pos2(1200.0 - BUTTON_WIDTH / 2.0, BAR / 2.0));
        assert_eq!(w.gesture(&[min]).1, vec![WindowAction::Minimize]);
        assert_eq!(w.gesture(&[max]).1, vec![WindowAction::Toggle]);
        let (cmds, actions) = w.gesture(&[close]);
        assert_eq!(actions, vec![WindowAction::Close]);
        assert!(!cmds.contains(&ViewportCommand::Close), "the app decides whether the window closes: {cmds:?}");
        // A drag from a caption button doesn't move the window.
        let (cmds, _) = w.gesture(&[min, min + vec2(-30.0, 8.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::StartDrag)), "{cmds:?}");
    }

    #[test]
    fn window_actions_parse_their_names() {
        for a in WindowAction::ALL {
            assert_eq!(WindowAction::parse(a.name()), Ok(a));
        }
        assert_eq!(WindowAction::parse("shrink"), Err("unknown window action `shrink` (minimize, maximize, restore, toggle, close)".to_string()));
    }

    #[test]
    fn viewport_has_the_shared_window_settings() {
        for custom in [false, true] {
            let v = viewport("Test", "org.example.Test", custom);
            assert_eq!(v.title.as_deref(), Some("Test"));
            assert_eq!(v.app_id.as_deref(), Some("org.example.Test"));
            assert_eq!(v.decorations, Some(!custom));
            assert_eq!((v.fullsize_content_view, v.titlebar_shown, v.title_shown), (Some(true), Some(false), Some(false)));
            assert_eq!((v.inner_size, v.min_inner_size), (Some(vec2(1600.0, 1000.0)), Some(vec2(960.0, 600.0))));
            assert_eq!(v.drag_and_drop, Some(true));
        }
    }

    #[test]
    fn resize_border_is_windows_default_for_the_scale() {
        for (dpi, px) in [(96.0, 8.0), (120.0, 10.0), (144.0, 12.0), (168.0, 14.0), (192.0, 16.0), (288.0, 24.0)] {
            assert_eq!(resize_border_px(dpi / 96.0), px, "at {dpi} DPI");
        }
        // Never 0 px, whatever the scale says.
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(resize_border_px(bad), 8.0, "{bad}");
        }
        assert_eq!(resize_border_px(0.01), 1.0);
        assert_eq!(corner_size(resize_border_px(1.5)), 24.0);
    }

    #[test]
    fn hit_test_edges_corners_and_inside() {
        use ResizeDirection::*;
        // A window at (100, 50), 1000 x 700 px, at 150 %: 12 px edges, 24 px corners.
        let w = Rect::from_min_size(pos2(100.0, 50.0), vec2(1000.0, 700.0));
        let e = resize_border_px(1.5);
        let (l, t, r, b) = (100.0, 50.0, 1099.0, 749.0); // the outermost pixels
        for (p, want) in [
            ((l, 400.0), Some(West)),
            ((l + 11.0, 400.0), Some(West)),
            ((l + 12.0, 400.0), None),
            ((r, 400.0), Some(East)),
            ((r - 11.0, 400.0), Some(East)),
            ((r - 12.0, 400.0), None),
            ((600.0, t), Some(North)),
            ((600.0, t + 12.0), None),
            ((600.0, b), Some(South)),
            ((600.0, b - 11.0), Some(South)),
            ((l, t), Some(NorthWest)),
            ((r, t), Some(NorthEast)),
            ((l, b), Some(SouthWest)),
            ((r, b), Some(SouthEast)),
            // Corners win over the edges, along both edges for the corner's length.
            ((l + 23.0, t), Some(NorthWest)),
            ((l, t + 23.0), Some(NorthWest)),
            ((l + 24.0, t), Some(North)),
            ((l, t + 24.0), Some(West)),
            ((r - 23.0, b), Some(SouthEast)),
            ((r, b - 23.0), Some(SouthEast)),
            ((r - 23.0, t), Some(NorthEast)),
            ((l, b - 23.0), Some(SouthWest)),
            ((600.0, 400.0), None),
            // Outside the window: not ours.
            ((l - 1.0, 400.0), None),
            ((r + 1.0, 400.0), None),
            ((600.0, t - 1.0), None),
            ((600.0, b + 1.0), None),
        ] {
            assert_eq!(resize_hit(w, pos2(p.0, p.1), e), want, "at {p:?}");
        }
        assert_eq!(resize_hit(w, pos2(l, 400.0), 0.0), None);
        assert_eq!(resize_hit(Rect::NOTHING, pos2(l, 400.0), e), None);
        // Every DPI has a zone on every edge.
        for dpi in [96.0, 120.0, 144.0, 192.0] {
            let e = resize_border_px(dpi / 96.0);
            for (p, d) in [((l, 400.0), West), ((r, 400.0), East), ((600.0, t), North), ((600.0, b), South)] {
                assert_eq!(resize_hit(w, pos2(p.0, p.1), e), Some(d), "{d:?} at {dpi} DPI");
            }
        }
    }

    #[test]
    fn dpi_change_keeps_the_logical_size() {
        assert_eq!(dpi_scaled_size((1600, 1000), 96, 144), Some((2400, 1500)));
        assert_eq!(dpi_scaled_size((2400, 1500), 144, 96), Some((1600, 1000)));
        assert_eq!(dpi_scaled_size((1001, 777), 120, 192), Some((1602, 1243)));
        assert_eq!(dpi_scaled_size((1001, 777), 96, 96), Some((1001, 777)));
        assert_eq!(dpi_scaled_size((1001, 777), 0, 96), None);
        assert_eq!(dpi_scaled_size((0, 777), 96, 144), None);
    }
}
