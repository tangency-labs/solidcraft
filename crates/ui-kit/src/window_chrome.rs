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
//! them there, so the OS shows its resize cursors and resizes from the first frame: for every edge,
//! or, with the system's own frame kept on the left, right and bottom, for the top edge only.
//! Elsewhere egui zones above everything else start an OS resize ([`resize_zones`]). The web build
//! has neither.
//!
//! With the system's frame kept, such an app can also answer the hit test from the bar's layout,
//! which the UI publishes every frame ([`publish_caption`]): the bare bar is the caption, so
//! Windows drags the window from the first pixel, and the maximize button is the OS's, so hovering
//! it shows Windows 11's snap layouts. A right-click on the bare bar asks for the window menu
//! ([`open_window_menu`]), and a press the OS takes on the frame closes the UI's menus as a press
//! outside them would ([`window_frame_pressed`]).
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

use crate::BackingTooltipExt as _;
use crate::icons::glyphs::paint_glyph;
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
    /// later, take their clicks), `contents` in the rect left for them, then the caption buttons
    /// and, when asked for, the window menu. The caption button clicked or the window menu's
    /// choice, as the action for the app to run.
    pub fn show(self, ui: &mut Ui, bar: Rect, contents: impl FnOnce(&mut Ui, Rect)) -> Option<WindowAction> {
        let layout = self.layout(bar);
        let drag = layout.drag.map(|r| drag_area(ui, r));
        contents(ui, layout.content);
        if !self.caption_buttons {
            return None;
        }
        let clicked = caption_buttons(ui, bar);
        let chosen = drag.as_ref().and_then(|drag| window_menu(ui, drag));
        clicked.or(chosen)
    }
}

/// The bar's background: a press-and-drag on empty space moves the window, a double-click
/// toggles maximize. Call it before laying out the bar so its buttons, registered later, take
/// their clicks. The response anchors the [`window_menu`].
pub fn drag_area(ui: &Ui, rect: Rect) -> egui::Response {
    let ctx = ui.ctx();
    // The bar this frame, for the native hit test ([`publish_caption`]). The pass number is read
    // before taking the data lock: egui's context lock is not re-entrant.
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(bar_key(), (rect, pass)));
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
    resp
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
        // Over a native caption the OS owns the maximize button's pointer (snap layouts): its
        // hover and press come from the window procedure ([`set_native_maximize`]).
        let (native_hover, native_pressed) = if c == Caption::Maximize { native_maximize() } else { (false, false) };
        let fill = match (resp.is_pointer_button_down_on() || native_pressed, resp.hovered() || native_hover) {
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

// ---------------------------------------------------------------------------------------------
// Native caption hit testing
//
// On Windows the window procedure answers the system's hit test from the bar's layout: bare bar
// is the caption (HTCAPTION: the OS drags the window, maximizes on double-click, shows the system
// menu and snaps), the maximize button is HTMAXBUTTON (Windows 11 snap layouts), the bar's
// controls stay the UI's. The UI publishes the layout at the end of each frame.

use std::sync::atomic::{AtomicBool, Ordering};

pub use solidcraft_frame::caption::CaptionLayout;

static CAPTION: std::sync::LazyLock<solidcraft_frame::CaptionMap> = std::sync::LazyLock::new(solidcraft_frame::CaptionMap::new);
static CAPTION_CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();
static MAX_HOVER: AtomicBool = AtomicBool::new(false);
static MAX_PRESSED: AtomicBool = AtomicBool::new(false);

fn bar_key() -> Id {
    Id::unique("titlebar-bar-this-pass")
}

/// The title bar's published layout, for the window procedure's hit test.
pub fn caption_map() -> &'static solidcraft_frame::CaptionMap {
    &CAPTION
}

/// The window procedure reports the OS-owned maximize button's pointer state.
pub fn set_native_maximize(hover: bool, pressed: bool) {
    let hover_changed = MAX_HOVER.swap(hover, Ordering::Relaxed) != hover;
    let pressed_changed = MAX_PRESSED.swap(pressed, Ordering::Relaxed) != pressed;
    if (hover_changed || pressed_changed)
        && let Some(ctx) = CAPTION_CTX.get()
    {
        ctx.request_repaint();
    }
}

fn native_maximize() -> (bool, bool) {
    (MAX_HOVER.load(Ordering::Relaxed), MAX_PRESSED.load(Ordering::Relaxed))
}

/// Most rectangles the map takes; more are merged (a merged area is the UI's, never a drag area).
const MAX_RECTS: usize = solidcraft_frame::caption::MAX_INTERACTIVE;

/// Publish this frame's title bar layout. Call at the end of the frame, after everything that can
/// lie over the bar (menus, popups, windows) is laid out. `native` is false where the OS doesn't
/// ask the map (no native frame): the map is then cleared. The layout it published, `None` when
/// it published none.
pub fn publish_caption(ctx: &egui::Context, native: bool) -> Option<CaptionLayout> {
    let _ = CAPTION_CTX.set(ctx.clone());
    let bar = ctx.data(|d| d.get_temp::<(Rect, u64)>(bar_key())).filter(|(_, pass)| *pass == ctx.cumulative_pass_nr()).map(|(r, _)| r);
    let Some(bar) = bar.filter(|_| native) else {
        if CAPTION.is_published() {
            CAPTION.clear();
        }
        return None;
    };
    let drag_id = Id::unique("titlebar-drag");
    let max_id = caption_id(Caption::Maximize);
    let mut interactive = Vec::new();
    let mut maximize = None;
    ctx.viewport(|v| {
        for (_, widgets) in v.this_pass.widgets.layers() {
            for w in widgets {
                if w.id == drag_id || !w.visible || !(w.sense.senses_click() || w.sense.senses_drag()) {
                    continue;
                }
                let r = w.interact_rect.intersect(bar);
                if !r.is_positive() {
                    continue;
                }
                if w.id == max_id {
                    maximize = Some(r);
                } else {
                    interactive.push(r);
                }
            }
        }
    });
    let ppp = ctx.pixels_per_point();
    let px = |r: Rect| {
        solidcraft_frame::PxRect::new(
            (r.min.x * ppp).floor() as i32,
            (r.min.y * ppp).floor() as i32,
            (r.max.x * ppp).ceil() as i32,
            (r.max.y * ppp).ceil() as i32,
        )
    };
    let interactive = merge_rects(interactive.into_iter().map(px).collect(), MAX_RECTS);
    let layout = CaptionLayout { bar: px(bar), drag: vec![px(bar)], interactive, maximize: maximize.map(px) };
    CAPTION.publish(&layout);
    Some(layout)
}

/// Merge overlapping rectangles, then (while there are more than `max`) the two nearest along x,
/// so the map never drops a control: a merged area only ever grows what is the UI's.
fn merge_rects(mut rects: Vec<solidcraft_frame::PxRect>, max: usize) -> Vec<solidcraft_frame::PxRect> {
    use solidcraft_frame::PxRect;
    let union = |a: PxRect, b: PxRect| PxRect::new(a.min_x.min(b.min_x), a.min_y.min(b.min_y), a.max_x.max(b.max_x), a.max_y.max(b.max_y));
    let touches = |a: &PxRect, b: &PxRect| a.min_x <= b.max_x && b.min_x <= a.max_x && a.min_y <= b.max_y && b.min_y <= a.max_y;
    rects.sort_by_key(|r| (r.min_x, r.min_y));
    let mut merged: Vec<PxRect> = Vec::with_capacity(rects.len());
    for r in rects {
        match merged.iter_mut().find(|m| touches(m, &r)) {
            Some(m) => *m = union(*m, r),
            None => merged.push(r),
        }
    }
    while merged.len() > max.max(1) {
        merged.sort_by_key(|r| r.min_x);
        let Some(i) = merged.windows(2).enumerate().min_by_key(|(_, w)| w[1].min_x - w[0].max_x).map(|(i, _)| i) else { break };
        let b = merged.remove(i + 1);
        if let Some(a) = merged.get_mut(i) {
            *a = union(*a, b);
        }
    }
    merged
}

// ---------------------------------------------------------------------------------------------
// Presses the OS takes
//
// On Windows the OS handles a press on the window's frame or title bar (move, size, the window
// menu), so egui never sees it. Menus and popups close on a press outside them: the window
// procedure reports these presses too ([`window_frame_pressed`]).

fn frame_press_key() -> Id {
    Id::unique("titlebar-frame-press")
}

/// The OS took a press on the window's frame or title bar (Windows' non-client area).
pub fn window_frame_pressed() {
    if let Some(ctx) = CAPTION_CTX.get() {
        report_frame_press(ctx);
    }
}

/// [`window_frame_pressed`] for the window of `ctx`.
pub fn report_frame_press(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(frame_press_key().with("pending"), true));
    ctx.request_repaint();
}

/// Start of a frame: a press the OS took since the last one closes egui's popups now, and the
/// app's own menus see it through [`frame_pressed`].
pub fn begin_frame(ctx: &egui::Context) {
    if ctx.data_mut(|d| d.remove_temp::<bool>(frame_press_key().with("pending"))).unwrap_or(false) {
        // Each window of the app counts its own frames: the press is this window's, this frame.
        let frame = (ctx.viewport_id(), ctx.cumulative_frame_nr());
        ctx.data_mut(|d| d.insert_temp(frame_press_key(), frame));
        egui::Popup::close_all(ctx);
    }
}

/// The OS took a press on this window's frame or title bar this frame: a menu or popup closes, as
/// for a press outside it. Another window of the app, such as a shared panel in its own desktop
/// window, sees no press.
pub fn frame_pressed(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<(egui::ViewportId, u64)>(frame_press_key())) == Some((ctx.viewport_id(), ctx.cumulative_frame_nr()))
}

// ---------------------------------------------------------------------------------------------
// The window menu
//
// Restore, Move, Size, Minimize, Maximize and Close in the app's own menu style. On Windows a
// right-click on the bare bar opens it: the bar is the OS's caption there, so the window
// procedure asks for it ([`open_window_menu`]). Alt+Space still opens the system's own menu.

/// The window's keyboard move and size, which only the platform can start (Windows' `SC_MOVE`,
/// `SC_SIZE`). Without them the window menu has no Move and Size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemCommand {
    Move,
    Size,
}

static SYSTEM_COMMANDS: std::sync::OnceLock<fn(SystemCommand)> = std::sync::OnceLock::new();

/// Let the window menu run Move and Size through `run` (the desktop app on Windows, once).
pub fn set_system_commands(run: fn(SystemCommand)) {
    let _ = SYSTEM_COMMANDS.set(run);
}

fn window_menu_key() -> Id {
    Id::unique("titlebar-window-menu")
}

/// Open the window menu at client pixel (`x`, `y`) of the window whose bar is published.
pub fn open_window_menu(x: i32, y: i32) {
    if let Some(ctx) = CAPTION_CTX.get() {
        request_window_menu(ctx, pos2(x as f32, y as f32));
    }
}

/// Ask `ctx`'s next frame to open the window menu at client pixel `at`.
pub fn request_window_menu(ctx: &egui::Context, at: Pos2) {
    ctx.data_mut(|d| d.insert_temp(window_menu_key().with("request"), at));
    ctx.request_repaint();
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WindowItem {
    Restore,
    Move,
    Size,
    Minimize,
    Maximize,
    Close,
}

/// The window menu, when the window asked for it; `anchor` is the bar's [`drag_area`]. The row
/// chosen, as the action for the app to run (as a caption button's); Move and Size run here.
pub fn window_menu(ui: &Ui, anchor: &egui::Response) -> Option<WindowAction> {
    let ctx = ui.ctx().clone();
    let key = window_menu_key();
    let popup_id = egui::Popup::default_response_id(anchor);
    let request = ctx.data_mut(|d| d.remove_temp::<Pos2>(key.with("request")));
    let ppp = ctx.pixels_per_point();
    if let Some(px) = request {
        ctx.data_mut(|d| {
            d.insert_temp(key.with("at"), pos2(px.x / ppp, px.y / ppp));
            d.insert_temp::<Option<usize>>(key.with("row"), None);
        });
    }
    let opening = request.is_some();
    let shown = egui::Popup::is_id_open(&ctx, popup_id);
    // Another app taking the focus closes it, as it does a system menu.
    if !opening && (!shown || !ctx.input(|i| i.focused)) {
        if shown {
            egui::Popup::close_id(&ctx, popup_id);
        }
        return None;
    }
    let max = maximized(&ctx);
    let system = SYSTEM_COMMANDS.get().copied();
    let mut rows = vec![(WindowItem::Restore, "window_restore", "ui.window_menu_restore", max)];
    if system.is_some() {
        rows.push((WindowItem::Move, "window_move", "ui.window_menu_move", !max));
        rows.push((WindowItem::Size, "window_size", "ui.window_menu_size", !max));
    }
    rows.push((WindowItem::Minimize, "window_minimize", "ui.window_menu_minimize", true));
    rows.push((WindowItem::Maximize, "window_maximize", "ui.window_menu_maximize", !max));
    rows.push((WindowItem::Close, "window_close", "ui.window_menu_close", true));
    let enabled: Vec<bool> = rows.iter().map(|r| r.3).collect();
    let mut row = ctx.data(|d| d.get_temp::<Option<usize>>(key.with("row"))).flatten();
    let mut chosen = crate::Menu::navigate(&ctx, &enabled, &mut row).then_some(row).flatten();
    ctx.data_mut(|d| d.insert_temp(key.with("row"), row));
    let at = ctx.data(|d| d.get_temp::<Pos2>(key.with("at"))).unwrap_or(anchor.rect.left_bottom());
    // The popup draws the menu's frame; the rows go straight inside it, as in the app's other menus.
    crate::Container::new(&ctx, crate::Surface::Popup)
        .menu(anchor)
        .anchor(at)
        .align(egui::RectAlign::BOTTOM_START)
        .frame(crate::Menu::frame())
        .open_memory(opening.then_some(egui::SetOpenCommand::Bool(true)))
        .show(|ui| {
            crate::Menu::rows(ui, crate::LIST_W, |ui| {
                for (k, (item, id, message, on)) in rows.iter().enumerate() {
                    if *item == WindowItem::Close {
                        crate::Menu::separator(ui);
                    }
                    let mut menu_item = crate::MenuItem::new(id, solidcraft_ui_text::tr(message), "");
                    menu_item.enabled = *on;
                    menu_item.highlighted = row == Some(k);
                    if *item == WindowItem::Close {
                        menu_item.shortcut = "Alt+F4";
                    }
                    if menu_item.show(ui).clicked() {
                        chosen = Some(k);
                    }
                }
            });
            crate::Container::finish_content(ui, crate::Menu::frame());
            if chosen.is_some() {
                ui.close();
            }
        });
    let item = chosen.and_then(|k| rows.get(k)).filter(|r| r.3).map(|r| r.0)?;
    match item {
        WindowItem::Restore => Some(WindowAction::Restore),
        WindowItem::Minimize => Some(WindowAction::Minimize),
        WindowItem::Maximize => Some(WindowAction::Maximize),
        WindowItem::Close => Some(WindowAction::Close),
        WindowItem::Move | WindowItem::Size => {
            if let Some(run) = system {
                run(if item == WindowItem::Move { SystemCommand::Move } else { SystemCommand::Size });
            }
            None
        }
    }
}

#[cfg(test)]
mod caption_tests {
    use super::*;
    use solidcraft_frame::PxRect;

    #[test]
    fn merging_keeps_every_control_covered_within_the_limit() {
        let rects: Vec<PxRect> = (0..100).map(|i| PxRect::new(i * 20, 0, i * 20 + 10, 30)).collect();
        let merged = merge_rects(rects.clone(), 32);
        assert!(merged.len() <= 32);
        for r in rects {
            assert!(merged.iter().any(|m| m.min_x <= r.min_x && m.max_x >= r.max_x), "{r:?} lost");
        }
    }

    #[test]
    fn overlapping_controls_merge_into_one() {
        let merged = merge_rects(vec![PxRect::new(0, 0, 10, 10), PxRect::new(5, 0, 20, 10), PxRect::new(100, 0, 110, 10)], 32);
        assert_eq!(merged, vec![PxRect::new(0, 0, 20, 10), PxRect::new(100, 0, 110, 10)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportCommand, ViewportId, pos2, vec2};

    const BAR: f32 = 34.0;

    #[test]
    fn a_frame_press_is_seen_in_the_window_it_came_from_only() {
        let ctx = crate::test_support::context();
        report_frame_press(&ctx);
        let mut seen = None;
        let _ = ctx.run_ui(RawInput::default(), |ui| {
            begin_frame(ui.ctx());
            seen = Some(frame_pressed(ui.ctx()));
        });
        assert_eq!(seen, Some(true));
        // A shared panel in its own desktop window counts its own frames; at the frame number of
        // the press it has no press.
        let id = ViewportId::from_hash_of("panel");
        let mut panel = RawInput { viewport_id: id, ..Default::default() };
        panel.viewports.insert(id, egui::ViewportInfo { parent: Some(ViewportId::ROOT), ..Default::default() });
        let _ = ctx.run_ui(panel, |ui| seen = Some(frame_pressed(ui.ctx())));
        assert_eq!(seen, Some(false));
    }

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
            Self { ctx: crate::test_support::context(), bar, width, time: 0.0, maximized: false, content: None, action: None }
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

    /// The only test here that publishes: the caption map is one per process.
    #[test]
    fn publishing_the_caption_returns_the_layout_the_hit_test_then_reads() {
        use solidcraft_frame::CaptionHit;
        let ctx = crate::test_support::context();
        let bar = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, BAR));
        let own = TitleBar { traffic_lights: false, caption_buttons: true };
        let frame = |native: bool| {
            let mut published = None;
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                let _ = own.show(ui, bar, |_, _| {});
                published = publish_caption(ui.ctx(), native);
            });
            published
        };
        let ppp = ctx.pixels_per_point();
        let hit = |p: Pos2| caption_map().hit((p.x * ppp) as i32, (p.y * ppp) as i32);
        let at = |c: Caption| ctx.read_response(caption_id(c)).map(|r| r.rect.center()).unwrap_or(Pos2::ZERO);
        // With the native caption the bar laid out this frame is published and returned.
        let layout = frame(true).expect("the bar laid out this frame is published");
        assert_eq!(caption_map().bar_and_maximize(), Some((layout.bar, layout.maximize)));
        assert!(layout.maximize.is_some(), "{layout:?}");
        assert_eq!(hit(pos2(600.0, BAR / 2.0)), CaptionHit::Drag, "the bare bar is the caption");
        assert_eq!(hit(at(Caption::Maximize)), CaptionHit::MaximizeButton);
        assert_eq!(hit(at(Caption::Minimize)), CaptionHit::Client);
        assert_eq!(hit(at(Caption::Close)), CaptionHit::Client);
        assert_eq!(hit(pos2(600.0, 300.0)), CaptionHit::Outside);
        // Without it nothing is published, and the layout published before is forgotten.
        assert_eq!(frame(false), None);
        assert!(!caption_map().is_published());
        assert_eq!(hit(pos2(600.0, BAR / 2.0)), CaptionHit::Outside);
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
