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
//! them there for the top edge (the system's own frame is kept on the left, right and bottom), so
//! the OS shows its resize cursors and resizes from the first frame; elsewhere egui zones above
//! everything else start an OS resize ([`resize_zones`]). The web build has neither.
//!
//! Such an app also answers the hit test from the bar itself ([`caption`]): every window that
//! draws the bar publishes what it laid out, each pass, and the app's one window procedure
//! answers for all of them. The bare bar is then the system's caption, so Windows drags the
//! window from the first pixel, and the maximize button is the system's, so hovering it shows
//! Windows 11's snap layouts. What the system keeps for itself there comes back from the
//! procedure as the input the UI would have had on a bar of its own: the pointer's place, a
//! right-click on the bare bar, and a press on the bare bar or on the maximize button. So what
//! shows while the pointer is on the bar stays shown, the bar's menu opens, and whatever closes
//! on a press elsewhere closes, with no code of its own. The bar leaves such a press to the
//! system ([`drag_area`], [`caption_buttons`]). A window nothing answers for keeps the bar as
//! the UI's own.
//!
//! Nothing here knows an app's state: closing is a [`WindowAction::Close`] the app answers (it may
//! ask about unsaved work first).
//!
//! A move or resize hands the press to the OS, which keeps the pointer until the button goes up.
//! On X11 the window manager then receives that release, not the app, so egui would keep the
//! button down and the next press would only end the old drag: the bar moved the window every
//! other time (#13334). An app calls [`release_press_taken_by_os`] first in its raw-input hook, then [`release_stale_input`].
//!
//! Adapted from VectorCraft's `crates/ui-egui/src/titlebar.rs` (MIT OR Apache-2.0).

use crate::BackingTooltipExt as _;
use crate::icons::glyphs::paint_glyph;
use egui::{
    Color32, CursorIcon, Id, LayerId, Order, PointerButton, Pos2, Rect, ResizeDirection, Sense, Stroke, Ui, ViewportBuilder, ViewportCommand, pos2,
    vec2,
};
use solidcraft_theme::Tokens;

pub mod caption;
pub mod native_menu;

/// Associate a native pop-out with the SolidCraft viewport that opened it.
pub fn popout_viewport(builder: ViewportBuilder, owner: egui::ViewportId) -> ViewportBuilder {
    builder.with_native_owner(owner)
}

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
///
/// Where the system answers for the window ([`caption`]) it does both itself, from the first
/// pixel: the bar laid out here is what the window hands it.
pub fn drag_area(ui: &Ui, rect: Rect) {
    let ctx = ui.ctx();
    install(ctx);
    // The bar this pass, for the system's hit test. The key and the pass number are read before
    // taking the data lock: egui's context lock is not re-entrant.
    let (key, pass) = (bar_key(ctx), ctx.cumulative_pass_nr());
    ctx.data_mut(|d| d.insert_temp(key, (rect, pass)));
    let resp = ui.interact(rect, Id::unique("titlebar-drag"), Sense::click_and_drag());
    // Where the system answers for the window, the move and the maximize are its own. A press on
    // the bare bar still reaches the UI ([`caption`]), so that whatever closes on a press
    // elsewhere closes: the bar does nothing with it.
    if systems_window(ctx).is_some() {
        return;
    }
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

#[derive(Clone, Copy, Default)]
struct StaleInput {
    buttons: [bool; 3],
}

fn button_index(button: PointerButton) -> Option<usize> {
    match button {
        PointerButton::Primary => Some(0),
        PointerButton::Secondary => Some(1),
        PointerButton::Middle => Some(2),
        _ => None,
    }
}

fn stale_input_id() -> Id {
    Id::unique("window-chrome-stale-input")
}

/// Note that a native dialog (a file picker) just closed: the OS may have kept a button release
/// or a modifier release for itself, so [`release_stale_input`] lets go of what is still held.
pub fn mark_input_stale(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(stale_input_id(), StaleInput { buttons: [true; 3] }));
}

/// Let go of a pointer button whose release the app never received, before the frame's own
/// events: after the window lost focus or a native dialog closed (`recover_after_focus`), when
/// the pointer comes back into the window after leaving it with a button held and no motion
/// reported outside, and when a button egui still holds is pressed again
/// (`release_lost_presses`). Otherwise egui keeps the button down and the canvas ignores every
/// pick (#16460).
pub fn release_stale_input(ctx: &egui::Context, raw: &mut egui::RawInput) {
    recover_after_focus(ctx, raw, true);
    release_lost_presses(ctx, raw);
}

/// [`release_stale_input`] for a platform that reports the buttons it holds itself and lets go of
/// them when the window really loses focus: the web app's `mouse_buttons`, which runs first. The
/// focus moving inside the page, from the canvas to eframe's hidden text input and back as a value
/// box takes or gives up the keyboard, reaches egui as the window losing and regaining focus; that
/// must not let go of a held button (#16209, #21283). Modifiers are still cleared.
pub fn release_stale_input_with_tracked_buttons(ctx: &egui::Context, raw: &mut egui::RawInput) {
    recover_after_focus(ctx, raw, false);
    release_lost_presses(ctx, raw);
}

/// After the window lost focus (or a native dialog closed), on the next frame the window has
/// focus: release each pointer button egui still holds down, at the last pointer position, before
/// the frame's own events. On Wayland a file dialog takes the pointer and keyboard, so a release
/// may never arrive, and egui would keep the button down and ignore canvas picks (#16460). A
/// regained focus also clears the modifiers; the OS reports the real ones after. With
/// `release_buttons` false the platform keeps the buttons right itself, and only the modifiers go.
fn recover_after_focus(ctx: &egui::Context, raw: &mut egui::RawInput, release_buttons: bool) {
    let last_lost = raw.events.iter().rposition(|e| matches!(e, egui::Event::WindowFocused(false)));
    let regained = raw.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)));
    let stale = ctx.data(|d| d.get_temp::<StaleInput>(stale_input_id()));
    let (was_focused, current_modifiers) = ctx.input(|i| (i.focused, i.modifiers));
    // Focus loss already cleared egui's modifiers; any non-empty modifiers present while unfocused
    // arrived after that loss (for example Ctrl held while moving into an unfocused window) and
    // must survive focus return, or the platform will not resend them until the key is re-pressed.
    let clear_modifiers = was_focused || current_modifiers.is_none();
    // Focus ends the frame lost: record which buttons were held at loss so a fresh press while
    // unfocused is not mistaken for a stale one when focus returns. Otherwise recover now: at the
    // loss when the batch also regains focus (later events, a fresh press among them, stay as they
    // are), or at the start of the frame for a marked dialog or returned focus.
    let (at, stale_buttons) = match last_lost {
        Some(lost) if !raw.focused => {
            let mut buttons = [false; 3];
            for (idx, b) in BUTTONS.into_iter().enumerate() {
                buttons[idx] = ctx.input(|i| i.pointer.button_down(b));
            }
            for (index, event) in raw.events.iter().enumerate() {
                if let egui::Event::PointerButton { button, pressed, .. } = event
                    && let Some(idx) = button_index(*button)
                {
                    buttons[idx] = if index <= lost { *pressed } else { false };
                }
            }
            ctx.data_mut(|d| d.insert_temp(stale_input_id(), StaleInput { buttons }));
            return;
        }
        Some(i) => {
            let mut buttons = [false; 3];
            for (idx, b) in BUTTONS.into_iter().enumerate() {
                buttons[idx] = ctx.input(|input| input.pointer.button_down(b));
            }
            for event in raw.events.iter().take(i + 1) {
                if let egui::Event::PointerButton { button, pressed, .. } = event
                    && let Some(idx) = button_index(*button)
                {
                    buttons[idx] = *pressed;
                }
            }
            (i + 1, buttons)
        }
        None if raw.focused && stale.is_some() => (0, stale.map_or([false; 3], |s| s.buttons)),
        None => {
            if !raw.focused
                && let Some(mut s) = stale
            {
                let mut changed = false;
                for event in &raw.events {
                    if let egui::Event::PointerButton { button, .. } = event
                        && let Some(idx) = button_index(*button)
                        && s.buttons[idx]
                    {
                        s.buttons[idx] = false;
                        changed = true;
                    }
                }
                if changed {
                    ctx.data_mut(|d| d.insert_temp(stale_input_id(), s));
                }
            }
            if regained && clear_modifiers {
                raw.events.insert(0, egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            }
            return;
        }
    };
    ctx.data_mut(|d| d.remove_temp::<StaleInput>(stale_input_id()));
    let (pos, modifiers) = ctx.input(|i| (i.pointer.latest_pos().or(i.pointer.interact_pos()).unwrap_or(Pos2::ZERO), i.modifiers));
    let held: Vec<PointerButton> = BUTTONS
        .into_iter()
        .enumerate()
        .filter(|&(idx, b)| release_buttons && stale_buttons[idx] && (last_lost.is_some() || ctx.input(|i| i.pointer.button_down(b))))
        .map(|(_, b)| b)
        .collect();
    let mut recovery = Vec::new();
    if !held.is_empty() {
        // The release must not complete the interrupted press as a click, which would activate
        // the widget under it: egui drops click eligibility once the pointer is far from the press.
        let away = pos + egui::vec2(1000.0, 1000.0);
        recovery.push(egui::Event::PointerMoved(away));
        recovery.extend(held.into_iter().map(|button| egui::Event::PointerButton { pos: away, button, pressed: false, modifiers }));
        recovery.push(egui::Event::PointerGone);
    }
    if last_lost.is_some() || clear_modifiers {
        recovery.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    }
    raw.events.splice(at..at, recovery);
}

fn left_holding_id() -> Id {
    Id::unique("window-chrome-left-holding")
}

const BUTTONS: [PointerButton; 3] = [PointerButton::Primary, PointerButton::Secondary, PointerButton::Middle];

/// A held button whose release went elsewhere. While a button is held the platform keeps the
/// pointer with the window: X11, Windows and macOS go on reporting its motion outside the window,
/// and Wayland does not let it leave at all. So a pointer that left with a button held and comes
/// back with no motion reported outside lost that press to the system (a compositor gesture or
/// grab took the release), and a press of a button egui still holds means its release was lost.
/// Either way the stale press is let go first, away from where it was pressed so it is no click.
fn release_lost_presses(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let window = raw.screen_rect.unwrap_or_else(|| ctx.viewport_rect());
    let mut held: Vec<PointerButton> = BUTTONS.into_iter().filter(|b| ctx.input(|i| i.pointer.button_down(*b))).collect();
    let mut left_holding = ctx.data(|d| d.get_temp::<bool>(left_holding_id())).unwrap_or(false);
    let (last, modifiers) = ctx.input(|i| (i.pointer.latest_pos().or(i.pointer.interact_pos()).unwrap_or(Pos2::ZERO), i.modifiers));
    let release_all = |held: &mut Vec<PointerButton>, at: Pos2, out: &mut Vec<egui::Event>| {
        let away = at + egui::vec2(1000.0, 1000.0);
        out.push(egui::Event::PointerMoved(away));
        out.extend(held.drain(..).map(|button| egui::Event::PointerButton { pos: away, button, pressed: false, modifiers }));
    };
    let mut events = Vec::with_capacity(raw.events.len());
    let mut at = last;
    for event in std::mem::take(&mut raw.events) {
        match &event {
            egui::Event::PointerGone => left_holding = !held.is_empty(),
            egui::Event::PointerMoved(p) => {
                if left_holding && !held.is_empty() && window.contains(*p) {
                    release_all(&mut held, at, &mut events);
                }
                left_holding = false;
                at = *p;
            }
            egui::Event::PointerButton { pos, button, pressed: true, .. } => {
                if held.contains(button) {
                    held.retain(|b| b != button);
                    let away = *pos + egui::vec2(1000.0, 1000.0);
                    events.push(egui::Event::PointerMoved(away));
                    events.push(egui::Event::PointerButton { pos: away, button: *button, pressed: false, modifiers });
                    events.push(egui::Event::PointerMoved(*pos));
                }
                held.push(*button);
                left_holding = false;
                at = *pos;
            }
            egui::Event::PointerButton { button, pressed: false, .. } => held.retain(|b| b != button),
            _ => {}
        }
        events.push(event);
    }
    raw.events = events;
    ctx.data_mut(|d| d.insert_temp(left_holding_id(), left_holding && !held.is_empty()));
}

/// Make the window set its pointer cursor again when focus returns or the pointer comes back
/// into the window. egui-winit sends a cursor to the window only when the requested icon
/// changes, but on Wayland the compositor shows the previous program's cursor after the pointer
/// enters until the client sets its own. `CursorVisible(true)` makes winit apply its current
/// cursor with the latest enter serial. A native app calls this in its raw-input hook; the web
/// build has no window cursor to refresh.
pub fn reapply_cursor_on_return(ctx: &egui::Context, raw: &egui::RawInput) {
    let focus_back = raw.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)));
    let pointer_back = ctx.input(|i| i.pointer.latest_pos().is_none()) && raw.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_)));
    if focus_back || pointer_back {
        ctx.send_viewport_cmd(ViewportCommand::CursorVisible(true));
    }
}

/// The tooltip of caption button `c`, in the current language; `max` says the window is maximized.
fn caption_tip(c: Caption, max: bool) -> &'static str {
    solidcraft_ui_text::tr(match c {
        Caption::Minimize => "ui.window_minimize",
        Caption::Maximize if max => "ui.window_restore",
        Caption::Maximize => "ui.window_maximize",
        Caption::Close => "ui.close",
    })
}

/// Minimize, Maximize/Restore and Close at the right end of `bar`, full bar height, flush with
/// the window's edge so a maximized window's corner hits Close. The one clicked, as the action
/// for the app to run.
pub fn caption_buttons(ui: &mut Ui, bar: Rect) -> Option<WindowAction> {
    let t = Tokens::get();
    let max = maximized(ui.ctx());
    // The system answers for this window: the maximize button is its own (snap layouts).
    let systems = systems_window(ui.ctx());
    let mut clicked = None;
    for (i, c) in CAPTIONS.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(bar.right() - WIDTH + i as f32 * BUTTON_WIDTH, bar.top()), vec2(BUTTON_WIDTH, bar.height()));
        let resp = ui.interact(r, caption_id(c), Sense::click());
        // The pointer over the system's maximize button reaches the UI as anywhere else; the
        // press is the system's, and the window procedure says while it is held.
        let systems_maximize = systems.filter(|_| c == Caption::Maximize);
        let held = systems_maximize.is_some_and(caption::maximize_pressed);
        let fill = match (resp.is_pointer_button_down_on() || held, resp.hovered()) {
            (true, _) => t.app_bar_hover_subtle,
            (_, true) => t.app_bar_hover_button,
            _ if resp.has_focus() => t.app_bar_hover_button,
            _ => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(r, 0.0, fill);
        paint_glyph(ui, c, max, r.center(), Stroke::new(1.0, t.app_bar_text));
        // No tooltip of the UI's over the system's button: its snap layouts open there. A screen
        // reader still gets the button's name.
        let tip = caption_tip(c, max);
        let resp = if systems_maximize.is_some() {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
            resp
        } else {
            resp.backing_on_hover_text(tip)
        };
        // A pointer click on the system's maximize button maximizes from the window procedure: here
        // it is a press like any other on the bar, and no action of the UI's. The keyboard and a
        // screen reader still press the button here.
        let systems_click = systems_maximize.is_some() && resp.clicked_by(PointerButton::Primary);
        if resp.clicked() && !systems_click {
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
/// starts an OS resize in that direction. None while the window is maximized or fullscreen, and
/// none in a window the system answers for: its frame resizes it.
pub fn resize_zones(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if maximized(&ctx) || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) || systems_window(&ctx).is_some() {
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
// The system's caption
//
// Where the desktop app answers the system's hit test (Windows), every window that draws the bar
// hands it over through [`caption`]. One egui plugin, added by the first bar drawn, does it for
// every window: at the end of each pass it publishes the bar that pass laid out, and at the start
// of each pass it gives the UI what the system kept for itself since the window's last one.

/// The native window the current viewport is shown in, where the integration tells (Windows).
fn native_window(ctx: &egui::Context) -> Option<u64> {
    ctx.input(|i| i.viewport().native_window)
}

/// The current viewport's native window when the system answers for it: its title bar and frame
/// are the system's. `None` where the bar is the UI's own.
fn systems_window(ctx: &egui::Context) -> Option<u64> {
    native_window(ctx).filter(|window| caption::is_hooked(*window))
}

/// Where a window keeps the bar it laid out this pass. Each window of the app has its own: a
/// pop-out view draws its bar inside the main window's frame.
fn bar_key(ctx: &egui::Context) -> Id {
    Id::unique("titlebar-bar-this-pass").with(ctx.viewport_id())
}

/// The bar this window laid out this pass ([`drag_area`]), if it laid one out.
fn bar_this_pass(ctx: &egui::Context) -> Option<Rect> {
    let (key, pass) = (bar_key(ctx), ctx.cumulative_pass_nr());
    ctx.data(|d| d.get_temp::<(Rect, u64)>(key)).filter(|(_, at)| *at == pass).map(|(r, _)| r)
}

/// Make every window of `ctx` that draws the bar hand it to the platform's side ([`caption`]).
/// Cheap to call every frame: the plugin is added once. The app calls it at its start, so the
/// first pass already hands its bar over; [`drag_area`] calls it too, and a plugin added during
/// a pass is first called in the next one.
pub fn install(ctx: &egui::Context) {
    let installed = Id::unique("titlebar-system-caption-installed");
    if !ctx.data(|d| d.get_temp::<bool>(installed).unwrap_or(false)) {
        ctx.add_plugin(SystemCaption);
        ctx.data_mut(|d| d.insert_temp(installed, true));
    }
}

/// The plugin: one for all the windows of a context.
struct SystemCaption;

impl egui::Plugin for SystemCaption {
    fn debug_name(&self) -> &'static str {
        "solidcraft_system_caption"
    }

    // Raw input, before the pass: what the system kept for itself goes in as the input the UI
    // would otherwise have had.
    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let Some(window) = input.viewports.get(&input.viewport_id).and_then(|v| v.native_window) else { return };
        if let Some(reported) = caption::take_reported(window) {
            give_reported(ctx, input, &reported);
        }
    }

    // Last: everything that can lie over the bar (menus, popups, windows) is laid out by now.
    fn on_end_pass(&mut self, ui: &mut Ui) {
        publish_caption(ui.ctx());
    }
}

/// The events of a click with `button` at `pos`: the pointer there, the button down, the button up.
fn click(pos: Pos2, button: PointerButton, modifiers: egui::Modifiers) -> [egui::Event; 3] {
    let button_event = |pressed| egui::Event::PointerButton { pos, button, pressed, modifiers };
    [egui::Event::PointerMoved(pos), button_event(true), button_event(false)]
}

/// Put what the platform `reported` for a window into the `input` of its pass, as the input the
/// UI would have had on a bar of its own. The pointer over the caption is the pointer anywhere
/// else (hover, a bar that shows while the pointer is on it). A press the system took is a press
/// where it was, on the bare bar or on the maximize button: whatever closes on a press elsewhere
/// closes, and the bar itself leaves the press to the system ([`drag_area`],
/// [`caption_buttons`]). A right-click is a right-click.
fn give_reported(ctx: &egui::Context, input: &mut egui::RawInput, reported: &caption::Reported) {
    let ppp = reported.pixels_per_point;
    let at = |(x, y): (i32, i32)| pos2(x as f32 / ppp, y as f32 / ppp);
    let modifiers = ctx.input(|i| i.modifiers);
    // Presses and clicks happened before whatever the pass's own events tell: they go first.
    let mut first: Vec<egui::Event> = Vec::new();
    for press in &reported.presses {
        first.extend(click(at(*press), PointerButton::Primary, modifiers));
    }
    for secondary in &reported.secondary_clicks {
        first.extend(click(at(*secondary), PointerButton::Secondary, modifiers));
    }
    match reported.pointer {
        // Before the pass's own events: it left for the client area, whose moves follow, or for
        // somewhere else.
        caption::PointerChange::Left => first.push(egui::Event::PointerGone),
        // After them: on its way onto the caption the pointer left the client area.
        caption::PointerChange::Moved(p) => input.events.push(egui::Event::PointerMoved(at(p))),
        // The window's own word that the pointer left the client area can come after the UI was
        // told it is on the caption: it still is.
        caption::PointerChange::Same => {
            if let Some(p) = reported.pointer_at
                && input.events.iter().any(|event| matches!(event, egui::Event::PointerGone))
            {
                input.events.push(egui::Event::PointerMoved(at(p)));
            }
        }
    }
    input.events.splice(0..0, first);
}

/// End of a pass: hand the bar this window laid out in it (none: the window has no caption now)
/// to the platform's side, and ask the platform to answer for the window the first time it has
/// a bar.
fn publish_caption(ctx: &egui::Context) {
    let Some(window) = native_window(ctx) else { return };
    let layout = bar_this_pass(ctx).and_then(|bar| caption_layout(ctx, bar));
    if caption::publish(window, ctx, ctx.viewport_id(), ctx.pixels_per_point(), layout)
        && let Some(hook) = caption::platform()
    {
        hook(window);
    }
}

/// `bar` as the platform's side needs it: whatever takes clicks or drags over the bar's own drag
/// area (the caption) in this pass is the UI's, but for the maximize button. As in egui's own hit
/// test, that is what the bar's layer lays out after the drag area, and the layers in front of
/// it: a bar that slides in over a view leaves the view under it out. `None` when the drag area
/// took no place among this pass's widgets (a pass that only measures): nothing is the caption.
fn caption_layout(ctx: &egui::Context, bar: Rect) -> Option<caption::CaptionLayout> {
    let drag_id = Id::unique("titlebar-drag");
    let max_id = caption_id(Caption::Maximize);
    // How far to the front a layer is, as the hit test sorts them: by its order, then by its place
    // among the areas. A layer that takes no pointer (a tooltip) is in front of nothing.
    let areas: Vec<LayerId> = ctx.memory(|m| m.layer_ids().collect());
    let depth = |layer: LayerId| (layer.order, areas.iter().position(|l| *l == layer));
    let mut interactive = Vec::new();
    let mut maximize = None;
    let laid_out = ctx.viewport(|v| {
        let Some((drag_layer, drag_index)) = v.this_pass.widgets.order(drag_id) else { return false };
        let drag_depth = depth(drag_layer);
        for (layer, widgets) in v.this_pass.widgets.layers() {
            let first = if *layer == drag_layer {
                drag_index + 1
            } else if layer.order.allow_interaction() && depth(*layer) > drag_depth {
                0
            } else {
                continue;
            };
            for w in widgets.iter().skip(first) {
                if !w.visible || !(w.sense.senses_click() || w.sense.senses_drag()) {
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
        true
    });
    if !laid_out {
        return None;
    }
    let ppp = ctx.pixels_per_point();
    let px = |r: Rect| {
        caption::PxRect::new(
            (r.min.x * ppp).floor() as i32,
            (r.min.y * ppp).floor() as i32,
            (r.max.x * ppp).ceil() as i32,
            (r.max.y * ppp).ceil() as i32,
        )
    };
    Some(caption::CaptionLayout { bar: px(bar), interactive: interactive.into_iter().map(px).collect(), maximize: maximize.map(px) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportCommand, ViewportId, pos2, vec2};

    const BAR: f32 = 34.0;

    /// Each test that hands a bar to the platform's side has native windows of its own: the list
    /// of them is one per process.
    const WINDOWS: u64 = 0xC4A0_0000;

    /// What the system took in one window is input for that window's UI only: a pop-out view
    /// draws inside the main window's frame, and each has its own presses.
    #[test]
    fn a_press_on_one_windows_caption_is_no_press_in_another() {
        let (main_window, pop_out_window) = (WINDOWS + 60, WINDOWS + 61);
        let ctx = crate::test_support::context();
        install(&ctx);
        let bar = Rect::from_min_size(Pos2::ZERO, vec2(600.0, BAR));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0));
        let pop_out = ViewportId::from_hash_of("pop-out view with a caption");
        // One frame of both windows: whether each saw a press.
        let frame = || {
            let mut input = RawInput { screen_rect: Some(screen), ..Default::default() };
            if let Some(v) = input.viewports.get_mut(&ViewportId::ROOT) {
                v.native_window = Some(main_window);
            }
            let (mut main, mut other) = (false, false);
            let _ = ctx.run_ui(input, |ui| {
                drag_area(ui, bar);
                main = ui.input(|i| i.pointer.any_pressed());
                let mut child = RawInput { viewport_id: pop_out, screen_rect: Some(screen), ..Default::default() };
                let info = egui::ViewportInfo { parent: Some(ViewportId::ROOT), native_window: Some(pop_out_window), ..Default::default() };
                child.viewports.insert(pop_out, info);
                let _ = ui.ctx().run_ui(child, |ui| {
                    drag_area(ui, bar);
                    other = ui.input(|i| i.pointer.any_pressed());
                });
            });
            (main, other)
        };
        assert_eq!(frame(), (false, false));
        caption::pressed(pop_out_window, 300, 17);
        assert_eq!(frame(), (false, true));
        assert_eq!(frame(), (false, false), "once");
        caption::pressed(main_window, 300, 17);
        assert_eq!(frame(), (true, false));
        caption::closed(main_window);
        caption::closed(pop_out_window);
    }

    /// A headless window `width` pt wide that draws only a title bar, from plain inputs.
    struct Bar {
        ctx: egui::Context,
        bar: TitleBar,
        width: f32,
        time: f64,
        maximized: bool,
        /// The native window it is shown in, as the desktop integration tells on Windows.
        window: Option<u64>,
        /// The rect the bar handed its contents, and the action it returned, last frame.
        content: Option<Rect>,
        action: Option<WindowAction>,
        /// Last frame had a pointer press.
        pressed: bool,
    }

    impl Bar {
        fn new(bar: TitleBar, width: f32) -> Self {
            let ctx = crate::test_support::context();
            // As the app does at its start.
            install(&ctx);
            Self { ctx, bar, width, time: 0.0, maximized: false, window: None, content: None, action: None, pressed: false }
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
                v.native_window = self.window;
            }
            let (bar, rect) = (self.bar, self.rect());
            let (mut content, mut action, mut pressed) = (None, None, false);
            let mut out = self.ctx.run_ui(input, |ui| {
                action = bar.show(ui, rect, |_, r| content = Some(r));
                pressed = ui.input(|i| i.pointer.any_pressed());
            });
            out.textures_delta.clear();
            (self.content, self.action, self.pressed) = (content, action, pressed);
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
    fn caption_tooltips_come_from_the_catalog_and_follow_the_window_state() {
        crate::test_support::install_catalog();
        let tips = |max| CAPTIONS.map(|c| caption_tip(c, max));
        assert_eq!(tips(false), ["Minimize", "Maximize", "Close"], "SolidCraft's English tooltips are unchanged");
        assert_eq!(tips(true), ["Minimize", "Restore", "Close"]);
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

    /// #21283: in the web app the focus moving from the canvas to eframe's hidden text input (and
    /// back) reaches egui as the window losing and regaining focus, in one batch or over two
    /// frames. With the buttons tracked by the platform, a held button stays held and only the
    /// modifiers are cleared; the native hook still lets go of it.
    #[test]
    fn a_focus_move_inside_the_page_keeps_a_tracked_button_held() {
        type Hook = fn(&egui::Context, &mut RawInput);
        let run = |hook: Hook, batches: &[(Vec<Event>, bool)]| {
            let ctx = crate::test_support::context();
            let at = pos2(200.0, 300.0);
            let press = Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: Default::default() };
            let mut time = 0.0;
            let mut frame = |events: Vec<Event>, focused: bool| {
                time += 0.05;
                let mut raw = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    time: Some(time),
                    focused,
                    events,
                    ..Default::default()
                };
                hook(&ctx, &mut raw);
                let mut out = ctx.run_ui(raw, |_| {});
                out.textures_delta.clear();
            };
            frame(vec![Event::WindowFocused(true), Event::PointerMoved(at)], true);
            frame(vec![press], true);
            for (events, focused) in batches {
                frame(events.clone(), *focused);
            }
            frame(vec![], true);
            ctx.input(|i| (i.pointer.button_down(PointerButton::Middle), i.modifiers))
        };
        let one_batch = vec![(vec![Event::WindowFocused(false), Event::WindowFocused(true)], true)];
        let two_frames = vec![(vec![Event::WindowFocused(false)], false), (vec![Event::WindowFocused(true)], true)];
        for batches in [&one_batch, &two_frames] {
            assert_eq!(
                run(release_stale_input_with_tracked_buttons, batches),
                (true, egui::Modifiers::NONE),
                "the platform's buttons are kept: {batches:?}"
            );
            assert!(!run(release_stale_input, batches).0, "the native hook lets go of a button it may not hear released: {batches:?}");
        }
    }

    const OWN: TitleBar = TitleBar { traffic_lights: false, caption_buttons: true };

    #[test]
    fn a_window_that_draws_the_bar_publishes_it_for_the_systems_hit_test() {
        use caption::CaptionHit;
        let window = WINDOWS + 1;
        let mut w = Bar::new(OWN, 1200.0);
        let ppp = w.ctx.pixels_per_point();
        let hit = |p: Pos2| caption::hit(window, (p.x * ppp) as i32, (p.y * ppp) as i32);
        let bare = pos2(600.0, BAR / 2.0);
        // Where the integration names no native window (other systems, the web), nothing is
        // handed over.
        w.frame(vec![]);
        assert_eq!((hit(bare), caption::title_bar(window)), (CaptionHit::Outside, None));
        // With one, each pass publishes the bar it laid out.
        w.window = Some(window);
        w.frame(vec![]);
        let at = |c: Caption| w.ctx.read_response(caption_id(c)).map(|r| r.rect.center()).unwrap_or(Pos2::ZERO);
        assert_eq!(hit(bare), CaptionHit::Drag, "the bare bar is the caption");
        assert_eq!(hit(at(Caption::Maximize)), CaptionHit::MaximizeButton);
        assert_eq!(hit(at(Caption::Minimize)), CaptionHit::Client);
        assert_eq!(hit(at(Caption::Close)), CaptionHit::Client);
        assert_eq!(hit(pos2(600.0, 300.0)), CaptionHit::Outside);
        let (bar, maximize) = caption::title_bar(window).expect("the bar is published");
        assert_eq!((bar.min_x, bar.min_y, bar.max_x, bar.max_y), (0, 0, (1200.0 * ppp).ceil() as i32, (BAR * ppp).ceil() as i32));
        assert!(maximize.is_some_and(|m| m.min_x > bar.min_x && m.max_x < bar.max_x), "{maximize:?} in {bar:?}");
        // A pass that draws no bar (the system's own title bar, full screen): no caption.
        w.bar = TitleBar { traffic_lights: false, caption_buttons: false };
        w.frame(vec![]);
        assert_eq!((hit(bare), caption::title_bar(window)), (CaptionHit::Outside, None));
        // And the bar again as soon as it is drawn again.
        w.bar = OWN;
        w.frame(vec![]);
        assert_eq!(hit(bare), CaptionHit::Drag);
        caption::closed(window);
    }

    /// A pop-out view draws its bar inside the main window's frame, as an immediate viewport
    /// does: each window publishes its own bar, and neither touches the other's.
    #[test]
    fn each_window_publishes_its_own_bar() {
        use caption::CaptionHit;
        let (main_window, pop_out_window) = (WINDOWS + 10, WINDOWS + 11);
        let ctx = crate::test_support::context();
        install(&ctx);
        let bar = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, BAR));
        let pop_out_bar = Rect::from_min_size(Pos2::ZERO, vec2(480.0, BAR));
        let pop_out = ViewportId::from_hash_of("pop-out view");
        let main_input = || {
            let mut input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))), ..Default::default() };
            if let Some(v) = input.viewports.get_mut(&ViewportId::ROOT) {
                v.native_window = Some(main_window);
            }
            input
        };
        let (mut main, mut other) = (None, None);
        let _ = ctx.run_ui(main_input(), |ui| {
            drag_area(ui, bar);
            let mut child =
                RawInput { viewport_id: pop_out, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(480.0, 320.0))), ..Default::default() };
            let info = egui::ViewportInfo { parent: Some(ViewportId::ROOT), native_window: Some(pop_out_window), ..Default::default() };
            child.viewports.insert(pop_out, info);
            let _ = ui.ctx().run_ui(child, |ui| {
                drag_area(ui, pop_out_bar);
                other = bar_this_pass(ui.ctx());
            });
            main = bar_this_pass(ui.ctx());
        });
        assert_eq!(other, Some(pop_out_bar), "the pop-out's own bar");
        assert_eq!(main, Some(bar), "the main window's bar, not the pop-out's laid out after it");
        let ppp = ctx.pixels_per_point();
        let px = |v: f32| (v * ppp) as i32;
        assert_eq!(caption::hit(main_window, px(900.0), px(BAR / 2.0)), CaptionHit::Drag);
        assert_eq!(caption::hit(pop_out_window, px(900.0), px(BAR / 2.0)), CaptionHit::Outside, "the pop-out's bar is 480 wide");
        assert_eq!(caption::hit(pop_out_window, px(240.0), px(BAR / 2.0)), CaptionHit::Drag);
        // A window that lays out no bar this pass has none: last pass's bar is not this pass's.
        // The pop-out, not drawn in this frame, keeps the bar of its own last pass.
        let mut stale = Some(bar);
        let _ = ctx.run_ui(main_input(), |ui| stale = bar_this_pass(ui.ctx()));
        assert_eq!(stale, None);
        assert_eq!(caption::hit(main_window, px(900.0), px(BAR / 2.0)), CaptionHit::Outside);
        assert_eq!(caption::hit(pop_out_window, px(240.0), px(BAR / 2.0)), CaptionHit::Drag);
        caption::closed(main_window);
        caption::closed(pop_out_window);
    }

    /// The system keeps the pointer over its caption for itself. The window procedure says where
    /// it is, and the UI sees it there as anywhere else in the window: what shows while the
    /// pointer is on the bar stays shown.
    #[test]
    fn the_pointer_over_the_systems_caption_still_reaches_the_ui() {
        let window = WINDOWS + 20;
        let mut w = Bar::new(OWN, 1200.0);
        w.window = Some(window);
        w.frame(vec![]);
        w.frame(vec![]);
        let ppp = w.ctx.pixels_per_point();
        let on_caption = Some(((600.0 * ppp) as i32, (10.0 * ppp) as i32));
        let hover = |w: &Bar| w.ctx.input(|i| i.pointer.hover_pos());
        assert_eq!(hover(&w), None);
        caption::pointer(window, on_caption);
        w.frame(vec![]);
        assert_eq!(hover(&w), Some(pos2(600.0, 10.0)));
        w.frame(vec![]);
        assert_eq!(hover(&w), Some(pos2(600.0, 10.0)), "it stays there");
        // The window's own word that the pointer left the client area may come a frame late.
        w.frame(vec![Event::PointerGone]);
        assert_eq!(hover(&w), Some(pos2(600.0, 10.0)), "still on the caption");
        // On into the client area: the window's own events say where it is.
        caption::pointer(window, None);
        w.frame(vec![Event::PointerMoved(pos2(600.0, 300.0))]);
        assert_eq!(hover(&w), Some(pos2(600.0, 300.0)));
        // Back onto the caption (the window's own events say it left the client area), then out
        // of the window altogether.
        caption::pointer(window, on_caption);
        w.frame(vec![Event::PointerGone]);
        assert_eq!(hover(&w), Some(pos2(600.0, 10.0)), "on the caption, not gone");
        caption::pointer(window, None);
        w.frame(vec![]);
        assert_eq!(hover(&w), None);
        caption::closed(window);
    }

    /// A right-click on the system's caption comes back from the window procedure as the
    /// right-click it is anywhere else in the window (the app's menu for the bar opens as it does
    /// on a bar that is the UI's own), and a press the system takes on the caption or on the
    /// maximize button as the press on the bar it is where the bar is the UI's own: whatever
    /// closes on a press elsewhere sees it, and the bar does nothing with it.
    #[test]
    fn what_the_system_takes_on_its_caption_comes_back_to_the_ui() {
        let window = WINDOWS + 30;
        let mut w = Bar::new(OWN, 1200.0);
        w.window = Some(window);
        w.frame(vec![]);
        caption::hooked(window);
        let at = pos2(400.0, BAR / 2.0);
        let ppp = w.ctx.pixels_per_point();
        let right_click = |w: &Bar| w.ctx.input(|i| (i.pointer.secondary_clicked(), i.pointer.interact_pos()));
        assert!(!right_click(&w).0);
        caption::secondary_click(window, (at.x * ppp) as i32, (at.y * ppp) as i32);
        w.frame(vec![]);
        assert_eq!(right_click(&w), (true, Some(at)), "a right-click, where it was");
        w.frame(vec![]);
        assert!(!right_click(&w).0, "once");
        // A press on the caption: this window's next frame has it, that frame only. The move and
        // the maximize on a double press are the system's: the bar sends the window nothing.
        assert!(!w.pressed);
        let (x, y) = ((at.x * ppp) as i32, (at.y * ppp) as i32);
        caption::pressed(window, x, y);
        let commands = w.frame(vec![]);
        assert!(w.pressed);
        assert!(commands.is_empty(), "{commands:?}");
        caption::pressed(window, x, y);
        let commands = w.frame(vec![]);
        assert!(w.pressed && commands.is_empty(), "a second press at once: {commands:?}");
        let commands = w.frame(vec![]);
        assert!(!w.pressed && commands.is_empty(), "for one frame: {commands:?}");
        // A press on the system's maximize button is a press too, and no click on the UI's.
        let maximize = w.ctx.read_response(caption_id(Caption::Maximize)).map(|r| r.rect.center()).unwrap_or(Pos2::ZERO);
        caption::pressed(window, (maximize.x * ppp) as i32, (maximize.y * ppp) as i32);
        let commands = w.frame(vec![]);
        assert!(w.pressed);
        assert_eq!((w.action, commands.len()), (None, 0));
        // On a bar that is the UI's own the same presses are the UI's to act on.
        caption::closed(window);
        w.window = None;
        let (commands, actions) = w.gesture(&[maximize]);
        assert_eq!((actions, commands.len()), (vec![WindowAction::Toggle], 0));
    }

    /// A pointer click on the system's maximize button is the system's. The keyboard has no such
    /// click: Enter on the focused button still presses the UI's.
    #[test]
    fn the_keyboard_still_presses_the_systems_maximize_button() {
        let window = WINDOWS + 70;
        let mut w = Bar::new(OWN, 1200.0);
        w.window = Some(window);
        w.frame(vec![]);
        caption::hooked(window);
        w.frame(vec![]);
        w.ctx.memory_mut(|m| m.request_focus(caption_id(Caption::Maximize)));
        w.frame(vec![]);
        let enter = |pressed| Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        w.frame(vec![enter(true)]);
        let on_press = w.action;
        w.frame(vec![enter(false)]);
        assert!(on_press == Some(WindowAction::Toggle) || w.action == Some(WindowAction::Toggle), "{on_press:?}, then {:?}", w.action);
        caption::closed(window);
    }

    /// A bar that slides in over a view (a pop-out's) is on a layer of its own. The view under it
    /// takes nothing of the caption; a menu in front of the bar takes its part.
    #[test]
    fn only_what_lies_over_the_bar_takes_the_caption_from_it() {
        use caption::CaptionHit;
        let window = WINDOWS + 50;
        let ctx = crate::test_support::context();
        install(&ctx);
        let bar = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, BAR));
        let frame = |menu: bool| {
            let mut input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))), ..Default::default() };
            if let Some(v) = input.viewports.get_mut(&ViewportId::ROOT) {
                v.native_window = Some(window);
            }
            let _ = ctx.run_ui(input, |ui| {
                // The view fills the window and takes clicks and drags.
                ui.interact(ui.max_rect(), Id::unique("view"), Sense::click_and_drag());
                let layer = |order, name: &'static str| LayerId::new(order, Id::unique(name));
                let top = ui.new_child(egui::UiBuilder::new().layer_id(layer(Order::Middle, "bar over the view")).max_rect(bar));
                drag_area(&top, bar);
                if menu {
                    let front = ui.new_child(egui::UiBuilder::new().layer_id(layer(Order::Foreground, "menu over the bar")).max_rect(ui.max_rect()));
                    front.interact(Rect::from_min_size(pos2(300.0, 0.0), vec2(200.0, 300.0)), Id::unique("menu row"), Sense::click());
                }
            });
        };
        let ppp = ctx.pixels_per_point();
        let hit = |x: f32| caption::hit(window, (x * ppp) as i32, (BAR / 2.0 * ppp) as i32);
        frame(false);
        assert_eq!(hit(400.0), CaptionHit::Drag, "the view under the bar takes nothing of it");
        frame(true);
        assert_eq!((hit(400.0), hit(900.0)), (CaptionHit::Client, CaptionHit::Drag), "a menu in front of the bar takes its part");
        caption::closed(window);
    }

    #[test]
    fn a_window_the_system_answers_for_lays_out_no_resize_zones() {
        let window = WINDOWS + 40;
        let ctx = crate::test_support::context();
        let zones_laid_out = || {
            // Two passes: a response is read from the pass before as well.
            for _ in 0..2 {
                let mut input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))), ..Default::default() };
                if let Some(v) = input.viewports.get_mut(&ViewportId::ROOT) {
                    v.native_window = Some(window);
                }
                let _ = ctx.run_ui(input, resize_zones);
            }
            ctx.read_response(Id::unique(("titlebar-resize", 0usize))).is_some()
        };
        assert!(zones_laid_out(), "the UI's own resize zones");
        caption::hooked(window);
        assert!(!zones_laid_out(), "the system's frame resizes the window");
        caption::closed(window);
        assert!(zones_laid_out());
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

    /// Frames of raw input through `release_stale_input`, as the apps' hooks run them.
    struct Hooked {
        ctx: egui::Context,
        time: f64,
    }

    impl Hooked {
        fn new() -> Self {
            Self { ctx: crate::test_support::context(), time: 0.0 }
        }

        fn frame(&mut self, events: Vec<Event>, focused: bool, add: impl FnMut(&mut egui::Ui)) -> Vec<Event> {
            self.time += 0.05;
            let mut raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
                time: Some(self.time),
                events,
                focused,
                ..Default::default()
            };
            release_stale_input(&self.ctx, &mut raw);
            let events = raw.events.clone();
            let mut add = add;
            self.ctx.run_ui(raw, |ui| add(ui)).textures_delta.clear();
            events
        }

        fn press(&self, at: Pos2, mods: egui::Modifiers) -> Vec<Event> {
            vec![
                Event::PointerMoved(at),
                Event::ModifiersChanged(mods),
                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: mods },
            ]
        }
    }

    #[test]
    fn focus_lost_with_a_button_held_releases_it_on_regain_and_clears_modifiers() {
        let mut h = Hooked::new();
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        let at = pos2(50.0, 60.0);
        h.frame(h.press(at, ctrl), true, |_| {});
        assert!(h.ctx.input(|i| i.pointer.primary_down()));
        h.frame(vec![Event::PointerGone, Event::WindowFocused(false)], false, |_| {});
        assert!(h.ctx.input(|i| i.pointer.primary_down()), "egui keeps the button down");
        let events = h.frame(vec![Event::WindowFocused(true)], true, |_| {});
        assert!(events.iter().any(|e| matches!(e, Event::PointerButton { button: PointerButton::Primary, pressed: false, .. })));
        assert!(events.iter().any(|e| matches!(e, Event::ModifiersChanged(m) if m.is_none())));
        assert!(!h.ctx.input(|i| i.pointer.primary_down()));
        assert!(h.ctx.input(|i| i.modifiers.is_none()));
        assert!(h.frame(vec![], true, |_| {}).is_empty(), "nothing is stale the second time");
    }

    #[test]
    fn a_closed_native_dialog_releases_a_held_button_and_modifier() {
        let mut h = Hooked::new();
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        h.frame(h.press(pos2(10.0, 10.0), ctrl), true, |_| {});
        assert!(h.ctx.input(|i| i.pointer.primary_down() && i.modifiers.ctrl));
        mark_input_stale(&h.ctx);
        h.frame(vec![], true, |_| {});
        assert!(!h.ctx.input(|i| i.pointer.primary_down()));
        assert!(h.ctx.input(|i| i.modifiers.is_none()), "Ctrl released inside the dialog is not still held");
    }

    #[test]
    fn a_press_after_focus_returns_in_the_same_batch_stays_held() {
        let mut h = Hooked::new();
        h.frame(h.press(pos2(10.0, 10.0), Default::default()), true, |_| {});
        let fresh = Event::PointerButton { pos: pos2(80.0, 80.0), button: PointerButton::Primary, pressed: true, modifiers: Default::default() };
        h.frame(vec![Event::WindowFocused(false), Event::WindowFocused(true), Event::PointerMoved(pos2(80.0, 80.0)), fresh], true, |_| {});
        assert!(h.ctx.input(|i| i.pointer.primary_down()), "the new press is the user's");
        assert!(h.frame(vec![], true, |_| {}).is_empty(), "and the next frame does not release it");
        assert!(h.ctx.input(|i| i.pointer.primary_down()));
    }

    #[test]
    fn recovering_a_held_press_does_not_click_the_widget_under_it() {
        let mut h = Hooked::new();
        let clicked = std::cell::Cell::new(false);
        let button = |ui: &mut egui::Ui| {
            let r = ui.put(Rect::from_min_size(pos2(20.0, 20.0), vec2(100.0, 40.0)), egui::Button::new("Go"));
            if r.clicked() {
                clicked.set(true);
            }
        };
        let at = pos2(40.0, 30.0);
        h.frame(vec![Event::PointerMoved(at)], true, button);
        h.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }], true, button);
        h.frame(vec![Event::WindowFocused(false)], false, button);
        h.frame(vec![Event::WindowFocused(true)], true, button);
        assert!(!h.ctx.input(|i| i.pointer.primary_down()));
        assert!(!clicked.get(), "the interrupted press must not activate the widget");
    }

    #[test]
    fn a_pointer_back_without_its_release_lets_go_but_a_drag_tracked_outside_stays() {
        let released = |events: &[Event]| events.iter().any(|e| matches!(e, Event::PointerButton { pressed: false, .. }));
        // A compositor took the release: the pointer left with the button held and comes back
        // with nothing reported in between (#16460).
        let mut h = Hooked::new();
        h.frame(h.press(pos2(50.0, 60.0), Default::default()), true, |_| {});
        h.frame(vec![Event::PointerGone], true, |_| {});
        assert!(h.ctx.input(|i| i.pointer.primary_down()), "egui keeps the button down");
        let events = h.frame(vec![Event::PointerMoved(pos2(70.0, 80.0))], true, |_| {});
        assert!(released(&events));
        assert!(!h.ctx.input(|i| i.pointer.primary_down()));
        assert_eq!(h.ctx.input(|i| i.pointer.latest_pos()), Some(pos2(70.0, 80.0)), "the pointer is where it came back");
        // A drag the platform kept reporting outside the window is still the user's.
        let mut h = Hooked::new();
        h.frame(h.press(pos2(50.0, 60.0), Default::default()), true, |_| {});
        h.frame(vec![Event::PointerGone, Event::PointerMoved(pos2(450.0, 60.0))], true, |_| {});
        assert!(!released(&h.frame(vec![Event::PointerMoved(pos2(300.0, 60.0))], true, |_| {})));
        assert!(h.ctx.input(|i| i.pointer.primary_down()));
    }

    #[test]
    fn a_second_press_of_a_held_button_lets_go_of_the_first() {
        let mut h = Hooked::new();
        let right =
            |pressed| Event::PointerButton { pos: pos2(40.0, 40.0), button: PointerButton::Secondary, pressed, modifiers: Default::default() };
        h.frame(vec![Event::PointerMoved(pos2(40.0, 40.0)), right(true)], true, |_| {});
        // Its release never came: the next press of the same button is a new one.
        let events = h.frame(vec![right(true)], true, |_| {});
        let release = events.iter().position(|e| matches!(e, Event::PointerButton { button: PointerButton::Secondary, pressed: false, .. }));
        let press = events.iter().rposition(|e| matches!(e, Event::PointerButton { button: PointerButton::Secondary, pressed: true, .. }));
        assert!(release.zip(press).is_some_and(|(r, p)| r < p), "the old press goes before the new one: {events:?}");
        assert!(h.ctx.input(|i| i.pointer.secondary_down()));
        h.frame(vec![right(false)], true, |_| {});
        assert!(!h.ctx.input(|i| i.pointer.secondary_down()), "and its own release lets go");
    }

    #[test]
    fn focus_or_pointer_return_asks_the_window_to_set_its_cursor_again() {
        let ctx = crate::test_support::context();
        let run = |events: Vec<Event>| {
            let raw =
                RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))), time: Some(0.1), events, ..Default::default() };
            reapply_cursor_on_return(&ctx, &raw);
            let mut out = ctx.run_ui(raw, |_| {});
            out.textures_delta.clear();
            out.viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()
        };
        let reapplies = |cmds: &[ViewportCommand]| cmds.iter().any(|c| matches!(c, ViewportCommand::CursorVisible(true)));
        assert!(reapplies(&run(vec![Event::WindowFocused(true)])), "focus regained");
        assert!(reapplies(&run(vec![Event::PointerMoved(pos2(5.0, 5.0))])), "pointer back in the window");
        assert!(!reapplies(&run(vec![Event::PointerMoved(pos2(6.0, 6.0))])), "an ordinary move changes nothing");
        assert!(!reapplies(&run(vec![Event::WindowFocused(false)])), "focus lost");
    }

    #[test]
    fn modifiers_reported_while_unfocused_survive_focus_return() {
        let ctrl = egui::Modifiers { ctrl: true, command: !cfg!(target_os = "macos"), ..Default::default() };
        let at = pos2(80.0, 90.0);
        for tracked in [false, true] {
            let ctx = crate::test_support::context();
            let mut time = 0.0;
            let mut frame = |events: Vec<Event>, focused: bool| {
                time += 0.05;
                let mut raw = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
                    time: Some(time),
                    focused,
                    events,
                    ..Default::default()
                };
                if tracked {
                    release_stale_input_with_tracked_buttons(&ctx, &mut raw);
                } else {
                    release_stale_input(&ctx, &mut raw);
                }
                ctx.run_ui(raw, |_| {}).textures_delta.clear();
            };
            frame(vec![Event::WindowFocused(true)], true);
            frame(vec![Event::WindowFocused(false)], false);
            assert!(ctx.input(|i| !i.focused && i.modifiers.is_none()));
            // Hovering over the unfocused window while holding Ctrl updates egui's modifiers.
            frame(vec![Event::ModifiersChanged(ctrl), Event::PointerMoved(at)], false);
            assert!(ctx.input(|i| !i.focused && i.modifiers.ctrl));
            // Clicking into the window regains focus without a duplicate ModifiersChanged.
            frame(
                vec![Event::WindowFocused(true), Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: ctrl }],
                true,
            );
            assert!(ctx.input(|i| i.focused && i.modifiers.ctrl), "tracked={tracked}: Ctrl must stay held");
            assert!(ctx.input(|i| i.pointer.middle_down()), "tracked={tracked}: fresh middle press stays held");
        }
    }

    #[test]
    fn a_fresh_button_press_while_unfocused_is_not_released_when_focus_arrives() {
        let mut h = Hooked::new();
        let at = pos2(80.0, 90.0);
        h.frame(vec![Event::WindowFocused(true), Event::PointerMoved(at)], true, |_| {});
        h.frame(vec![Event::WindowFocused(false)], false, |_| {});
        // User presses the middle button while the window is still unfocused.
        h.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: Default::default() }], false, |_| {});
        assert!(h.ctx.input(|i| i.pointer.middle_down()));
        // Focus and held modifiers arrive on the following frame.
        let ctrl = egui::Modifiers { ctrl: true, command: !cfg!(target_os = "macos"), ..Default::default() };
        h.frame(vec![Event::WindowFocused(true), Event::ModifiersChanged(ctrl)], true, |_| {});
        assert!(h.ctx.input(|i| i.pointer.middle_down()), "fresh unfocused press must not be killed on focus gain");
        assert!(h.ctx.input(|i| i.modifiers.ctrl), "modifiers arriving with focus must apply");
    }
}
