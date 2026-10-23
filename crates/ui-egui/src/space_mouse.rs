//! 3D mouse input, shared by the desktop and web apps. Each app reads the device its own way (the
//! desktop through HID, the web app through the browser's Gamepad API) and hands its six raw axes
//! to [`navigate_frame`] once per frame. This module maps them, applies the dead zone and drops
//! 3DxWare's emulated wheel; the page showing the camera moves it (`navigation::device`), so the
//! cap does what that page's camera may do.

use std::time::Duration;

use crate::SolveApp;

pub use solidcraft_space_mouse::{AxisCalibration, Calibration};

/// What a 3Dconnexion device reports at full deflection, in raw units, on every axis.
pub const FULL_SCALE: i16 = 350;

/// One reading of the six axes in the device's raw units: X right, Y towards the user, Z down,
/// and the rotations about those axes (right-handed).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axes {
    pub translation: [i16; 3],
    pub rotation: [i16; 3],
}

impl Axes {
    /// Some axis is past the dead zone: the cap is being moved.
    pub fn moving(self) -> bool {
        self.translation.iter().chain(&self.rotation).any(|&v| normalized(v) != 0.0)
    }
}

impl From<Axes> for [i16; 6] {
    fn from(axes: Axes) -> Self {
        let ([x, y, z], [rx, ry, rz]) = (axes.translation, axes.rotation);
        [x, y, z, rx, ry, rz]
    }
}

impl From<[i16; 6]> for Axes {
    fn from([x, y, z, rx, ry, rz]: [i16; 6]) -> Self {
        Self { translation: [x, y, z], rotation: [rx, ry, rz] }
    }
}

/// The 3D mouse as of this frame.
#[derive(Debug, Default)]
pub struct Input {
    /// The device being read: where from and its vendor and product ids (`hid 256f:c63a`,
    /// `web 256f:c63a`), or where from alone (`spacenavd`). Its calibration is saved under it.
    pub device: Option<String>,
    /// This frame's reading, before calibration.
    pub raw: Axes,
    /// The guided calibration while its window is open, and the device it is for.
    pub calibrating: Option<(String, solidcraft_space_mouse::Wizard)>,
    /// The driver mapping navigation used last frame, which says what each axis does to the view.
    pub mapping: Option<DriverMapping>,
    /// Which of the Linux readers exist, as the app's reader last saw them; none where there is
    /// only one way to read the device.
    pub drivers: Option<DriversPresent>,
}

/// What the Linux reader found: the spacenavd socket and a direct (hidraw) 3D mouse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriversPresent {
    pub spacenavd: bool,
    pub direct: bool,
}

/// Seconds after the cap last moved that wheel events are still taken for 3DxWare's: one can be
/// queued just before the cap stops and only reach the next frame.
pub const WHEEL_QUIET_S: f64 = 0.15;

/// 3DxWare turns the cap's motion into mouse-wheel turns for programs it has no profile for (its
/// keyboard and mouse emulation), browsers included (#1336, #1380). Both apps read the cap
/// themselves, so those are dropped: wheel events while the cap is moving and for
/// [`WHEEL_QUIET_S`] after. 3DxWare sends them only well past the dead zone (from about a third
/// of full deflection), never at rest, so a wheel turn with the cap at rest is the user's.
#[derive(Debug, Default)]
pub struct WheelGuard {
    moved_at: Option<f64>,
}

impl WheelGuard {
    /// Takes this frame's reading (`now` in seconds) and drops the frame's wheel events if they
    /// are 3DxWare's.
    pub fn filter(&mut self, moving: bool, now: f64, events: &mut Vec<egui::Event>) {
        if moving {
            self.moved_at = Some(now);
        }
        let recent = self.moved_at.is_some_and(|at| (0.0..=WHEEL_QUIET_S).contains(&(now - at)));
        if recent {
            events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. } | egui::Event::Zoom(_)));
        }
    }
}

/// Where one physical axis goes: its logical axis, and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisRule {
    pub output: usize,
    pub enabled: bool,
    pub reversed: bool,
    pub scale: f64,
}

/// The physical-to-logical axis mapping 3DxWare applies (the desktop reads it from the driver's
/// profile; direct reads bypass the driver).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriverMapping {
    pub axes: [AxisRule; 6],
    pub overall_scale: f64,
}

impl Default for DriverMapping {
    fn default() -> Self {
        Self { axes: std::array::from_fn(|output| AxisRule { output, enabled: true, reversed: false, scale: 1.0 }), overall_scale: 1.0 }
    }
}

impl DriverMapping {
    /// What 3DxWare applies by default in object mode (#1172): the physical Ry and Rz swapped and
    /// reversed, so a tilt left/right rolls and a twist spins.
    pub fn object_mode() -> Self {
        let mut mapping = Self::default();
        mapping.axes[4] = AxisRule { output: 5, enabled: true, reversed: true, scale: 1.0 };
        mapping.axes[5] = AxisRule { output: 4, enabled: true, reversed: true, scale: 1.0 };
        mapping
    }

    /// The mapping with the user's per-axis choices folded in: `reverse` flips and `speed` scales
    /// each physical axis of the cap. A speed that is not a finite number counts as 1.
    pub fn with_user_axes(mut self, reverse: [bool; 6], speed: [f32; 6]) -> Self {
        for ((rule, reverse), speed) in self.axes.iter_mut().zip(reverse).zip(speed) {
            rule.reversed ^= reverse;
            rule.scale *= if speed.is_finite() { f64::from(speed).clamp(0.0, 3.0) } else { 1.0 };
        }
        self
    }

    pub fn apply(self, axes: Axes) -> Motion {
        let raw = [
            normalized(axes.translation[0]),
            normalized(axes.translation[1]),
            normalized(axes.translation[2]),
            normalized(axes.rotation[0]),
            normalized(axes.rotation[1]),
            normalized(axes.rotation[2]),
        ];
        let mut logical = [0.0; 6];
        for (input, rule) in self.axes.iter().enumerate() {
            if rule.enabled {
                let sign = if rule.reversed { -1.0 } else { 1.0 };
                if let Some(slot) = logical.get_mut(rule.output) {
                    *slot = raw[input] * sign * rule.scale * self.overall_scale;
                }
            }
        }
        Motion { translation: [logical[0], logical[1], logical[2]], rotation: [logical[3], logical[4], logical[5]] }
    }
}

/// The logical axes after the mapping, each in -1..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
}

/// One frame of 3D mouse motion, waiting for the page that shows the camera: the logical axes,
/// and how many seconds of motion they stand for (the frame's time, scaled by the speed).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeviceMotion {
    pub motion: Motion,
    pub seconds: f64,
}

/// The saved calibration of the device being read, when it has one.
pub fn calibration(app: &SolveApp) -> Option<Calibration> {
    app.space_mouse.device.as_ref().and_then(|device| app.preferences.space_mouse_calibrations.get(device)).copied()
}

/// A reading on 3Dconnexion's axes: through `calibration` when there is one. Whether the cap is
/// moving is asked of this, never of the raw reading, so a device resting off zero isn't "moving"
/// and a short-range one moves as soon as it does.
pub fn calibrated(axes: Axes, calibration: Option<Calibration>) -> Axes {
    calibration.map_or(axes, |calibration| calibration.apply(axes.into()).into())
}

/// The device's six calibrated motions, in the order of a reading: what each axis can be driven
/// by. They name no kind of device, since calibration defined them: Translation X is whatever the
/// user did for "slide right".
pub const MOTIONS: [&str; 6] = [
    "ui.space_mouse_motion_translation_x",
    "ui.space_mouse_motion_translation_y",
    "ui.space_mouse_motion_translation_z",
    "ui.space_mouse_motion_rotation_x",
    "ui.space_mouse_motion_rotation_y",
    "ui.space_mouse_motion_rotation_z",
];

/// What each logical axis does to the view, as `navigation::device_camera` applies it.
const VIEW_AXES: [&str; 6] = [
    "ui.space_mouse_view_pan_sideways",
    "ui.space_mouse_view_zoom",
    "ui.space_mouse_view_pan_up_down",
    "ui.space_mouse_view_orbit_up_down",
    "ui.space_mouse_view_orbit_sideways",
    "ui.space_mouse_view_roll",
];

/// The label of axis row `row`: what it does to the view through `mapping`, the mapping
/// navigation really uses, or "off" when that mapping leaves it out. Without a mapping (a build
/// with no 3D mouse reader, since the apps run navigation every frame before drawing), a plain
/// "Axis {n}": neither a guess at the view nor a motion name, which the dropdown already uses.
/// Two rows a driver profile sends to the same output both get its label; that's what they do.
pub fn row_label(mapping: Option<DriverMapping>, row: usize) -> &'static str {
    match mapping.and_then(|mapping| mapping.axes.get(row).copied()) {
        Some(rule) if rule.enabled => VIEW_AXES.get(rule.output).copied().unwrap_or("ui.space_mouse_view_off"),
        Some(_) => "ui.space_mouse_view_off",
        None => "ui.space_mouse_view_unmapped",
    }
}

/// Which of the cap's motions drives each of its axes (`Prefs::space_mouse_axis_source`): row
/// `r` reads motion `source[r]`. Each motion drives exactly one row.
pub const SAME_AXES: [usize; 6] = [0, 1, 2, 3, 4, 5];

/// The reading with its rows reassigned: row `r` takes motion `source[r]`. A `source` that
/// isn't one motion per row reads as [`SAME_AXES`].
pub fn reassigned(axes: Axes, source: [usize; 6]) -> Axes {
    let source = if drives_each_once(source) { source } else { SAME_AXES };
    let raw: [i16; 6] = axes.into();
    source.map(|from| raw.get(from).copied().unwrap_or(0)).into()
}

/// Makes `row` read motion `from`. The row that read `from` before takes this row's old motion,
/// so each motion still drives exactly one row.
pub fn reassign(source: &mut [usize; 6], row: usize, from: usize) {
    if let Some(other) = source.iter().position(|&s| s == from)
        && row < source.len()
    {
        source.swap(row, other);
    }
}

/// Whether each of the six motions drives exactly one row.
pub fn drives_each_once(source: [usize; 6]) -> bool {
    let mut seen = [false; 6];
    source.iter().all(|&from| seen.get_mut(from).is_some_and(|seen| !std::mem::replace(seen, true)))
}

/// Hands one frame of 3D mouse input, `seconds` after the previous frame, to the page showing
/// the camera. Nothing happens with navigation switched off, on the start page or while a file
/// picker is open, or while the reading is inside the dead zone (after calibration, which takes
/// off the device's own rest offset), so a cap resting just off zero doesn't keep redrawing.
/// Returns whether there was motion to hand over; it then asks for the next frame itself. The
/// reading goes through the device's saved calibration first, when it has one; while the
/// calibration window is open, the reading goes to it instead and the view stays put.
pub fn navigate_frame(app: &mut SolveApp, ctx: &egui::Context, mapping: DriverMapping, axes: Axes, seconds: f64) -> bool {
    app.space_mouse.raw = axes;
    app.space_mouse.mapping = Some(mapping);
    if let Some((device, wizard)) = &mut app.space_mouse.calibrating {
        if app.space_mouse.device.as_ref() == Some(device) {
            wizard.feed(axes.into(), seconds);
            ctx.request_repaint_after(Duration::from_millis(16));
            return false;
        }
        // Another device is being read now, or none: what was recorded isn't its.
        app.space_mouse.calibrating = None;
    }
    // Calibrated first, so the calibration puts the device's axes in order, then the user's choice
    // of which motion drives which axis. Invert and speed then go with the axis, not the motion.
    let axes = reassigned(calibrated(axes, calibration(app)), app.preferences.space_mouse_axis_source);
    // Only the window with focus: with two windows open, the cap moves the one being used, not both.
    if !app.preferences.space_mouse_enabled || app.home.open || app.file_picker_busy() || !axes.moving() || !ctx.input(|i| i.focused) {
        return false;
    }
    let speed = f64::from(app.preferences.space_mouse_speed);
    if !speed.is_finite() {
        return false;
    }
    app.begin_external_navigation();
    // A long gap (the first frame after a pause) moves as far as a 50 ms one.
    let step = if seconds.is_finite() { seconds.min(0.05) } else { 0.0 };
    app.viewport.device_motion = Some(DeviceMotion {
        motion: mapping.with_user_axes(app.preferences.space_mouse_reverse, app.preferences.space_mouse_axis_speed).apply(axes),
        seconds: step * speed.clamp(0.1, 5.0),
    });
    ctx.request_repaint_after(Duration::from_millis(16));
    true
}

/// A small dead zone avoids drift at rest. A full cap deflection takes roughly one viewport
/// per second to pan and about two radians per second to turn, independent of frame rate.
fn normalized(value: i16) -> f64 {
    let v = f64::from(value);
    if v.abs() <= 20.0 { 0.0 } else { v.signum() * ((v.abs() - 20.0) / 330.0).min(1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use solidcraft_engine::Session;
    use solidcraft_engine::render::Camera;

    /// The camera math the page applies the motion with (`navigation::device`): `sketch` keeps
    /// the orientation, as editing a sketch does.
    fn navigate(cam: &mut Camera, motion: Motion, seconds: f64, height: f64, sketch: bool, reverse_zoom: bool) {
        crate::navigation::device_camera(cam, motion, seconds, height, !sketch, reverse_zoom, false);
    }

    /// A window that has run a frame, with or without focus. A context that has never run one
    /// reads as unfocused, so the tests run one first, as the app always has.
    fn window(focused: bool) -> egui::Context {
        let ctx = crate::fonts::test_context();
        let mut output = ctx.run_ui(egui::RawInput { focused, ..Default::default() }, |_| {});
        output.textures_delta.clear();
        ctx
    }

    /// One frame: the 3D mouse hands its motion over, and the modelling viewport applies it.
    fn frame(app: &mut SolveApp, ctx: &egui::Context, mapping: DriverMapping, axes: Axes, seconds: f64) -> bool {
        let handed = navigate_frame(app, ctx, mapping, axes, seconds);
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        crate::navigation::device(app, page, crate::navigation::NavCaps::MODEL);
        handed
    }

    fn raw(axes: Axes) -> Motion {
        DriverMapping::default().apply(axes)
    }

    fn app() -> SolveApp {
        let mut app = SolveApp::new(Session::default(), Services::default());
        app.home.open = false;
        app
    }

    #[test]
    fn per_axis_choices_flip_and_scale_one_physical_axis() {
        let mapping = DriverMapping::default().with_user_axes([false, true, false, false, false, false], [1.0, 1.0, 1.0, 0.5, f32::NAN, 1.0]);
        let still = Motion::default();
        assert_eq!(mapping.apply(Axes { translation: [0, 350, 0], rotation: [0; 3] }).translation[1], -raw(PUSH).translation[1]);
        assert_eq!(
            mapping.apply(Axes { translation: [350, 0, 0], rotation: [0; 3] }).translation[0],
            raw(Axes { translation: [350, 0, 0], rotation: [0; 3] }).translation[0]
        );
        assert_eq!(mapping.apply(Axes { translation: [0; 3], rotation: [350, 0, 0] }).rotation[0], 0.5);
        assert_eq!(mapping.apply(Axes { translation: [0; 3], rotation: [0, 350, 0] }).rotation[1], 1.0);
        assert_eq!(mapping.apply(Axes::default()), still);
        let off = DriverMapping::default().with_user_axes([false; 6], [0.0; 6]);
        assert_eq!(off.apply(PUSH), still);
    }

    #[test]
    fn solidworks_free_orbit_respects_per_axis_settings_through_both_poles() {
        let ctx = window(true);
        let initial = Camera { yaw: 0.0, pitch: 1.4, roll: 0.0, ..Default::default() };
        let axes = Axes { translation: [0; 3], rotation: [350, 0, 0] };
        let step = std::f64::consts::TAU / 128.0;
        for mapping in [DriverMapping::default(), DriverMapping::object_mode()] {
            for reversed in [false, true] {
                for speed in [0.5, 2.0] {
                    let mut app = app();
                    app.preferences.nav = "solidworks".into();
                    app.preferences.free_orbit = true;
                    app.preferences.space_mouse_reverse[3] = reversed;
                    app.preferences.space_mouse_axis_speed[3] = speed;
                    app.cam = initial;
                    for i in 0..128 {
                        let before = app.cam;
                        assert!(frame(&mut app, &ctx, mapping, axes, step / (2.0 * f64::from(speed))));
                        assert!((before.back().dot(app.cam.back()) - step.cos()).abs() < 1e-10);
                        if i == 0 {
                            let direction = if reversed { -1.0 } else { 1.0 };
                            assert!((app.cam.back().dot(initial.basis().1) - direction * step.sin()).abs() < 1e-10);
                        }
                    }
                    assert!(app.cam.back().dist(initial.back()) < 1e-9 && app.cam.basis().0.dist(initial.basis().0) < 1e-9);
                    assert_eq!((app.cam.target, app.cam.distance, app.cam.fov), (initial.target, initial.distance, initial.fov));
                    app.preferences.space_mouse_axis_speed[3] = 0.0;
                    let before = app.cam;
                    frame(&mut app, &ctx, mapping, axes, 0.04);
                    assert_eq!(app.cam, before);
                }
            }
        }
    }

    const PUSH: Axes = Axes { translation: [0, 350, 0], rotation: [0; 3] };

    fn wheel(dy: f32) -> egui::Event {
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        }
    }

    fn other() -> egui::Event {
        egui::Event::PointerMoved(egui::pos2(1.0, 2.0))
    }

    #[test]
    fn six_axes_move_the_camera_and_sketches_keep_their_orientation() {
        let mut cam = Camera::default();
        let before = cam;
        navigate(&mut cam, raw(Axes { translation: [350, 350, 350], rotation: [350, 350, 350] }), 0.02, 600.0, false, false);
        assert_ne!(cam.target, before.target);
        assert!(cam.distance < before.distance);
        assert_ne!(cam.yaw, before.yaw);
        assert_ne!(cam.pitch, before.pitch);
        assert_ne!(cam.roll, before.roll);
        let orientation = (cam.yaw, cam.pitch, cam.roll);
        navigate(&mut cam, raw(Axes { translation: [350, 0, 0], rotation: [350, 350, 350] }), 0.02, 600.0, true, false);
        assert_eq!((cam.yaw, cam.pitch, cam.roll), orientation);
        assert!(cam.is_valid());
    }

    #[test]
    fn individual_axes_have_physical_spacemouse_meaning() {
        let before = Camera::default();

        let mut slide = before;
        navigate(&mut slide, raw(Axes { translation: [350, 0, 0], rotation: [0; 3] }), 0.02, 600.0, false, false);
        assert_ne!(slide.target, before.target);
        assert_eq!(slide.distance, before.distance);

        let mut push = before;
        navigate(&mut push, raw(Axes { translation: [0, 350, 0], rotation: [0; 3] }), 0.02, 600.0, false, false);
        assert_eq!(push.target, before.target);
        assert!(push.distance < before.distance);

        let mut lift = before;
        navigate(&mut lift, raw(Axes { translation: [0, 0, 350], rotation: [0; 3] }), 0.02, 600.0, false, false);
        assert_ne!(lift.target, before.target);
        assert_eq!(lift.distance, before.distance);

        let mut tilt = before;
        navigate(&mut tilt, raw(Axes { translation: [0; 3], rotation: [350, 0, 0] }), 0.02, 600.0, false, false);
        assert_eq!((tilt.yaw, tilt.roll), (before.yaw, before.roll));
        assert_ne!(tilt.pitch, before.pitch);

        let mut roll = before;
        navigate(&mut roll, raw(Axes { translation: [0; 3], rotation: [0, 350, 0] }), 0.02, 600.0, false, false);
        assert_eq!((roll.pitch, roll.roll), (before.pitch, before.roll));
        assert_ne!(roll.yaw, before.yaw);

        let mut twist = before;
        navigate(&mut twist, raw(Axes { translation: [0; 3], rotation: [0, 0, 350] }), 0.02, 600.0, false, false);
        assert_eq!((twist.yaw, twist.pitch), (before.yaw, before.pitch));
        assert_ne!(twist.roll, before.roll);
    }

    #[test]
    fn reverse_zoom_setting_applies_to_spacemouse() {
        let before = Camera::default();
        let motion = raw(Axes { translation: [0, 350, 0], rotation: [0; 3] });
        let mut normal = before;
        navigate(&mut normal, motion, 0.02, 600.0, false, false);
        let mut reversed = before;
        navigate(&mut reversed, motion, 0.02, 600.0, false, true);
        assert!(normal.distance < before.distance);
        assert!(reversed.distance > before.distance);
    }

    #[test]
    fn object_mode_tilts_roll_and_twists_spin() {
        let before = Camera::default();
        let mut tilt = before;
        navigate(&mut tilt, DriverMapping::object_mode().apply(Axes { translation: [0; 3], rotation: [0, 350, 0] }), 0.02, 600.0, false, false);
        assert_eq!((tilt.yaw, tilt.pitch), (before.yaw, before.pitch));
        assert_ne!(tilt.roll, before.roll);
        let mut twist = before;
        navigate(&mut twist, DriverMapping::object_mode().apply(Axes { translation: [0; 3], rotation: [0, 0, 350] }), 0.02, 600.0, false, false);
        assert_ne!(twist.yaw, before.yaw);
        assert_eq!((twist.pitch, twist.roll), (before.pitch, before.roll));
        // The physical tilt right (Ry−) rolls one way and the clockwise twist (Rz+) spins the
        // other, as 3DxWare's profile has them (#1172).
        let tilt_right = DriverMapping::object_mode().apply(Axes { translation: [0; 3], rotation: [0, -350, 0] });
        assert_eq!(tilt_right, Motion { translation: [0.0; 3], rotation: [0.0, 0.0, 1.0] });
        let twist_clockwise = DriverMapping::object_mode().apply(Axes { translation: [0; 3], rotation: [0, 0, 350] });
        assert_eq!(twist_clockwise, Motion { translation: [0.0; 3], rotation: [0.0, -1.0, 0.0] });
        // Translations and the forward tilt are the raw axes.
        let all = Axes { translation: [100, -200, 300], rotation: [-150, 0, 0] };
        assert_eq!(DriverMapping::object_mode().apply(all), DriverMapping::default().apply(all));
    }

    #[test]
    fn the_driver_choice_is_saved_and_an_older_file_reads_automatic() {
        use crate::prefs::SpaceMouseDriver;
        let mut saved = serde_json::to_value(crate::prefs::Prefs::default()).unwrap();
        assert_eq!(saved["space_mouse_driver"], "automatic");
        for driver in SpaceMouseDriver::ALL {
            saved["space_mouse_driver"] = serde_json::to_value(driver).unwrap();
            assert_eq!(crate::prefs::lenient(&saved).space_mouse_driver, driver);
            assert_eq!(SpaceMouseDriver::from_u8(driver.to_u8()), driver);
        }
        saved["space_mouse_driver"] = "direct".into();
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_driver, SpaceMouseDriver::Direct);
        // One saved before the setting existed, or with a driver this version doesn't know.
        saved.as_object_mut().unwrap().remove("space_mouse_driver");
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_driver, SpaceMouseDriver::Automatic);
        saved["space_mouse_driver"] = "evdev".into();
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_driver, SpaceMouseDriver::Automatic);
        assert_eq!(SpaceMouseDriver::from_u8(200), SpaceMouseDriver::Automatic);
    }

    #[test]
    fn the_dead_zone_is_rest() {
        assert!(!Axes::default().moving());
        assert!(!Axes { translation: [20, -20, 0], rotation: [0, 20, -20] }.moving());
        for i in 0..6 {
            let mut values = [0_i16; 6];
            values[i] = 21;
            let axes = Axes { translation: [values[0], values[1], values[2]], rotation: [values[3], values[4], values[5]] };
            assert!(axes.moving(), "axis {i}");
            values[i] = -21;
            let axes = Axes { translation: [values[0], values[1], values[2]], rotation: [values[3], values[4], values[5]] };
            assert!(axes.moving(), "axis {i} reversed");
        }
        assert!(Axes { translation: [i16::MIN, 0, 0], rotation: [0, 0, i16::MAX] }.moving());
    }

    #[test]
    fn a_mapping_never_reaches_past_the_six_axes() {
        let mut mapping = DriverMapping::default();
        mapping.axes[0].output = 6;
        mapping.axes[1].output = usize::MAX;
        let motion = mapping.apply(Axes { translation: [350, 350, 350], rotation: [350; 3] });
        assert_eq!(motion.translation, [0.0, 0.0, 1.0]);
        assert_eq!(motion.rotation, [1.0; 3]);
    }

    #[test]
    fn a_frame_moves_the_view_and_keeps_the_next_frame_coming() {
        let ctx = window(true);
        let mut app = app();
        let before = app.cam;
        assert!(frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02));
        assert!(app.cam.distance < before.distance);
        // At rest, or with no time passed, nothing moves.
        let still = app.cam;
        assert!(!frame(&mut app, &ctx, DriverMapping::default(), Axes::default(), 0.02));
        assert!(frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.0));
        assert_eq!(app.cam.distance, still.distance);
    }

    #[test]
    fn only_the_focused_window_moves() {
        // Two windows side by side (#2077): the cap moves the one with focus and leaves the other.
        let mut app = app();
        let before = app.cam;
        assert!(!frame(&mut app, &window(false), DriverMapping::default(), PUSH, 0.02));
        assert_eq!(app.cam.distance, before.distance);
        assert!(frame(&mut app, &window(true), DriverMapping::default(), PUSH, 0.02));
        assert!(app.cam.distance < before.distance);
    }

    #[test]
    fn an_open_file_picker_holds_the_view_until_it_closes() {
        let ctx = window(true);
        let mut app = app();
        let slot: crate::picker_test_support::Slot = Default::default();
        crate::picker_test_support::open(&mut app, &slot);
        app.pick_open(|_, _| {});
        assert!(app.file_picker_busy());
        let before = app.cam;
        assert!(!frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02));
        assert_eq!(app.cam.distance, before.distance);
        // Cancelled: the 3D mouse moves the view again.
        *slot.borrow_mut() = Some(None);
        crate::file_picker::tick(&mut app, &ctx);
        assert!(!app.file_picker_busy());
        assert!(frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02));
        assert!(app.cam.distance < before.distance);
    }

    #[test]
    fn a_long_gap_between_frames_moves_as_far_as_a_short_one() {
        let ctx = window(true);
        let (mut short, mut long) = (app(), app());
        frame(&mut short, &ctx, DriverMapping::default(), PUSH, 0.05);
        frame(&mut long, &ctx, DriverMapping::default(), PUSH, 30.0);
        assert_eq!(short.cam.distance, long.cam.distance);
        for seconds in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            let mut app = app();
            let before = app.cam;
            frame(&mut app, &ctx, DriverMapping::default(), PUSH, seconds);
            assert_eq!(app.cam.distance, before.distance, "{seconds}");
        }
    }

    #[test]
    fn navigation_off_the_start_page_and_a_hostile_speed_leave_the_view_alone() {
        let ctx = window(true);
        let mut off = app();
        off.preferences.space_mouse_enabled = false;
        let mut home = app();
        home.home.open = true;
        let mut nan = app();
        nan.preferences.space_mouse_speed = f32::NAN;
        for mut app in [off, home, nan] {
            let before = app.cam;
            assert!(!frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02));
            assert_eq!(app.cam.distance, before.distance);
        }
        // Speed follows the preference, within 0.1 to 5 times.
        let speeds = [0.5, 2.0, 5.0, f32::MAX, 0.25, 0.0, -3.0];
        let distances: Vec<f64> = speeds
            .iter()
            .map(|&speed| {
                let mut app = app();
                app.preferences.space_mouse_speed = speed;
                frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02);
                assert!(app.cam.is_valid(), "{speed}");
                app.cam.distance
            })
            .collect();
        assert!(distances[1] < distances[0]);
        assert_eq!(distances[3], distances[2], "past 5 times is 5 times");
        // Under 0.1 times is 0.1 times (the f32 preference can't hold 0.1 exactly): still a
        // motion, less than at 0.25.
        assert_eq!(distances[6], distances[5], "under 0.1 times is 0.1 times");
        assert!(distances[4] < distances[5] && distances[5] < app().cam.distance);
    }

    #[test]
    fn wheel_turns_are_dropped_while_the_cap_moves_and_kept_at_rest() {
        let mut guard = WheelGuard::default();
        // At rest, the wheel is the user's.
        let mut events = vec![wheel(100.0), other()];
        guard.filter(false, 10.0, &mut events);
        assert_eq!(events.len(), 2);
        // Moving: 3DxWare's wheel (and a pinch zoom) goes; everything else stays.
        let mut events = vec![wheel(-100.0), other(), egui::Event::Zoom(1.1), wheel(100.0)];
        guard.filter(true, 11.0, &mut events);
        assert_eq!(events, vec![other()]);
        // Just after the cap stops, a queued one still goes.
        let mut events = vec![wheel(100.0)];
        guard.filter(false, 11.0 + WHEEL_QUIET_S - 0.01, &mut events);
        assert!(events.is_empty());
        // Later, the wheel is the user's again.
        let mut events = vec![wheel(100.0)];
        guard.filter(false, 11.0 + WHEEL_QUIET_S + 0.01, &mut events);
        assert_eq!(events.len(), 1);
        // A clock that goes back never swallows the user's wheel.
        let mut events = vec![wheel(100.0)];
        guard.filter(false, 5.0, &mut events);
        assert_eq!(events.len(), 1);
        let mut events = vec![wheel(100.0)];
        guard.filter(false, f64::NAN, &mut events);
        assert_eq!(events.len(), 1);
    }

    /// A homemade device that sends push forward/back as its fifth axis, the other way round, at
    /// ten times 3Dconnexion's range.
    fn homemade() -> solidcraft_space_mouse::Calibration {
        let axis = |source, reversed| solidcraft_space_mouse::AxisCalibration { source, reversed, rest: 0, dead_zone: 0, full: [3500; 2] };
        solidcraft_space_mouse::Calibration {
            dead_zone_share: 0.0,
            axes: [axis(0, false), axis(4, true), axis(2, false), axis(3, false), axis(1, false), axis(5, false)],
        }
    }

    #[test]
    fn a_devices_saved_calibration_puts_its_axes_on_3dconnexions() {
        let ctx = window(true);
        let mut reference = app();
        frame(&mut reference, &ctx, DriverMapping::default(), PUSH, 0.02);
        let sent = Axes { translation: [0; 3], rotation: [0, -3500, 0] };
        let mut calibrated = app();
        calibrated.space_mouse.device = Some("hid 1234:5678".into());
        calibrated.preferences.space_mouse_calibrations.insert("hid 1234:5678".into(), homemade());
        assert!(frame(&mut calibrated, &ctx, DriverMapping::default(), sent, 0.02));
        assert_eq!(calibrated.cam, reference.cam);
        // Another device's calibration isn't applied: the reading turns the view as it comes.
        let mut other = app();
        other.space_mouse.device = Some("hid 256f:c63a".into());
        other.preferences.space_mouse_calibrations.insert("hid 1234:5678".into(), homemade());
        frame(&mut other, &ctx, DriverMapping::default(), sent, 0.02);
        assert_ne!(other.cam, reference.cam);
    }

    #[test]
    fn while_calibrating_the_cap_feeds_the_calibration_and_the_view_stays() {
        let ctx = window(true);
        let mut app = app();
        crate::space_mouse_calibration::start(&mut app);
        assert!(app.space_mouse.calibrating.is_none(), "nothing to calibrate without a device");
        app.space_mouse.device = Some("hid 256f:c63a".into());
        crate::space_mouse_calibration::start(&mut app);
        let before = app.cam;
        assert!(!frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02));
        assert_eq!(app.cam, before);
        assert_eq!(app.space_mouse.raw, PUSH);
        assert!(app.space_mouse.calibrating.as_ref().is_some_and(|(_, wizard)| wizard.progress() > 0.0));
        // Closing Preferences ends it, so the cap moves the view again.
        app.prefs_window.open = false;
        crate::prefs::show(&mut app, &ctx);
        assert!(app.space_mouse.calibrating.is_none());
        assert!(frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02));
    }

    #[test]
    fn a_reading_resting_off_zero_is_kept_for_calibration_and_doesnt_move_the_view() {
        let ctx = window(true);
        // Inside the dead zone and not calibrated: no motion, so no redraw either.
        let mut app = app();
        let resting = Axes { translation: [0, 15, 0], rotation: [0; 3] };
        let before = app.cam;
        assert!(!frame(&mut app, &ctx, DriverMapping::default(), resting, 0.02));
        assert_eq!(app.cam, before);
        // The calibration window gets it as it is, to measure where the cap rests.
        app.space_mouse.device = Some("web 1234:5678".into());
        crate::space_mouse_calibration::start(&mut app);
        frame(&mut app, &ctx, DriverMapping::default(), resting, 0.02);
        assert_eq!(app.space_mouse.raw, resting);
        app.space_mouse.calibrating = None;
        // A device resting far off zero, calibrated: still at rest, moving when pushed.
        let mut offset = homemade();
        for axis in &mut offset.axes {
            axis.rest = 1000;
        }
        app.preferences.space_mouse_calibrations.insert("web 1234:5678".into(), offset);
        let at_rest = Axes { translation: [1000; 3], rotation: [1000; 3] };
        assert!(!frame(&mut app, &ctx, DriverMapping::default(), at_rest, 0.02));
        assert_eq!(app.cam, before);
        assert!(frame(&mut app, &ctx, DriverMapping::default(), Axes { rotation: [1000, 1000 - 3500, 1000], ..at_rest }, 0.02));
    }

    #[test]
    fn another_device_turning_up_mid_calibration_ends_it() {
        let ctx = window(true);
        let mut app = app();
        app.space_mouse.device = Some("hid 256f:c63a".into());
        crate::space_mouse_calibration::start(&mut app);
        frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02);
        app.space_mouse.device = Some("spacenavd".into());
        assert!(frame(&mut app, &ctx, DriverMapping::default(), PUSH, 0.02), "the new device moves the view");
        assert!(app.space_mouse.calibrating.is_none());
        assert!(app.preferences.space_mouse_calibrations.is_empty());
    }

    #[test]
    fn calibrations_are_saved_with_the_preferences() {
        let mut prefs = crate::prefs::Prefs::default();
        prefs.space_mouse_calibrations.insert("hid 1234:5678".into(), homemade());
        let mut saved = serde_json::to_value(&prefs).unwrap();
        assert_eq!(crate::prefs::lenient(&saved), prefs);
        // One that can't be read, or that could spin the view, is dropped on its own.
        let mut spinning = homemade();
        spinning.axes[0].full = [0; 2];
        saved["space_mouse_calibrations"]["spacenavd"] = serde_json::to_value(spinning).unwrap();
        saved["space_mouse_calibrations"]["web 046d:c62b"] = serde_json::json!({ "axes": "garbled" });
        assert_eq!(crate::prefs::lenient(&saved), prefs);
        // One saved before the dead zone setting existed reads with none.
        let mut older = serde_json::to_value(homemade()).unwrap();
        older.as_object_mut().unwrap().remove("dead_zone_share");
        saved["space_mouse_calibrations"]["hid 1234:5678"] = older;
        assert_eq!(crate::prefs::lenient(&saved), prefs);
    }

    #[test]
    fn picking_a_motion_another_axis_uses_swaps_the_two() {
        let mut source = SAME_AXES;
        reassign(&mut source, 1, 2);
        assert_eq!(source, [0, 2, 1, 3, 4, 5], "Y and Z swapped");
        reassign(&mut source, 4, 5);
        assert_eq!(source, [0, 2, 1, 3, 5, 4], "then roll and twist");
        reassign(&mut source, 1, 1);
        assert_eq!(source, [0, 1, 2, 3, 5, 4], "back again");
        assert!(drives_each_once(source));
        // A row or a motion that doesn't exist changes nothing.
        reassign(&mut source, 9, 0);
        reassign(&mut source, 0, 9);
        assert_eq!(source, [0, 1, 2, 3, 5, 4]);
        assert!(!drives_each_once([0, 0, 2, 3, 4, 5]) && !drives_each_once([0, 1, 2, 3, 4, 9]));
        // A value that doesn't drive each axis once is read as the default.
        let reading = Axes { translation: [1, 2, 3], rotation: [4, 5, 6] };
        assert_eq!(reassigned(reading, [0, 2, 1, 3, 5, 4]), Axes { translation: [1, 3, 2], rotation: [4, 6, 5] });
        assert_eq!(reassigned(reading, [0, 0, 2, 3, 4, 5]), reading);
    }

    #[test]
    fn a_reassigned_motion_drives_its_new_axis_and_invert_stays_with_the_axis() {
        let ctx = window(true);
        let press = Axes { translation: [0, 0, 350], rotation: [0; 3] };
        // With Y and Z swapped, pressing the cap does what pushing it back would.
        let mut reference = app();
        frame(&mut reference, &ctx, DriverMapping::default(), PUSH, 0.02);
        let mut swapped = app();
        swapped.preferences.space_mouse_axis_source = [0, 2, 1, 3, 4, 5];
        assert!(frame(&mut swapped, &ctx, DriverMapping::default(), press, 0.02));
        assert_eq!(swapped.cam, reference.cam);
        // Inverting the Y axis inverts whatever motion drives it now, not the Y motion.
        let mut inverted = app();
        inverted.preferences.space_mouse_reverse[1] = true;
        frame(&mut inverted, &ctx, DriverMapping::default(), PUSH, 0.02);
        let mut swapped_inverted = app();
        swapped_inverted.preferences.space_mouse_axis_source = [0, 2, 1, 3, 4, 5];
        swapped_inverted.preferences.space_mouse_reverse[1] = true;
        frame(&mut swapped_inverted, &ctx, DriverMapping::default(), press, 0.02);
        assert_eq!(swapped_inverted.cam, inverted.cam);
        assert_ne!(swapped_inverted.cam, reference.cam);
        // The same for the axis's speed.
        let mut slowed = app();
        slowed.preferences.space_mouse_axis_speed[1] = 0.5;
        frame(&mut slowed, &ctx, DriverMapping::default(), PUSH, 0.02);
        let mut swapped_slowed = app();
        swapped_slowed.preferences.space_mouse_axis_source = [0, 2, 1, 3, 4, 5];
        swapped_slowed.preferences.space_mouse_axis_speed[1] = 0.5;
        frame(&mut swapped_slowed, &ctx, DriverMapping::default(), press, 0.02);
        assert_eq!(swapped_slowed.cam, slowed.cam);
        assert_ne!(swapped_slowed.cam, reference.cam);
    }

    #[test]
    fn a_motion_swapped_into_a_row_the_driver_turns_off_moves_nothing_until_swapped_back() {
        let ctx = window(true);
        let mut pan_up_down_off = DriverMapping::default();
        pan_up_down_off.axes[2].enabled = false;
        let mut slowed = app();
        slowed.preferences.space_mouse_axis_speed[1] = 0.5;
        frame(&mut slowed, &ctx, pan_up_down_off, PUSH, 0.02);
        // Picking the push motion in the off row swaps the off row's motion into row 1.
        let mut app = app();
        app.preferences.space_mouse_axis_speed[1] = 0.5;
        reassign(&mut app.preferences.space_mouse_axis_source, 2, 1);
        assert_eq!(app.preferences.space_mouse_axis_source, [0, 2, 1, 3, 4, 5]);
        let still = app.cam;
        assert_ne!(slowed.cam, still);
        frame(&mut app, &ctx, pan_up_down_off, PUSH, 0.02);
        assert_eq!(app.cam, still);
        // Picking it back in row 1 recovers it, at the speed row 1 kept.
        reassign(&mut app.preferences.space_mouse_axis_source, 1, 1);
        assert_eq!(app.preferences.space_mouse_axis_source, SAME_AXES);
        frame(&mut app, &ctx, pan_up_down_off, PUSH, 0.02);
        assert_eq!(app.cam, slowed.cam);
    }

    #[test]
    fn the_choice_is_between_calibrated_motions_on_a_device_whose_calibration_shuffles_its_channels() {
        // `homemade()` reads Y from raw channel 4, reversed, and Ry from raw 1. With Y and Z
        // swapped, the choices are calibrated motions: pressing the cap (calibrated Z, raw 2)
        // drives Y, and pushing it back (calibrated Y, raw 4 the other way) drives Z.
        let ctx = window(true);
        let device = "hid 1234:5678";
        let setup = |source: [usize; 6]| {
            let mut app = app();
            app.space_mouse.device = Some(device.into());
            app.preferences.space_mouse_calibrations.insert(device.into(), homemade());
            app.preferences.space_mouse_axis_source = source;
            app
        };
        // Mixed motion: a press and a push back together, on the device's own channels.
        let mixed = Axes { translation: [0, 0, 1750], rotation: [0, -3500, 0] };
        let expected = Axes { translation: [0, 175, 350], rotation: [0; 3] };
        let mut reference = app();
        frame(&mut reference, &ctx, DriverMapping::default(), expected, 0.02);
        let mut swapped = setup([0, 2, 1, 3, 4, 5]);
        assert!(frame(&mut swapped, &ctx, DriverMapping::default(), mixed, 0.02));
        assert_eq!(swapped.cam, reference.cam, "calibrated first, then the motions swapped, once");
        // Saved and loaded, it's the same choice; reset, the device reads as calibrated.
        let saved = serde_json::to_value(&swapped.preferences).unwrap();
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_axis_source, [0, 2, 1, 3, 4, 5]);
        let mut unswapped_reference = app();
        frame(&mut unswapped_reference, &ctx, DriverMapping::default(), Axes { translation: [0, 350, 175], rotation: [0; 3] }, 0.02);
        let mut reset = setup(SAME_AXES);
        frame(&mut reset, &ctx, DriverMapping::default(), mixed, 0.02);
        assert_eq!(reset.cam, unswapped_reference.cam);
    }

    #[test]
    fn rows_say_what_they_do_to_the_view_through_the_mapping_navigation_uses() {
        let labels = |mapping| (0..6).map(|row| row_label(mapping, row)).collect::<Vec<_>>();
        assert_eq!(labels(Some(DriverMapping::default())), VIEW_AXES);
        // Linux, and Windows without a 3DxWare profile: object mode swaps the last two rows'
        // effect, so their labels swap with it.
        let object = labels(Some(DriverMapping::object_mode()));
        assert_eq!((object[4], object[5]), (VIEW_AXES[5], VIEW_AXES[4]));
        // An axis the mapping switches off says so.
        let mut off = DriverMapping::default();
        off.axes[2].enabled = false;
        assert_eq!(row_label(Some(off), 2), "ui.space_mouse_view_off");
        // Without a mapping, rows claim no view effect and name none of the dropdown's motions.
        assert_eq!(labels(None), ["ui.space_mouse_view_unmapped"; 6]);
    }

    #[test]
    fn each_frame_records_the_mapping_it_navigated_with() {
        let ctx = window(true);
        let mut app = app();
        assert!(app.space_mouse.mapping.is_none());
        frame(&mut app, &ctx, DriverMapping::object_mode(), Axes::default(), 0.02);
        assert_eq!(app.space_mouse.mapping, Some(DriverMapping::object_mode()));
        // A changed profile shows on the next frame, calibrating or not; a reassignment changes
        // which motion drives a row, never the row's label.
        app.space_mouse.device = Some("hid 1234:5678".into());
        app.preferences.space_mouse_axis_source = [0, 2, 1, 3, 4, 5];
        crate::space_mouse_calibration::start(&mut app);
        frame(&mut app, &ctx, DriverMapping::default(), Axes::default(), 0.02);
        assert_eq!(app.space_mouse.mapping, Some(DriverMapping::default()));
        assert_eq!(row_label(app.space_mouse.mapping, 1), VIEW_AXES[1]);
    }

    #[test]
    fn calibrating_records_the_device_before_the_axis_choice() {
        // With Y and Z swapped, a new calibration still learns the device's own channels.
        // Recorded after the swap, it would bake the swap in, and the swap would then apply twice.
        let ctx = window(true);
        let mut app = app();
        app.space_mouse.device = Some("hid 1234:5678".into());
        app.preferences.space_mouse_axis_source = [0, 2, 1, 3, 4, 5];
        crate::space_mouse_calibration::start(&mut app);
        let feed = |app: &mut SolveApp, axes: Axes, seconds: f64| {
            for _ in 0..(seconds / 0.02).round() as usize {
                frame(app, &ctx, DriverMapping::default(), axes, 0.02);
            }
        };
        feed(&mut app, Axes::default(), 1.6);
        for step in solidcraft_space_mouse::STEPS {
            let mut raw = [0_i16; 6];
            raw[step.axis] = if step.positive { 300 } else { -300 };
            feed(&mut app, raw.into(), 0.5);
            feed(&mut app, Axes::default(), 0.1);
        }
        let (_, wizard) = app.space_mouse.calibrating.as_ref().unwrap();
        let calibration = wizard.result().unwrap().unwrap();
        assert!(calibration.axes.iter().enumerate().all(|(c, axis)| axis.source == c && !axis.reversed), "{calibration:?}");
    }

    #[test]
    fn a_saved_axis_assignment_that_doesnt_drive_each_axis_once_loads_as_the_default() {
        let mut saved = serde_json::to_value(crate::prefs::Prefs::default()).unwrap();
        saved["space_mouse_axis_source"] = serde_json::json!([0, 2, 1, 3, 4, 5]);
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_axis_source, [0, 2, 1, 3, 4, 5]);
        saved["space_mouse_axis_source"] = serde_json::json!([1, 1, 2, 3, 4, 5]);
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_axis_source, SAME_AXES);
        // An older file without it, or one with the wrong length, loads with the default and
        // keeps every other setting.
        saved["space_mouse_speed"] = serde_json::json!(2.0);
        saved.as_object_mut().unwrap().remove("space_mouse_axis_source");
        let older = crate::prefs::lenient(&saved);
        assert_eq!((older.space_mouse_axis_source, older.space_mouse_speed), (SAME_AXES, 2.0));
        saved["space_mouse_axis_source"] = serde_json::json!([0, 1, 2, 3, 4]);
        let short = crate::prefs::lenient(&saved);
        assert_eq!((short.space_mouse_axis_source, short.space_mouse_speed), (SAME_AXES, 2.0));
    }
}
