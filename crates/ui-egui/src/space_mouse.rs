//! 3D mouse input, shared by the desktop and web apps. Each app reads the device its own way (the
//! desktop through HID, the web app through the browser's Gamepad API) and hands its six raw axes
//! to [`navigate_frame`] once per frame. This module maps them, applies the dead zone and drops
//! 3DxWare's emulated wheel; the page showing the camera moves it (`navigation::device`), so the
//! cap does what that page's camera may do. A device with a program of its own can drive the view
//! through that program instead: the desktop app reads it over the navigation bridge and hands
//! its motion to [`bridge_frame`], which goes to the same page.

use std::time::Duration;

use crate::SolveApp;

pub use solidcraft_space_mouse::{AxisCalibration, Calibration};

/// The view state native navigation libraries need. Camera updates still belong to the page
/// (`navigation::device`), so Render and future 2D pages retain their navigation capabilities.
pub struct View {
    pub camera: solidcraft_engine::render::Camera,
    pub bounds: solidcraft_engine::geom::Aabb3,
    pub selection: Option<solidcraft_engine::geom::Aabb3>,
    pub aspect: f64,
    pub rotatable: bool,
    pub enabled: bool,
}

pub fn view(app: &SolveApp, ctx: &egui::Context) -> View {
    let enabled = app.preferences.space_mouse_enabled
        && !app.home.open
        && !app.file_picker_busy()
        && app.issue_report.draft.is_none()
        && !app.prefs_window.open
        && !app.file_load.busy()
        && !crate::keymap::capturing(app)
        && !ctx.egui_wants_keyboard_input()
        && !crate::dialogs_sheet::flat_active(app)
        && ctx.input(|i| i.focused)
        && !egui::Popup::is_any_open(ctx);
    View {
        camera: app.cam,
        bounds: if crate::render_workspace::active(app) {
            crate::render_workspace::navigation_bounds(app)
        } else {
            crate::viewport::view_bounds(app)
        },
        selection: crate::viewport::selection_bounds(app),
        aspect: app.viewport.rect.map_or(1.0, |r| f64::from(r.width() / r.height().max(1.0))),
        rotatable: app.session.active_sketch.is_none(),
        enabled,
    }
}

/// Queue a driver-calculated camera; validate it before taking over from pointer navigation.
pub fn navigate_camera_frame(app: &mut SolveApp, ctx: &egui::Context, camera: solidcraft_engine::render::Camera) -> bool {
    app.viewport.device_camera = None;
    if !view(app, ctx).enabled || !camera.is_valid() || !(1e-3..=1e7).contains(&camera.distance) {
        return false;
    }
    app.begin_external_navigation();
    app.viewport.device_camera = Some(camera);
    ctx.request_repaint();
    true
}

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
    /// Drops readings that have stopped changing close to the dead zone.
    pub settle: solidcraft_space_mouse::Settle,
    /// The buttons down as of the last frame, one bit each (bit N for button N + 1).
    pub buttons: u32,
    /// Every button pressed since the app started, so Preferences can list the ones the device has.
    pub seen: u32,
    /// The modifiers the buttons hold down, as of the last frame.
    pub held: egui::Modifiers,
    /// The modifiers the keyboard holds, as the window last reported them (without `held`).
    pub keyboard: egui::Modifiers,
    /// The device `seen` is about, and the device and calibration `settle` is about.
    pub read_device: Option<String>,
    /// The device `buttons` are of, as the reader said with them.
    pub buttons_device: Option<String>,
    pub settled_for: Option<Calibration>,
    /// The device the calibration notice is up for and when it was shown or last pointed at
    /// (egui time), and the devices it went away from for this run ().
    pub offer_since: Option<(String, f64)>,
    pub offer_hidden: std::collections::BTreeSet<String>,
    /// When the offer was last looked at (egui time): a long gap is time away, not time shown.
    pub offer_seen: f64,
    /// The device last written to the log (one line per connect), `None` for none.
    pub logged_device: Option<String>,
    /// "Restore defaults" was clicked and waits for its answer.
    pub restore_pending: bool,
    /// The last look at the buttons had no state yet (`Buttons::pending`).
    pub buttons_pending: bool,
    /// The reader's connection the last look at the buttons came from (`Buttons::generation`).
    pub buttons_generation: u32,
    /// Nothing arrived since the last frame: the reading is the last value kept (spacenavd sends only on
    /// change, a HID device goes quiet), not a report or record just received, whatever its value. The
    /// calibration window needs the difference.
    pub retained: bool,
    /// The motion `navigate_frame` handed the view this frame (calibrated, reassigned, inverted and
    /// scaled), for the test model on the Preferences page to move with; none when the view got none.
    pub motion: Option<DeviceMotion>,
    /// The test model's view on that page, and when it was last drawn (`test_boat`).
    pub boat: Option<crate::test_boat::Boat>,
    /// The driver mapping navigation used last frame, which says what each axis does to the view.
    pub mapping: Option<DriverMapping>,
    /// Which of the Linux readers exist, as the app's reader last saw them; none where there is
    /// only one way to read the device.
    pub drivers: Option<DriversPresent>,
    /// The device app connected over the navigation bridge, by the name it gave (or "?" before it
    /// gave one); none while no app is connected, and in builds without a bridge.
    pub bridge: Option<String>,
}

/// What the Linux reader found: the spacenavd socket and a direct (hidraw) 3D mouse, and whether that
/// device was found but could not be opened (no permission), the last time it tried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriversPresent {
    pub spacenavd: bool,
    pub direct: bool,
    pub denied: bool,
}

/// What the reader saw of the buttons, taken in one look: the device they came from, those down
/// now (bit N for button N + 1) and those that went down since the last look, even when they
/// came up again before it. The taps are a set of buttons, not a queue of presses: two taps of one
/// button between two looks reach the app as one, and run its job once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub device: Option<String>,
    pub down: u32,
    pub taps: u32,
    /// The reader has not yet seen the state of this device's buttons (a HID device's first report that
    /// carries them is still to come), so `down` is not known yet. The first look after that is a
    /// baseline, not a press.
    pub pending: bool,
    /// Which connection of the reader these buttons come from; a different one than the last look means
    /// the reader connected again in between, so what is down now is held, not pressed.
    pub generation: u32,
}

/// A key a 3D mouse button can stand in for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonKey {
    Esc,
    Shift,
    Ctrl,
    Alt,
}

/// What a 3D mouse button does.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonJob {
    /// Opens Preferences at Navigation, where the 3D mouse's settings are.
    Settings,
    /// A view as `SolveApp::animate_view` takes it: `fit`, `home` or a standard view (`top`, …).
    View(String),
    /// Stands in for a key: Esc when pressed, Shift, Ctrl and Alt for as long as it's held.
    Key(ButtonKey),
    /// Runs a command, as its keyboard shortcut would.
    Command(String),
}

/// What each of `device`'s buttons does, by number from 1: what the user set, else for a device
/// we have measured, its printed keys' jobs.
pub fn button_jobs(app: &SolveApp, device: &str) -> std::collections::BTreeMap<u8, ButtonJob> {
    app.preferences.space_mouse_buttons.get(device).cloned().unwrap_or_else(|| default_button_jobs(device))
}

/// The printed keys' jobs on a device we have measured (`solidcraft_space_mouse::buttons`), from
/// its key (`hid 046d:c62b`, `web 046d:c62b`). Only a key with vendor and product ids gets them,
/// which in practice is a raw HID device: the `spacenavd` and `3dxware` keys carry no ids, and
/// spacenavd numbers its buttons from 0, not 1 as the table here does. Keys with no job here (Roll,
/// the rotation lock) start empty.
pub fn default_button_jobs(device: &str) -> std::collections::BTreeMap<u8, ButtonJob> {
    use solidcraft_space_mouse::buttons::{Action, defaults};
    let ids = device.split_once(' ').and_then(|(_, ids)| ids.split_once(':'));
    let table = ids.and_then(|(vendor, product)| Some((u16::from_str_radix(vendor, 16).ok()?, u16::from_str_radix(product, 16).ok()?)));
    let Some(table) = table.and_then(|(vendor, product)| defaults(vendor, product)) else { return Default::default() };
    table
        .iter()
        .enumerate()
        .filter_map(|(index, action)| {
            let job = match (*action)? {
                Action::Menu => ButtonJob::Settings,
                Action::Fit => ButtonJob::View("fit".into()),
                Action::ViewTop => ButtonJob::View("top".into()),
                Action::ViewRight => ButtonJob::View("right".into()),
                Action::ViewFront => ButtonJob::View("front".into()),
                Action::Esc => ButtonJob::Key(ButtonKey::Esc),
                Action::Shift => ButtonJob::Key(ButtonKey::Shift),
                Action::Ctrl => ButtonJob::Key(ButtonKey::Ctrl),
                Action::Alt => ButtonJob::Key(ButtonKey::Alt),
                Action::Roll | Action::RotationLock => return None,
            };
            Some((u8::try_from(index + 1).ok()?, job))
        })
        .collect()
}

/// Whether a button may do anything now: only where navigation and the keyboard shortcuts would
/// work (`navigate_frame`, `toolbar::shortcuts`), so a press in the background, with a picker open
/// or while a field takes the keyboard doesn't move the view or run a command.
fn buttons_active(app: &SolveApp, ctx: &egui::Context, focused: bool) -> bool {
    app.preferences.space_mouse_enabled
        && focused
        && !app.prefs_window.open
        && app.space_mouse.calibrating.is_none()
        && !app.home.open
        && !app.file_picker_busy()
        && !app.file_load.busy()
        // The keyboard shortcuts wait for a report draft too (`SolveApp::logic`).
        && app.issue_report.draft.is_none()
        && !crate::keymap::capturing(app)
        && !egui::Popup::is_any_open(ctx)
        && !ctx.egui_wants_keyboard_input()
        && !crate::dim_view::editing()
        && !crate::dialogs_save_mesh::popup_open(ctx)
        && crate::toolbar::design_shown(app)
}

/// The 3D mouse's buttons this frame (`buttons`, bit N for button N + 1), before the app sees the
/// frame's input: each button just pressed does its job, and those standing in for Shift, Ctrl or
/// Alt hold them while down. Nothing runs where navigation wouldn't (`buttons_active`), so pressing
/// a button in Preferences finds it in the list instead, and a press made then isn't run later.
pub fn buttons_hook(app: &mut SolveApp, ctx: &egui::Context, seen: Buttons, raw: &mut egui::RawInput) {
    // The device the buttons came from, not the one the last frame ended with: a switch of device
    // (or a first one) starts the button history afresh at what is down now, so a button already down
    // when the device changes (held through a switch of reader) is not a press on the new device. A
    // press made on it is in `taps`, so it still runs the new device's own mapping.
    // The same for the first look at the buttons' state after a look that had none to give (the reader
    // had connected but the device had not yet reported them): what is down in it is held, not pressed.
    if app.space_mouse.buttons_device != seen.device
        || app.space_mouse.buttons_generation != seen.generation
        || (app.space_mouse.buttons_pending && !seen.pending)
    {
        app.space_mouse.buttons_device = seen.device.clone();
        app.space_mouse.buttons = seen.down;
    }
    app.space_mouse.buttons_pending = seen.pending;
    app.space_mouse.buttons_generation = seen.generation;
    let buttons = seen.down;
    let pressed = (buttons & !app.space_mouse.buttons) | seen.taps;
    app.space_mouse.buttons = buttons;
    app.space_mouse.seen |= buttons;
    // The keyboard's own modifiers, from what the window was told before anything is added to it.
    // egui drops its modifiers when the window loses focus, so those go too.
    let mut focused = ctx.input(|i| i.focused);
    for event in &raw.events {
        match event {
            egui::Event::ModifiersChanged(modifiers) => app.space_mouse.keyboard = *modifiers,
            egui::Event::WindowFocused(now) => {
                focused = *now;
                if !now {
                    app.space_mouse.keyboard = egui::Modifiers::NONE;
                }
            }
            _ => {}
        }
    }
    let keyboard = app.space_mouse.keyboard;
    let mut held = egui::Modifiers::NONE;
    let device = seen.device.clone().filter(|_| buttons_active(app, ctx, focused));
    let jobs = device.map(|device| button_jobs(app, &device)).unwrap_or_default();
    let before = std::mem::replace(&mut app.space_mouse.held, egui::Modifiers::NONE);
    for (number, job) in jobs {
        let Some(bit) = number.checked_sub(1).and_then(|index| 1_u32.checked_shl(u32::from(index))) else { continue };
        if (buttons | seen.taps) & bit == 0 {
            continue;
        }
        match job {
            ButtonJob::Key(ButtonKey::Shift) if buttons & bit != 0 => held.shift = true,
            ButtonJob::Key(ButtonKey::Alt) if buttons & bit != 0 => held.alt = true,
            ButtonJob::Key(ButtonKey::Ctrl) if buttons & bit != 0 => {
                held.ctrl = true;
                // Ctrl is the command key everywhere but on a Mac.
                held.command |= !cfg!(target_os = "macos");
            }
            _ => {}
        }
        if pressed & bit != 0 {
            run_button_job(app, &job, keyboard, raw);
        }
    }
    app.space_mouse.held = held;
    let add = |modifiers: &mut egui::Modifiers| {
        modifiers.shift |= held.shift;
        modifiers.alt |= held.alt;
        modifiers.ctrl |= held.ctrl;
        modifiers.command |= held.command;
    };
    // egui keeps modifiers from frame to frame and takes new ones from `ModifiersChanged`: the
    // keyboard's get the held ones added, and a change in what's held is one of its own.
    let mut changed = false;
    for event in &mut raw.events {
        match event {
            egui::Event::ModifiersChanged(modifiers) => {
                add(modifiers);
                changed = true;
            }
            egui::Event::PointerButton { modifiers, .. } | egui::Event::Key { modifiers, .. } | egui::Event::MouseWheel { modifiers, .. } => {
                add(modifiers);
            }
            _ => {}
        }
    }
    if !changed && held != before {
        let mut modifiers = keyboard;
        add(&mut modifiers);
        raw.events.insert(0, egui::Event::ModifiersChanged(modifiers));
    }
}

// `modifiers` are the keyboard's own; the Esc event pushed here still gets the modifiers other buttons hold
// down, because `buttons_hook` adds them to every key event of the frame after all the jobs have run.
fn run_button_job(app: &mut SolveApp, job: &ButtonJob, modifiers: egui::Modifiers, raw: &mut egui::RawInput) {
    match job {
        // Navigation, where the 3D mouse's section is.
        ButtonJob::Settings => crate::prefs::open_at(app, crate::prefs::PAGE_NAVIGATION),
        ButtonJob::View(view) => app.animate_view(view),
        ButtonJob::Key(ButtonKey::Esc) => {
            for pressed in [true, false] {
                raw.events.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers });
            }
        }
        ButtonJob::Key(_) => {}
        ButtonJob::Command(id) => {
            if solidcraft_engine::find_command(id).is_some_and(|c| c.info(&app.session).enabled) {
                app.start(id);
            }
        }
    }
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
    "ui.3d_mouse_motion_translation_x",
    "ui.3d_mouse_motion_translation_y",
    "ui.3d_mouse_motion_translation_z",
    "ui.3d_mouse_motion_rotation_x",
    "ui.3d_mouse_motion_rotation_y",
    "ui.3d_mouse_motion_rotation_z",
];

/// What each logical axis does to the view, as `navigation::device_camera` applies it.
const VIEW_AXES: [&str; 6] = [
    "ui.3d_mouse_view_pan_sideways",
    "ui.3d_mouse_view_zoom",
    "ui.3d_mouse_view_pan_up_down",
    "ui.3d_mouse_view_orbit_up_down",
    "ui.3d_mouse_view_orbit_sideways",
    "ui.3d_mouse_view_roll",
];

/// The label of axis row `row`: what it does to the view through `mapping`, the mapping
/// navigation really uses, or "off" when that mapping leaves it out. Without a mapping (a build
/// with no 3D mouse reader, since the apps run navigation every frame before drawing), a plain
/// "Axis {n}": neither a guess at the view nor a motion name, which the dropdown already uses.
/// Two rows a driver profile sends to the same output both get its label; that's what they do.
pub fn row_label(mapping: Option<DriverMapping>, row: usize) -> &'static str {
    match mapping.and_then(|mapping| mapping.axes.get(row).copied()) {
        Some(rule) if rule.enabled => VIEW_AXES.get(rule.output).copied().unwrap_or("ui.3d_mouse_view_off"),
        Some(_) => "ui.3d_mouse_view_off",
        None => "ui.3d_mouse_view_unmapped",
    }
}

/// What the view does with calibrated motion `motion` under the axis assignment `source`: the label
/// of the row that reads it, the same label the rows show (and so the same after an invert or a
/// reassignment). `None` for a motion no row reads (an assignment that is not one motion per row).
pub fn motion_story(mapping: Option<DriverMapping>, source: [usize; 6], motion: usize) -> Option<&'static str> {
    source.iter().position(|&s| s == motion).map(|row| row_label(mapping, row))
}

/// What the circles' tooltips say each calibrated motion does, in `language`: `motion_story`, worded
/// as the rows are (a motion nothing reads says nothing). It names the view axis the motion drives,
/// not a direction, so an inverted axis reads the same; the page says the circles are before invert.
pub(crate) fn stories(language: &str, mapping: Option<DriverMapping>, source: [usize; 6]) -> [String; 6] {
    std::array::from_fn(|motion| {
        let row = source.iter().position(|&s| s == motion);
        motion_story(mapping, source, motion)
            .zip(row)
            .map(|(label, row)| crate::i18n::Locale::for_language(language).format(label, &[("n", &(row + 1).to_string())]))
            .unwrap_or_default()
    })
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

/// How long the calibration window waits before the next frame: 30 a second while the reading
/// changes, 10 while it holds still (the wizard goes by elapsed time, so a still reading needs only
/// enough frames for its timers), and 4 while the window is not in use. Every frame redraws the 3D
/// view behind the window as well, so this is most of what the window costs.
pub(crate) fn calibration_repaint_after(reading_changed: bool, focused: bool) -> Duration {
    Duration::from_millis(match (focused, reading_changed) {
        (false, _) => 250,
        (true, true) => 33,
        (true, false) => 100,
    })
}

/// Hands one frame of 3D mouse input, `seconds` after the previous frame, to the page showing
/// the camera. Nothing happens with navigation switched off, on the start page or while a file
/// picker is open, or while the reading is inside the dead zone (after calibration, which takes
/// off the device's own rest offset), so a cap resting just off zero doesn't keep redrawing.
/// Returns whether there was motion to hand over; it then asks for the next frame itself. The
/// reading goes through the device's saved calibration first, when it has one; while the
/// calibration window is open, the reading goes to it instead and the view stays put.
pub fn navigate_frame(app: &mut SolveApp, ctx: &egui::Context, mapping: DriverMapping, axes: Axes, seconds: f64) -> bool {
    let changed = app.space_mouse.raw != axes;
    app.space_mouse.raw = axes;
    app.space_mouse.motion = None;
    app.space_mouse.mapping = Some(mapping);
    if app.space_mouse.read_device != app.space_mouse.device {
        // Another device: its buttons and its resting hand are not the last one's.
        app.space_mouse.read_device = app.space_mouse.device.clone();
        app.space_mouse.seen = 0;
        app.space_mouse.settle = Default::default();
    }
    let calibration = calibration(app);
    if app.space_mouse.settled_for != calibration {
        app.space_mouse.settled_for = calibration;
        app.space_mouse.settle = Default::default();
    }
    if let Some((device, wizard)) = &mut app.space_mouse.calibrating {
        if app.space_mouse.device.as_ref() == Some(device) {
            wizard.feed_reading(axes.into(), seconds, !app.space_mouse.retained);
            ctx.request_repaint_after(calibration_repaint_after(changed, ctx.input(|i| i.focused)));
            return false;
        }
        // Another device is being read now, or none: what was recorded isn't its.
        app.space_mouse.calibrating = None;
    }
    let axes = calibrated(axes, calibration);
    // A hand leaning on a cap, holding it just past the dead zone, doesn't creep the view. A
    // trackball's slow roll is steady and meant, and a device without a calibration is read as it reports.
    let axes: Axes = match calibration {
        Some(calibration) if calibration.kind == solidcraft_space_mouse::Kind::ReturnToCenter => {
            app.space_mouse.settle.filter(axes.into(), seconds).into()
        }
        _ => axes,
    };
    // Then the user's choice of which motion drives which axis (invert and speed then go with the
    // axis, not the motion). After the settle: that works on the device's own calibrated axes.
    let axes = reassigned(axes, app.preferences.space_mouse_axis_source);
    // Back at rest: the next push picks what the view turns around afresh.
    if !axes.moving() {
        app.viewport.device_pivot = None;
    }
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
    let handed = DeviceMotion {
        motion: mapping.with_user_axes(app.preferences.space_mouse_reverse, app.preferences.space_mouse_axis_speed).apply(axes),
        seconds: step * speed.clamp(0.1, 5.0),
    };
    app.space_mouse.motion = Some(handed);
    app.viewport.device_motion = Some(handed);
    ctx.request_repaint_after(Duration::from_millis(16));
    true
}

/// How far one message of a device app may move the view along one of its motions (points, or
/// zoom steps), and how far a frame may sum to: a flood of large values can't throw the camera out.
pub const BRIDGE_MAX_STEP: f64 = 10_000.0;

/// The standard views a device app may ask for over the navigation bridge, by the names
/// `SolveApp::animate_view` takes ("fit" frames everything, as Fit does).
pub const BRIDGE_VIEWS: [&str; 8] = ["front", "back", "left", "right", "top", "bottom", "iso", "fit"];

/// What a device app (a homemade 3D input device's own program, read by the desktop app through
/// its navigation bridge) moved the view by since the last frame. Unlike a 3D mouse's deflection,
/// these are distances: orbit and pan as a pointer drag of that many points would move the view
/// (the model follows), and zoom in steps (positive zooms in, as a wheel turned forward).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BridgeMotion {
    pub orbit: [f64; 2],
    pub pan: [f64; 2],
    pub zoom: f64,
}

impl BridgeMotion {
    /// Nothing to move.
    pub fn is_still(self) -> bool {
        self == Self::default()
    }

    /// Both motions summed, each within [`BRIDGE_MAX_STEP`]; a value that isn't a finite number
    /// counts as none.
    pub fn plus(self, other: Self) -> Self {
        let sum = |a: f64, b: f64| (bridge_step(a) + bridge_step(b)).clamp(-BRIDGE_MAX_STEP, BRIDGE_MAX_STEP);
        Self {
            orbit: [sum(self.orbit[0], other.orbit[0]), sum(self.orbit[1], other.orbit[1])],
            pan: [sum(self.pan[0], other.pan[0]), sum(self.pan[1], other.pan[1])],
            zoom: sum(self.zoom, other.zoom),
        }
    }
}

/// One value of a device app's message: finite and within [`BRIDGE_MAX_STEP`], else none.
pub fn bridge_step(value: f64) -> f64 {
    if value.is_finite() { value.clamp(-BRIDGE_MAX_STEP, BRIDGE_MAX_STEP) } else { 0.0 }
}

/// Hands one frame of a device app's navigation to the page showing the camera, and shows the
/// standard `views` it asked for (names outside [`BRIDGE_VIEWS`] are ignored). As with a 3D mouse,
/// nothing happens with the bridge switched off, on the start page or while a file picker is open,
/// or in a window without focus. Returns whether there was anything to hand over.
pub fn bridge_frame(app: &mut SolveApp, ctx: &egui::Context, motion: BridgeMotion, views: &[String]) -> bool {
    if !app.preferences.nav_bridge_enabled || app.home.open || app.file_picker_busy() || !ctx.input(|i| i.focused) {
        return false;
    }
    let motion = BridgeMotion::default().plus(motion);
    let moved = !motion.is_still();
    if moved {
        app.begin_external_navigation();
        app.viewport.bridge_motion = Some(app.viewport.bridge_motion.unwrap_or_default().plus(motion));
        ctx.request_repaint();
    }
    // After the motion, which would stop the view's animation.
    let mut viewed = false;
    for view in views.iter().filter_map(|name| BRIDGE_VIEWS.iter().find(|v| **v == name.as_str())) {
        if *view == "fit" {
            app.animate_fit_all();
        } else {
            app.animate_view(view);
        }
        viewed = true;
    }
    moved || viewed
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
    fn navigate(cam: &mut Camera, motion: Motion, seconds: f64, height: f64, sketch: bool) {
        crate::navigation::device_camera(cam, motion, seconds, height, !sketch, false, None, None);
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

    /// The calibration window is told whether a reading is a report the device still sends or a value a
    /// daemon keeps: a ball that slows to a lower speed and keeps sending it is rolling (fresh), while the
    /// same readings as a kept value end the step.
    #[test]
    fn the_calibration_window_is_told_whether_the_reading_is_fresh() {
        use solidcraft_space_mouse::{Kind, Stage, Wizard};
        for (retained, step_done) in [(false, false), (true, true)] {
            let mut app = app();
            app.space_mouse.device = Some("hid 046d:c62b".into());
            app.space_mouse.retained = retained;
            app.space_mouse.calibrating = Some(("hid 046d:c62b".into(), Wizard::new(Kind::Trackball)));
            let ctx = egui::Context::default();
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            let mut feed = |axes: Axes, frames: usize| {
                for _ in 0..frames {
                    frame(&mut app, &ctx, DriverMapping::default(), axes, 0.02);
                }
            };
            feed(Axes::default(), 400);
            feed(Axes { translation: [40, 3, 0], rotation: [0; 3] }, 30);
            feed(Axes { translation: [10, 0, 0], rotation: [0; 3] }, 35);
            let stage = app.space_mouse.calibrating.as_ref().map(|(_, wizard)| wizard.stage());
            assert_eq!(stage == Some(Stage::Step(1)), step_done, "retained {retained}: {stage:?}");
        }
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
    fn the_calibration_window_repaints_fast_only_while_the_reading_moves() {
        use std::time::Duration;
        assert_eq!(calibration_repaint_after(true, true), Duration::from_millis(33));
        assert_eq!(calibration_repaint_after(false, true), Duration::from_millis(100), "a still reading: 10 frames a second");
        assert_eq!(calibration_repaint_after(true, false), Duration::from_millis(250));
        assert_eq!(calibration_repaint_after(false, false), Duration::from_millis(250));
    }

    #[test]
    fn a_measured_device_starts_with_its_printed_keys() {
        let jobs = default_button_jobs("hid 046d:c62b");
        assert_eq!(jobs.get(&1), Some(&ButtonJob::Settings));
        assert_eq!(jobs.get(&3), Some(&ButtonJob::View("top".into())));
        assert_eq!(jobs.get(&25), Some(&ButtonJob::Key(ButtonKey::Shift)));
        // Roll and the rotation lock have nothing to do here yet; the numbered keys start empty.
        assert_eq!((jobs.get(&9), jobs.get(&27), jobs.get(&13)), (None, None, None));
        // Unmeasured, spacenavd (its own numbers) and nonsense keys start empty.
        for device in ["hid 256f:c63a", "spacenavd", "web 046d", "hid zz:c62b", ""] {
            assert!(default_button_jobs(device).is_empty(), "{device}");
        }
    }

    /// One frame of the window: the hook sees the frame's input, then egui runs it, as the app does.
    fn press(app: &mut SolveApp, ctx: &egui::Context, buttons: u32, events: Vec<egui::Event>) -> egui::RawInput {
        let mut raw = egui::RawInput { focused: true, events, ..Default::default() };
        let device = app.space_mouse.device.clone();
        // As the reader reports them: a button newly down since its own last look is a tap, and a reader
        // that has just connected to this device starts from nothing down.
        let before = if app.space_mouse.buttons_device == device { app.space_mouse.buttons } else { 0 };
        buttons_hook(app, ctx, Buttons { device, down: buttons, taps: buttons & !before, pending: false, generation: 0 }, &mut raw);
        ctx.run_ui(raw.clone(), |_| {}).textures_delta.clear();
        raw
    }

    fn escapes(raw: &egui::RawInput) -> usize {
        raw.events.iter().filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. })).count()
    }

    /// One frame with the reader's own snapshot (`taps` included).
    fn press_seen(app: &mut SolveApp, ctx: &egui::Context, seen: Buttons) -> egui::RawInput {
        let mut raw = egui::RawInput { focused: true, ..Default::default() };
        buttons_hook(app, ctx, seen, &mut raw);
        ctx.run_ui(raw.clone(), |_| {}).textures_delta.clear();
        raw
    }

    const ESC: u32 = 1 << 22;
    const SHIFT: u32 = 1 << 24;

    #[test]
    fn a_button_does_its_job_once_per_press_and_holds_its_modifier() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        let ctx = window(true);
        let shifted = || ctx.input(|i| i.modifiers.shift);
        // Esc (button 23) presses Escape once, not again while it's held.
        assert_eq!(escapes(&press(&mut app, &ctx, ESC, vec![])), 1);
        assert_eq!(escapes(&press(&mut app, &ctx, ESC, vec![])), 0);
        // Shift (button 25) holds Shift for as long as it's down.
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(shifted());
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(shifted());
        press(&mut app, &ctx, 0, vec![]);
        assert!(!shifted());
        // Menu (button 1) opens Preferences; there, buttons only light up their rows.
        press(&mut app, &ctx, 1, vec![]);
        assert!(app.prefs_window.open);
        press(&mut app, &ctx, 0, vec![]);
        assert_eq!(escapes(&press(&mut app, &ctx, ESC, vec![])), 0);
        assert_eq!(app.space_mouse.seen & ESC, ESC);
        // A job the user set replaces the defaults.
        app.prefs_window.open = false;
        press(&mut app, &ctx, 0, vec![]);
        app.preferences.space_mouse_buttons.insert("hid 046d:c62b".into(), [(23, ButtonJob::Key(ButtonKey::Alt))].into());
        let raw = press(&mut app, &ctx, ESC, vec![]);
        assert_eq!((escapes(&raw), ctx.input(|i| i.modifiers.alt)), (0, true));
    }

    #[test]
    fn a_button_set_to_perspective_view_switches_it_once_per_press() {
        let device = "hid 046d:c62b";
        let mut app = app();
        app.space_mouse.device = Some(device.into());
        app.preferences.space_mouse_buttons.insert(device.into(), [(2, ButtonJob::Command("view.perspective".into()))].into());
        let ctx = window(true);
        let before = app.ui.perspective;
        press(&mut app, &ctx, 0b10, vec![]);
        assert_eq!(app.ui.perspective, !before);
        // Held, it doesn't switch again; the next press switches back.
        press(&mut app, &ctx, 0b10, vec![]);
        assert_eq!(app.ui.perspective, !before);
        press(&mut app, &ctx, 0, vec![]);
        press(&mut app, &ctx, 0b10, vec![]);
        assert_eq!(app.ui.perspective, before);
        assert!(app.session.undo.is_empty(), "a view setting, not an edit");
    }

    /// The keyboard and a button hold the same modifier: it stays down until both let go, whichever
    /// lets go first, for each of Shift, Ctrl and Alt.
    #[test]
    fn mapped_buttons_do_nothing_behind_an_issue_report_draft() {
        let device = "hid 046d:c62b";
        let mut app = app();
        app.space_mouse.device = Some(device.into());
        app.preferences.space_mouse_buttons.insert(
            device.into(),
            [
                (1, ButtonJob::Settings),
                (2, ButtonJob::Key(ButtonKey::Esc)),
                (3, ButtonJob::Key(ButtonKey::Shift)),
                (4, ButtonJob::View("top".into())),
                (5, ButtonJob::Command("edit.undo".into())),
            ]
            .into(),
        );
        let ctx = window(true);
        // A document change that Undo would take back.
        app.run("document.metadata.set", serde_json::json!({"values": {"author": "Jane"}})).unwrap();
        app.issue_report.draft = Some(crate::issue_report::draft_for(&app, None, None));
        let (cam, undo, dialog) = (app.cam, app.session.undo.len(), app.dialog.is_some());
        assert!(undo > 0 && app.session.doc.metadata.author() == Some("Jane"));
        let raw = press(&mut app, &ctx, 0b11111, vec![]);
        assert_eq!(app.session.doc.metadata.author(), Some("Jane"), "the mapped Undo did not run");
        assert_eq!(escapes(&raw), 0, "no Esc");
        assert!(!app.prefs_window.open, "no Preferences");
        assert!(!ctx.input(|i| i.modifiers.shift), "no held Shift");
        assert_eq!((app.cam, app.session.undo.len(), app.dialog.is_some()), (cam, undo, dialog));
        // Once the draft is closed the same buttons work again (a fresh press).
        app.issue_report.draft = None;
        press(&mut app, &ctx, 0, vec![]);
        press(&mut app, &ctx, 0b11111, vec![]);
        assert!(app.prefs_window.open);
        assert_eq!(app.session.doc.metadata.author(), None, "and the mapped Undo runs");
    }

    #[test]
    fn an_esc_button_pressed_while_a_shift_button_is_held_goes_out_shifted() {
        let device = "hid 046d:c62b";
        let mut app = app();
        app.space_mouse.device = Some(device.into());
        app.preferences
            .space_mouse_buttons
            .insert(device.into(), [(1, ButtonJob::Key(ButtonKey::Shift)), (2, ButtonJob::Key(ButtonKey::Esc))].into());
        let ctx = window(true);
        press(&mut app, &ctx, 0b01, vec![]);
        let raw = press(&mut app, &ctx, 0b11, vec![]);
        let shifted =
            raw.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. } if modifiers.shift));
        assert!(shifted, "{:?}", raw.events);
    }

    #[test]
    fn a_tap_between_two_frames_runs_its_job_once() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        let ctx = window(true);
        // Down and up before the hook looked: nothing is held, the press is in `taps`.
        let tap = Buttons { device: Some("hid 046d:c62b".into()), down: 0, taps: ESC, pending: false, generation: 0 };
        assert_eq!(escapes(&press_seen(&mut app, &ctx, tap)), 1);
        assert_eq!(app.space_mouse.buttons, 0);
        let nothing = Buttons { device: Some("hid 046d:c62b".into()), down: 0, taps: 0, pending: false, generation: 0 };
        assert_eq!(escapes(&press_seen(&mut app, &ctx, nothing)), 0, "and not again");
    }

    #[test]
    fn a_press_on_a_new_device_runs_that_devices_mapping_whatever_the_frame_before_had() {
        let (a, b) = ("hid 046d:c62b", "hid 256f:c63a");
        let device = |name: Option<&str>, down: u32| Buttons { device: name.map(Into::into), down, taps: 0, pending: false, generation: 0 };
        let setup = || {
            let mut app = app();
            app.preferences.space_mouse_buttons.insert(a.into(), [(1, ButtonJob::Key(ButtonKey::Shift))].into());
            app.preferences.space_mouse_buttons.insert(b.into(), [(1, ButtonJob::Key(ButtonKey::Esc))].into());
            (app, window(true))
        };
        // A -> B with button 1 still down from before: A's Shift lets go, and the held button is not a
        // press on B. A fresh press on B (it arrives as a tap) runs B's Esc once.
        let (mut app, ctx) = setup();
        app.space_mouse.device = Some(a.into());
        press_seen(&mut app, &ctx, device(Some(a), 1));
        assert!(app.space_mouse.held.shift);
        assert_eq!(escapes(&press_seen(&mut app, &ctx, device(Some(b), 1))), 0, "held through the switch is not a press");
        assert!(!app.space_mouse.held.shift, "A's hold does not carry over");
        assert_eq!(escapes(&press_seen(&mut app, &ctx, device(Some(b), 1))), 0, "and holding is not pressing");
        press_seen(&mut app, &ctx, device(Some(b), 0));
        let tap = Buttons { device: Some(b.into()), down: 1, taps: 1, pending: false, generation: 0 };
        assert_eq!(escapes(&press_seen(&mut app, &ctx, tap.clone())), 1, "a press on B runs B's job");
        // None -> B: the first press is not swallowed by there having been no device (it is a tap).
        let (mut app, ctx) = setup();
        app.space_mouse.device = None;
        press_seen(&mut app, &ctx, device(None, 0));
        assert_eq!(escapes(&press_seen(&mut app, &ctx, tap)), 1);
        // The device appears (reader connected, no state yet), a frame is drawn, and only then the first
        // report shows button 1 still down: held, not pressed.
        let (mut app, ctx) = setup();
        app.space_mouse.device = None;
        press_seen(&mut app, &ctx, device(None, 0));
        let waiting = Buttons { device: Some(b.into()), down: 0, taps: 0, pending: true, generation: 0 };
        assert_eq!(escapes(&press_seen(&mut app, &ctx, waiting)), 0);
        assert_eq!(escapes(&press_seen(&mut app, &ctx, device(Some(b), 1))), 0, "the first state is a baseline");
        press_seen(&mut app, &ctx, device(Some(b), 0));
        assert_eq!(escapes(&press_seen(&mut app, &ctx, Buttons { device: Some(b.into()), down: 1, taps: 1, pending: false, generation: 0 })), 1);
        // And a button already down when the first device appears is not a press.
        let (mut app, ctx) = setup();
        press_seen(&mut app, &ctx, device(None, 0));
        assert_eq!(escapes(&press_seen(&mut app, &ctx, device(Some(b), 1))), 0);
    }

    #[test]
    fn a_modifier_the_keyboard_and_a_button_share_stays_down_until_both_let_go() {
        for key in [ButtonKey::Shift, ButtonKey::Ctrl, ButtonKey::Alt] {
            let down = |ctx: &egui::Context| {
                let m = ctx.input(|i| i.modifiers);
                match key {
                    ButtonKey::Shift => m.shift,
                    ButtonKey::Ctrl => m.ctrl,
                    _ => m.alt,
                }
            };
            let keyboard = |on: bool| {
                let mut m = egui::Modifiers::NONE;
                match key {
                    ButtonKey::Shift => m.shift = on,
                    ButtonKey::Ctrl => m.ctrl = on,
                    _ => m.alt = on,
                }
                vec![egui::Event::ModifiersChanged(m)]
            };
            for button_first in [true, false] {
                let mut app = app();
                app.space_mouse.device = Some("hid 046d:c62b".into());
                app.preferences.space_mouse_buttons.insert("hid 046d:c62b".into(), [(1, ButtonJob::Key(key))].into());
                let ctx = window(true);
                press(&mut app, &ctx, 0, keyboard(true));
                press(&mut app, &ctx, 1, vec![]);
                assert!(down(&ctx), "{key:?} both");
                if button_first {
                    press(&mut app, &ctx, 0, vec![]);
                    assert!(down(&ctx), "{key:?} the keyboard still holds it");
                    press(&mut app, &ctx, 0, keyboard(false));
                } else {
                    press(&mut app, &ctx, 1, keyboard(false));
                    assert!(down(&ctx), "{key:?} the button still holds it");
                    press(&mut app, &ctx, 0, vec![]);
                }
                assert!(!down(&ctx), "{key:?} neither");
            }
        }
    }

    #[test]
    fn buttons_do_nothing_where_navigation_would_not() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        app.preferences
            .space_mouse_buttons
            .insert("hid 046d:c62b".into(), [(1, ButtonJob::Key(ButtonKey::Esc)), (2, ButtonJob::Key(ButtonKey::Shift))].into());
        let (focused, unfocused) = (window(true), window(false));
        // A press while the window is in the background does nothing, and isn't run on refocus.
        assert_eq!(escapes(&press(&mut app, &unfocused, 1, vec![])), 0);
        assert_eq!(escapes(&press(&mut app, &focused, 1, vec![])), 0, "still held: not a new press");
        press(&mut app, &focused, 0, vec![]);
        assert_eq!(escapes(&press(&mut app, &focused, 1, vec![])), 1, "a press that is its own");
        // A held modifier button lets go in the background, and takes hold again on refocus.
        press(&mut app, &focused, 2, vec![]);
        assert!(focused.input(|i| i.modifiers.shift));
        press(&mut app, &unfocused, 2, vec![]);
        assert!(!unfocused.input(|i| i.modifiers.shift));
        // A file picker being open is no place for one either.
        press(&mut app, &focused, 0, vec![]);
        let slot: crate::picker_test_support::Slot = Default::default();
        crate::picker_test_support::open(&mut app, &slot);
        app.pick_open(|_, _| {});
        assert_eq!(escapes(&press(&mut app, &focused, 1, vec![])), 0);
        app.file_picker = None;
        press(&mut app, &focused, 0, vec![]);
        assert_eq!(escapes(&press(&mut app, &focused, 1, vec![])), 1);
    }

    #[test]
    fn preferences_and_calibration_let_go_of_a_held_modifier_button() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        let ctx = window(true);
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(ctx.input(|i| i.modifiers.shift));
        app.prefs_window.open = true;
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(!ctx.input(|i| i.modifiers.shift), "Preferences open");
        app.prefs_window.open = false;
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(ctx.input(|i| i.modifiers.shift));
        crate::space_mouse_calibration::start(&mut app);
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(!ctx.input(|i| i.modifiers.shift), "calibrating");
    }

    #[test]
    fn losing_focus_clears_what_the_buttons_hold_and_the_keyboard_is_remembered_again_on_regain() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        let ctx = window(true);
        press(&mut app, &ctx, SHIFT, vec![]);
        assert!(app.space_mouse.held.shift);
        press(&mut app, &ctx, SHIFT, vec![egui::Event::WindowFocused(false)]);
        assert_eq!(app.space_mouse.held, egui::Modifiers::NONE);
        assert!(!ctx.input(|i| i.modifiers.shift));
        // Back, the button still down: it holds Shift again, and a keyboard Shift lost with the
        // focus stays lost until the window says it is down.
        press(&mut app, &ctx, SHIFT, vec![egui::Event::WindowFocused(true)]);
        assert!(ctx.input(|i| i.modifiers.shift));
        assert_eq!(escapes(&press(&mut app, &ctx, SHIFT, vec![])), 0);
    }

    #[test]
    fn wheel_events_carry_the_held_modifiers() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        let ctx = window(true);
        let raw = press(&mut app, &ctx, SHIFT, vec![wheel(1.0)]);
        assert!(raw.events.iter().any(|e| matches!(e, egui::Event::MouseWheel { modifiers, .. } if modifiers.shift)));
    }

    /// What the cap turns the view around is chosen when it starts turning and let go when it is
    /// back at rest.
    #[test]
    fn the_cap_lets_go_of_its_turning_point_at_rest() {
        let mut app = app();
        app.preferences.space_mouse_model_pivot = true;
        let ctx = window(true);
        app.space_mouse.device = Some("hid 046d:c62b".into());
        frame(&mut app, &ctx, DriverMapping::default(), Axes { translation: [300, 0, 0], rotation: [0; 3] }, 0.016);
        assert_eq!(app.viewport.device_pivot, None, "a pan alone turns around nothing");
        frame(&mut app, &ctx, DriverMapping::default(), Axes { translation: [300, 0, 0], rotation: [300, 0, 0] }, 0.016);
        assert!(app.viewport.device_pivot.is_some(), "a turn holds a point");
        frame(&mut app, &ctx, DriverMapping::default(), Axes::default(), 0.016);
        assert_eq!(app.viewport.device_pivot, None, "at rest it is let go");
    }

    #[test]
    fn another_device_starts_with_no_buttons_seen_and_no_settling() {
        let mut app = app();
        let ctx = window(true);
        app.space_mouse.device = Some("hid 046d:c62b".into());
        frame(&mut app, &ctx, DriverMapping::default(), Axes::default(), 0.016);
        app.space_mouse.seen = 0b101;
        frame(&mut app, &ctx, DriverMapping::default(), Axes::default(), 0.016);
        assert_eq!(app.space_mouse.seen, 0b101);
        app.space_mouse.device = Some("hid 256f:c63a".into());
        frame(&mut app, &ctx, DriverMapping::default(), Axes::default(), 0.016);
        assert_eq!(app.space_mouse.seen, 0);
    }

    #[test]
    fn native_camera_obeys_focus_enable_and_sketch_locks() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput { focused: true, ..Default::default() }, |_| {});
        let mut app = app();
        let before = app.cam;
        let next = solidcraft_engine::render::Camera { yaw: before.yaw + 0.5, distance: before.distance * 0.8, ..before };
        assert!(navigate_camera_frame(&mut app, &ctx, next));
        // The driver queues; the active page owns application of its capabilities.
        assert_eq!(app.cam, before);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        crate::navigation::device(&mut app, rect, crate::navigation::NavCaps { orbit: false, pick: false });
        assert_eq!(app.cam.yaw, before.yaw);
        assert_eq!(app.cam.distance, next.distance);
        assert!(app.viewport.device_camera.is_none());
        app.preferences.space_mouse_enabled = false;
        assert!(!navigate_camera_frame(&mut app, &ctx, next));
        assert!(app.viewport.device_camera.is_none());
        app.preferences.space_mouse_enabled = true;
        let _ = ctx.run_ui(egui::RawInput { focused: false, ..Default::default() }, |_| {});
        assert!(!navigate_camera_frame(&mut app, &ctx, next));
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
        navigate(&mut cam, raw(Axes { translation: [350, 350, 350], rotation: [350, 350, 350] }), 0.02, 600.0, false);
        assert_ne!(cam.target, before.target);
        assert!(cam.distance < before.distance);
        assert_ne!(cam.yaw, before.yaw);
        assert_ne!(cam.pitch, before.pitch);
        assert_ne!(cam.roll, before.roll);
        let orientation = (cam.yaw, cam.pitch, cam.roll);
        navigate(&mut cam, raw(Axes { translation: [350, 0, 0], rotation: [350, 350, 350] }), 0.02, 600.0, true);
        assert_eq!((cam.yaw, cam.pitch, cam.roll), orientation);
        assert!(cam.is_valid());
    }

    #[test]
    fn individual_axes_have_physical_spacemouse_meaning() {
        let before = Camera::default();

        let mut slide = before;
        navigate(&mut slide, raw(Axes { translation: [350, 0, 0], rotation: [0; 3] }), 0.02, 600.0, false);
        assert_ne!(slide.target, before.target);
        assert_eq!(slide.distance, before.distance);

        let mut push = before;
        navigate(&mut push, raw(Axes { translation: [0, 350, 0], rotation: [0; 3] }), 0.02, 600.0, false);
        assert_eq!(push.target, before.target);
        assert!(push.distance < before.distance);

        let mut lift = before;
        navigate(&mut lift, raw(Axes { translation: [0, 0, 350], rotation: [0; 3] }), 0.02, 600.0, false);
        assert_ne!(lift.target, before.target);
        assert_eq!(lift.distance, before.distance);

        let mut tilt = before;
        navigate(&mut tilt, raw(Axes { translation: [0; 3], rotation: [350, 0, 0] }), 0.02, 600.0, false);
        assert_eq!((tilt.yaw, tilt.roll), (before.yaw, before.roll));
        assert_ne!(tilt.pitch, before.pitch);

        let mut roll = before;
        navigate(&mut roll, raw(Axes { translation: [0; 3], rotation: [0, 350, 0] }), 0.02, 600.0, false);
        assert_eq!((roll.pitch, roll.roll), (before.pitch, before.roll));
        assert_ne!(roll.yaw, before.yaw);

        let mut twist = before;
        navigate(&mut twist, raw(Axes { translation: [0; 3], rotation: [0, 0, 350] }), 0.02, 600.0, false);
        assert_eq!((twist.yaw, twist.pitch), (before.yaw, before.pitch));
        assert_ne!(twist.roll, before.roll);
    }

    /// Reverse zoom direction is a wheel and trackpad setting: it turns the wheel's zoom around
    /// and leaves the cap's, which has its own invert switch (#8993).
    #[test]
    fn reverse_zoom_setting_flips_the_wheel_and_leaves_the_cap() {
        let wheel_zooms_in = |reverse: bool| {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.zoom_reverse = reverse;
            h.app.preferences.smooth_wheel_zoom = false;
            h.call("ui.move", serde_json::json!({"x":800,"y":500}));
            let before = h.app.cam.half_height();
            h.call("ui.scroll", serde_json::json!({"x":800,"y":500,"delta":120}));
            h.app.cam.half_height() < before
        };
        assert!(wheel_zooms_in(false));
        assert!(!wheel_zooms_in(true), "the wheel turns around");
        // The cap's zoom axis pushed forward with the wheel reversed or not, and with the cap's own
        // invert switch for that axis on or off.
        let cap_distance = |wheel_reversed: bool, cap_inverted: bool| {
            let mut app = app();
            app.preferences.zoom_reverse = wheel_reversed;
            app.preferences.space_mouse_reverse[1] = cap_inverted;
            let ctx = window(true);
            app.space_mouse.device = Some("hid 046d:c62b".into());
            let before = app.cam.distance;
            frame(&mut app, &ctx, DriverMapping::default(), Axes { translation: [0, 350, 0], rotation: [0; 3] }, 0.02);
            (before, app.cam.distance)
        };
        let (before, normal) = cap_distance(false, false);
        assert!(normal < before, "pushing the cap forward zooms in");
        assert_eq!(cap_distance(true, false).1, normal, "the cap zooms the same way with the wheel reversed");
        assert!(cap_distance(false, true).1 > before, "the cap's own invert switch turns it around");
        assert_eq!(cap_distance(true, true).1, cap_distance(false, true).1);
    }

    #[test]
    fn object_mode_tilts_roll_and_twists_spin() {
        let before = Camera::default();
        let mut tilt = before;
        navigate(&mut tilt, DriverMapping::object_mode().apply(Axes { translation: [0; 3], rotation: [0, 350, 0] }), 0.02, 600.0, false);
        assert_eq!((tilt.yaw, tilt.pitch), (before.yaw, before.pitch));
        assert_ne!(tilt.roll, before.roll);
        let mut twist = before;
        navigate(&mut twist, DriverMapping::object_mode().apply(Axes { translation: [0; 3], rotation: [0, 0, 350] }), 0.02, 600.0, false);
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

    /// A steady lean of 60 on X for two seconds, 20 ms a frame: whether the cap still moves the view at the end.
    fn still_moving_after_a_steady_lean(calibration: Option<Calibration>) -> bool {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        if let Some(calibration) = calibration {
            app.preferences.space_mouse_calibrations.insert("hid 046d:c62b".into(), calibration);
        }
        let ctx = window(true);
        let lean = Axes { translation: [60, 0, 0], rotation: [0; 3] };
        (0..100).map(|_| frame(&mut app, &ctx, DriverMapping::default(), lean, 0.02)).last().unwrap()
    }

    #[test]
    fn only_a_calibrated_cap_settles() {
        let axes = std::array::from_fn(|source| AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] });
        let calibration = |kind| Calibration { dead_zone_share: 0.0, kind, axes };
        assert!(!still_moving_after_a_steady_lean(Some(calibration(solidcraft_space_mouse::Kind::ReturnToCenter))), "a leaning hand on a cap");
        assert!(still_moving_after_a_steady_lean(Some(calibration(solidcraft_space_mouse::Kind::Trackball))), "a slow steady roll");
        assert!(still_moving_after_a_steady_lean(None), "a device read as it reports");
    }

    #[test]
    fn a_new_calibration_starts_settling_afresh() {
        let mut app = app();
        app.space_mouse.device = Some("hid 046d:c62b".into());
        let ctx = window(true);
        let axes = std::array::from_fn(|source| AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] });
        app.preferences
            .space_mouse_calibrations
            .insert("hid 046d:c62b".into(), Calibration { dead_zone_share: 0.0, kind: solidcraft_space_mouse::Kind::ReturnToCenter, axes });
        let lean = Axes { translation: [60, 0, 0], rotation: [0; 3] };
        for _ in 0..30 {
            frame(&mut app, &ctx, DriverMapping::default(), lean, 0.02);
        }
        assert!(!frame(&mut app, &ctx, DriverMapping::default(), lean, 0.02), "settled");
        app.preferences.space_mouse_calibrations.get_mut("hid 046d:c62b").unwrap().dead_zone_share = 0.01;
        assert!(frame(&mut app, &ctx, DriverMapping::default(), lean, 0.02), "a changed calibration is a fresh start");
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
            kind: Default::default(),
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
        // It asks what kind of device it is first.
        let Some((_, wizard)) = &mut app.space_mouse.calibrating else { panic!("calibration didn't start") };
        assert_eq!(wizard.stage(), solidcraft_space_mouse::Stage::Choose);
        wizard.choose(solidcraft_space_mouse::Kind::ReturnToCenter);
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
        // A press at full travel, where the calibration's feather no longer shows, so the comparison is exact.
        let mixed = Axes { translation: [0, 0, 3500], rotation: [0; 3] };
        let expected = Axes { translation: [0, 350, 0], rotation: [0; 3] };
        let mut reference = app();
        frame(&mut reference, &ctx, DriverMapping::default(), expected, 0.02);
        let mut swapped = setup([0, 2, 1, 3, 4, 5]);
        assert!(frame(&mut swapped, &ctx, DriverMapping::default(), mixed, 0.02));
        assert_eq!(swapped.cam, reference.cam, "calibrated first, then the motions swapped, once");
        // Saved and loaded, it's the same choice; reset, the device reads as calibrated.
        let saved = serde_json::to_value(&swapped.preferences).unwrap();
        assert_eq!(crate::prefs::lenient(&saved).space_mouse_axis_source, [0, 2, 1, 3, 4, 5]);
        let mut unswapped_reference = app();
        frame(&mut unswapped_reference, &ctx, DriverMapping::default(), Axes { translation: [0, 0, 350], rotation: [0; 3] }, 0.02);
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
        assert_eq!(row_label(Some(off), 2), "ui.3d_mouse_view_off");
        // Without a mapping, rows claim no view effect and name none of the dropdown's motions.
        assert_eq!(labels(None), ["ui.3d_mouse_view_unmapped"; 6]);
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
        // The calibration asks what kind of device it is first.
        app.space_mouse.calibrating.as_mut().unwrap().1.choose(solidcraft_space_mouse::Kind::ReturnToCenter);
        let feed = |app: &mut SolveApp, axes: Axes, seconds: f64| {
            for _ in 0..(seconds / 0.02).round() as usize {
                frame(app, &ctx, DriverMapping::default(), axes, 0.02);
            }
        };
        feed(&mut app, Axes::default(), 1.6);
        // And a hand resting on the cap, before the steps.
        feed(&mut app, Axes::default(), 2.6);
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

    /// One frame of a device app's navigation, taken by the modelling viewport.
    fn bridge(app: &mut SolveApp, ctx: &egui::Context, motion: BridgeMotion, views: &[&str]) -> bool {
        let views: Vec<String> = views.iter().map(|v| v.to_string()).collect();
        let handed = bridge_frame(app, ctx, motion, &views);
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        crate::navigation::device(app, page, crate::navigation::NavCaps::MODEL);
        handed
    }

    const ORBIT: BridgeMotion = BridgeMotion { orbit: [40.0, 10.0], pan: [0.0; 2], zoom: 0.0 };

    #[test]
    fn a_device_app_moves_the_view_only_with_the_bridge_on_and_the_window_focused() {
        let ctx = window(true);
        let mut off = app();
        let before = off.cam;
        assert!(!bridge(&mut off, &ctx, ORBIT, &[]), "the bridge is off by default");
        assert_eq!(off.cam, before);
        let on = || {
            let mut app = app();
            app.preferences.nav_bridge_enabled = true;
            app
        };
        let mut home = on();
        home.home.open = true;
        assert!(!bridge(&mut home, &ctx, ORBIT, &[]));
        let mut unfocused = on();
        assert!(!bridge(&mut unfocused, &window(false), ORBIT, &[]));
        assert_eq!((home.cam, unfocused.cam), (before, before));
        let mut app = on();
        assert!(bridge(&mut app, &ctx, ORBIT, &[]));
        assert_ne!((app.cam.yaw, app.cam.pitch), (before.yaw, before.pitch), "orbit turns the view");
        assert!(app.viewport.bridge_motion.is_none(), "the page took the motion");
        // At rest, nothing to hand over.
        assert!(!bridge(&mut app, &ctx, BridgeMotion::default(), &[]));
    }

    #[test]
    fn a_device_apps_motion_pans_and_zooms_as_a_drag_and_the_wheel_would() {
        let ctx = window(true);
        let mut app = app();
        app.preferences.nav_bridge_enabled = true;
        let before = app.cam;
        bridge(&mut app, &ctx, BridgeMotion { pan: [30.0, -20.0], ..Default::default() }, &[]);
        let mut dragged = before;
        dragged.pan(30.0, -20.0, 600.0);
        assert_eq!(app.cam, dragged, "a pan of so many points is a pan drag of as many");
        let panned = app.cam;
        bridge(&mut app, &ctx, BridgeMotion { zoom: 2.0, ..Default::default() }, &[]);
        assert!(app.cam.distance < panned.distance, "positive steps zoom in");
        bridge(&mut app, &ctx, BridgeMotion { zoom: -2.0, ..Default::default() }, &[]);
        assert!((app.cam.distance - panned.distance).abs() < 1e-9, "and back out to where it was");
        // Reverse zoom direction reverses it, as it does the wheel's.
        app.preferences.zoom_reverse = true;
        bridge(&mut app, &ctx, BridgeMotion { zoom: 2.0, ..Default::default() }, &[]);
        assert!(app.cam.distance > panned.distance);
    }

    #[test]
    fn a_device_app_asks_for_standard_views_by_name() {
        let ctx = window(true);
        let mut app = app();
        app.preferences.nav_bridge_enabled = true;
        app.preferences.look_at_anim = false;
        assert!(bridge(&mut app, &ctx, BridgeMotion::default(), &["top"]));
        let mut top = Camera::default();
        top.set_view(solidcraft_engine::render::StandardView::Top);
        let target = app.cam_anim.map_or(app.cam, |a| a.to);
        assert_eq!((target.yaw, target.pitch), (top.yaw, top.pitch));
        // Names it doesn't know are no view.
        let still = app.cam;
        assert!(!bridge(&mut app, &ctx, BridgeMotion::default(), &["TOP", "named view", ""]));
        assert_eq!(app.cam, still);
    }

    #[test]
    fn a_flood_of_device_app_motion_stays_within_range() {
        let ctx = window(true);
        let mut app = app();
        app.preferences.nav_bridge_enabled = true;
        let hostile = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, -f64::MAX, 1e12];
        for value in hostile {
            let motion = BridgeMotion { orbit: [value, -value], pan: [value, value], zoom: value };
            for _ in 0..3 {
                bridge(&mut app, &ctx, motion, &[]);
                assert!(app.cam.is_valid(), "{value}");
            }
        }
        let summed = (0..10).fold(BridgeMotion::default(), |sum, _| sum.plus(BridgeMotion { pan: [f64::MAX, 1.0], ..Default::default() }));
        assert_eq!(summed.pan, [BRIDGE_MAX_STEP, 10.0]);
        assert_eq!(BridgeMotion { zoom: f64::NAN, ..Default::default() }.plus(BridgeMotion::default()), BridgeMotion::default());
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
