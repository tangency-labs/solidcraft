//! Camera navigation shared by every page that shows the model through a camera: the modelling
//! viewport and the Render preview today, and any future canvas (a 2D drawing or flat pattern
//! page calls it with orbit off, so pan and zoom still follow the user's bindings).
//!
//! One entry point, [`handle`], reads a frame's pointer, wheel, trackpad and modifier input over a
//! view and moves `app.cam` the way Preferences › Navigation says: the navigation preset (Fusion,
//! SolidWorks, Blender, Touchpad…), Orbit around, zoom to the cursor, the middle double-click and
//! orbit snapping. The 3D mouse's motion goes through [`device`] too, with the page's
//! [`NavCaps`], and so does a device app's over the navigation bridge. The view keys and the navigators act on `app.cam` independently of the page. New
//! canvases must call `navigation::handle` rather than reading drags or the wheel themselves;
//! `hand_rolled_camera_input_is_rejected` fails when they do.

use egui::{Pos2, Rect, pos2, vec2};
use serde_json::json;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::SolveApp;
use crate::prefs::{OrbitAround, pivot_under};
use crate::space_mouse::{BridgeMotion, Motion};
use crate::viewport::{Hit, NavMode, axis_rect, nearest_quarter_turn, pick, pick_cached, projection};

/// What a page's camera may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NavCaps {
    /// Orbit (and continuous orbit) is allowed; off for flat 2D pages, where pan and zoom remain.
    pub orbit: bool,
    /// What is under the pointer can be picked or dragged, so presses on it don't navigate the
    /// way presses on empty space do (OpenSCAD's left drag, Ctrl+left orbit).
    pub pick: bool,
}

impl NavCaps {
    /// The modelling viewport.
    pub const MODEL: NavCaps = NavCaps { orbit: true, pick: true };
    /// The Render workspace's product preview: shared navigation and product picking.
    pub const RENDER: NavCaps = NavCaps { orbit: true, pick: true };
}

/// Supplies the scroll phases browsers omit, before the shared navigation sees input.
#[derive(Default)]
pub struct WebWheelGestures {
    last_scroll: Option<f64>,
}

impl WebWheelGestures {
    /// Bracket point-mode swipes and return the delay until their idle timeout.
    ///
    /// Browsers expose no device identity. A bare vertical delta of at least
    /// `WHEEL_STEP_MIN` points is a wheel notch (browsers report about 100 points per notch,
    /// scaled by page zoom and display scaling); smaller point deltas and horizontal input
    /// start a swipe. Once started, larger deltas and momentum belong to that swipe too.
    pub fn update(&mut self, raw: &mut egui::RawInput, now: f64) -> Option<std::time::Duration> {
        use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, Vec2};
        const IDLE: f64 = 0.1;
        const WHEEL_STEP_MIN: f32 = 40.0;
        let phase_event = |phase| Event::MouseWheel { unit: MouseWheelUnit::Point, delta: Vec2::ZERO, modifiers: Modifiers::NONE, phase };
        let valid_time = now.is_finite() && now >= 0.0;
        let events = std::mem::take(&mut raw.events);
        if self.last_scroll.is_some_and(|last| !valid_time || now < last || now - last >= IDLE) {
            raw.events.push(phase_event(TouchPhase::End));
            self.last_scroll = None;
        }
        for event in events {
            match &event {
                Event::MouseWheel { unit, delta, modifiers, .. } => {
                    let point = *unit == MouseWheelUnit::Point;
                    let valid_delta = delta.is_finite() && *delta != Vec2::ZERO;
                    let step = delta.x == 0.0 && delta.y.abs() >= WHEEL_STEP_MIN;
                    let swipe = valid_time && point && valid_delta && !modifiers.ctrl && (self.last_scroll.is_some() || !step);
                    if swipe {
                        if self.last_scroll.is_none() {
                            raw.events.push(phase_event(TouchPhase::Start));
                        }
                        self.last_scroll = Some(now);
                    } else if self.last_scroll.take().is_some() {
                        raw.events.push(phase_event(TouchPhase::End));
                    }
                }
                // eframe converts browser pinch Ctrl+wheel to Zoom before the hook.
                Event::Zoom(_) if self.last_scroll.take().is_some() => {
                    raw.events.push(phase_event(TouchPhase::End));
                }
                _ => {}
            }
            raw.events.push(event);
        }
        self.last_scroll.and_then(|last| std::time::Duration::try_from_secs_f64((IDLE - (now - last)).clamp(0.0, IDLE)).ok())
    }
}

/// Raw wheel/trackpad input for one frame, split by source.
///
/// Mouse wheels report lines or pages; trackpads report points. Trackpad
/// scrolls arrive bracketed (`Start` … `Move` … `End`, momentum included)
/// while mouse wheels send bare `Move` events, so phases tell them apart.
pub(crate) struct RawWheel {
    /// Vertical mouse-wheel input in points (zooms).
    zoom: f32,
    /// Trackpad point input, routed like the wheel (zooms when panning is off).
    point_zoom: f32,
    /// Vertical scroll with the chosen modifier alone (always zooms).
    modifier_zoom: f32,
    /// Modifier input was consumed, including horizontal-only or zero-sum scroll.
    modifier_scroll: bool,
    /// Bracketed two-finger scroll in points (pans when panning is on).
    pan: egui::Vec2,
    shift_pan: egui::Vec2,
    /// Bare horizontal input in points (thumb wheel: always pans sideways).
    mouse_pan: egui::Vec2,
}

/// Route wheel points to the vertical axis, matching egui's scroll modifiers.
pub(crate) fn route_wheel_vertical(points: egui::Vec2, modifiers: egui::Modifiers, options: egui::InputOptions) -> f32 {
    let horizontal = modifiers.matches_any(options.horizontal_scroll_modifier);
    let vertical = modifiers.matches_any(options.vertical_scroll_modifier);
    if horizontal && !vertical {
        0.0
    } else if vertical && !horizontal {
        points.x + points.y
    } else {
        points.y
    }
}

/// The inertia macOS sends once the fingers lift (#7327): its own scroll bracket, starting as the
/// fingers' bracket ends, with the modifiers held at the time. It coasts in the mode the fingers
/// had; when the modifiers change it ends, so the rest never orbits a pan or pans an orbit.
/// Fingers that touch during a coast end it and start a new swipe, so a bracket that follows a
/// coast is never one. Browsers keep the inertia inside the swipe's bracket ([`WebWheelGestures`]),
/// so there are no coasts on the web.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TrackpadCoast {
    /// Whether the current bracket's last move orbited (Shift) rather than panned.
    last_orbit: Option<bool>,
    /// The current bracket is a coast, keeping this mode (orbit or not).
    keeps: Option<bool>,
    /// The coast's mode changed: the rest of its moves are dropped.
    stopped: bool,
    /// When the last swipe of the fingers ended, and whether its last move orbited.
    lifted: Option<(f64, bool)>,
}

impl TrackpadCoast {
    /// A coast starts at most this long (seconds) after the fingers lift; macOS sends it at once.
    const GAP: f64 = 0.1;

    fn start(&mut self, now: f64) {
        let lifted = self.lifted.take().filter(|(at, _)| !cfg!(target_arch = "wasm32") && now >= *at && now - *at <= Self::GAP);
        *self = TrackpadCoast { keeps: lifted.map(|(_, orbit)| orbit), ..Default::default() };
    }

    fn end(&mut self, now: f64) {
        let lifted = if self.keeps.is_none() { self.last_orbit.map(|orbit| (now, orbit)) } else { None };
        *self = TrackpadCoast { lifted, ..Default::default() };
    }

    /// Whether a move in this mode still counts.
    fn moves(&mut self, orbit: bool) -> bool {
        self.last_orbit = Some(orbit);
        self.stopped |= self.keeps.is_some_and(|keeps| keeps != orbit);
        !self.stopped
    }
}

/// Match egui's wheel units and modifier routing, without its temporal smoothing.
///
/// `gesture` tracks an ongoing bracketed trackpad scroll (`Start` … `End`):
/// moves inside one pan, bare `Move` input is a mouse (vertical zooms,
/// horizontal pans). Only point input brackets; line/page input always zooms.
/// With `shift_orbits` (Shift + two fingers orbits), `coast` ends the inertia
/// when the modifiers change.
pub(crate) fn immediate_wheel(
    input: &egui::InputState,
    options: egui::InputOptions,
    page_height: f32,
    gesture: &mut bool,
    coast: &mut TrackpadCoast,
    shift_orbits: bool,
    zoom_modifier: Option<egui::Modifiers>,
) -> RawWheel {
    let mut out = RawWheel {
        zoom: 0.0,
        point_zoom: 0.0,
        modifier_zoom: 0.0,
        modifier_scroll: false,
        pan: egui::Vec2::ZERO,
        shift_pan: egui::Vec2::ZERO,
        mouse_pan: egui::Vec2::ZERO,
    };
    for event in &input.events {
        let egui::Event::MouseWheel { unit, delta, modifiers, phase } = event else { continue };
        let modifier_zoom = claims_scroll(*modifiers, zoom_modifier);
        if !modifier_zoom && modifiers.matches_any(options.zoom_modifier) {
            continue;
        }
        out.modifier_scroll |= modifier_zoom;
        let point = matches!(unit, egui::MouseWheelUnit::Point);
        match phase {
            egui::TouchPhase::Start => {
                if point {
                    if !*gesture {
                        coast.start(input.time);
                    }
                    *gesture = true;
                }
                continue;
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if point {
                    if *gesture {
                        coast.end(input.time);
                    }
                    *gesture = false;
                }
                continue;
            }
            egui::TouchPhase::Move => {}
        }
        let points = match unit {
            egui::MouseWheelUnit::Point => *delta,
            egui::MouseWheelUnit::Line => *delta * options.line_scroll_speed,
            egui::MouseWheelUnit::Page => *delta * page_height,
        };
        if !points.is_finite() {
            continue;
        }
        if modifier_zoom {
            out.modifier_zoom += points.y;
            continue;
        }
        let shift = modifiers.shift && !modifiers.ctrl && !modifiers.command && !modifiers.alt;
        if point && *gesture && !coast.moves(shift) && shift_orbits {
            continue;
        }
        if point && *gesture {
            if shift {
                if points.is_finite() {
                    out.shift_pan += points.clamp(vec2(-1e4, -1e4), vec2(1e4, 1e4));
                }
            } else {
                out.pan += points;
            }
            // Legacy path only (ignored while panning is on).
            out.point_zoom += route_wheel_vertical(points, *modifiers, options);
        } else if point {
            // Bare moves are a mouse: vertical zooms, horizontal pans sideways.
            // A wheel needs the opposite sign of fingers for the same motion.
            out.zoom += route_wheel_vertical(points, *modifiers, options);
            out.mouse_pan += egui::vec2(-points.x, 0.0);
        } else {
            out.zoom += route_wheel_vertical(points, *modifiers, options);
        }
    }
    out
}

/// Whether the chosen trackpad zoom modifier (Preferences › Navigation, #8322) claims a wheel
/// event: that key held alone. egui aliases Ctrl/Command on Windows/Linux; macOS can hold both.
pub(crate) fn claims_scroll(modifiers: egui::Modifiers, zoom_modifier: Option<egui::Modifiers>) -> bool {
    zoom_modifier.is_some_and(|chosen| modifiers.matches_exact(chosen) && !(modifiers.mac_cmd && modifiers.ctrl))
}

/// Combined pinch-to-zoom factor for one frame (1 = no pinch input).
///
/// Some input paths deliver pinch as a zoom-modified wheel instead of `Event::Zoom`.
/// Read the raw movement that [`immediate_wheel`] leaves to the zoom modifier, without
/// replaying egui's smoothed zoom delta on later frames. Gesture boundaries carry no movement.
pub(crate) fn pinch_factor(input: &egui::InputState, options: egui::InputOptions, page_height: f32, zoom_modifier: Option<egui::Modifiers>) -> f32 {
    let mut factor = 1.0;
    for event in &input.events {
        match event {
            egui::Event::Zoom(f) => {
                if f.is_finite() && *f > 0.0 {
                    factor *= f;
                }
            }
            // A wheel the chosen zoom modifier already turns into zoom is not also a pinch.
            egui::Event::MouseWheel { unit, delta, modifiers, phase: egui::TouchPhase::Move }
                if modifiers.matches_any(options.zoom_modifier) && !claims_scroll(*modifiers, zoom_modifier) =>
            {
                let points = match unit {
                    egui::MouseWheelUnit::Point => *delta,
                    egui::MouseWheelUnit::Line => *delta * options.line_scroll_speed,
                    egui::MouseWheelUnit::Page => *delta * page_height,
                };
                let f = (options.scroll_zoom_speed * (points.x + points.y)).exp();
                if f.is_finite() && f > 0.0 {
                    factor *= f;
                }
            }
            _ => {}
        }
    }
    factor
}

/// The wheel and pinch of this frame for a widget that zooms and moves along one axis itself (a
/// range track), claimed so that nothing under it moves as well: the wheel's vertical points
/// (zoom), its sideways points (the content moves by them) and the pinch factor. Units and
/// modifier routing are egui's, as for the camera.
pub(crate) fn axis_wheel(ctx: &egui::Context, page_height: f32) -> (f32, f32, f32) {
    let options = ctx.options(|o| o.input_options);
    let (zoom, sideways, pinch) = ctx.input(|i| {
        let (mut zoom, mut sideways) = (0.0, 0.0);
        for event in &i.events {
            let egui::Event::MouseWheel { unit, delta, modifiers, phase } = event else { continue };
            if *phase != egui::TouchPhase::Move || modifiers.matches_any(options.zoom_modifier) {
                continue;
            }
            let points = match unit {
                egui::MouseWheelUnit::Point => *delta,
                egui::MouseWheelUnit::Line => *delta * options.line_scroll_speed,
                egui::MouseWheelUnit::Page => *delta * page_height,
            };
            let up = route_wheel_vertical(points, *modifiers, options);
            zoom += up;
            sideways += if up == 0.0 { points.x + points.y } else { points.x };
        }
        // A zoom-modified wheel, skipped above, zooms the track once, as a pinch: no chosen
        // trackpad modifier turns it into anything else here.
        (zoom, sideways, pinch_factor(i, options, page_height, None))
    });
    let (zoom, sideways) = (if zoom.is_finite() { zoom } else { 0.0 }, if sideways.is_finite() { sideways } else { 0.0 });
    if zoom != 0.0 || sideways != 0.0 || pinch != 1.0 {
        claim_wheel(ctx);
    }
    (zoom, sideways, pinch)
}

/// Give this frame's wheel to the widget under the pointer that uses it itself (a value field
/// stepping its number, `value_step`): no scroll area or camera under it moves as well.
pub(crate) fn claim_wheel(ctx: &egui::Context) {
    ctx.input_mut(|i| {
        i.smooth_scroll_delta = egui::Vec2::ZERO;
        i.events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
    });
}

/// Zoom anchor: the model point under the cursor, else the target's depth.
/// Preferences › Navigation › Zoom to the cursor (off: toward the view's centre).
pub(crate) fn zoom_anchor(app: &mut SolveApp, rect: egui::Rect, hover: Option<egui::Pos2>, surface: bool) -> Option<Vec3> {
    let hover = hover.filter(|_| app.preferences.zoom_to_cursor);
    zoom_point(app, rect, hover, surface)
}

/// The cap, bridge and drag-zoom keep their old behaviour unless explicitly opted in.
fn model_zoom_anchor(app: &mut SolveApp, rect: Rect, point: Option<Pos2>, surface: bool) -> Option<Vec3> {
    let point = point.filter(|_| app.preferences.zoom_stop_at_model);
    zoom_point(app, rect, point, surface)
}

fn zoom_point(app: &mut SolveApp, rect: Rect, point: Option<Pos2>, surface: bool) -> Option<Vec3> {
    point.map(|p| {
        let proj = projection(app, rect);
        // Toward the model's surface under the cursor, else the target's depth.
        if surface && let Some(q) = pick_cached(app, &proj, p).iter().find_map(|h| if let Hit::Face { point, .. } = h { Some(*point) } else { None })
        {
            return q;
        }
        let (o, d) = proj.ray(p);
        let n = app.cam.back();
        let den = d.dot(n);
        if den.abs() > 1e-9 { o + d * ((app.cam.target - o).dot(n) / den) } else { app.cam.target }
    })
}

/// Modifier-only navigation never borrows pointer movement from another widget or gesture.
/// Tool fields keep keyboard focus without blocking motion over the exposed viewport.
pub(crate) fn modifier_motion(
    app: &mut SolveApp,
    ui: &egui::Ui,
    inside: bool,
    hover: Option<Pos2>,
    mods: egui::Modifiers,
    picking: bool,
) -> Option<(NavMode, egui::Vec2)> {
    let owns = inside
        && ui.input(|i| i.focused && !i.pointer.any_down() && !i.pointer.any_released() && !i.pointer.any_pressed())
        && !egui::Popup::is_any_open(ui.ctx())
        && !crate::dim_view::editing()
        && app.viewport.context_menu.is_none()
        && app.viewport.axis_drag.is_none()
        && app.viewport.point_drag.is_none()
        && app.viewport.text_drag.is_none()
        && app.viewport.boxsel.is_none()
        && !(picking
            && !crate::render_workspace::active(app)
            && hover.is_some_and(|p| app.viewport.rect.is_some_and(|rect| crate::canvas::owns_pointer(app, &projection(app, rect), p))))
        && !hover.is_some_and(|p| app.viewport.layout.as_ref().is_some_and(|l| l.navigation.contains(p)));
    let next = crate::prefs::nav_preset(&app.preferences.nav).motion_mode(mods).zip(hover.filter(|p| owns && p.is_finite()));
    let previous = app.viewport.modifier_nav;
    if previous.map(|(m, _)| m) != next.map(|(m, _)| m) {
        app.viewport.orbit_pivot = None;
        app.viewport.orbit_from = None;
        app.viewport.orbit_free = None;
    }
    app.viewport.modifier_nav = next;
    next.map(|(mode, p)| {
        let delta = previous.filter(|(m, _)| *m == mode).map_or(egui::Vec2::ZERO, |(_, last)| p - last);
        let delta = if delta.is_finite() && delta.x.abs() <= 1e4 && delta.y.abs() <= 1e4 { delta } else { egui::Vec2::ZERO };
        (mode, delta)
    })
}

/// A tap (press and release) shorter than this is one animated step; held longer, the arrow
/// turns the view continuously.
const TAP_SECONDS: f64 = 0.15;
/// How fast a held arrow turns the view (Shift: twice as fast).
const HOLD_DEGREES_PER_SECOND: f64 = 90.0;

/// An arrow key turning the view that is still down (`ViewportState::key_hold`).
#[derive(Clone, Copy)]
pub(crate) struct KeyHold {
    key: egui::Key,
    action: &'static str,
    shift: bool,
    since: f64,
}

/// A menu, the command box or a focused widget has the keyboard, so the view keys are not ours.
fn view_keys_blocked(app: &SolveApp, ctx: &egui::Context) -> bool {
    egui::Popup::is_any_open(ctx)
        || app.viewport.context_menu.is_some()
        || app.ui.palette_open
        || ctx.memory(|m| m.focused().is_some())
        || ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::new("sc_panel_open")).is_some())
}

/// View keys (#1541): the arrows turn the view 15° (with Shift a quarter turn, as the navigation
/// cube's arrows), Ctrl+arrows pan it a tenth of the view, and Ctrl+8 looks straight at the
/// selected face, plane or sketch. Held longer than a tap, an arrow keeps turning the view at a
/// steady rate (`step_key_hold`); the operating system's key repeat does nothing. Not while a
/// menu, the command box or a focused widget has the arrows. Returns whether the key was used.
pub(crate) fn view_key(app: &mut SolveApp, ctx: &egui::Context, key: egui::Key, mods: egui::Modifiers, repeat: bool) -> bool {
    use egui::Key;
    let menu_open = view_keys_blocked(app, ctx);
    let ctrl = mods.command || mods.ctrl;
    if mods.alt || menu_open {
        return false;
    }
    let result = crate::render_workspace::result_active(app);
    if key == Key::Num8 && ctrl && !mods.shift && !result {
        crate::viewport::look_at_selection(app);
        return true;
    }
    let (action, dx, dy): (&'static str, f32, f32) = match key {
        Key::ArrowLeft => ("left", -1.0, 0.0),
        Key::ArrowRight => ("right", 1.0, 0.0),
        Key::ArrowUp => ("up", 0.0, -1.0),
        Key::ArrowDown => ("down", 0.0, 1.0),
        _ => return false,
    };
    if ctrl {
        // The model moves with the arrow, as it does under a pan drag.
        if let Some(rect) = app.viewport.rect {
            let step = rect.height() * 0.1;
            let params = json!({"motion": "pan", "delta": [dx * step, dy * step]});
            if result {
                crate::render_workspace::with_result_camera(app, |app| camera_motion(app, rect, params));
            } else {
                camera_motion(app, rect, params);
            }
        }
    } else if !repeat && !result {
        let degrees = if mods.shift { 90.0 } else { 15.0 };
        app.viewport.key_hold = Some(KeyHold { key, action, shift: mods.shift, since: app.now });
        // Turn from where a step in flight is going, so taps add up and the one animation is
        // retargeted rather than a second started.
        let from = app.cam_anim.map_or(app.cam, |a| a.to);
        let _ = app.run("view.navigate", json!({"camera": from, "action": action, "degrees": degrees}));
    }
    ctrl || !result
}

/// Each frame an arrow turning the view is held past a tap: turn at a steady rate by the frame's
/// time, with no animation of its own. Ends when the key is released or the keyboard goes
/// elsewhere.
pub(crate) fn step_key_hold(app: &mut SolveApp, ctx: &egui::Context) {
    if crate::render_workspace::result_active(app) {
        app.viewport.key_hold = None;
        return;
    }
    let Some(hold) = app.viewport.key_hold else { return };
    let (down, dt) = ctx.input(|i| (i.key_down(hold.key), f64::from(i.unstable_dt)));
    if !down || view_keys_blocked(app, ctx) {
        app.viewport.key_hold = None;
        return;
    }
    ctx.request_repaint();
    if app.now - hold.since < TAP_SECONDS {
        return;
    }
    let rate = if hold.shift { 2.0 * HOLD_DEGREES_PER_SECOND } else { HOLD_DEGREES_PER_SECOND };
    let degrees = rate * if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
    if degrees <= 1e-6 {
        return;
    }
    let params = json!({"camera": app.cam, "action": hold.action, "degrees": degrees});
    if let Ok(v) = app.session.execute("view.navigate", &params)
        && let Some(cam) = v.get("camera").and_then(|c| serde_json::from_value::<Camera>(c.clone()).ok())
        && cam.is_valid()
    {
        app.cancel_view_animation();
        app.viewport.orbit_free = None;
        app.cam = cam;
    }
}

/// Continuous camera changes use the same JSON command as programmatic callers.
pub(crate) fn camera_motion(app: &mut SolveApp, rect: Rect, mut params: serde_json::Value) {
    let stored_fov = app.cam.fov;
    params["camera"] = json!(projection(app, rect).cam);
    params["viewport"] = json!([rect.width().max(1.0), rect.height().max(1.0)]);
    params["free_orbit"] = json!(app.preferences.free_orbit);
    let _ = app.run("view.navigate", params);
    app.cam.fov = stored_fov;
}

/// What one frame of camera input saw and did, for the caller's hover, selection and drawing.
pub(crate) struct CameraInput {
    pub hover: Option<Pos2>,
    pub scroll: f32,
    pub mods: egui::Modifiers,
    pub middle: bool,
    pub secondary_down: bool,
    pub primary_down: bool,
    pub primary_pressed: bool,
    /// The pointer is over this view (not a window, menu or the axis view above it).
    pub inside: bool,
    /// The axis view or navigation cube's rectangle.
    pub axes: Rect,
    pub pinch: f32,
    pub pan: egui::Vec2,
    /// The navigation bar's fixed mode, or the preset's left-button binding.
    pub primary_nav: Option<NavMode>,
    /// The navigation this frame's pointer input drives.
    pub mode: Option<NavMode>,
}

/// Camera input for one frame over `rect`: the navigation preset's buttons and modifiers, Orbit
/// around, wheel, trackpad and pinch zoom (to the cursor), middle double-click, continuous orbit
/// and orbit snapping. Every camera page calls this, so the mouse moves the camera the same way
/// in every workspace (#1454). With `caps.pick` off nothing under the pointer is treated as
/// selectable, so a press that would start a selection navigates as on empty space; with
/// `caps.orbit` off, bindings that orbit do nothing and pan and zoom work as usual.
pub(crate) fn handle(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response, caps: NavCaps) -> CameraInput {
    let picking = caps.pick;
    let (hover, scroll, mods, middle, secondary_down, primary_down, primary_pressed, primary_released, delta) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.smooth_scroll_delta.y,
            i.modifiers,
            i.pointer.middle_down(),
            i.pointer.secondary_down(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.delta(),
        )
    });
    let hover = hover.filter(|p| !crate::frame::dock_dragging(ui.ctx()) && !crate::shell::covered(app, *p));
    // The right button belongs to the marking menu while it is open.
    let secondary_down = secondary_down && app.viewport.context_menu.is_none();
    // The axis view handles its own clicks; the model underneath must not see them.
    let axes = if caps.orbit { axis_rect(app, rect) } else { Rect::NOTHING };
    // So is the View angles readout under it (#6032).
    let readout = crate::view_angles::readout_rect(ui.ctx(), rect).filter(|_| caps.orbit && app.preferences.show_view_angles);
    // Floating windows and menus own input over the model, including wheel navigation. egui's
    // hit test also counts the model as under the pointer a few points past its own edge and
    // inside a window's or a list's edge (#678), so the pointer must be in the viewport too,
    // with no window or menu above it.
    let inside = ui.is_enabled()
        && resp.contains_pointer()
        && ui.rect_contains_pointer(rect)
        && hover.is_some_and(|p| !axes.contains(p) && !readout.is_some_and(|r| r.contains(p)));
    // The wheel and trackpad gestures go on under a box anchored to the model (a value box such
    // as Extrude's distance, a dimension's editor): it scrolls nothing, and a pan, orbit or zoom
    // stopped as it crossed one (#10061). Its clicks stay its own. Not while the wheel steps the
    // value under the pointer (Preferences › Design, value stepping).
    let wheel_inside = inside
        || (ui.is_enabled()
            && !crate::value_step::enabled(ui.ctx())
            && hover.is_some_and(|p| {
                rect.contains(p)
                    && !axes.contains(p)
                    && !readout.is_some_and(|r| r.contains(p))
                    && ui.ctx().layer_id_at(p).is_some_and(crate::ui_kit::container::is_canvas)
            }));
    app.viewport.mouse = hover.filter(|_| inside);
    // Sketch snap suppression is set once in `viewport::show` from preferences.
    middle_double_click(app, ui, inside);
    // The 3D mouse's motion this frame, with what this page's camera may do.
    device(app, rect, caps);

    // ---- navigation ----
    let trackpad_pan = app.preferences.trackpad_pans();
    if !app.preferences.smooth_wheel_zoom {
        app.viewport.wheel_zoom_raw_until_idle = true;
    } else if !ui.input(|i| i.is_scrolling()) {
        app.viewport.wheel_zoom_raw_until_idle = false;
    }
    let options = ui.ctx().options(|o| o.input_options);
    let page_height = ui.ctx().viewport_rect().height();
    let trackpad_orbit = trackpad_pan && app.preferences.nav == "fusion";
    let zoom_modifier = app.preferences.trackpad_zoom_modifier.modifiers();
    let viewport = &mut app.viewport;
    let raw = ui.input(|i| {
        immediate_wheel(i, options, page_height, &mut viewport.trackpad_gesture, &mut viewport.trackpad_coast, trackpad_orbit, zoom_modifier)
    });
    if raw.modifier_scroll {
        // Apply modifier scroll once, without egui's scroll smoothing or its tail.
        app.viewport.wheel_zoom_raw_until_idle = true;
    }
    if trackpad_pan && (raw.pan != egui::Vec2::ZERO || raw.shift_pan != egui::Vec2::ZERO) {
        // Swallow egui's smoothing tail while the trackpad runs (a wheel-only
        // tail ends the moment trackpad input starts; mixed input is rare).
        app.viewport.trackpad_until_idle = true;
    } else if !ui.input(|i| i.is_scrolling()) {
        app.viewport.trackpad_until_idle = false;
        // Self-healing: with no scroll activity no gesture can be mid-flight
        // (its End was lost); a stuck flag would pan future mouse input.
        app.viewport.trackpad_gesture = false;
    }
    let mut pan =
        if trackpad_pan { raw.pan + if trackpad_orbit { egui::Vec2::ZERO } else { raw.shift_pan } } else { egui::Vec2::ZERO } + raw.mouse_pan;
    pan.x *= crate::prefs::pan_sign_x(app);
    pan.y *= crate::prefs::pan_sign_y(app);
    let scroll = if app.viewport.wheel_zoom_raw_until_idle || (trackpad_pan && app.viewport.trackpad_until_idle) {
        raw.zoom + if trackpad_pan { 0.0 } else { raw.point_zoom }
    } else {
        scroll
    } + raw.modifier_zoom;
    // Pinch-to-zoom (trackpad): direct manipulation, never smoothed.
    let pinch = ui.input(|i| pinch_factor(i, options, page_height, zoom_modifier));
    let h = rect.height().max(1.0) as f64;
    if wheel_inside && pinch != 1.0 {
        app.viewport.orbit_free = None;
        // Spread zooms in: the camera distance shrinks by the pinch factor.
        let f = f64::from(pinch).powf(-crate::prefs::zoom_sign(app));
        let anchor = zoom_anchor(app, rect, hover, caps.orbit);
        if app.preferences.nav == crate::prefs::TOUCHPAD_NAV {
            let mut params = json!({"motion":"zoom", "factor":f.clamp(0.05,20.0)});
            if let Some(anchor) = anchor {
                params["anchor"] = json!(anchor);
            }
            camera_motion(app, rect, params);
        } else {
            app.cancel_view_animation();
            app.cam.zoom_at(f, anchor);
        }
    }
    if wheel_inside && scroll.is_finite() && scroll != 0.0 {
        app.viewport.orbit_free = None;
        let f = (-scroll as f64 * 0.0015 * f64::from(app.preferences.wheel_zoom_speed) * crate::prefs::zoom_sign(app)).exp();
        let anchor = zoom_anchor(app, rect, hover, caps.orbit);
        if app.preferences.nav == crate::prefs::TOUCHPAD_NAV {
            let mut params = json!({"motion":"zoom", "factor":f.clamp(0.05,20.0)});
            if let Some(anchor) = anchor {
                params["anchor"] = json!(anchor);
            }
            camera_motion(app, rect, params);
        } else {
            app.cancel_view_animation();
            app.cam.zoom_at(f, anchor);
        }
    }
    // Two-finger scroll pans the model with the fingers.
    if wheel_inside && pan.is_finite() && pan != egui::Vec2::ZERO {
        app.viewport.orbit_free = None;
        if app.preferences.nav == crate::prefs::TOUCHPAD_NAV {
            camera_motion(app, rect, json!({"motion":"pan","delta":[pan.x,pan.y]}));
        } else {
            app.cancel_view_animation();
            app.cam.pan(f64::from(pan.x), f64::from(pan.y), h);
        }
    }
    if wheel_inside && caps.orbit && trackpad_orbit && raw.shift_pan != egui::Vec2::ZERO {
        app.cancel_view_animation();
        app.viewport.orbit_free = None;
        let sign = crate::prefs::orbit_sign(app);
        orbit(app, rect, sign * f64::from(raw.shift_pan.x), sign * f64::from(raw.shift_pan.y), hover);
    }
    // A left press with the preset's keys (Alias: Shift+Alt) navigates until the button is let
    // go (kept through the release frame, so the end of the drag doesn't select either).
    if primary_pressed && inside {
        let empty_space = |app: &SolveApp| {
            app.viewport.context_menu.is_none()
                && hover.is_some_and(|p| {
                    if !picking {
                        return true;
                    }
                    let proj = projection(app, rect);
                    !crate::canvas::owns_pointer(app, &proj, p)
                        && pick(app, &proj, p).is_empty()
                        && (crate::render_workspace::active(app) || (crate::dim_view::hit(p).is_none() && crate::sketch_tools::glyph_at(p).is_none()))
                })
        };
        let openscad_orbit =
            app.preferences.nav == crate::prefs::OPENSCAD_NAV && app.tool.is_none() && app.dialog.is_none() && !mods.shift && empty_space(app);
        let preset = if app.preferences.nav != crate::prefs::OPENSCAD_NAV || openscad_orbit {
            crate::prefs::nav_mode(app, crate::prefs::NavButton::Left, mods)
        } else {
            None
        };
        // Ctrl+left drag from empty space orbits in every preset (Cmd on macOS). A Ctrl+click
        // still selects, and Ctrl on geometry still drags it without snapping.
        let ctrl_orbit = (mods.ctrl || mods.command) && !mods.shift && !mods.alt && empty_space(app);
        app.viewport.primary_nav = preset.or(ctrl_orbit.then_some(NavMode::Orbit));
    } else if !primary_down && !primary_released {
        app.viewport.primary_nav = None;
    }
    // Hold Space for a momentary pan: the hand tool of design apps. It overrides whatever the
    // left button would otherwise do - selecting, dragging geometry, the preset's binding or the
    // navigation bar's fixed mode - and lasts only while the key is held. Space still types in a
    // text field, and while a dimension's text is being edited, so those keep the keyboard.
    let space_pan = (inside || primary_down)
        && !ui.ctx().egui_wants_keyboard_input()
        && !crate::dim_view::editing()
        && ui.input(|i| i.key_down(egui::Key::Space));
    if space_pan {
        ui.ctx().set_cursor_icon(if primary_down { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    // The fixed navigation mode (navigation bar), a held Space, or the preset's left-button binding.
    let primary_nav = if space_pan { Some(NavMode::Pan) } else { app.viewport.nav.or(app.viewport.primary_nav) };
    // The right button also clicks (the context menu), so it navigates only once the press can no
    // longer be a click: a Magic Mouse moves the pointer a fraction of a point as it is clicked (#5517).
    // On that frame the drag catches up with the whole way from the press, so none of it is lost.
    let (right_drag, travel) = ui.input(|i| {
        let drag = secondary_down && !i.pointer.could_any_button_be_click();
        (drag, i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(from, to)| to - from))
    });
    let delta = match travel {
        Some(travel) if right_drag && !app.viewport.right_drag && !middle && travel.is_finite() => travel,
        _ => delta,
    };
    app.viewport.right_drag = right_drag;
    let button = if middle {
        Some(crate::prefs::NavButton::Middle)
    } else if right_drag {
        Some(crate::prefs::NavButton::Right)
    } else {
        None
    };
    // An orbit started on the viewport owns its button until release, including over a
    // floating shell card that disables the underlying egui response.
    let captured_orbit = app.viewport.orbit_from.is_some() && button.is_some();
    let dragging = resp.dragged() || (inside && button.is_some()) || captured_orbit;
    // Without orbit (a flat page), bindings that orbit do nothing.
    let allowed = |m: NavMode| caps.orbit || m != NavMode::Orbit;
    let motion = modifier_motion(app, ui, inside, hover, mods, picking).filter(|(m, _)| allowed(*m));
    let mode = motion
        .map(|(m, _)| m)
        .or_else(|| {
            if dragging && app.viewport.axis_drag.is_none() {
                match button {
                    Some(b) => crate::prefs::nav_mode(app, b, mods),
                    // A press held still that opened Select Other stays its own: egui calls it a
                    // drag after 0.8 s, and moving to a row must not turn the view under the list.
                    None if primary_down && !crate::select_other::is_open(app) => primary_nav,
                    None => None,
                }
            } else {
                None
            }
        })
        .filter(|m| allowed(*m));
    if let Some((mode, delta)) = motion {
        if delta != egui::Vec2::ZERO {
            let mut params = json!({"motion":if mode == NavMode::Pan {"pan"} else {"orbit"},"delta":[delta.x,delta.y]});
            if mode == NavMode::Orbit {
                let pivot = match app.preferences.orbit_around {
                    crate::prefs::OrbitAround::ViewCentre => None,
                    crate::prefs::OrbitAround::Origin => Some(Vec3::ZERO),
                    crate::prefs::OrbitAround::Cursor => Some(match app.viewport.orbit_pivot {
                        Some(pivot) => pivot,
                        None => {
                            let pivot = crate::prefs::pivot_under(app, &projection(app, rect), hover.unwrap_or(rect.center()) - delta);
                            app.viewport.orbit_pivot = Some(pivot);
                            pivot
                        }
                    }),
                };
                if let Some(pivot) = pivot {
                    params["pivot"] = json!(pivot);
                }
            }
            camera_motion(app, rect, params);
        }
    } else if mode == Some(NavMode::Orbit) {
        let press = ui.input(|i| i.pointer.press_origin());
        app.cancel_view_animation();
        // Shift can be required to enter orbit (middle drag, Alias Shift+Alt),
        // rather than an additional request for snapping. Only an extra modifier snaps.
        let without_shift = egui::Modifiers { shift: false, ..mods };
        let snap = app.preferences.nav != crate::prefs::OPENSCAD_NAV
            && mods.shift
            && match button {
                Some(b) => crate::prefs::nav_mode(app, b, without_shift) == Some(NavMode::Orbit),
                None => app.viewport.nav.is_some(),
            };
        orbit_drag(app, rect, f64::from(delta.x), f64::from(delta.y), press, snap, true);
    } else {
        if app.viewport.axis_drag.is_none() {
            app.viewport.orbit_free = None;
        }
        if delta != egui::Vec2::ZERO {
            if mode.is_some() {
                app.cancel_view_animation();
            }
            match mode {
                Some(NavMode::Pan) => app.cam.pan(f64::from(delta.x), f64::from(delta.y), h),
                // Opt-in drag-zoom holds the press point (#7435). Following the moving pointer
                // would slide the zoom away from what was pressed on.
                Some(NavMode::Zoom) => {
                    let press = ui.input(|i| i.pointer.press_origin());
                    let anchor = model_zoom_anchor(app, rect, press.or(hover), caps.orbit);
                    app.cam.zoom_at((f64::from(delta.y) * 0.01).exp(), anchor);
                }
                _ => {}
            }
        }
    }
    if !dragging && motion.is_none() && !(trackpad_orbit && app.viewport.trackpad_gesture) {
        app.viewport.orbit_pivot = None;
        app.viewport.orbit_from = None;
    }
    if app.viewport.spin {
        // A click on the navigation bar (its own button) is not a stop.
        let bar = app
            .viewport
            .layout
            .as_ref()
            .map(|l| l.navigation)
            .unwrap_or_else(|| Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 22.0), vec2(320.0, 30.0)));
        let (pressed, key, dt) = ui.input(|i| {
            (
                i.pointer.any_pressed() && inside && !i.pointer.press_origin().is_some_and(|p| bar.contains(p)),
                i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })),
                i.stable_dt,
            )
        });
        if pressed || key || dragging || motion.is_some() || !caps.orbit {
            app.viewport.spin = false;
        } else {
            let pivot = spin_pivot(app, rect);
            let dx = f64::from(dt.min(0.1)) * 30.0;
            match pivot {
                Some(p) => orbit_about(app, rect, dx, 0.0, p),
                None => orbit_camera(app, dx, 0.0),
            }
            ui.ctx().request_repaint();
        }
    }
    if !app.viewport.spin {
        app.viewport.spin_pivot = None;
    }
    if !app.cam.is_valid() {
        app.viewport.orbit_free = None;
        app.cam = Camera::default();
        app.fit_view();
    }
    CameraInput { hover, scroll, mods, middle, secondary_down, primary_down, primary_pressed, inside, axes, pinch, pan, primary_nav, mode }
}

/// The 3D mouse's motion this frame (#1336), on a page `rect` whose camera may do `caps`: the cap
/// pans and zooms every page, and turns the view where the page may orbit. In a sketch, turning
/// stays off while the camera is in its initial sketch orientation, unless the user lets the 3D
/// mouse turn sketches (`space_mouse_sketch_orbit`, #2227), as the mouse can. [`handle`] calls it
/// for its page; the 3D mouse only reads the device and queues the motion
/// ([`crate::space_mouse::navigate_frame`]).
pub(crate) fn device(app: &mut SolveApp, rect: Rect, caps: NavCaps) {
    if app.viewport.device_motion.is_none() && app.viewport.bridge_motion.is_none() {
        return;
    }
    let sketch_turns = app.preferences.space_mouse_sketch_orbit;
    let sketch_view_is_initial = !sketch_turns
        && solidcraft_engine::view::open_sketch_plane(&app.session).is_some_and(|plane| {
            let normal = plane.normal();
            let mut initial = app.cam.looking_from(normal);
            if normal.z.abs() > 0.999 {
                initial.yaw = if normal.z > 0.0 { (-plane.x.y).atan2(plane.x.x) } else { plane.x.y.atan2(plane.x.x) };
            }
            let (right, up, back) = app.cam.basis();
            let (initial_right, initial_up, initial_back) = initial.basis();
            right.dot(initial_right) > 0.999 && up.dot(initial_up) > 0.999 && back.dot(initial_back) > 0.999
        });
    let turn = caps.orbit && !sketch_view_is_initial;
    let height = f64::from(rect.height().max(1.0));
    // A device has no pointer, so opt-in zoom holds the model at the middle of the view (#7435).
    // Off by default: the cap and bridge can pass through the model, as before. The wheel and
    // pinch keep their separate "Zoom to the cursor" preference.
    let anchor = model_zoom_anchor(app, rect, Some(rect.center()), caps.orbit);
    // The view turns around the model in the middle of the view, else the middle of the bodies in
    // view, so what the user is looking at stays put. Off: around the view's centre, as before.
    // The cap keeps the point it started turning around until it is back at rest (and the view
    // shows it there); a device app sends no rest, so its turns pick it each frame.
    let pivot = (turn && app.preferences.space_mouse_model_pivot).then(|| {
        let cap = app.viewport.device_motion.is_some_and(|m| m.motion.rotation != [0.0; 3]);
        match app.viewport.device_pivot.filter(|_| cap) {
            Some(p) => p,
            None => {
                let p = crate::prefs::pivot_under(app, &projection(app, rect), rect.center());
                if cap {
                    app.viewport.device_pivot = Some(p);
                }
                p
            }
        }
    });
    if let Some(queued) = app.viewport.device_motion.take() {
        device_camera(&mut app.cam, queued.motion, queued.seconds, height, turn, app.preferences.free_orbit, anchor, pivot);
    }
    // A device app's motion (the navigation bridge), on the same terms.
    if let Some(queued) = app.viewport.bridge_motion.take() {
        bridge_camera(&mut app.cam, queued, height, turn, app.preferences.zoom_reverse, app.preferences.free_orbit, anchor, pivot);
    }
}

/// How far one zoom step of a device app (a click of its ring or wheel) moves the camera: this
/// share of its distance, compounded, so zooming in and back out returns where it was.
const BRIDGE_ZOOM_PER_STEP: f64 = 0.08;

/// A device app's `motion` (`space_mouse::bridge_frame`) on `cam`, in a view `height` points
/// tall: orbit and pan as a pointer drag of that many points, zoom by steps. With `turn` off (a
/// flat page, a sketch being edited) the view keeps its orientation and the orbit motion pans
/// instead, so the device still moves the view. `anchor` is the model point the view holds while
/// it zooms, as the wheel's cursor point (none zooms through the model, #7435). `pivot` is the
/// point the view turns around and keeps still on screen (none: the view's centre).
pub(crate) fn bridge_camera(
    cam: &mut Camera,
    motion: BridgeMotion,
    height: f64,
    turn: bool,
    reverse_zoom: bool,
    free_orbit: bool,
    anchor: Option<Vec3>,
    pivot: Option<Vec3>,
) {
    let motion = BridgeMotion::default().plus(motion);
    let [mut px, mut py] = motion.pan;
    let [ox, oy] = motion.orbit;
    if turn {
        let keep = pivot.map(|p| (p, cam.frame_coords(p)));
        if free_orbit {
            cam.rotate_in_view(Vec3::new(-oy * 0.008, -ox * 0.008, 0.0));
        } else {
            cam.orbit(ox, oy);
        }
        if let Some((p, coords)) = keep {
            cam.place(p, coords);
        }
    } else {
        px += ox;
        py += oy;
    }
    if px != 0.0 || py != 0.0 {
        cam.pan(px, py, height);
    }
    if motion.zoom != 0.0 {
        let zoom_sign = if reverse_zoom { -1.0 } else { 1.0 };
        cam.zoom_at((-motion.zoom * zoom_sign * BRIDGE_ZOOM_PER_STEP).exp(), anchor);
    }
}

/// `seconds` of 3D mouse `motion` on `cam`, in a view `height` points tall. A full deflection
/// pans about 0.6 of the view's height a second and turns about two radians a second, whatever
/// the frame rate. With `turn` off the view keeps its orientation. `anchor` is the model point the
/// view holds while it zooms, as the wheel's cursor point (none zooms through the model, #7435).
/// `pivot` is the point the view turns around and keeps still on screen (none: the view's centre).
/// Reverse zoom direction is a wheel setting and leaves the cap alone: the cap's own direction is
/// its per-axis invert, already in `motion` (#8993).
pub(crate) fn device_camera(
    cam: &mut Camera,
    motion: Motion,
    seconds: f64,
    height: f64,
    turn: bool,
    free_orbit: bool,
    anchor: Option<Vec3>,
    pivot: Option<Vec3>,
) {
    if !(seconds > 0.0) || !seconds.is_finite() {
        return;
    }
    let [x, y, z] = motion.translation;
    let [rx, ry, rz] = motion.rotation;
    // Logical 3DxWare coordinates are X = left/right, Y = zoom and Z = up/down. The user's
    // profile can swap the physical forward/back and up/down axes before they reach here.
    cam.pan(x * height * 0.6 * seconds, z * height * 0.6 * seconds, height);
    cam.zoom_at((-y * 1.5 * seconds).exp(), anchor);
    if turn {
        // These are logical 3DxWare coordinates after the profile mapping: Rx is Tilt, Ry is
        // Roll and Rz is Spin. The KMJ profile swaps physical Ry/Rz for the selected navigation
        // mode, so swapping them again here would make Roll behave like Spin.
        let keep = pivot.map(|p| (p, cam.frame_coords(p)));
        if free_orbit {
            cam.rotate_in_view(Vec3::new(-rx, -ry, rz) * (2.0 * seconds));
        } else {
            cam.orbit(ry * 250.0 * seconds, rx * 250.0 * seconds);
            cam.roll_by(rz * 2.0 * seconds);
        }
        if let Some((p, coords)) = keep {
            cam.place(p, coords);
        }
    }
}

/// Six principal views, twelve edge diagonals and eight isometric corner directions.
pub(crate) fn snap_directions() -> impl Iterator<Item = Vec3> {
    [-1.0, 0.0, 1.0].into_iter().flat_map(|x| {
        [-1.0, 0.0, 1.0].into_iter().flat_map(move |y| [-1.0, 0.0, 1.0].into_iter().filter_map(move |z| Vec3::new(x, y, z).normalized()))
    })
}

/// Snap to the nearest standard or diagonal direction in the user's up-axis frame.
pub(crate) fn snapped_axis_camera(cam: Camera) -> Camera {
    let b = cam.to_frame(cam.back());
    let dir = snap_directions().max_by(|a, c| a.dot(b).total_cmp(&c.dot(b))).unwrap_or(Vec3::Z);
    let mut to = cam.looking_from(cam.from_frame(dir));
    if dir.x == 0.0 && dir.y == 0.0 {
        to.yaw = nearest_quarter_turn(cam.yaw);
    }
    to
}

/// Choose the closest quarter-turn picture orientation without losing roll at a pole.
fn snapped_quarter_roll(mut target: Camera, reference: Camera) -> Camera {
    let (right, up, back) = reference.basis();
    let mut best = target;
    let mut best_score = f64::NEG_INFINITY;
    for quarter in 0..4 {
        target.roll = f64::from(quarter) * std::f64::consts::FRAC_PI_2;
        let (r, u, b) = target.basis();
        // Maximizing the rotation trace minimizes the angular distance between orientations.
        let score = r.dot(right) + u.dot(up) + b.dot(back);
        if score > best_score {
            best = target;
            best_score = score;
        }
    }
    best
}

/// Orbit by a pointer drag. With `snap` the displayed view is the nearest standard or diagonal
/// direction at once; the unsnapped orbit is kept in `orbit_free` so many small deltas still
/// cross snap boundaries. A small angular margin avoids flicker at a boundary.
pub(crate) fn orbit_drag(app: &mut SolveApp, rect: Rect, dx: f64, dy: f64, press: Option<Pos2>, snap: bool, around_cursor: bool) {
    let displayed = app.cam;
    let previous_free = app.viewport.orbit_free.take();
    if let Some(free) = previous_free {
        app.cam = free;
    }
    if around_cursor {
        orbit(app, rect, dx, dy, press);
    } else {
        orbit_camera(app, dx, dy);
    }
    let free = app.cam;
    app.viewport.orbit_free = Some(free);
    if !snap {
        if app.preferences.free_orbit {
            app.viewport.orbit_free = None;
        }
        return;
    }
    let mut target = snapped_axis_camera(free);
    // Keep the direction already chosen unless the orbit has clearly left it.
    if previous_free.is_some_and(|p| p != displayed) {
        let b = free.back();
        if displayed.back().dot(b) + 0.025 >= target.back().dot(b) {
            target.yaw = displayed.yaw;
            target.pitch = displayed.pitch;
        }
    }
    if app.preferences.free_orbit {
        target = snapped_quarter_roll(target, free);
    }
    // The cursor and origin pivots stay still; the axis gizmo has no pivot (it orbits about the
    // view centre, see `axis_view`).
    let pivot = if around_cursor {
        match app.preferences.orbit_around {
            crate::prefs::OrbitAround::Origin => Some(Vec3::ZERO),
            _ => app.viewport.orbit_pivot,
        }
    } else {
        None
    };
    app.cam = target;
    if let Some(p) = pivot {
        app.cam.place(p, displayed.frame_coords(p));
    }
}

/// A middle-button double-click goes Home (or fits, or nothing: Preferences › Navigation). Only
/// clicks count: a press that moves (a pan or orbit drag) or is held is not one, and the two
/// clicks must both be the middle button's, close together in time and place.
pub(crate) fn middle_double_click(app: &mut SolveApp, ui: &egui::Ui, inside: bool) {
    use egui::PointerButton::{Extra1, Extra2, Middle, Primary, Secondary};
    let (double, releases, middle, other, dragging) = ui.input(|i| {
        (
            i.pointer.button_double_clicked(Middle),
            i.events.iter().filter(|e| matches!(e, egui::Event::PointerButton { button: Middle, pressed: false, .. })).count(),
            i.pointer.button_clicked(Middle),
            [Primary, Secondary, Extra1, Extra2].into_iter().any(|b| i.pointer.button_clicked(b)),
            i.pointer.is_decidedly_dragging(),
        )
    });
    // egui pairs clicks of any buttons; the first click must have been the middle one too (or
    // both arrive in this frame).
    let go = double && inside && !other && (app.viewport.middle_clicked || releases >= 2);
    app.viewport.middle_clicked = if other || dragging || go { false } else { middle || app.viewport.middle_clicked };
    if !go {
        return;
    }
    match crate::prefs::MiddleDoubleClick::of(app) {
        crate::prefs::MiddleDoubleClick::Home => app.animate_view("home"),
        crate::prefs::MiddleDoubleClick::Fit => app.animate_fit_all(),
        crate::prefs::MiddleDoubleClick::Off => {}
    }
}

/// Orbit by pixel deltas about the configured centre: the view's centre (default), the world
/// origin, or the model point that was under the cursor when the drag began. The origin and the
/// cursor pivot are kept still on screen; a cursor drag that begins over empty space turns around
/// the centre of the visible geometry instead, so the model never swings around a far-off point.
/// The cursor pivot is chosen once, when the drag begins.
pub fn orbit(app: &mut SolveApp, rect: egui::Rect, dx: f64, dy: f64, press: Option<Pos2>) {
    let pivot = match app.preferences.orbit_around {
        OrbitAround::ViewCentre => None,
        OrbitAround::Origin => Some(Vec3::ZERO),
        OrbitAround::Cursor => Some(match app.viewport.orbit_pivot {
            Some(p) => p,
            None => {
                let proj = crate::viewport::projection(app, rect);
                let at = press.unwrap_or(rect.center());
                let p = pivot_under(app, &proj, at);
                app.viewport.orbit_pivot = Some(p);
                p
            }
        }),
    };
    if app.preferences.free_orbit {
        app.viewport.orbit_from = None;
        match pivot {
            Some(p) => orbit_about(app, rect, dx, dy, p),
            None => orbit_camera(app, dx, dy),
        }
        return;
    }
    // The drag's whole turn so far, applied to the camera it began with: the view depends on
    // where the pointer is, not on the path it took, so fast or erratic drags (a pitch held at
    // straight up or down for a while, many tiny steps) can't drift. Anything else that moves
    // the camera meanwhile (a wheel zoom, a new pivot) starts the sum again from there.
    let from = match app.viewport.orbit_from {
        Some(f) if f.last == app.cam && f.pivot == pivot => f,
        _ => OrbitFrom { start: app.cam, dx: 0.0, dy: 0.0, pivot, last: app.cam },
    };
    let (dx, dy) = (from.dx + dx, from.dy + dy);
    app.cam = from.start;
    match pivot {
        Some(p) => app.cam.orbit_about(dx, dy, p),
        None => app.cam.orbit(dx, dy),
    }
    app.viewport.orbit_from = Some(OrbitFrom { dx, dy, last: app.cam, ..from });
}

/// What continuous orbit turns about, by Orbit around: the view's centre (none), the world origin,
/// or around the cursor the model point in the middle of the view (else the middle of the bodies
/// in view), chosen once when the spin starts. Turning about the view's centre instead let the
/// model swing out of view once earlier cursor orbits had left that centre off the model (#1044).
fn spin_pivot(app: &mut SolveApp, rect: Rect) -> Option<Vec3> {
    match app.preferences.orbit_around {
        OrbitAround::ViewCentre => None,
        OrbitAround::Origin => Some(Vec3::ZERO),
        OrbitAround::Cursor => Some(match app.viewport.spin_pivot {
            Some(p) => p,
            None => {
                let p = pivot_under(app, &projection(app, rect), rect.center());
                app.viewport.spin_pivot = Some(p);
                p
            }
        }),
    }
}

/// An orbit drag so far: the camera it began with, the pointer's total motion since, the pivot
/// and the camera that produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitFrom {
    start: Camera,
    dx: f64,
    dy: f64,
    pivot: Option<Vec3>,
    last: Camera,
}

/// Rotate rigidly about the chosen pivot, keeping its camera-frame position unchanged.
pub(crate) fn orbit_about(app: &mut SolveApp, _rect: egui::Rect, dx: f64, dy: f64, pivot: Vec3) {
    let coords = app.cam.frame_coords(pivot);
    orbit_camera(app, dx, dy);
    app.cam.place(pivot, coords);
}

fn orbit_camera(app: &mut SolveApp, dx: f64, dy: f64) {
    if !app.preferences.free_orbit {
        app.cam.orbit(dx, dy);
    } else if app.preferences.free_orbit_upright {
        app.cam.orbit_unlimited(dx, dy);
    } else {
        app.cam.rotate_in_view(Vec3::new(-dy * 0.008, -dx * 0.008, 0.0));
    }
}

#[cfg(test)]
mod tests {
    fn web_wheel(unit: egui::MouseWheelUnit, delta: egui::Vec2, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::MouseWheel { unit, delta, modifiers, phase: egui::TouchPhase::Move }
    }

    fn web_frame(
        state: &mut super::WebWheelGestures,
        time: f64,
        events: Vec<egui::Event>,
        gesture: &mut bool,
    ) -> (super::RawWheel, Vec<egui::Event>) {
        let mut raw = egui::RawInput { time: Some(time), events, focused: true, ..Default::default() };
        let _ = state.update(&mut raw, time);
        let events = raw.events.clone();
        let mut input = egui::InputState::default();
        input.events = raw.events;
        (
            super::immediate_wheel(
                &input,
                egui::InputOptions::default(),
                600.0,
                gesture,
                &mut Default::default(),
                false,
                Some(egui::Modifiers::COMMAND),
            ),
            events,
        )
    }

    #[test]
    fn modifier_zoom_routes_only_the_chosen_key_and_vertical_points() {
        use egui::{Modifiers as M, MouseWheelUnit as U, TouchPhase as P, vec2};
        let options = egui::InputOptions::default();
        for chosen in [Some(M::COMMAND), Some(M::ALT), Some(M::CTRL), None] {
            for held in [
                M::COMMAND,
                M::CTRL | M::COMMAND,
                M::MAC_CMD | M::COMMAND,
                M::CTRL,
                M::ALT,
                M::SHIFT,
                M::COMMAND | M::SHIFT,
                M::COMMAND | M::ALT,
                M::MAC_CMD | M::COMMAND | M::CTRL,
                M::ALT | M::SHIFT,
            ] {
                for unit in [U::Point, U::Line, U::Page] {
                    for bracket in [false, true] {
                        let event = |phase| egui::Event::MouseWheel { unit, delta: vec2(8.0, 12.0), modifiers: held, phase };
                        let mut input = egui::InputState::default();
                        input.events = if bracket { vec![event(P::Start), event(P::Move), event(P::End)] } else { vec![event(P::Move)] };
                        let raw = super::immediate_wheel(&input, options, 600.0, &mut false, &mut Default::default(), true, chosen);
                        let expected = match chosen {
                            Some(m) if m == M::COMMAND => [M::COMMAND, M::CTRL | M::COMMAND, M::MAC_CMD | M::COMMAND].contains(&held),
                            Some(m) if m == M::CTRL => [M::CTRL, M::CTRL | M::COMMAND].contains(&held),
                            Some(m) if m == M::ALT => held == M::ALT,
                            _ => false,
                        };
                        let scale = match unit {
                            U::Point => 1.0,
                            U::Line => options.line_scroll_speed,
                            U::Page => 600.0,
                        };
                        assert_eq!(raw.modifier_zoom, if expected { 12.0 * scale } else { 0.0 }, "chosen={chosen:?}, held={held:?}");
                        if expected {
                            assert_eq!(raw.pan, egui::Vec2::ZERO);
                            assert_eq!(raw.mouse_pan, egui::Vec2::ZERO);
                            assert_eq!(raw.zoom, 0.0);
                            assert_eq!(raw.point_zoom, 0.0);
                            assert_eq!(raw.shift_pan, egui::Vec2::ZERO);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn web_swipes_pan_from_the_first_delta_and_end_after_idle() {
        use egui::{Modifiers, MouseWheelUnit::Point, TouchPhase, vec2};
        let mut state = super::WebWheelGestures::default();
        let mut gesture = false;
        for (time, delta) in [(0.0, vec2(0.0, 7.5)), (0.04, vec2(3.0, 0.0)), (0.09, vec2(0.0, 120.0))] {
            let (wheel, events) = web_frame(&mut state, time, vec![web_wheel(Point, delta, Modifiers::NONE)], &mut gesture);
            assert_eq!(wheel.pan, delta);
            assert_eq!(wheel.zoom, 0.0);
            assert!(gesture);
            let starts = events.iter().filter(|e| matches!(e, egui::Event::MouseWheel { phase: TouchPhase::Start, .. })).count();
            assert_eq!(starts, usize::from(time == 0.0));
        }
        let delay = state.update(&mut egui::RawInput::default(), 0.10).expect("schedule the idle end even without more input");
        assert!((delay.as_secs_f64() - 0.09).abs() < 1e-9);
        web_frame(&mut state, 0.18, vec![], &mut gesture);
        assert!(gesture);
        let (_, events) = web_frame(&mut state, 0.20, vec![], &mut gesture);
        assert!(!gesture);
        assert!(matches!(events.as_slice(), [egui::Event::MouseWheel { phase: TouchPhase::End, delta, .. }] if *delta == egui::Vec2::ZERO));
        let (wheel, _) = web_frame(&mut state, 0.21, vec![web_wheel(Point, vec2(0.0, -2.0), Modifiers::NONE)], &mut gesture);
        assert_eq!(wheel.pan, vec2(0.0, -2.0));
    }

    #[test]
    fn web_wheel_steps_and_pinch_keep_zooming() {
        use egui::{Modifiers, MouseWheelUnit::*, vec2};
        for (unit, delta, mods) in [
            (Point, vec2(0.0, 120.0), Modifiers::NONE),
            (Point, vec2(0.0, -100.0), Modifiers::NONE),
            (Point, vec2(0.0, 125.0), Modifiers::NONE),
            (Point, vec2(0.0, -111.111), Modifiers::NONE),
            (Line, vec2(1.0, 3.0), Modifiers::NONE),
            (Page, vec2(0.0, 1.0), Modifiers::NONE),
            (Point, vec2(0.0, 2.5), Modifiers::CTRL),
        ] {
            let mut state = super::WebWheelGestures::default();
            let mut gesture = false;
            let event = web_wheel(unit, delta, mods);
            let (wheel, events) = web_frame(&mut state, 0.0, vec![event.clone()], &mut gesture);
            assert_eq!(events, vec![event.clone()]);
            assert_eq!(wheel.pan, egui::Vec2::ZERO);
            if !mods.ctrl {
                assert_ne!(wheel.zoom, 0.0);
            }
            web_frame(&mut state, 0.01, vec![web_wheel(Point, vec2(0.0, 2.5), Modifiers::NONE)], &mut gesture);
            assert!(gesture);
            let (wheel, _) = web_frame(&mut state, 0.02, vec![event], &mut gesture);
            // Point steps can occur during a swipe; explicit line/page wheels and Ctrl cannot.
            if unit != Point || mods.ctrl {
                assert!(!gesture);
                assert_eq!(wheel.pan, egui::Vec2::ZERO);
            }
        }
        let mut state = super::WebWheelGestures::default();
        let mut gesture = false;
        web_frame(&mut state, 0.0, vec![web_wheel(Point, vec2(0.0, 2.5), Modifiers::NONE)], &mut gesture);
        assert!(gesture);
        let (_, events) = web_frame(&mut state, 0.01, vec![egui::Event::Zoom(1.1)], &mut gesture);
        assert!(!gesture);
        assert_eq!(events.last(), Some(&egui::Event::Zoom(1.1)));
        let mut input = egui::InputState::default();
        input.events = events;
        assert!((super::pinch_factor(&input, egui::InputOptions::default(), 600.0, None) - 1.1).abs() < 1e-6);
    }

    #[test]
    fn web_gesture_timeout_and_invalid_input_do_not_leave_a_gesture_active() {
        use egui::{Modifiers, MouseWheelUnit::Point, vec2};
        let mut state = super::WebWheelGestures::default();
        let mut gesture = false;
        web_frame(&mut state, 1.0, vec![web_wheel(Point, vec2(0.0, 3.5), Modifiers::NONE)], &mut gesture);
        let (wheel, events) = web_frame(&mut state, 1.2, vec![web_wheel(Point, vec2(0.0, 120.0), Modifiers::NONE)], &mut gesture);
        assert_eq!(wheel.zoom, 120.0);
        assert!(!gesture);
        assert_eq!(events.len(), 2);
        for time in [f64::NAN, f64::INFINITY, -1.0] {
            web_frame(&mut state, 2.0, vec![web_wheel(Point, vec2(0.0, 3.5), Modifiers::NONE)], &mut gesture);
            web_frame(&mut state, time, vec![], &mut gesture);
            assert!(!gesture);
        }
        for delta in [vec2(f32::NAN, 1.0), vec2(0.0, f32::INFINITY), vec2(0.0, 0.0)] {
            let (_, events) = web_frame(&mut state, 3.0, vec![web_wheel(Point, delta, Modifiers::NONE)], &mut gesture);
            assert_eq!(events.len(), 1);
            assert!(!gesture);
        }
    }

    #[test]
    fn clamped_drag_stays_reversible_and_restarts_after_free_orbit() {
        use super::*;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        for fov in [0.0, 0.6] {
            let mut app = crate::SolveApp::new(Default::default(), crate::Services::default());
            app.preferences.orbit_around = OrbitAround::Origin;
            app.cam = Camera { target: Vec3::new(2.0, 3.0, 4.0), pitch: 1.4, roll: 0.7, fov, ..Default::default() };
            let start = app.cam;
            for (dx, dy) in [(120.0, 400.0), (90.0, -200.0), (-210.0, -200.0)] {
                orbit(&mut app, rect, dx, dy, None);
            }
            assert!(app.cam.back().dist(start.back()) < 1e-9);
            assert!(app.cam.target.dist(start.target) < 1e-9);
            app.preferences.free_orbit = true;
            orbit(&mut app, rect, 0.0, 80.0, None);
            assert!((app.cam.back().dot(start.back()) - 0.64_f64.cos()).abs() < 1e-9);
            let mut next = app.cam;
            next.orbit_about(10.0, 20.0, Vec3::ZERO);
            app.preferences.free_orbit = false;
            orbit(&mut app, rect, 10.0, 20.0, None);
            assert!(app.cam.back().dist(next.back()) < 1e-9);
            assert!(app.cam.target.dist(next.target) < 1e-9);
        }
    }

    fn touchpad_frame(app: &mut crate::SolveApp, ctx: &egui::Context, caps: super::NavCaps, at: egui::Pos2, mods: egui::Modifiers, time: &mut f64) {
        *time += 1.0 / 60.0;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 1400.0))),
            time: Some(*time),
            focused: true,
            events: vec![egui::Event::ModifiersChanged(mods), egui::Event::PointerMoved(at)],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let rect = ui.max_rect();
            app.viewport.rect = Some(rect);
            let response = ui.interact(rect, ui.id().with("touchpad-setting"), egui::Sense::click_and_drag());
            super::handle(app, ui, rect, &response, caps);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn touchpad_free_orbit_setting_reaches_the_shared_command() {
        use super::*;
        for caps in [NavCaps::MODEL, NavCaps::RENDER, NavCaps { orbit: false, pick: false }] {
            for up in [solidcraft_engine::render::UpAxis::Y, solidcraft_engine::render::UpAxis::Z] {
                for free in [false, true] {
                    let ctx = egui::Context::default();
                    let mut app = crate::SolveApp::new(Default::default(), crate::Services::default());
                    app.preferences.nav = crate::prefs::TOUCHPAD_NAV.into();
                    app.preferences.free_orbit = free;
                    app.ui.up_axis = up;
                    app.cam = Camera { pitch: 1.4, roll: 0.7, up, ..Default::default() };
                    let before = app.cam;
                    let design = app.session.doc.to_json();
                    let mut time = 0.0;
                    touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 300.0), egui::Modifiers::NONE, &mut time);
                    touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 300.0), egui::Modifiers::ALT, &mut time);
                    touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 380.0), egui::Modifiers::ALT, &mut time);
                    if !caps.orbit {
                        assert_eq!(app.cam, before);
                    } else if free {
                        let expected = before.back() * 0.64_f64.cos() + before.basis().1 * 0.64_f64.sin();
                        assert!(app.cam.back().dist(expected) < 1e-8, "{caps:?} {up:?}: {:?}", app.cam);
                    } else {
                        assert!(app.cam.pitch > before.pitch && app.cam.pitch < std::f64::consts::FRAC_PI_2);
                    }
                    assert_eq!((app.cam.target, app.cam.distance, app.cam.fov), (before.target, before.distance, before.fov));
                    assert!(app.viewport.orbit_free.is_none());
                    assert!(!ctx.input(|i| i.pointer.any_down()));
                    assert_eq!(app.session.doc.to_json(), design);
                    assert!(app.session.undo.is_empty() && app.session.selection.is_empty());
                }
            }
        }
    }

    #[test]
    fn touchpad_free_orbit_keeps_the_rigid_pivot_in_both_projections() {
        use super::*;
        for up in [solidcraft_engine::render::UpAxis::Y, solidcraft_engine::render::UpAxis::Z] {
            for fov in [0.0, 0.6] {
                for around in [OrbitAround::Origin, OrbitAround::Cursor] {
                    let ctx = egui::Context::default();
                    let mut app = crate::SolveApp::new(Default::default(), crate::Services::default());
                    app.preferences.nav = crate::prefs::TOUCHPAD_NAV.into();
                    app.preferences.free_orbit = true;
                    app.preferences.orbit_around = around;
                    app.ui.perspective = fov > 0.0;
                    app.ui.up_axis = up;
                    app.cam = Camera { target: Vec3::new(2.0, 3.0, 4.0), pitch: 1.4, roll: 0.7, fov, up, ..Default::default() };
                    let caps = NavCaps::MODEL;
                    let mut time = 0.0;
                    touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 300.0), egui::Modifiers::NONE, &mut time);
                    touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 300.0), egui::Modifiers::ALT, &mut time);
                    let pivot = if around == OrbitAround::Origin { Vec3::ZERO } else { Vec3::new(10.0, 20.0, 30.0) };
                    app.viewport.orbit_pivot = Some(pivot);
                    let rect = app.viewport.rect.unwrap();
                    let coords = app.cam.frame_coords(pivot);
                    let pixel = projection(&app, rect).to_screen(pivot).unwrap();
                    touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 380.0), egui::Modifiers::ALT, &mut time);
                    assert!(app.cam.frame_coords(pivot).dist(coords) < 1e-8, "{up:?} {fov} {around:?}");
                    assert!(projection(&app, rect).to_screen(pivot).unwrap().distance(pixel) < 1e-3);
                    assert_eq!((app.cam.distance, app.cam.fov), (Camera::default().distance, fov));
                }
            }
        }
    }

    #[test]
    fn touchpad_free_orbit_completes_a_turn_and_stops_on_release() {
        use super::*;
        let ctx = egui::Context::default();
        let mut app = crate::SolveApp::new(Default::default(), crate::Services::default());
        app.preferences.nav = crate::prefs::TOUCHPAD_NAV.into();
        app.preferences.free_orbit = true;
        app.cam = Camera { pitch: 1.4, roll: 0.7, ..Default::default() };
        let initial = app.cam;
        let design = app.session.doc.to_json();
        let log = app.session.log.len();
        let mut time = 0.0;
        let caps = NavCaps::MODEL;
        touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 300.0), egui::Modifiers::NONE, &mut time);
        touchpad_frame(&mut app, &ctx, caps, pos2(400.0, 300.0), egui::Modifiers::ALT, &mut time);
        let mut at = pos2(400.0, 300.0);
        for step in 1..=128 {
            at.y = 300.0 + (std::f64::consts::TAU * step as f64 / (128.0 * 0.008)) as f32;
            touchpad_frame(&mut app, &ctx, caps, at, egui::Modifiers::ALT, &mut time);
        }
        assert!(app.cam.back().dist(initial.back()) < 1e-6);
        assert!(app.cam.basis().0.dist(initial.basis().0) < 1e-6);
        let stopped = app.cam;
        for _ in 0..60 {
            touchpad_frame(&mut app, &ctx, caps, at, egui::Modifiers::NONE, &mut time);
        }
        assert_eq!(app.cam, stopped);
        assert_eq!(app.session.doc.to_json(), design);
        assert_eq!(app.session.log.len(), log);
        assert!(app.session.undo.is_empty() && app.session.selection.is_empty());
    }

    #[test]
    fn every_standard_direction_preserves_its_nearby_quarter_orientation() {
        use super::*;
        use solidcraft_engine::render::UpAxis;
        for up in [UpAxis::Z, UpAxis::Y] {
            for fov in [0.0, 0.6] {
                for direction in snap_directions() {
                    for quarter in 0..4 {
                        let base = Camera { up, fov, ..Default::default() };
                        let mut expected = snapped_axis_camera(base.looking_from(base.from_frame(direction)));
                        expected.roll = f64::from(quarter) * std::f64::consts::FRAC_PI_2;
                        let mut free = expected;
                        free.rotate_in_view(Vec3::new(0.008, -0.01, 0.0));
                        let result = snapped_quarter_roll(snapped_axis_camera(free), free);
                        let (r, u, b) = result.basis();
                        let (er, eu, eb) = expected.basis();
                        assert!(
                            r.dist(er) < 4e-6 && u.dist(eu) < 4e-6 && b.dist(eb) < 4e-6,
                            "up={up:?} direction={direction:?} quarter={quarter}: {result:?}"
                        );
                        assert_eq!(result.target, free.target);
                        assert_eq!(result.distance, free.distance);
                        assert_eq!(result.fov, free.fov);
                    }
                }
            }
        }
    }

    #[test]
    fn free_orbit_shift_snaps_direction_and_quarter_turn_roll() {
        use super::*;
        use solidcraft_engine::render::UpAxis;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        for up in [UpAxis::Z, UpAxis::Y] {
            for fov in [0.0, 0.6] {
                for quarter in 0..4 {
                    let roll = f64::from(quarter) * std::f64::consts::FRAC_PI_2;
                    let mut app = crate::SolveApp::new(Default::default(), crate::Services::default());
                    app.preferences.free_orbit = true;
                    app.cam = Camera { target: Vec3::new(2.0, 3.0, 4.0), yaw: 0.08, pitch: 0.1, roll: roll + 0.08, distance: 47.0, fov, up };
                    let expected = Camera { yaw: 0.0, pitch: 0.0, roll, ..app.cam };
                    orbit_drag(&mut app, rect, 0.0, 0.0, None, true, false);
                    let (right, vertical, back) = app.cam.basis();
                    let (r, u, b) = expected.basis();
                    assert!(
                        right.dist(r) < 1e-9 && vertical.dist(u) < 1e-9 && back.dist(b) < 1e-9,
                        "up={up:?}, fov={fov}, quarter={quarter}: {:?}",
                        app.cam
                    );
                    assert_eq!(app.cam.target, expected.target);
                    assert_eq!(app.cam.distance, expected.distance);
                    assert_eq!(app.cam.fov, expected.fov);
                    assert!(app.viewport.orbit_free.is_some());
                }
            }
        }
    }

    #[test]
    fn free_orbit_shift_keeps_the_pivot_and_resumes_the_unsnapped_drag() {
        use super::*;
        use solidcraft_engine::render::UpAxis;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        for up in [UpAxis::Z, UpAxis::Y] {
            for fov in [0.0, 0.6] {
                for around in [OrbitAround::ViewCentre, OrbitAround::Origin, OrbitAround::Cursor] {
                    let mut app = crate::SolveApp::new(Default::default(), crate::Services::default());
                    let mut reference = crate::SolveApp::new(Default::default(), crate::Services::default());
                    let initial = Camera { target: Vec3::new(2.0, 3.0, 4.0), yaw: 0.2, pitch: 0.3, roll: 2.8, fov, up, ..Default::default() };
                    let pivot = match around {
                        OrbitAround::ViewCentre => None,
                        OrbitAround::Origin => Some(Vec3::ZERO),
                        OrbitAround::Cursor => Some(Vec3::new(7.0, -2.0, 3.0)),
                    };
                    for a in [&mut app, &mut reference] {
                        a.preferences.free_orbit = true;
                        a.preferences.orbit_around = around;
                        a.cam = initial;
                        a.viewport.orbit_pivot = pivot;
                    }
                    let design = app.session.doc.to_json();
                    let log = app.session.log.len();
                    for (dx, dy) in [(12.0, 5.0), (-6.0, 8.0), (3.0, -4.0)] {
                        orbit_drag(&mut reference, rect, dx, dy, None, false, true);
                        orbit_drag(&mut app, rect, dx, dy, None, true, true);
                        assert!(app.viewport.orbit_free.is_some());
                        if let Some(p) = pivot {
                            assert!(app.cam.frame_coords(p).dist(initial.frame_coords(p)) < 1e-8);
                        }
                    }
                    orbit_drag(&mut app, rect, 0.0, 0.0, None, false, true);
                    let (right, vertical, back) = app.cam.basis();
                    let (r, u, b) = reference.cam.basis();
                    assert!(right.dist(r) < 1e-9 && vertical.dist(u) < 1e-9 && back.dist(b) < 1e-9);
                    assert!(app.cam.target.dist(reference.cam.target) < 1e-8);
                    assert!(app.viewport.orbit_free.is_none());
                    assert_eq!(app.session.doc.to_json(), design);
                    assert_eq!(app.session.log.len(), log);
                    assert!(app.session.undo.is_empty());
                }
            }
        }
    }

    #[test]
    fn free_orbit_is_opt_in_for_every_preset_and_shared_by_mouse_and_device() {
        use super::*;
        use crate::space_mouse::DeviceMotion;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let initial = Camera { yaw: 0.0, pitch: 1.4, ..Default::default() };
        for preset in crate::prefs::NAV_PRESETS {
            for enabled in [false, true] {
                let mut h = crate::scenario::Harness::new();
                h.app.preferences.nav = preset.id.into();
                h.app.preferences.free_orbit = enabled;
                h.app.cam = initial;
                orbit_drag(&mut h.app, rect, 0.0, 80.0, None, !enabled, false);
                if enabled {
                    assert!((h.app.cam.back().dot(initial.back()) - 0.64_f64.cos()).abs() < 1e-9);
                    assert!(h.app.viewport.orbit_free.is_none());
                } else {
                    assert!(h.app.cam.pitch < std::f64::consts::FRAC_PI_2);
                    assert!(h.app.viewport.orbit_free.is_some(), "{} keeps Shift snap", preset.id);
                }
                h.app.cam = initial;
                h.app.viewport.device_motion =
                    Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [1.0, 0.0, 0.0] }, seconds: 0.32 });
                device(&mut h.app, rect, NavCaps::MODEL);
                if enabled {
                    assert!((h.app.cam.back().dot(initial.back()) - 0.64_f64.cos()).abs() < 1e-9);
                } else {
                    assert!(h.app.cam.pitch > initial.pitch && h.app.cam.pitch < std::f64::consts::FRAC_PI_2);
                }
            }
        }
    }

    /// Free orbit turning around the vertical axis: sideways drags keep the elevation and the
    /// world's up axis on screen (no tumbling), and up/down drags go on over the top; past it,
    /// sideways still turns about the vertical axis, the model following the cursor.
    #[test]
    fn free_orbit_upright_turns_about_the_vertical_axis_past_the_pole() {
        use super::*;
        for up in [solidcraft_engine::render::UpAxis::Z, solidcraft_engine::render::UpAxis::Y] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.free_orbit = true;
            h.app.preferences.free_orbit_upright = true;
            h.app.cam = Camera { yaw: 0.2, pitch: 0.6, up, ..Default::default() };
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
            let height = |c: &Camera| c.back().dot(c.up_dir());
            let before = h.app.cam;
            for _ in 0..50 {
                orbit_drag(&mut h.app, rect, 7.0, 0.0, None, false, false);
                assert!((height(&h.app.cam) - height(&before)).abs() < 1e-9, "{up:?}: {:?}", h.app.cam);
                assert!(h.app.cam.basis().0.dot(h.app.cam.up_dir()).abs() < 1e-9, "{up:?}: the horizon stays level");
            }
            assert!((h.app.cam.yaw - (before.yaw + 350.0 * 0.008)).rem_euclid(std::f64::consts::TAU) < 1e-9);
            // Up over the top: 2 rad of tilt from 0.6 rad elevation passes straight down.
            h.app.cam = Camera { yaw: 0.0, pitch: 0.6, up, ..Default::default() };
            for _ in 0..100 {
                orbit_drag(&mut h.app, rect, 0.0, 2.5, None, false, false);
            }
            let past = h.app.cam;
            assert!((height(&past) - 2.6_f64.sin()).abs() < 1e-9, "{up:?}: {past:?}");
            assert!(past.basis().1.dot(past.up_dir()) < 0.0, "{up:?}: upside down past the pole");
            orbit_drag(&mut h.app, rect, 10.0, 0.0, None, false, false);
            assert!((height(&h.app.cam) - height(&past)).abs() < 1e-9);
            // Upright and upside down alike, a drag to the right moves the near side right.
            let near = |c: &Camera| c.target + (c.back() - c.up_dir() * height(c)) * 10.0;
            assert!(h.app.cam.frame_coords(near(&past)).x > 0.5, "{up:?}: {:?}", h.app.cam);
            h.app.cam = before;
            orbit_drag(&mut h.app, rect, 10.0, 0.0, None, false, false);
            assert!(h.app.cam.frame_coords(near(&before)).x > 0.5, "{up:?}: {:?}", h.app.cam);
        }
        // Without the option, free orbit still turns about the screen's axes.
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.free_orbit = true;
        h.app.cam = Camera { yaw: 0.0, pitch: 0.6, ..Default::default() };
        orbit_drag(&mut h.app, Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)), 50.0, 0.0, None, false, false);
        assert!((h.app.cam.back().z - 0.6_f64.sin()).abs() > 1e-3);
    }

    #[test]
    fn solidworks_mouse_orbit_passes_the_pole_and_shift_snaps() {
        use serde_json::json;
        for up in [solidcraft_engine::render::UpAxis::Z, solidcraft_engine::render::UpAxis::Y] {
            for button in ["middle", "right"] {
                let mut h = crate::scenario::Harness::new();
                h.app.home.open = false;
                h.app.preferences.nav = "solidworks".into();
                h.app.preferences.free_orbit = true;
                h.app.ui.up_axis = up;
                h.app.cam = solidcraft_engine::render::Camera { yaw: 0.0, pitch: 1.4, up, ..Default::default() };
                let before = h.app.cam;
                h.call("ui.drag", json!({"x0":800,"y0":400,"x1":800,"y1":480,"button":button,"steps":40}));
                assert!((h.app.cam.back().dot(before.back()) - 0.64_f64.cos()).abs() < 1e-6, "{up:?} {button}: {:?}", h.app.cam);
                assert!(h.app.viewport.orbit_free.is_none());
                h.call("ui.drag", json!({"x0":800,"y0":400,"x1":850,"y1":400,"button":"right","shift":true,"steps":20}));
                let snapped = super::snapped_axis_camera(h.app.cam);
                assert!(h.app.cam.back().dist(snapped.back()) < 4e-6);
                assert!((h.app.cam.roll / std::f64::consts::FRAC_PI_2 - (h.app.cam.roll / std::f64::consts::FRAC_PI_2).round()).abs() < 1e-9);
                assert!(h.app.viewport.orbit_free.is_none());
            }
        }
    }

    #[test]
    fn right_drag_orbits_off_leaves_the_camera_to_the_middle_button() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::choose_nav_preset(&mut h.app.preferences, crate::prefs::nav_preset("solidworks"));
        let before = h.app.cam;
        for shift in [false, true] {
            h.call("ui.drag", json!({"x0":800,"y0":400,"x1":850,"y1":440,"button":"right","shift":shift,"steps":20}));
            assert_eq!(h.app.cam.back(), before.back(), "shift {shift}: a right drag leaves the camera alone (#6492)");
        }
        h.call("ui.drag", json!({"x0":800,"y0":400,"x1":850,"y1":440,"button":"middle","steps":20}));
        assert_ne!(h.app.cam.back(), before.back(), "the middle button still orbits");
    }

    #[test]
    fn solidworks_device_and_mouse_keep_turning_with_the_same_screen_axes() {
        use super::*;
        use crate::space_mouse::DeviceMotion;
        for caps in [NavCaps::MODEL, NavCaps::RENDER] {
            for rotation in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                let mut h = crate::scenario::Harness::new();
                h.app.preferences.nav = "solidworks".into();
                h.app.preferences.free_orbit = true;
                let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
                let initial = Camera { yaw: 0.0, pitch: 0.0, roll: 0.7, ..Default::default() };
                h.app.cam = initial;
                let mut mouse = initial;
                let seconds = std::f64::consts::TAU / 512.0;
                for _ in 0..256 {
                    let before = h.app.cam;
                    h.app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation }, seconds });
                    device(&mut h.app, rect, caps);
                    assert!((before.back().dot(h.app.cam.back()) - (seconds * 2.0).cos()).abs() < 1e-9);
                    let device_cam = h.app.cam;
                    h.app.cam = mouse;
                    // Compare continuous rotation; Shift snapping is exercised separately.
                    orbit_drag(&mut h.app, rect, rotation[1] * seconds * 250.0, rotation[0] * seconds * 250.0, None, false, false);
                    mouse = h.app.cam;
                    assert!(mouse.back().dist(device_cam.back()) < 1e-9 && mouse.basis().0.dist(device_cam.basis().0) < 1e-9);
                    h.app.cam = device_cam;
                }
                assert!(h.app.cam.back().dist(initial.back()) < 1e-8 && h.app.cam.basis().0.dist(initial.basis().0) < 1e-8);
            }
        }
        // Other presets keep the existing upright camera behavior.
        let mut cam = Camera { yaw: 0.0, pitch: 1.5, ..Default::default() };
        device_camera(&mut cam, Motion { translation: [0.0; 3], rotation: [1.0, 0.0, 0.0] }, 0.1, 600.0, true, false, None, None);
        assert!(cam.pitch < std::f64::consts::FRAC_PI_2 && cam.pitch > 1.5);
    }

    #[test]
    fn solidworks_free_orbit_keeps_the_configured_pivot_on_screen() {
        use super::*;
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "solidworks".into();
        h.app.preferences.free_orbit = true;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        for fov in [0.0, 0.6] {
            h.app.ui.perspective = fov > 0.0;
            for pivot in [Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0), Vec3::new(10.0, -20.0, -30.0)] {
                h.app.cam = Camera { target: Vec3::new(2.0, 3.0, 4.0), yaw: 0.0, pitch: 1.5, roll: 0.7, fov, ..Default::default() };
                let before = projection(&h.app, rect).to_screen(pivot).unwrap();
                orbit_about(&mut h.app, rect, 30.0, 50.0, pivot);
                let after = projection(&h.app, rect).to_screen(pivot).unwrap();
                assert!(before.distance(after) < 1e-3, "{fov} {pivot:?}: {before:?} -> {after:?}");
            }
        }
    }

    /// Every page with a camera navigates through [`super::handle`] (#1454). A page that reads
    /// the wheel or moves the view camera itself would drift from the user's navigation settings,
    /// so outside this module the camera may only be moved through it, the view commands or the
    /// navigators. The 3D mouse in both apps hands its motion to [`super::device`].
    #[test]
    fn hand_rolled_camera_input_is_rejected() {
        const PATTERNS: [&str; 8] = [
            "smooth_scroll_delta",
            "raw_scroll_delta",
            "zoom_delta()",
            "cam.orbit(",
            "cam.pan(",
            "cam.zoom_at(",
            "cam.roll_by(",
            "cam.rotate_in_view(",
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let dirs = [root.join("src"), root.join("../../apps/solidcraft/src"), root.join("../../apps/solidcraft-web/src")];
        let allowed = ["navigation.rs"];
        let mut offenders = Vec::new();
        let mut scanned = 0;
        let mut stack: Vec<std::path::PathBuf> = dirs.iter().filter(|d| d.is_dir()).cloned().collect();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !name.ends_with(".rs") || name.ends_with("_tests.rs") || allowed.contains(&name.as_str()) {
                    continue;
                }
                scanned += 1;
                // A Windows checkout's CRLF lines, read as written, so the split below finds the tests.
                let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
                // In-file unit tests set cameras up directly; only the code above them counts.
                let code = text.split("\n#[cfg(test)]\nmod ").next().unwrap_or_default();
                for (n, line) in code.lines().enumerate() {
                    if !line.trim_start().starts_with("//") && PATTERNS.iter().any(|p| line.contains(p)) {
                        offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(scanned > 50, "the source tree was scanned ({scanned} files)");
        assert!(offenders.is_empty(), "camera input outside navigation::handle; call it instead:\n{}", offenders.join("\n"));
    }

    #[test]
    fn wheel_zoom_speed_scales_each_step_on_every_canvas() {
        use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, pos2, vec2};
        use serde_json::json;

        for caps in [super::NavCaps::MODEL, super::NavCaps::RENDER, super::NavCaps { orbit: false, pick: false }] {
            for speed in [None, Some(0.25), Some(1.0), Some(4.0)] {
                for reverse in [false, true] {
                    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
                    crate::prefs::set(&mut app, &json!({"smooth_wheel_zoom":false, "zoom_to_cursor":false, "zoom_reverse":reverse})).unwrap();
                    if let Some(speed) = speed {
                        crate::prefs::set(&mut app, &json!({"wheel_zoom_speed":speed})).unwrap();
                    }
                    let ctx = crate::fonts::test_context();
                    let mut frame = |events| {
                        let raw = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                            focused: true,
                            events,
                            ..Default::default()
                        };
                        let mut out = ctx.run_ui(raw, |ui| {
                            let rect = ui.max_rect();
                            let resp = ui.interact(rect, ui.id().with("wheel-speed"), egui::Sense::click_and_drag());
                            super::handle(&mut app, ui, rect, &resp, caps);
                        });
                        out.textures_delta.clear();
                    };
                    frame(vec![Event::PointerMoved(pos2(200.0, 300.0))]);
                    frame(vec![Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta: vec2(0.0, 40.0),
                        modifiers: Modifiers::NONE,
                        phase: TouchPhase::Move,
                    }]);
                    let expected = (-40.0_f64 * 0.0015 * speed.unwrap_or(1.0) * if reverse { -1.0 } else { 1.0 }).exp();
                    let default_distance = super::Camera::default().distance;
                    assert!((app.cam.distance / default_distance - expected).abs() < 1e-6, "{caps:?}, {speed:?}, reverse={reverse}");
                }
            }
        }
    }

    #[test]
    fn wheel_zoom_speed_persists_defaults_and_rejects_invalid_values() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let old = crate::prefs::lenient(&json!({"zoom_reverse":true}));
        assert_eq!(serde_json::to_value(old).unwrap()["wheel_zoom_speed"], 1.0);
        crate::prefs::control(&mut app, &json!({"open":"Navigation", "wheel_zoom_speed":2.5, "button":"apply"})).unwrap();
        let saved = serde_json::to_value(&app.preferences).unwrap();
        assert_eq!(serde_json::to_value(crate::prefs::lenient(&saved)).unwrap()["wheel_zoom_speed"], 2.5);
        for invalid in [0.0, 0.24, 4.01, 1e30] {
            assert!(crate::prefs::set(&mut app, &json!({"wheel_zoom_speed":invalid})).is_err());
            let loaded = crate::prefs::lenient(&json!({"wheel_zoom_speed":invalid, "zoom_reverse":true}));
            assert_eq!(serde_json::to_value(&loaded).unwrap()["wheel_zoom_speed"], 1.0);
            assert!(loaded.zoom_reverse);
        }
        crate::prefs::control(&mut app, &json!({"button":"defaults"})).unwrap();
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["wheel_zoom_speed"], 1.0);
        crate::prefs::control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["wheel_zoom_speed"], 2.5);
    }

    #[test]
    fn wheel_zoom_speed_does_not_change_3d_mouse_zoom() {
        use crate::space_mouse::{DeviceMotion, Motion};
        let mut distances = Vec::new();
        for speed in [0.25, 1.0, 4.0] {
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            crate::prefs::set(&mut app, &serde_json::json!({"wheel_zoom_speed":speed})).unwrap();
            let before = app.cam.distance;
            app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0, 1.0, 0.0], rotation: [0.0; 3] }, seconds: 0.05 });
            super::device(&mut app, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)), super::NavCaps::MODEL);
            assert!(app.cam.distance < before);
            distances.push(app.cam.distance);
        }
        assert!(distances.windows(2).all(|pair| pair[0] == pair[1]));
    }

    /// Continuous orbit turns about what Orbit around names (#1044): around the cursor, the model
    /// point in the middle of the view stays still on screen for the whole spin, even when the
    /// view's centre has been left far behind the model; around the view's centre, as before.
    #[test]
    fn continuous_orbit_turns_about_the_orbit_around_point() {
        use crate::prefs::OrbitAround;
        for fov in [0.0, 0.6] {
            for around in [OrbitAround::Cursor, OrbitAround::Origin, OrbitAround::ViewCentre] {
                let mut s = solidcraft_engine::Session::default();
                s.execute("solid.box", &serde_json::json!({"length":40,"width":30,"height":20})).unwrap();
                let mut app = crate::SolveApp::new(s, crate::Services::default());
                app.preferences.orbit_around = around;
                app.cam.fov = fov;
                let ctx = crate::fonts::test_context();
                let mut time = 0.0;
                let mut frame = |app: &mut crate::SolveApp| {
                    time += 1.0 / 60.0;
                    let raw = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                        time: Some(time),
                        focused: true,
                        ..Default::default()
                    };
                    let mut out = ctx.run_ui(raw, |ui| {
                        let rect = ui.max_rect();
                        app.viewport.rect = Some(rect);
                        let resp = ui.interact(rect, ui.id().with("spin"), egui::Sense::click_and_drag());
                        super::handle(app, ui, rect, &resp, super::NavCaps::MODEL);
                    });
                    out.textures_delta.clear();
                };
                frame(&mut app);
                app.fit_view();
                // Earlier cursor orbits leave the view's centre far behind the model, which still
                // shows in the middle of the view.
                let back = app.cam.basis().2;
                app.cam.target -= back * 300.0;
                app.cam.distance += 300.0;
                let rect = app.viewport.rect.unwrap();
                let held = match around {
                    OrbitAround::Cursor => crate::prefs::pivot_under(&app, &super::projection(&app, rect), rect.center()),
                    OrbitAround::Origin => super::Vec3::ZERO,
                    OrbitAround::ViewCentre => app.cam.target,
                };
                let start = super::projection(&app, rect).to_screen(held).unwrap();
                let (yaw, target) = (app.cam.yaw, app.cam.target);
                app.viewport.spin = true;
                for i in 0..90 {
                    frame(&mut app);
                    assert!(app.viewport.spin, "{around:?}: nothing stopped the spin");
                    let now = super::projection(&app, rect).to_screen(held).unwrap();
                    assert!(now.distance(start) < 0.5, "fov {fov} {around:?}, frame {i}: the held point moved {start:?} -> {now:?}");
                }
                assert!((app.cam.yaw - yaw).abs() > 0.1, "{around:?}: the view turned");
                match around {
                    OrbitAround::Cursor => {
                        assert_eq!(app.viewport.spin_pivot, Some(held), "the point is chosen once");
                        let middle = crate::prefs::pivot_under(&app, &super::projection(&app, rect), rect.center());
                        assert!(middle.dist(held) < 30.0, "fov {fov}: the model is still in the middle: {middle:?} vs {held:?}");
                    }
                    OrbitAround::ViewCentre => assert!((app.cam.target - target).len() < 1e-9, "the view's centre stays as before"),
                    OrbitAround::Origin => assert!(app.viewport.spin_pivot.is_none()),
                }
                app.viewport.spin = false;
                frame(&mut app);
                assert!(app.viewport.spin_pivot.is_none(), "a stopped spin forgets its point");
            }
        }
    }

    /// A flat page (orbit off) pans and zooms with the same bindings and never turns the view.
    #[test]
    fn flat_pages_pan_and_zoom_with_the_same_bindings_but_never_orbit() {
        use egui::{Event, Modifiers, PointerButton, pos2, vec2};
        let flat = super::NavCaps { orbit: false, pick: false };
        for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
            let mut s = solidcraft_engine::Session::default();
            s.execute("solid.box", &serde_json::json!({"length":40,"width":30,"height":20})).unwrap();
            let mut app = crate::SolveApp::new(s, crate::Services::default());
            app.preferences.nav = preset.into();
            app.viewport.spin = true;
            let ctx = crate::fonts::test_context();
            let mut time = 0.0;
            let mut frame = |app: &mut crate::SolveApp, mods: Modifiers, events: Vec<Event>| {
                time += 1.0 / 60.0;
                let events = std::iter::once(Event::ModifiersChanged(mods)).chain(events).collect();
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                    time: Some(time),
                    focused: true,
                    events,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    let rect = ui.max_rect();
                    app.viewport.rect = Some(rect);
                    let resp = ui.interact(rect, ui.id().with("flat"), egui::Sense::click_and_drag());
                    super::handle(app, ui, rect, &resp, flat);
                });
                out.textures_delta.clear();
            };
            frame(&mut app, Modifiers::NONE, vec![]);
            assert!(!app.viewport.spin, "{preset}: continuous orbit stops on a flat page");
            let mut panned = false;
            let at = pos2(200.0, 300.0);
            for button in [PointerButton::Primary, PointerButton::Middle, PointerButton::Secondary] {
                for mods in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT, Modifiers::SHIFT | Modifiers::ALT] {
                    let before = app.cam;
                    frame(&mut app, mods, vec![Event::PointerMoved(at)]);
                    frame(&mut app, mods, vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: mods }]);
                    for k in 1..5 {
                        frame(&mut app, mods, vec![Event::PointerMoved(at + vec2(12.0, 7.0) * k as f32)]);
                    }
                    frame(&mut app, mods, vec![Event::PointerButton { pos: at + vec2(48.0, 28.0), button, pressed: false, modifiers: mods }]);
                    frame(&mut app, Modifiers::NONE, vec![]);
                    assert_eq!((app.cam.yaw, app.cam.pitch), (before.yaw, before.pitch), "{preset}: {button:?} with {mods:?} turned a flat page");
                    panned |= app.cam.target != before.target;
                }
            }
            assert!(panned, "{preset}: some binding pans");
            frame(&mut app, Modifiers::NONE, vec![Event::PointerMoved(at)]);
            for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
                let before = app.cam;
                for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
                    frame(
                        &mut app,
                        modifiers,
                        vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(20.0, -10.0), modifiers, phase }],
                    );
                }
                assert_eq!(app.cam.back(), before.back(), "{preset}: trackpad turned a flat page");
                if preset == "fusion" && modifiers.shift {
                    assert_eq!(app.cam, before, "orbit gestures must do nothing on a flat page");
                }
            }
            let before = app.cam;
            let wheel = Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: vec2(0.0, 3.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            };
            frame(&mut app, Modifiers::NONE, vec![Event::PointerMoved(at), wheel]);
            for _ in 0..20 {
                frame(&mut app, Modifiers::NONE, vec![]);
            }
            assert_ne!(app.cam.distance, before.distance, "{preset}: the wheel zooms");
        }
    }

    /// A zoom-modified wheel must reach the camera even though the ordinary wheel path skips it.
    #[test]
    fn native_ctrl_wheel_pinch_zooms_the_view() {
        use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, pos2, vec2};
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &serde_json::json!({"length":40,"width":30,"height":20})).unwrap();
        let mut app = crate::SolveApp::new(s, crate::Services::default());
        let ctx = crate::fonts::test_context();
        let mut time = 0.0;
        let at = pos2(200.0, 300.0);
        let mut frame = |app: &mut crate::SolveApp, events: Vec<Event>| {
            time += 1.0 / 60.0;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(time),
                focused: true,
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let rect = ui.max_rect();
                app.viewport.rect = Some(rect);
                let resp = ui.interact(rect, ui.id().with("native-pinch"), egui::Sense::click_and_drag());
                super::handle(app, ui, rect, &resp, super::NavCaps::MODEL);
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![Event::PointerMoved(at)]);
        let before = app.cam;
        frame(
            &mut app,
            vec![
                Event::ModifiersChanged(Modifiers::CTRL),
                Event::PointerMoved(at),
                Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, 50.0), modifiers: Modifiers::CTRL, phase: TouchPhase::Move },
            ],
        );
        assert!(app.cam.distance < before.distance, "a positive Ctrl+wheel pinch zooms in: {} -> {}", before.distance, app.cam.distance);
        // Pinch uses egui's zoom speed; the unmodified wheel still uses the wheel path.
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &serde_json::json!({"length":40,"width":30,"height":20})).unwrap();
        let mut plain = crate::SolveApp::new(s, crate::Services::default());
        frame(&mut plain, vec![Event::PointerMoved(at)]);
        let plain_before = plain.cam;
        frame(
            &mut plain,
            vec![
                Event::PointerMoved(at),
                Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, 50.0), modifiers: Modifiers::NONE, phase: TouchPhase::Move },
            ],
        );
        let pinched = before.distance / app.cam.distance;
        let expected = (0.005f64 * 50.0).exp();
        assert!((pinched / expected - 1.0).abs() < 0.02, "the pinch factor is exp(scroll_zoom_speed * delta) = {expected:.3}x, got {pinched:.3}x");
        let wheeled = plain_before.distance / plain.cam.distance;
        assert!(wheeled > 1.0, "the bare wheel still zooms in on its own path: {wheeled:.3}x");
    }

    #[test]
    fn ctrl_wheel_replay_zooms_once_in_the_viewport() {
        use serde_json::json;
        for preset in ["fusion", "touchpad"] {
            for smooth in [false, true] {
                let mut h = crate::scenario::Harness::new();
                h.app.preferences.nav = preset.into();
                h.app.preferences.smooth_wheel_zoom = smooth;
                h.app.preferences.zoom_to_cursor = false;
                h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
                h.step(&json!({"call":"ui.camera", "params":{"target":[20,15,10],"distance":100}})).unwrap();
                let at = h.app.viewport.rect.unwrap().center();
                let before = h.app.cam;
                if preset == "fusion" && !smooth {
                    h.step(&json!({"shot":"initial"})).unwrap();
                }
                h.step(&json!({"call":"ui.scroll", "params":{"x":at.x,"y":at.y,"delta":80,"ctrl":true}})).unwrap();
                if preset == "fusion" && !smooth {
                    h.step(&json!({"shot":"pinch-result"})).unwrap();
                }
                let after = h.app.cam;
                let expected = before.distance * (-0.4f64).exp();
                assert!(
                    (after.distance - expected).abs() < 1e-5,
                    "{preset}, smooth={smooth}: {} -> {}, expected {expected}",
                    before.distance,
                    after.distance
                );
                assert_eq!(after.target, before.target, "a centred pinch must not pan");
                assert_eq!(after.back(), before.back(), "a pinch must not orbit");
                h.frames(90);
                assert_eq!(h.app.cam, after, "a pinch must not replay egui's smoothing tail");
                h.step(&json!({"call":"ui.scroll", "params":{"x":at.x,"y":at.y,"delta":-80,"ctrl":true}})).unwrap();
                assert!((h.app.cam.distance - before.distance).abs() < 1e-5, "the opposite pinch restores the scale");
                let restored = h.app.cam;
                h.step(&json!({"call":"ui.scroll", "params":{"x":-20,"y":-20,"delta":80,"ctrl":true}})).unwrap();
                assert_eq!(h.app.cam, restored, "a pinch outside the viewport must not zoom it");
            }
        }
    }

    #[test]
    fn pinch_factor_ignores_wheel_phase_markers() {
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::End, egui::TouchPhase::Cancel] {
            let mut input = egui::InputState::default();
            input.events = vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 40.0),
                modifiers: egui::Modifiers::CTRL,
                phase,
            }];
            assert_eq!(super::pinch_factor(&input, egui::InputOptions::default(), 600.0, None), 1.0, "{phase:?} is a gesture boundary, not movement");
        }
    }

    /// The pinch factor reads native Ctrl+wheel pinches in every wheel unit, scaled the way egui
    /// scales them, and leaves wheels without the zoom modifier to [`immediate_wheel`].
    #[test]
    fn pinch_factor_reads_native_ctrl_wheel_in_every_unit() {
        use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, vec2};
        let options = egui::InputOptions::default();
        let factor = |events: Vec<Event>| {
            let mut input = egui::InputState::default();
            input.events = events;
            super::pinch_factor(&input, options, 600.0, None)
        };
        let wheel = |unit, delta, modifiers| Event::MouseWheel { unit, delta, modifiers, phase: TouchPhase::Move };
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let point = factor(vec![wheel(MouseWheelUnit::Point, vec2(0.0, 40.0), Modifiers::CTRL)]);
        assert!(close(point, (options.scroll_zoom_speed * 40.0).exp()), "point pinch {point}");
        let line = factor(vec![wheel(MouseWheelUnit::Line, vec2(0.0, 1.0), Modifiers::CTRL)]);
        assert!(close(line, (options.scroll_zoom_speed * options.line_scroll_speed).exp()), "line pinch {line}");
        let page = factor(vec![wheel(MouseWheelUnit::Page, vec2(0.0, 0.5), Modifiers::CTRL)]);
        assert!(close(page, (options.scroll_zoom_speed * 300.0).exp()), "page pinch {page}");
        assert_eq!(factor(vec![wheel(MouseWheelUnit::Point, vec2(0.0, 40.0), Modifiers::NONE)]), 1.0, "a bare wheel belongs to the scroll path");
        assert_eq!(factor(vec![Event::Zoom(1.25)]), 1.25);
        let both = factor(vec![Event::Zoom(1.25), wheel(MouseWheelUnit::Point, vec2(0.0, 40.0), Modifiers::CTRL)]);
        assert!(close(both, 1.25 * (options.scroll_zoom_speed * 40.0).exp()), "zoom events and pinch wheel combine {both}");
    }

    /// The 3D mouse pans and zooms a flat page with the cap and never turns it, as the mouse
    /// does there; pages that orbit turn with it (#1336).
    #[test]
    fn the_3d_mouse_pans_and_zooms_a_flat_page_but_never_turns_it() {
        use crate::space_mouse::{DeviceMotion, Motion};
        let every_axis = DeviceMotion { motion: Motion { translation: [1.0, 1.0, 1.0], rotation: [1.0, 1.0, 1.0] }, seconds: 0.05 };
        let flat = super::NavCaps { orbit: false, pick: false };
        for (caps, turns) in [(flat, false), (super::NavCaps::MODEL, true), (super::NavCaps::RENDER, true)] {
            let mut s = solidcraft_engine::Session::default();
            s.execute("solid.box", &serde_json::json!({"length":40,"width":30,"height":20})).unwrap();
            let mut app = crate::SolveApp::new(s, crate::Services::default());
            let ctx = crate::fonts::test_context();
            let before = app.cam;
            app.viewport.device_motion = Some(every_axis);
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                time: Some(0.0),
                focused: true,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let rect = ui.max_rect();
                app.viewport.rect = Some(rect);
                let resp = ui.interact(rect, ui.id().with("page"), egui::Sense::click_and_drag());
                super::handle(&mut app, ui, rect, &resp, caps);
            });
            out.textures_delta.clear();
            assert!(app.viewport.device_motion.is_none(), "{caps:?}: the page takes the motion");
            assert_ne!(app.cam.target, before.target, "{caps:?}: the cap pans");
            assert!(app.cam.distance < before.distance, "{caps:?}: the cap zooms");
            let turned = (app.cam.yaw, app.cam.pitch, app.cam.roll) != (before.yaw, before.pitch, before.roll);
            assert_eq!(turned, turns, "{caps:?}: the cap turns the view only where the page orbits");
        }
    }

    /// A device app's orbit turns pages that orbit and pans the others (a flat page, a sketch seen
    /// square-on), so the device still moves the view there without turning it.
    #[test]
    fn a_device_apps_orbit_pans_where_the_view_may_not_turn() {
        use crate::space_mouse::BridgeMotion;
        let orbit = BridgeMotion { orbit: [30.0, 20.0], ..Default::default() };
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let flat = super::NavCaps { orbit: false, pick: false };
        let mut sketching = solidcraft_engine::Session::default();
        sketching.execute("sketch.create", &serde_json::json!({"plane": "XY"})).unwrap();
        for (session, caps, turns) in [
            (solidcraft_engine::Session::default(), super::NavCaps::MODEL, true),
            (solidcraft_engine::Session::default(), flat, false),
            (sketching, super::NavCaps::MODEL, false),
        ] {
            let mut app = crate::SolveApp::new(session, crate::Services::default());
            if app.session.active_sketch.is_some() {
                // Square to the sketch, as a sketch opens.
                app.cam = solidcraft_engine::render::Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..Default::default() };
            }
            let before = app.cam;
            app.viewport.bridge_motion = Some(orbit);
            super::device(&mut app, page, caps);
            assert!(app.viewport.bridge_motion.is_none(), "{caps:?}: the page takes the motion");
            let turned = (app.cam.yaw, app.cam.pitch, app.cam.roll) != (before.yaw, before.pitch, before.roll);
            assert_eq!(turned, turns, "{caps:?}");
            assert_eq!(app.cam.target != before.target, !turns, "{caps:?}: pans where it doesn't turn");
        }
    }

    /// Surface anchoring for the cap, bridge and drag-zoom is opt-in (#7435), independent of
    /// the wheel preference. Off keeps the old target fixed and lets the eye enter the model.
    #[test]
    fn the_3d_mouse_bridge_and_drag_zoom_hold_the_view_at_the_model() {
        use super::*;
        use crate::space_mouse::{BridgeMotion, DeviceMotion, Motion};
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let outside = |cam: &Camera| {
            let e = cam.eye();
            e.x.abs() > 20.0 - 1e-6 || e.y.abs() > 20.0 - 1e-6 || e.z.abs() > 20.0 - 1e-6
        };
        for (stop_at_model, to_cursor) in [(false, false), (false, true), (true, false), (true, true)] {
            for cap in [true, false] {
                let mut h = crate::scenario::Harness::new();
                h.app.session.execute("solid.box", &json!({"length": 40, "width": 40, "height": 40, "corner": [-20, -20, -20]})).unwrap();
                h.app.preferences.zoom_to_cursor = to_cursor;
                h.app.preferences.zoom_stop_at_model = stop_at_model;
                h.settle();
                // The iso view looks straight at the box, as the report's sample does.
                h.app.cam = Camera::default();
                for _ in 0..60 {
                    if cap {
                        h.app.viewport.device_motion =
                            Some(DeviceMotion { motion: Motion { translation: [0.0, 1.0, 0.0], rotation: [0.0; 3] }, seconds: 0.1 });
                    } else {
                        h.app.viewport.bridge_motion = Some(BridgeMotion { zoom: 1.0, ..Default::default() });
                    }
                    device(&mut h.app, rect, NavCaps::MODEL);
                }
                assert_eq!(outside(&h.app.cam), stop_at_model, "stop={stop_at_model} cursor={to_cursor} cap={cap}: eye {:?}", h.app.cam.eye());
                if !stop_at_model {
                    assert_eq!(h.app.cam.target, Camera::default().target, "legacy zoom keeps the target fixed");
                }
                if !cap && to_cursor {
                    h.step(&json!({"shot": if stop_at_model { "model-zoom-enabled" } else { "model-zoom-default" }})).unwrap();
                }
            }
        }
        // Drag-zoom: the "Zoom" navigation tool holds the model point where the drag started, so
        // that point stays under the press while the pointer moves away from it.
        for (stop_at_model, to_cursor) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut h = crate::scenario::Harness::new();
            h.app.session.execute("solid.box", &json!({"length": 40, "width": 40, "height": 40, "corner": [-20, -20, -20]})).unwrap();
            h.app.preferences.zoom_to_cursor = to_cursor;
            h.app.preferences.zoom_stop_at_model = stop_at_model;
            h.app.run("view.tool", json!({"tool": "zoom"})).unwrap();
            h.settle();
            h.app.cam = Camera::default();
            let rect = h.app.viewport.rect.expect("the viewport");
            let at = projection(&h.app, rect).to_screen(Vec3::new(10.0, 0.0, 10.0)).expect("the box on screen");
            let held = model_zoom_anchor(&mut h.app, rect, Some(at), true);
            assert_eq!(held.is_some(), stop_at_model);
            assert_eq!(zoom_anchor(&mut h.app, rect, Some(at), true).is_some(), to_cursor, "wheel anchoring stays independent");
            for _ in 0..12 {
                h.call("ui.drag", json!({"x0": at.x, "y0": at.y, "x1": at.x, "y1": at.y - 160.0, "steps": 40}));
            }
            assert_eq!(outside(&h.app.cam), stop_at_model, "drag stop={stop_at_model} cursor={to_cursor}: eye {:?}", h.app.cam.eye());
            if !stop_at_model {
                assert_eq!(h.app.cam.target, Camera::default().target, "legacy drag-zoom keeps the target fixed");
            }
            if let Some(held) = held {
                let now = projection(&h.app, rect).to_screen(held).expect("the held point on screen");
                assert!((now - at).length() < 1.0, "the pressed point moved from {at:?} to {now:?}");
            }
        }
    }

    /// The 3D mouse turns the view around the model in the middle of the view, so that point stays
    /// put on screen, when Traced orbit is enabled. With Classic (the default), it
    /// turns around the view's centre and the point swings away.
    #[test]
    fn the_3d_mouse_turns_around_the_model_in_the_middle_of_the_view() {
        use super::*;
        use crate::space_mouse::{BridgeMotion, DeviceMotion, Motion};
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        for free in [false, true] {
            for (model_pivot, cap) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut h = crate::scenario::Harness::new();
                h.app.session.execute("solid.box", &json!({"length": 40, "width": 40, "height": 40, "corner": [-20, -20, -20]})).unwrap();
                h.app.preferences.space_mouse_model_pivot = model_pivot;
                h.app.preferences.free_orbit = free;
                h.settle();
                // Looking at the box from far away, so the view's centre is well behind the face.
                h.app.cam = Camera { distance: 400.0, ..Camera::default() };
                let held = crate::prefs::pivot_under(&h.app, &projection(&h.app, rect), rect.center());
                for _ in 0..10 {
                    if cap {
                        h.app.viewport.device_motion =
                            Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.5, 0.5, 0.0] }, seconds: 0.05 });
                    } else {
                        h.app.viewport.bridge_motion = Some(BridgeMotion { orbit: [6.25, 6.25], ..Default::default() });
                    }
                    device(&mut h.app, rect, NavCaps::MODEL);
                }
                let now = projection(&h.app, rect).to_screen(held).expect("the held point on screen");
                let still = (now - rect.center()).length() < 1.0;
                assert_eq!(still, model_pivot, "free={free} model_pivot={model_pivot}: the middle point moved to {now:?}");
                if !cap {
                    assert!(h.app.viewport.device_pivot.is_none(), "the bridge picks its point each frame");
                    continue;
                }
                // Panning while it turns brings other parts of the model to the middle; the turn
                // keeps the point it started with until the cap is back at rest.
                for _ in 0..10 {
                    h.app.viewport.device_motion =
                        Some(DeviceMotion { motion: Motion { translation: [0.5, 0.0, 0.0], rotation: [0.5, 0.5, 0.0] }, seconds: 0.05 });
                    device(&mut h.app, rect, NavCaps::MODEL);
                }
                assert_eq!(h.app.viewport.device_pivot, model_pivot.then_some(held), "free={free}: the turn kept its point");
            }
        }
    }

    /// While a sketch is edited the cap pans and zooms without turning a square-on view; after the
    /// view is tilted away from the sketch, the cap can turn it again.
    #[test]
    fn the_3d_mouse_turns_a_sketch_only_after_the_view_is_tilted() {
        use crate::space_mouse::{DeviceMotion, Motion};
        use solidcraft_engine::render::Camera;
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &serde_json::json!({"plane": "XY"})).unwrap();
        let mut app = crate::SolveApp::new(s, crate::Services::default());
        assert!(app.session.active_sketch.is_some());
        app.cam = Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..Camera::default() };
        let before = app.cam;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [1.0; 3], rotation: [1.0; 3] }, seconds: 0.05 });
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_eq!((app.cam.yaw, app.cam.pitch, app.cam.roll), (before.yaw, before.pitch, before.roll));
        assert_ne!(app.cam.target, before.target);
        assert!(app.cam.distance < before.distance);

        app.cam.roll = 0.1;
        let tilted = app.cam;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.0, 0.0, 0.1] }, seconds: 0.05 });
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_ne!((app.cam.yaw, app.cam.pitch, app.cam.roll), (tilted.yaw, tilted.pitch, tilted.roll));

        app.cam.pitch = 1.2;
        let tilted = app.cam;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.0, 0.1, 0.0] }, seconds: 0.05 });
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_ne!((app.cam.yaw, app.cam.pitch, app.cam.roll), (tilted.yaw, tilted.pitch, tilted.roll));

        // Nothing queued, nothing moves.
        let still = app.cam;
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_eq!(app.cam, still);
    }

    /// With "3D mouse turns sketches" on (#2227), the cap and a device app turn a sketch seen square
    /// on, as the mouse does; pages that may not orbit still only pan.
    #[test]
    fn the_3d_mouse_turns_a_square_on_sketch_when_the_preference_allows_it() {
        use crate::space_mouse::{BridgeMotion, DeviceMotion, Motion};
        use solidcraft_engine::render::Camera;
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &serde_json::json!({"plane": "XY"})).unwrap();
        let mut app = crate::SolveApp::new(s, crate::Services::default());
        assert!(!app.preferences.space_mouse_sketch_orbit, "off by default");
        app.preferences.space_mouse_sketch_orbit = true;
        let square_on = Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..Camera::default() };
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let turn = |c: &Camera| (c.yaw, c.pitch, c.roll);

        app.cam = square_on;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.3, 0.3, 0.0] }, seconds: 0.05 });
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_ne!(turn(&app.cam), turn(&square_on), "the cap turns the square-on sketch");

        app.cam = square_on;
        app.viewport.bridge_motion = Some(BridgeMotion { orbit: [6.0, 6.0], ..Default::default() });
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_ne!(turn(&app.cam), turn(&square_on), "a device app turns it too");

        // A page that may not orbit keeps its orientation whatever the preference.
        app.cam = square_on;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.3, 0.3, 0.3] }, seconds: 0.05 });
        super::device(&mut app, page, super::NavCaps { orbit: false, ..super::NavCaps::MODEL });
        assert_eq!(turn(&app.cam), turn(&square_on));

        // Off again: the square-on sketch keeps its orientation, as before.
        app.preferences.space_mouse_sketch_orbit = false;
        app.cam = square_on;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.3, 0.3, 0.3] }, seconds: 0.05 });
        super::device(&mut app, page, super::NavCaps::MODEL);
        assert_eq!(turn(&app.cam), turn(&square_on));
    }
}

#[cfg(test)]
mod view_key_tests {
    use crate::scenario::Harness;
    use serde_json::json;

    fn turned(h: &mut Harness, key: serde_json::Value) -> (f64, f64) {
        let before = h.app.cam;
        h.step(&key).unwrap();
        h.frames(2);
        (h.app.cam.yaw - before.yaw, h.app.cam.pitch - before.pitch)
    }

    /// #1541: the arrows turn the view 15°, Shift+arrows a quarter turn, Ctrl+arrows pan, and none
    /// of them is a shortcut a command can take.
    #[test]
    fn arrow_keys_turn_and_pan_the_view() {
        let mut h = Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "front"}})).unwrap();
        h.frames(3);
        let close = |a: f64, b: f64| (a.abs() - b.to_radians()).abs() < 1e-6;
        let (yaw, pitch) = turned(&mut h, json!({"key": "ArrowLeft"}));
        assert!(close(yaw, 15.0) && close(pitch, 0.0), "Left turns 15°: {yaw} {pitch}");
        let (yaw, pitch) = turned(&mut h, json!({"key": "ArrowUp"}));
        assert!(close(yaw, 0.0) && close(pitch, 15.0), "Up turns 15° over: {yaw} {pitch}");
        let (yaw, _) = turned(&mut h, json!({"key": "ArrowRight", "shift": true}));
        assert!(close(yaw, 90.0), "Shift+Right is a quarter turn: {yaw}");
        let (before, angles) = (h.app.cam.target, (h.app.cam.yaw, h.app.cam.pitch));
        h.step(&json!({"key": "ArrowRight", "cmd": true})).unwrap();
        h.frames(2);
        assert_ne!(h.app.cam.target, before, "Ctrl+Right pans");
        assert_eq!((h.app.cam.yaw, h.app.cam.pitch), angles, "and does not turn");
        for k in [egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown] {
            for mods in [egui::Modifiers::NONE, egui::Modifiers::SHIFT, egui::Modifiers::COMMAND] {
                let name = crate::keymap::key_name(k, mods);
                assert!(crate::keymap::RESERVED.contains(&name.as_str()), "{name} must be reserved");
            }
        }
        assert!(crate::keymap::RESERVED.contains(&crate::keymap::key_name(egui::Key::Num8, egui::Modifiers::COMMAND).as_str()));
    }

    fn arrow(pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key { key: egui::Key::ArrowLeft, physical_key: None, pressed, repeat, modifiers: egui::Modifiers::NONE }
    }

    /// A press and release in one frame, without the harness settling the view after it.
    fn tap(h: &mut Harness) {
        h.app.synthetic.push(arrow(true, false));
        h.app.synthetic.push(arrow(false, false));
        h.frame();
    }

    /// The signed turn from `a` to `b`, past the wrap.
    fn turn(a: f64, b: f64) -> f64 {
        (b - a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    }

    fn front_view() -> Harness {
        let mut h = Harness::new();
        h.app.preferences.look_at_anim = true;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "front"}})).unwrap();
        h.settle();
        h
    }

    /// Holding an arrow turns the view steadily, by frame time, whatever the key repeat does:
    /// one animation at most, never a second one fighting the first.
    #[test]
    fn a_held_arrow_turns_the_view_steadily_and_ignores_key_repeat() {
        let hold = |repeats: bool| {
            let mut h = front_view();
            let mut last = h.app.cam.yaw;
            h.app.synthetic.push(arrow(true, false));
            h.frame();
            let log = h.app.session.log.len();
            let mut deltas = Vec::new();
            for frame in 0..60 {
                if repeats && frame % 3 == 2 {
                    h.app.synthetic.push(arrow(true, true));
                }
                h.frame();
                deltas.push(turn(last, h.app.cam.yaw));
                last = h.app.cam.yaw;
            }
            assert!(h.app.cam_anim.is_none(), "the held turn runs on key state, not on a tween");
            assert_eq!(h.app.session.log.len(), log, "turn frames do not flood the command log");
            assert!(deltas.iter().all(|d| *d > 0.0), "the view never stops or reverses while held: {deltas:?}");
            let steady = &deltas[20..];
            let per_frame = 90f64.to_radians() / 60.0;
            assert!(steady.iter().all(|d| (d - per_frame).abs() < 1e-3), "{steady:?}");
            let held = h.app.cam.yaw;
            h.app.synthetic.push(arrow(false, false));
            h.frames(5);
            assert_eq!(h.app.cam.yaw, held, "releasing stops the turn at once");
            h.app.cam.yaw
        };
        assert!((hold(false) - hold(true)).abs() < 1e-9, "OS key repeat changes nothing");
    }

    /// A tap is still one animated step of 15°, with one animation.
    #[test]
    fn a_tap_is_one_animated_step() {
        let mut h = front_view();
        let before = h.app.cam.yaw;
        tap(&mut h);
        h.frames(2);
        assert!(h.app.cam_anim.is_some(), "the step animates");
        h.settle();
        assert!(h.app.cam_anim.is_none());
        assert!((turn(before, h.app.cam.yaw).abs() - 15f64.to_radians()).abs() < 1e-6);
    }

    /// A tap while a step is in flight retargets that animation: the turns add up and only one
    /// animation exists, starting where the camera is.
    #[test]
    fn a_tap_during_a_step_retargets_the_one_animation() {
        let mut h = front_view();
        let before = h.app.cam.yaw;
        tap(&mut h);
        h.frames(5);
        let mid = h.app.cam;
        tap(&mut h);
        h.frames(1);
        let anim = h.app.cam_anim.expect("one animation");
        assert!((turn(before, anim.to.yaw).abs() - 30f64.to_radians()).abs() < 1e-6, "retargeted past the first step");
        assert!(turn(mid.yaw, anim.from.yaw).abs() < 15f64.to_radians(), "continues from the camera as it is");
        h.settle();
        assert!((turn(before, h.app.cam.yaw).abs() - 30f64.to_radians()).abs() < 1e-6);
    }

    /// Ctrl+1…7 run Front … Isometric View by default, turning the view as the view cube's faces
    /// do; the keys are ordinary shortcuts, so one can be moved to another command and back.
    #[test]
    fn ctrl_digits_turn_to_the_standard_views_and_can_be_rebound() {
        use solidcraft_engine::render::StandardView;
        let mut h = Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.frames(2);
        let looks_from = |h: &mut Harness, digit: &str, view: StandardView| {
            // Start from somewhere else, so the key has to move the view.
            h.step(&json!({"key": "ArrowLeft"})).unwrap();
            h.frames(2);
            h.step(&json!({"key": digit, "cmd": true})).unwrap();
            h.frames(2);
            let mut want = h.app.cam;
            want.set_view(view);
            (h.app.cam.back() - want.back()).len() < 1e-6
        };
        for (digit, view) in [
            ("1", StandardView::Front),
            ("2", StandardView::Back),
            ("3", StandardView::Left),
            ("4", StandardView::Right),
            ("5", StandardView::Top),
            ("6", StandardView::Bottom),
            ("7", StandardView::Iso),
        ] {
            assert!(looks_from(&mut h, digit, view), "Ctrl+{digit} turns to {view:?}: {:?}", h.app.cam.back());
        }
        // Top moves to Ctrl+T; Ctrl+5 then does nothing, Ctrl+T turns to the top.
        crate::keymap::bind(&mut h.app, "view.top", "Ctrl+T", false).unwrap();
        assert!(!looks_from(&mut h, "5", StandardView::Top), "Ctrl+5 no longer turns to the top");
        assert!(looks_from(&mut h, "T", StandardView::Top), "Ctrl+T does");
    }

    /// #1886: Ctrl+8 looks straight at the sketch; pressed again while the view faces it, it
    /// looks from the other side, and a third press turns it back. Opening a sketch never flips.
    #[test]
    fn ctrl_8_again_looks_from_the_other_side() {
        let mut h = Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.frames(2);
        let back_z = |h: &Harness| h.app.cam.back().z;
        for want in [1.0, -1.0, 1.0] {
            h.step(&json!({"key": "8", "cmd": true})).unwrap();
            h.frames(2);
            assert!((back_z(&h) - want).abs() < 1e-3, "looking from z {want}: back {:?}", h.app.cam.back());
        }
        // Programmatic look-at (as on opening a sketch) keeps the sketch's own side.
        crate::dialogs::look_at_sketch(&mut h.app);
        h.frames(2);
        crate::dialogs::look_at_sketch(&mut h.app);
        h.frames(2);
        assert!((back_z(&h) - 1.0).abs() < 1e-3, "{:?}", h.app.cam.back());
    }
}
