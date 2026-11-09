//! The 3D mouse in the browser (#1336), through the Gamepad API.
//!
//! Chromium browsers list a 3Dconnexion device itself, with its six axes scaled to -1..1 from the
//! device's ±350. Firefox lists only joysticks and gamepads (a 3D mouse is a HID multi-axis
//! controller), but 3DxWare's virtual joystick carries the same six axes once 3DxWare's profile
//! for Firefox sends them there. Both come in the device's own order and signs, so the shared
//! navigation takes them with 3DxWare's object-mode mapping, as the Linux app does without a
//! profile.

use solidcraft_ui_egui::space_mouse::{Axes, Calibration, FULL_SCALE, calibrated};

/// Logitech's vendor id, which older 3Dconnexion devices carry, and those devices' product ids
/// (usb.ids). Logitech's own gamepads and wheels carry the same vendor id.
const LOGITECH: u16 = 0x046d;
const LOGITECH_3D_MICE: [u16; 11] = [0xc603, 0xc605, 0xc606, 0xc621, 0xc623, 0xc625, 0xc626, 0xc627, 0xc628, 0xc629, 0xc62b];
/// 3Dconnexion's own vendor id. The browser lists only its 3D mice as gamepads: its mice, and the
/// mouse side of its receivers, are not gamepads.
const THREEDCONNEXION: u16 = 0x256f;
/// 3DxWare's virtual joystick.
const VIRTUAL_JOYSTICK: (u16, u16) = (0xbeef, 0x046d);

/// What a gamepad the browser lists is, when it is a 3D mouse. A device is read before the
/// joystick: in a Chromium browser both are listed, and the joystick stays at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Device,
    VirtualJoystick,
}

/// The vendor and product ids in a gamepad's id: `Name (Vendor: 046d Product: c62b)` in Chromium
/// browsers, `046d-c62b-Name` in Firefox.
fn vendor_product(id: &str) -> Option<(u16, u16)> {
    fn hex(digits: &str) -> Option<u16> {
        if (1..=4).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_hexdigit()) { u16::from_str_radix(digits, 16).ok() } else { None }
    }
    if let Some((_, ids)) = id.rsplit_once("Vendor: ") {
        let (vendor, rest) = ids.split_once(" Product: ")?;
        let product = rest.split(|c: char| !c.is_ascii_hexdigit()).next()?;
        return Some((hex(vendor)?, hex(product)?));
    }
    let mut parts = id.splitn(3, '-');
    let (vendor, product, _name) = (parts.next()?, parts.next()?, parts.next()?);
    Some((hex(vendor)?, hex(product)?))
}

/// Whether a gamepad is a 3D mouse, from the id the browser gives it.
pub fn source(id: &str) -> Option<Source> {
    match vendor_product(id)? {
        VIRTUAL_JOYSTICK => Some(Source::VirtualJoystick),
        (THREEDCONNEXION, _) => Some(Source::Device),
        (LOGITECH, product) if LOGITECH_3D_MICE.contains(&product) => Some(Source::Device),
        _ => None,
    }
}

/// The six axes in the device's raw units, from a gamepad's axes (-1..1). Fewer than six: not a
/// 3D mouse. A value that isn't a number reads as rest.
pub fn axes(values: &[f64]) -> Option<Axes> {
    let raw = |i: usize| values.get(i).map(|&v| if v.is_finite() { (v.clamp(-1.0, 1.0) * f64::from(FULL_SCALE)).round() as i16 } else { 0 });
    Some(Axes { translation: [raw(0)?, raw(1)?, raw(2)?], rotation: [raw(3)?, raw(4)?, raw(5)?] })
}

/// What a calibration of the gamepad with this id is saved under: `web` and its vendor and
/// product ids.
pub fn device_key(id: &str) -> Option<String> {
    vendor_product(id).map(|(vendor, product)| format!("web {vendor:04x}:{product:04x}"))
}

/// The 3D mouse to read among the ones listed: the first device, else the first joystick.
pub fn choose<T>(found: impl IntoIterator<Item = (Source, T)>) -> Option<T> {
    found.into_iter().min_by_key(|(source, _)| *source).map(|(_, reading)| reading)
}

/// Whether the cap is moving as the next frame will see it: through the device's calibration
/// (as of the last frame) when it has one. The idle page wakes for a short-range device's push,
/// and not for a device resting off zero; the wheel filter goes by the same.
pub fn moving(axes: Axes, calibration: Option<Calibration>) -> bool {
    calibrated(axes, calibration).moving()
}

/// The calibration as of the last frame, and the device it was for.
pub type Cached = (Option<String>, Option<Calibration>);

/// Whether the cap of `device` is moving, judged with the calibration kept from the last frame:
/// `None` when that was for another device, so another's calibration never judges this one. The
/// waker then lets a frame look, which brings the cache up to date.
pub fn moving_as_cached(device: &Option<String>, axes: Axes, cached: &Cached) -> Option<bool> {
    (*device == cached.0).then(|| moving(axes, cached.1))
}

/// Whether the idle page's timer redraws: when the cap is moving as the last frame's calibration
/// judges it, when the device read isn't the one that calibration was for (a frame then judges
/// it with its own), and once when the device the last frame read is gone, so the page shows
/// that. With no device, and none last frame, it stays quiet.
pub fn wakes(reading: Option<(Option<String>, Axes)>, cached: &Cached) -> bool {
    match reading {
        Some((device, axes)) => moving_as_cached(&device, axes, cached).unwrap_or(true),
        None => cached.0.is_some(),
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::SpaceMouse;

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use solidcraft_ui_egui::SolveApp;
    use solidcraft_ui_egui::space_mouse::{Axes, DriverMapping, calibration, navigate_frame};
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::closure::Closure;

    use solidcraft_ui_egui::space_mouse::WheelGuard;

    use super::{Cached, axes, choose, device_key, moving_as_cached, source, wakes};

    /// How often the page looks for the cap starting to move while nothing else redraws.
    const WAKE_MS: i32 = 50;

    /// The 3D mouse the browser lists, if any: its calibration key and its axes.
    fn read() -> Option<(Option<String>, Axes)> {
        let pads = web_sys::window()?.navigator().get_gamepads().ok()?;
        choose(pads.iter().filter_map(|pad| {
            let pad = pad.dyn_into::<web_sys::Gamepad>().ok().filter(web_sys::Gamepad::connected)?;
            let source = source(&pad.id())?;
            let values: Vec<f64> = pad.axes().iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
            Some((source, (device_key(&pad.id()), axes(&values)?)))
        }))
    }

    /// Reads the 3D mouse once per frame and moves the view with it.
    #[derive(Default)]
    pub struct SpaceMouse {
        axes: Axes,
        /// The 3D mouse being read, as `SolveApp::space_mouse` keeps it.
        device: Option<String>,
        /// The previous frame's time and the step since it (seconds).
        last: Option<f64>,
        seconds: f64,
        wheel: WheelGuard,
        /// Listening for gamepads (from the first frame with navigation on).
        listening: bool,
        /// The timer that wakes the app when the cap starts moving runs (from when a 3D mouse
        /// is first listed).
        waking: bool,
        /// Navigation is on (the waker reads it).
        enabled: Rc<Cell<bool>>,
        /// The calibration as of the last frame and its device (the waker and the wheel filter read it).
        cached: Rc<RefCell<Cached>>,
    }

    impl SpaceMouse {
        /// At the start of a frame: reads the cap and drops 3DxWare's emulated wheel from the
        /// frame's input, before the app sees it.
        pub fn raw_input_hook(&mut self, ctx: &egui::Context, enabled: bool, raw: &mut egui::RawInput) {
            let now = raw.time.unwrap_or_else(|| js_sys::Date::now() / 1000.0);
            self.seconds = self.last.map_or(0.0, |last| now - last);
            self.last = Some(now);
            self.enabled.set(enabled);
            self.axes = Axes::default();
            if !enabled {
                return;
            }
            if !self.listening {
                self.listening = true;
                listen(ctx);
            }
            let reading = read();
            if reading.is_some() && !self.waking {
                self.waking = wake(ctx, &self.enabled, &self.cached);
            }
            // As read: calibration needs the readings at rest too, and `navigate_frame` leaves a
            // cap resting inside the dead zone alone.
            (self.device, self.axes) = reading.unwrap_or_default();
            let moving = moving_as_cached(&self.device, self.axes, &self.cached.borrow()).unwrap_or_else(|| self.axes.moving());
            self.wheel.filter(moving, now, &mut raw.events);
        }

        pub fn logic(&self, app: &mut SolveApp, ctx: &egui::Context) {
            app.space_mouse.device.clone_from(&self.device);
            *self.cached.borrow_mut() = (self.device.clone(), calibration(app));
            navigate_frame(app, ctx, DriverMapping::object_mode(), self.axes, self.seconds);
        }
    }

    /// A 3D mouse is listed only once it has been moved on the page: redraw then, so the frame
    /// finds it without waiting for the pointer.
    fn listen(ctx: &egui::Context) {
        let Some(window) = web_sys::window() else { return };
        let ctx = ctx.clone();
        let on_connect = Closure::<dyn FnMut()>::new(move || ctx.request_repaint());
        let _ = window.add_event_listener_with_callback("gamepadconnected", on_connect.as_ref().unchecked_ref());
        // The listener lives as long as the page.
        on_connect.forget();
    }

    /// The browser sends no event when a gamepad moves, so a timer looks while the app is idle and
    /// redraws when the cap starts moving; the frames that follow keep going on their own.
    fn wake(ctx: &egui::Context, enabled: &Rc<Cell<bool>>, cached: &Rc<RefCell<Cached>>) -> bool {
        let Some(window) = web_sys::window() else { return false };
        let (ctx, enabled, cached) = (ctx.clone(), Rc::clone(enabled), Rc::clone(cached));
        let tick = Closure::<dyn FnMut()>::new(move || {
            if enabled.get() && wakes(read(), &cached.borrow()) {
                ctx.request_repaint();
            }
        });
        let started = window.set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), WAKE_MS).is_ok();
        // The timer lives as long as the page.
        tick.forget();
        started
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_3d_mice_in_each_browsers_ids() {
        // As Edge and Firefox list them (Windows 11, SpaceMouse Pro, 3DxWare 10).
        assert_eq!(source("SpaceMouse Pro (Vendor: 046d Product: c62b)"), Some(Source::Device));
        assert_eq!(source("Unknown Gamepad (Vendor: beef Product: 046d)"), Some(Source::VirtualJoystick));
        assert_eq!(source("beef-046d-Unknown Gamepad"), Some(Source::VirtualJoystick));
        // 3Dconnexion's own vendor id, any product; the older devices under Logitech's.
        assert_eq!(source("SpaceMouse Compact (Vendor: 256f Product: c635)"), Some(Source::Device));
        assert_eq!(source("256f-c652-3Dconnexion Universal Receiver"), Some(Source::Device));
        assert_eq!(source("46d-c626-SpaceNavigator"), Some(Source::Device));
        assert_eq!(source("SpaceNavigator (STANDARD GAMEPAD Vendor: 046d Product: c626)"), Some(Source::Device));
        for product in LOGITECH_3D_MICE {
            assert_eq!(source(&format!("x (Vendor: 046d Product: {product:04x})")), Some(Source::Device));
            assert_eq!(source(&format!("046d-{product:04X}-x")), Some(Source::Device));
        }
    }

    #[test]
    fn a_calibration_is_saved_under_the_ids_whichever_browser_lists_it() {
        assert_eq!(device_key("SpaceMouse Pro (Vendor: 046d Product: c62b)").as_deref(), Some("web 046d:c62b"));
        assert_eq!(device_key("46d-c62b-SpaceMouse Pro").as_deref(), Some("web 046d:c62b"));
        assert_eq!(device_key("Xbox Wireless Controller"), None);
    }

    #[test]
    fn the_idle_page_wakes_for_motion_as_calibrated_not_as_read() {
        let axis =
            |source, rest, full| solidcraft_ui_egui::space_mouse::AxisCalibration { source, reversed: false, rest, dead_zone: 0, full: [full; 2] };
        // A short-range device: 15 of its 50 is under the raw dead zone, but 105 of 350 calibrated.
        let short = Calibration { dead_zone_share: 0.0, kind: Default::default(), axes: std::array::from_fn(|r| axis(r, 0, 50)) };
        let push = Axes { translation: [15, 0, 0], rotation: [0; 3] };
        assert!(!push.moving() && moving(push, Some(short)));
        // A device resting at 100 on every axis: past the raw dead zone, but at rest calibrated.
        let offset = Calibration { dead_zone_share: 0.0, kind: Default::default(), axes: std::array::from_fn(|r| axis(r, 100, 350)) };
        let resting = Axes { translation: [100; 3], rotation: [100; 3] };
        assert!(resting.moving() && !moving(resting, Some(offset)));
        // Without a calibration, as read.
        assert!(!moving(push, None) && moving(resting, None));
        // Kept from the last frame: only for the device it was for.
        let (a, b) = (Some("web 256f:c63a".to_string()), Some("web 1234:5678".to_string()));
        assert_eq!(moving_as_cached(&b, push, &(b.clone(), Some(short))), Some(true));
        assert_eq!(moving_as_cached(&b, push, &(a.clone(), Some(offset))), None, "A's calibration doesn't judge B");
        assert_eq!(moving_as_cached(&b, push, &(None, None)), None);
        assert_eq!(moving_as_cached(&a, resting, &(a.clone(), None)), Some(true), "A was read without a calibration");
    }

    #[test]
    fn the_idle_page_wakes_once_for_a_device_change_and_stays_quiet_without_one() {
        let (a, b) = (Some("web 256f:c63a".to_string()), Some("web 1234:5678".to_string()));
        let still = Axes::default();
        let last_frame_read_a: Cached = (a.clone(), None);
        assert!(!wakes(Some((a.clone(), still)), &last_frame_read_a), "A, at rest");
        assert!(wakes(Some((b.clone(), still)), &last_frame_read_a), "A to B: a frame looks at B");
        assert!(wakes(None, &last_frame_read_a), "A to none: a frame shows it's gone");
        assert!(!wakes(None, &(None, None)), "none to none: quiet");
    }

    #[test]
    fn gamepads_and_other_logitech_devices_are_not_3d_mice() {
        for id in [
            "Logitech Dual Action (STANDARD GAMEPAD Vendor: 046d Product: c216)",
            "046d-c21d-Logitech Gamepad F310",
            "G29 Driving Force Racing Wheel (Vendor: 046d Product: c24f)",
            "046d-c215-Logitech Extreme 3D",
            "NuLOOQ navigator (Vendor: 046d Product: c640)",
            "Xbox 360 Controller (XInput STANDARD GAMEPAD)",
            "45e-28e-Xbox 360 Wired Controller",
            "beef-046e-Unknown Gamepad",
            "beee-046d-Unknown Gamepad",
            "",
            "-",
            "--",
            "256f-",
            "256f-c635",
            "Vendor: 256f",
            "Vendor: 256f Product: ",
            "x (Vendor: 256fff Product: c635)",
            "x (Vendor: zz6f Product: c635)",
            "12345-c635-x",
            "256f--x",
            "SpaceMouse Pro",
        ] {
            assert_eq!(source(id), None, "{id:?}");
        }
    }

    #[test]
    fn ids_never_panic() {
        // Hostile and odd ids: multi-byte text around the markers, markers repeated or cut short.
        for id in [
            "Vendor: é Product: ü",
            "Vendor: Vendor: 256f Product: c635)",
            "é-ü-ö",
            "Ⅰ (Vendor: 256f Product: c6Ⅰ)",
            "\u{0}-\u{0}-\u{0}",
            "Vendor: 256f Product: c635 Vendor: ",
        ] {
            let _ = source(id);
        }
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let alphabet: Vec<char> = "0123456789abcdefABCDEF- ():VendorProductéⅠ\u{0}".chars().collect();
        for _ in 0..20_000 {
            let len = (seed % 40) as usize;
            let id: String = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    alphabet[(seed % alphabet.len() as u64) as usize]
                })
                .collect();
            if let Some(Source::Device) = source(&id) {
                let (vendor, product) = vendor_product(&id).expect("recognised ids have both");
                assert!(vendor == THREEDCONNEXION || (vendor == LOGITECH && LOGITECH_3D_MICE.contains(&product)), "{id:?}");
            }
        }
    }

    #[test]
    fn gamepad_axes_are_the_devices_raw_units() {
        // Full deflection is the desktop's 350, and the browser's scale keeps the device's steps.
        let axes = axes(&[1.0, -1.0, 0.5, -0.5, 2.0 / 700.0, 0.0]).expect("six axes");
        assert_eq!(axes, Axes { translation: [350, -350, 175], rotation: [-175, 1, 0] });
        for raw in -350_i16..=350 {
            let value = f64::from(raw) / 350.0;
            assert_eq!(super::axes(&[value; 6]).map(|a| a.translation[0]), Some(raw));
        }
        // Past full deflection is full deflection; anything that isn't a number is rest.
        let hostile = super::axes(&[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300, f64::MIN_POSITIVE]).expect("six axes");
        assert_eq!(hostile, Axes { translation: [0, 0, 0], rotation: [350, -350, 0] });
        // A gamepad with fewer than six axes is not a 3D mouse; extra axes are ignored.
        assert_eq!(super::axes(&[1.0; 5]), None);
        assert_eq!(super::axes(&[]), None);
        assert_eq!(super::axes(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]), Some(Axes::default()));
    }

    #[test]
    fn a_device_is_read_before_the_virtual_joystick() {
        let moving = Axes { translation: [100, 0, 0], rotation: [0; 3] };
        let other = Axes { translation: [0, 200, 0], rotation: [0; 3] };
        assert_eq!(choose([(Source::VirtualJoystick, other), (Source::Device, moving)]), Some(moving));
        assert_eq!(choose([(Source::Device, moving), (Source::VirtualJoystick, other)]), Some(moving));
        assert_eq!(choose([(Source::VirtualJoystick, other)]), Some(other));
        // Two devices: the first listed.
        assert_eq!(choose([(Source::Device, moving), (Source::Device, other)]), Some(moving));
        assert_eq!(choose::<Axes>([]), None);
    }
}
