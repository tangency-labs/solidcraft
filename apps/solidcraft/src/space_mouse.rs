//! 3D mouse navigation on Windows and macOS (through hidapi) and Linux (through spacenavd when it runs, else
//! hidraw). Read directly, any HID multi-axis controller works: 3Dconnexion's and homemade ones.
//! The device reader never touches the document or UI; it publishes the latest axes, and the egui
//! thread applies them once per frame.

#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "linux")]
mod spacenavd;
#[cfg(target_os = "windows")]
mod windows_hid;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::Context;
#[cfg(target_os = "macos")]
use hidapi::DeviceInfo;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use hidapi::HidApi;
use quick_xml::Reader;
use quick_xml::events::Event;
use solidcraft_space_mouse::Layout;
use solidcraft_ui_egui::SolveApp;
#[cfg(target_os = "linux")]
use solidcraft_ui_egui::prefs::SpaceMouseDriver;
#[cfg(target_os = "linux")]
use solidcraft_ui_egui::space_mouse::DriversPresent;
use solidcraft_ui_egui::space_mouse::{Axes, AxisRule, Calibration, DriverMapping, WheelGuard, calibrated, calibration, navigate_frame};

/// The fastest the window repaints for the device's reports: a device sends them faster than anyone
/// sees, and each one used to be a frame of its own.
const REPAINT_EVERY: Duration = Duration::from_millis(16);

const STALE_AFTER: Duration = Duration::from_millis(150);
/// How long a held (spacenavd) reading that is nearly at rest may go without a new record before
/// it reads as rest: spacenavd sends no final record when the cap springs back, only the last
/// small leftover. Longer than `STALE_AFTER` so a light, steady hold gets a moment before it ends.
const HELD_SETTLE_AFTER: Duration = Duration::from_millis(500);
/// How far from rest (raw units, full deflection is 350) every axis of a held reading must be for
/// it to settle. The dead zone is 20, but the last records of real releases reach 129 on one axis
/// (tangency-labs/solidcraft#3523), so this sits well above it. A firmer push is a deliberate
/// hold and stays.
const HELD_SETTLE_BAND: u16 = 150;
/// Whether the 3Dconnexion driver (macOS) has the device: it then reports the cap, and reading
/// the HID device would fail or report it twice.
static DRIVER_CONNECTED: AtomicBool = AtomicBool::new(false);
/// Whether this app registered with the 3Dconnexion driver (macOS). The driver then has the device
/// whether or not it has said so yet, so a failure to open it by HID is no news.
#[cfg(any(target_os = "windows", target_os = "macos"))]
static DRIVER_REGISTERED: AtomicBool = AtomicBool::new(false);

const RETRY_AFTER: Duration = Duration::from_secs(2);
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
/// Room for one input report: the 4096 bytes `solidcraft_space_mouse` places fields in, plus the
/// report id. A smaller buffer would cut a longer report short, and a cut report is dropped whole.
const REPORT_BUFFER_BYTES: usize = 4096 + 1;

/// The mapping used when no 3DxWare profile is found: what the driver applies by default in
/// object mode. On Linux there is no 3DxWare (#1172); on Windows 3DxWare has no profile for
/// SolidCraft until one is made, and the raw axes swap roll and spin (#1380).
fn without_profile() -> DriverMapping {
    DriverMapping::object_mode()
}

#[derive(Clone, Copy, Debug, Default)]
struct ProfileAxis {
    input: Option<usize>,
    output: Option<usize>,
    enabled: bool,
    reversed: bool,
    scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProfilePart {
    Input,
    Output,
}

fn axis_index(name: &str) -> Option<usize> {
    match name.trim() {
        "HIDMultiAxis_X" => Some(0),
        "HIDMultiAxis_Y" => Some(1),
        "HIDMultiAxis_Z" => Some(2),
        "HIDMultiAxis_Rx" => Some(3),
        "HIDMultiAxis_Ry" => Some(4),
        "HIDMultiAxis_Rz" => Some(5),
        _ => None,
    }
}

/// 3DxWare stores the active KMJ application's axis selection, swapping, inversion and speed in
/// `%APPDATA%/3Dconnexion/3DxWare/Cfg/<app>-KMJ.xml`. Direct HID reads bypass the driver, so apply
/// that profile explicitly when it exists. Unknown or incomplete fields retain the raw HID axis.
fn parse_driver_mapping(xml: &[u8]) -> Option<DriverMapping> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut mapping = DriverMapping::default();
    let mut axis: Option<ProfileAxis> = None;
    let mut part = None;
    let mut field = Vec::new();
    let mut found = false;
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(start) => match start.local_name().as_ref() {
                b"Axis" => axis = Some(ProfileAxis { enabled: true, scale: 1.0, ..Default::default() }),
                b"Input" => part = Some(ProfilePart::Input),
                b"Output" => part = Some(ProfilePart::Output),
                name @ (b"ActionID" | b"Enabled" | b"Reversed" | b"Scale" | b"OverallScale") => field = name.to_vec(),
                _ => {}
            },
            Event::Text(text) => {
                let value = String::from_utf8_lossy(text.as_ref());
                match field.as_slice() {
                    b"ActionID" => {
                        if let Some(current) = axis.as_mut() {
                            match part {
                                Some(ProfilePart::Input) => current.input = axis_index(&value),
                                Some(ProfilePart::Output) => current.output = axis_index(&value),
                                None => {}
                            }
                        }
                    }
                    b"Enabled" => {
                        if let Some(current) = axis.as_mut() {
                            current.enabled = value.trim().eq_ignore_ascii_case("true");
                        }
                    }
                    b"Reversed" => {
                        if part == Some(ProfilePart::Output)
                            && let Some(current) = axis.as_mut()
                        {
                            current.reversed = value.trim().eq_ignore_ascii_case("true");
                        }
                    }
                    b"Scale" => {
                        if part == Some(ProfilePart::Output)
                            && let Some(scale) = value.trim().parse::<f64>().ok().filter(|scale| scale.is_finite())
                            && let Some(current) = axis.as_mut()
                        {
                            current.scale = scale.clamp(0.0, 10.0);
                        }
                    }
                    b"OverallScale" => {
                        if let Some(scale) = value.trim().parse::<f64>().ok().filter(|scale| scale.is_finite()) {
                            mapping.overall_scale = scale.clamp(0.0, 10.0);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(end) => match end.local_name().as_ref() {
                b"Axis" => {
                    if let Some(current) = axis.take()
                        && let (Some(input), Some(output)) = (current.input, current.output)
                    {
                        mapping.axes[input] = AxisRule { output, enabled: current.enabled, reversed: current.reversed, scale: current.scale };
                        found = true;
                    }
                }
                b"Input" | b"Output" => part = None,
                b"ActionID" | b"Enabled" | b"Reversed" | b"Scale" | b"OverallScale" => field.clear(),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    found.then_some(mapping)
}

fn load_driver_mapping() -> Option<DriverMapping> {
    let cfg = std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("3Dconnexion").join("3DxWare").join("Cfg");
    // The driver names the profile after the executable; one made for the pre-rename executable
    // still applies until the driver writes a new one (compatibility shim).
    let profile = ["solidcraft-KMJ.xml", "solvecraft-KMJ.xml"].into_iter().map(|n| cfg.join(n)).find(|p| p.is_file())?;
    if std::fs::metadata(&profile).ok()?.len() > MAX_PROFILE_BYTES {
        log::warn!("3D mouse: ignoring oversized 3DxWare profile {}", profile.display());
        return None;
    }
    let xml = std::fs::read(&profile).ok()?;
    parse_driver_mapping(&xml)
}

/// Where the reading comes from, which a calibration is saved under (`Source::key`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    /// The device itself, with its vendor and product ids when known.
    Hid(Option<(u16, u16)>),
    /// spacenavd, which may have swapped or inverted axes on its own (`spnavcfg`).
    #[cfg(target_os = "linux")]
    Spacenavd,
    /// The 3Dconnexion driver (macOS), which applies its own settings first.
    #[cfg(target_os = "macos")]
    Driver,
}

impl Source {
    /// Which read path the source is, for the log.
    fn path(self) -> &'static str {
        match self {
            Self::Hid(_) => "the hidraw device",
            #[cfg(target_os = "linux")]
            Self::Spacenavd => "spacenavd",
            #[cfg(target_os = "macos")]
            Self::Driver => "the 3Dconnexion driver",
        }
    }

    /// The key a device's saved settings are kept under: where the reading comes from and, for a device
    /// read directly, its vendor and product ids (`hid 046d:c62b`). Never the hidraw node or a device
    /// number, which change at every replug. Two mice of one model share a slot (the kernel gives no
    /// serial here); the calibration is of the model's axes, so that is the same calibration. A reading
    /// through spacenavd or the 3Dconnexion driver is a different reading of the device (processed by
    /// them), so it has its own key and its own calibration. That key is shared by every device the
    /// daemon reads, so it cannot hold a device's own settings: Automatic therefore reads a device
    /// directly whenever it opens, and the daemon only when no device does (see `choose`).
    fn key(self) -> String {
        match self {
            Self::Hid(Some((vendor, product))) => format!("hid {vendor:04x}:{product:04x}"),
            Self::Hid(None) => "hid".into(),
            #[cfg(target_os = "linux")]
            Self::Spacenavd => "spacenavd".into(),
            #[cfg(target_os = "macos")]
            Self::Driver => "3dxware".into(),
        }
    }
}

#[derive(Clone, Copy)]
struct Reading {
    /// The device being read; none between devices, and none while a daemon's socket is only reachable.
    device: Option<Source>,
    /// A source that is connected but has not yet shown a device behind it (spacenavd's socket answers
    /// with no mouse plugged in; its protocol has no enumeration, so the first record is the evidence).
    #[cfg(target_os = "linux")]
    awaiting: Option<Source>,
    axes: Axes,
    /// When each of X, Y, Z, Rx, Ry and Rz last arrived. Each ages on its own: a device may send
    /// translation and rotation in separate reports, or spread even one group over several.
    at: [Instant; 6],
    /// The source sends a record only when the values change, so a reading stays until the next
    /// one: a steady hold can go seconds without one. It never says when the cap springs back,
    /// though, so a reading within `HELD_SETTLE_BAND` of rest on every axis settles after
    /// `HELD_SETTLE_AFTER`. A HID device repeats its report while the cap is held, so it ages out
    /// per axis instead.
    held: bool,
    /// The buttons that are down: bit N for button N + 1, as the device (or spacenavd) numbers them.
    buttons: u32,
    /// The buttons that went down since the UI last looked, even if they came up again before it
    /// did: a quick tap between two frames is still a press. It is a set, not a queue: two taps of one
    /// button between two looks are one press, and run its job once.
    taps: u32,
    /// A motion report or record arrived since the UI last looked. Not whether the value changed: a
    /// ball rolling at one speed sends the same reading again and again, and a daemon that truncates
    /// it can send two different records that decode alike. The calibration window needs the
    /// difference between that and a last value kept with nothing arriving.
    arrived: bool,
    /// The buttons' state has been seen once since this source connected. A HID device's first report
    /// that carries buttons says which are down; one already down then is held (a reader swapped under
    /// it, or the device was plugged in with it down), not pressed. Sources that send each press and
    /// release as an event (spacenavd, the macOS driver) start from nothing down.
    buttons_known: bool,
    /// Which connection of the reader this reading belongs to; it changes at every connect, so a look
    /// that missed a whole disconnect and reconnect of the same device can tell it was one.
    generation: u32,
}

impl Reading {
    /// The buttons that are down now; those that just went down are kept as taps until taken. The first
    /// state a HID source reports is only taken as the baseline: a button down in it is held, not a tap.
    fn set_buttons(&mut self, buttons: u32) {
        if !self.buttons_known {
            self.buttons_known = true;
            self.buttons = buttons;
            return;
        }
        self.taps |= buttons & !self.buttons;
        self.buttons = buttons;
    }

    /// A record arrived from the source being waited on: there is a device.
    #[cfg(target_os = "linux")]
    fn heard(&mut self) {
        if self.device.is_none() {
            self.device = self.awaiting.take();
        }
    }

    fn new(now: Instant) -> Self {
        Self {
            device: None,
            #[cfg(target_os = "linux")]
            awaiting: None,
            axes: Axes::default(),
            at: [now; 6],
            held: false,
            buttons: 0,
            taps: 0,
            arrived: false,
            buttons_known: true,
            generation: 0,
        }
    }

    /// The axes at `now`. One with nothing fresh reads as `rest`: the device's own rest when it
    /// is calibrated, so a device that rests off zero calibrates to no motion rather than drifting.
    #[cfg(test)]
    fn current(self, now: Instant, rest: Axes) -> Axes {
        self.current_with(now, rest, Some(HELD_SETTLE_BAND))
    }

    /// [`Reading::current`] with the held-reading settle's `band` (see [`settle_band`]); `None`
    /// turns it off, as for a trackball, whose reading is a speed and a steady slow roll is meant.
    ///
    /// Reads only `self.at[0]` for the age: `publish_held`, the only producer of a held reading,
    /// stamps all six axes together, so they are never apart.
    fn current_with(self, now: Instant, rest: Axes, band: Option<u16>) -> Axes {
        let mut axes = self.axes;
        if self.held {
            let values = axes.translation.into_iter().chain(axes.rotation);
            let rests = rest.translation.into_iter().chain(rest.rotation);
            let near_rest = band.is_some_and(|band| values.zip(rests).all(|(value, rest)| value.abs_diff(rest) <= band));
            if near_rest && now.duration_since(self.at[0]) >= HELD_SETTLE_AFTER {
                return rest;
            }
        } else {
            let rest = rest.translation.into_iter().chain(rest.rotation);
            for ((value, at), rest) in axes.translation.iter_mut().chain(axes.rotation.iter_mut()).zip(self.at).zip(rest) {
                if now.duration_since(at) >= STALE_AFTER {
                    *value = rest;
                }
            }
        }
        axes
    }
}

/// The raw axes at the device's rest, which calibrate to no motion; 0 without a calibration.
/// How near rest (raw units) every axis of a held reading must be for it to settle, or `None` for a
/// device whose steady reading is meant (a trackball). Without a calibration it is
/// `HELD_SETTLE_BAND`; with one it shrinks in step with the device's shortest calibrated travel (the
/// band is for a travel of 350), so a cap whose full push is 50 units does not lose a held full push.
fn settle_band(calibration: Option<Calibration>) -> Option<u16> {
    let Some(calibration) = calibration else { return Some(HELD_SETTLE_BAND) };
    if calibration.kind == solidcraft_space_mouse::Kind::Trackball {
        return None;
    }
    let travel =
        calibration.axes.iter().filter(|axis| axis.source != solidcraft_space_mouse::NO_SOURCE).flat_map(|axis| axis.full).min().unwrap_or(350);
    let band = u32::from(HELD_SETTLE_BAND) * u32::from(travel) / 350;
    Some(u16::try_from(band).unwrap_or(HELD_SETTLE_BAND).min(HELD_SETTLE_BAND))
}

fn at_rest(calibration: Option<Calibration>) -> Axes {
    calibration.map(|calibration| calibration.raw_rest().into()).unwrap_or_default()
}

/// The Generic Desktop "Multi-Axis Controller" usage names a 3D mouse's motion interface, whoever
/// made it, and excludes ordinary mice and the keyboard interfaces of 3D mice. (Windows asks Plug
/// and Play for the same usage: `windows_hid`.)
#[cfg(target_os = "macos")]
fn is_3d_mouse(info: &DeviceInfo) -> bool {
    info.usage_page() == 0x01 && info.usage() == 0x08
}

/// Takes the axes an input report carries, as the device's descriptor laid them out, and leaves
/// the others as they were: translation and rotation may come in separate reports (3Dconnexion's
/// wired devices) or one (its wireless ones and most homemade devices).
fn decode_report(report: &[u8], layout: Layout, reading: &mut Reading, now: Instant) -> bool {
    let Some(decoded) = layout.decode(report) else { return false };
    let values = reading.axes.translation.iter_mut().chain(reading.axes.rotation.iter_mut());
    for ((value, at), new) in values.zip(reading.at.iter_mut()).zip(decoded.axes) {
        if let Some(new) = new {
            *value = new;
            *at = now;
        }
    }
    // A report that carries buttons says which are down, all up included.
    if let Some(buttons) = decoded.buttons {
        reading.set_buttons(buttons);
    }
    true
}

/// Publishes one input report from the device to the egui thread. A report that reads the same as
/// the reading it replaces is no news — a device may repeat its report at rest — so it asks for no
/// frame; while the cap moves, the view asks for the next frame itself.
fn publish(ctx: &Context, shared: &Mutex<Reading>, layout: Layout, report: &[u8]) {
    if DRIVER_CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut reading) = shared.lock() {
        let before = (reading.axes, reading.buttons, reading.taps);
        if decode_report(report, layout, &mut reading, Instant::now()) {
            // The calibration window needs to know a report arrived even when it reads the same.
            reading.arrived = true;
            if (reading.axes, reading.buttons, reading.taps) != before {
                ctx.request_repaint_after(REPAINT_EVERY);
            }
        }
    }
}

/// Publishes the cap's position from spacenavd.
#[cfg(target_os = "linux")]
fn publish_held(ctx: &Context, shared: &Mutex<Reading>, axes: Axes) {
    if let Ok(mut reading) = shared.lock() {
        reading.heard();
        *reading = Reading {
            device: reading.device,
            axes,
            held: true,
            buttons: reading.buttons,
            taps: reading.taps,
            arrived: true,
            generation: reading.generation,
            ..Reading::new(Instant::now())
        };
        ctx.request_repaint_after(REPAINT_EVERY);
    }
}

/// A button pressed or released, from spacenavd, which numbers them from 0.
#[cfg(target_os = "linux")]
fn publish_button(ctx: &Context, shared: &Mutex<Reading>, button: u32, down: bool) {
    if let (Ok(mut reading), Some(bit)) = (shared.lock(), 1_u32.checked_shl(button)) {
        reading.heard();
        let buttons = if down { reading.buttons | bit } else { reading.buttons & !bit };
        reading.set_buttons(buttons);
        ctx.request_repaint_after(REPAINT_EVERY);
    }
}

/// Whether a connection to `source` says nothing about a device being there until it sends a record.
fn needs_evidence(source: Source) -> bool {
    #[cfg(target_os = "linux")]
    {
        matches!(source, Source::Spacenavd)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = source;
        false
    }
}

/// How many times a reader has connected to a source, for `Reading::generation`.
static CONNECTIONS: AtomicU32 = AtomicU32::new(0);

/// A device is open: say which, so its calibration applies.
fn connected(ctx: &Context, shared: &Mutex<Reading>, source: Source) {
    if let Ok(mut reading) = shared.lock() {
        // Nothing has arrived from it yet, so every axis starts out stale.
        let now = Instant::now();
        let stale = now.checked_sub(STALE_AFTER).unwrap_or(now);
        // A daemon is not yet a device: it counts once a record comes from it.
        let waiting = needs_evidence(source);
        // A HID device reports the state of its buttons, so the first state seen is a baseline; the daemon
        // and the driver send presses and releases, which start from nothing down.
        let buttons_known = !matches!(source, Source::Hid(_));
        let generation = CONNECTIONS.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        *reading = Reading {
            device: (!waiting).then_some(source),
            #[cfg(target_os = "linux")]
            awaiting: waiting.then_some(source),
            at: [stale; 6],
            buttons_known,
            generation,
            ..Reading::new(now)
        };
    }
    ctx.request_repaint();
}

/// The device is gone: stop reporting its last position.
fn reset(ctx: &Context, shared: &Mutex<Reading>) {
    if let Ok(mut reading) = shared.lock() {
        *reading = Reading::new(Instant::now());
    }
    ctx.request_repaint();
}

/// The HID interfaces that are 3D mice, by path. Windows asks Plug and Play, which sends nothing to
/// the devices: hidapi's enumeration would ask every keyboard and mouse for its strings on each scan
/// (#2614, `windows_hid`).
#[cfg(target_os = "windows")]
fn candidates(_api: &mut HidApi) -> Vec<CString> {
    windows_hid::multi_axis_controllers()
}

/// The HID interfaces that are 3D mice, by path, from hidapi's enumeration (IOKit's registry).
#[cfg(target_os = "macos")]
fn candidates(api: &mut HidApi) -> Vec<CString> {
    if let Err(error) = api.refresh_devices() {
        log::warn!("3D mouse: device scan failed: {error}");
        return Vec::new();
    }
    api.device_list().filter(|info| is_3d_mouse(info)).map(|info| info.path().to_owned()).collect()
}

/// The HID context. On Windows hidapi's own device discovery is off, so making it asks no device
/// anything either (`candidates` finds the 3D mice). Turning discovery off panics only once a
/// context with discovery exists; the reader is the one place the app makes a context, once.
#[cfg(target_os = "windows")]
fn hid_api() -> hidapi::HidResult<HidApi> {
    HidApi::disable_device_discovery();
    HidApi::new()
}

#[cfg(target_os = "macos")]
fn hid_api() -> hidapi::HidResult<HidApi> {
    HidApi::new()
}

/// An open 3D-mouse interface: the device, how its reports are laid out, and its vendor and
/// product ids.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
struct Candidate<D> {
    device: D,
    layout: Layout,
    ids: (u16, u16),
    /// The device's manufacturer and product strings ([`device_strings`]).
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    strings: Vec<String>,
}

/// Input reports read with a timeout in milliseconds: hidapi's devices, and the tests' simulated
/// ones. `Ok(0)` is a timeout; an error means the device is gone.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
trait ReportSource {
    fn read_report(&self, report: &mut [u8], timeout_ms: i32) -> Result<usize, String>;
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl ReportSource for hidapi::HidDevice {
    fn read_report(&self, report: &mut [u8], timeout_ms: i32) -> Result<usize, String> {
        self.read_timeout(report, timeout_ms).map_err(|error| error.to_string())
    }
}

/// What following the candidates tells the reader.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
enum Followed<'a> {
    /// This device is the one read now, so its calibration applies. With the buttons it last
    /// reported down, when it reported them while another was read: they are its baseline, so a
    /// press that makes it the device in use counts as a press, not as a button already held.
    Device((u16, u16), Option<u32>),
    /// An input report from it.
    Report(&'a Layout, &'a [u8]),
    /// It is gone.
    Gone,
}

/// Whether a report shows its device in use: an axis off zero or a button down. A cap at rest
/// sends zeros at most, and an idle 3Dconnexion Universal Receiver sends nothing at all.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn shows_use(layout: Layout, report: &[u8]) -> bool {
    layout.decode(report).is_some_and(|decoded| decoded.axes.iter().flatten().any(|axis| *axis != 0) || decoded.buttons.is_some_and(|down| down != 0))
}

/// Reads every candidate and follows the one in use (#3988). The first is followed from the start,
/// as when it was the only one read. Another takes over once it shows use while the followed one
/// has shown none for `STALE_AFTER`, so two devices moved together don't hand the reading back
/// and forth. An idle Universal Receiver opens fine and never fails; read on its own it kept a
/// SpaceMouse next to it from ever being read. A candidate that fails is dropped. If it was the
/// followed one, the reading is reset and the next takes over. Returns when none is left.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn follow<D: ReportSource>(mut candidates: Vec<Candidate<D>>, now: impl Fn() -> Instant, mut emit: impl FnMut(Followed<'_>)) {
    let Some(first) = candidates.first() else { return };
    emit(Followed::Device(first.ids, None));
    let mut followed = 0;
    // When the followed device last showed use.
    let mut used_at: Option<Instant> = None;
    // The buttons each candidate last reported down, by index, as long as it has reported them.
    let mut buttons: Vec<Option<u32>> = candidates.iter().map(|_| None).collect();
    let mut report = [0_u8; REPORT_BUFFER_BYTES];
    while !candidates.is_empty() {
        // One device is waited on as before; several take turns, briefly, so none waits behind
        // an idle one. Reports queue in the system meanwhile, so none is lost.
        let timeout_ms = if candidates.len() == 1 { 100 } else { 10 };
        let mut index = 0;
        while let Some(candidate) = candidates.get(index) {
            match candidate.device.read_report(&mut report, timeout_ms) {
                Ok(0) => {}
                Ok(size) => {
                    let bytes = report.get(..size).unwrap_or_default();
                    let before = buttons.get(index).copied().flatten();
                    if let (Some(seen), Some(down)) = (buttons.get_mut(index), candidate.layout.decode(bytes).and_then(|decoded| decoded.buttons)) {
                        *seen = Some(down);
                    }
                    if shows_use(candidate.layout, bytes) {
                        let at = now();
                        if index != followed && used_at.is_none_or(|used| at.duration_since(used) >= STALE_AFTER) {
                            let (vendor, product) = candidate.ids;
                            log::info!("3D mouse: reading {vendor:04x}:{product:04x}, the device in use");
                            followed = index;
                            emit(Followed::Device(candidate.ids, before));
                        }
                        if index == followed {
                            used_at = Some(at);
                        }
                    }
                    if index == followed {
                        emit(Followed::Report(&candidate.layout, bytes));
                    }
                }
                Err(error) => {
                    log::info!("3D mouse disconnected: {error}");
                    candidates.remove(index);
                    if index < buttons.len() {
                        buttons.remove(index);
                    }
                    if index == followed {
                        emit(Followed::Gone);
                        followed = 0;
                        used_at = None;
                        if let Some(next) = candidates.first() {
                            emit(Followed::Device(next.ids, buttons.first().copied().flatten()));
                        }
                    } else if index < followed {
                        followed -= 1;
                    }
                    continue;
                }
            }
            index += 1;
        }
    }
}

/// The device's manufacturer and product strings, which say whether a device app has it
/// ([`crate::nav_bridge::Claim`]). Asked of the opened 3D mouse only.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn device_strings(device: &hidapi::HidDevice) -> Vec<String> {
    [device.get_manufacturer_string(), device.get_product_string()].into_iter().filter_map(|s| s.ok().flatten()).collect()
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn reader(ctx: Context, shared: Arc<Mutex<Reading>>, bridge: crate::nav_bridge::Claim) {
    let Ok(mut api) = hid_api() else {
        log::warn!("3D mouse: HID initialization failed");
        return;
    };
    loop {
        let open: Vec<_> = candidates(&mut api)
            .into_iter()
            .filter_map(|path| {
                let device = api
                    .open_path(&path)
                    .inspect_err(|error| {
                        // On macOS: missing Input Monitoring permission, or the 3DxWare helper holding the device.
                        // The driver having the device is the reason it cannot be opened, not a problem.
                        if !DRIVER_CONNECTED.load(Ordering::Relaxed) && !DRIVER_REGISTERED.load(Ordering::Relaxed) {
                            log::warn!("3D mouse: cannot open the device: {error}");
                        }
                    })
                    .ok()?;
                // Read when the device was opened, from this device alone.
                let info = device.get_device_info().ok()?;
                let layout = device_layout(&device, info.vendor_id())?;
                let strings = device_strings(&device);
                Some(Candidate { device, layout, ids: (info.vendor_id(), info.product_id()), strings })
            })
            .collect();
        let names: Vec<_> = open.iter().map(|candidate| (candidate.ids, candidate.strings.clone())).collect();
        // Whether the device read now is one a device app has: it moves the view over the navigation
        // bridge, so read here as well the device would move it twice. What was read before ages out.
        let mut strings: &[String] = &[];
        follow(open, Instant::now, |event| match event {
            Followed::Device(ids, buttons) => {
                strings = names.iter().find(|(known, _)| *known == ids).map_or(&[][..], |(_, strings)| strings.as_slice());
                connected(&ctx, &shared, Source::Hid(Some(ids)));
                // Taken over from another device: what it reported meanwhile is its baseline.
                if let Some(buttons) = buttons
                    && let Ok(mut reading) = shared.lock()
                {
                    reading.set_buttons(buttons);
                }
            }
            Followed::Report(..) if bridge.claims(strings) => {}
            Followed::Report(layout, report) => publish(&ctx, &shared, *layout, report),
            Followed::Gone => reset(&ctx, &shared),
        });
        std::thread::sleep(RETRY_AFTER);
    }
}

/// How the device packs its axes, from its report descriptor, or None when it isn't one to read:
/// a controller whose descriptor names no motion axis. Without a descriptor to go on (it can't be
/// read, or it's empty), a 3Dconnexion device (Logitech's id for the old SpaceNavigator,
/// 3Dconnexion's own for the rest) gets 3Dconnexion's layout, and any other device is left alone
/// rather than guessed at.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn device_layout(device: &hidapi::HidDevice, vendor_id: u16) -> Option<Layout> {
    let mut descriptor = [0_u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
    let read = device.get_report_descriptor(&mut descriptor);
    if let Ok(size @ 1..) = read {
        return solidcraft_space_mouse::Descriptor::parse(descriptor.get(..size).unwrap_or_default()).layout();
    }
    let why = match read {
        Err(error) => error.to_string(),
        Ok(_) => "it is empty".to_string(),
    };
    if matches!(vendor_id, 0x046d | 0x256f) {
        log::info!("3D mouse: no report descriptor ({why}), assuming separate translation and rotation reports");
        Some(Layout::default())
    } else {
        log::debug!("3D mouse: skipping a multi-axis controller without a report descriptor ({why})");
        None
    }
}

/// What the user chose for the reader and what the reader found, shared with the egui thread.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct Drivers {
    /// `SpaceMouseDriver::to_u8` of the preference.
    choice: AtomicU8,
    /// `PRESENT_SPACENAVD`, `PRESENT_DIRECT` and `PRESENT_DENIED`, as of the reader's last look.
    found: AtomicU8,
    /// Ends the reader.
    quit: AtomicBool,
}

#[cfg(target_os = "linux")]
const PRESENT_SPACENAVD: u8 = 1;
#[cfg(target_os = "linux")]
const PRESENT_DIRECT: u8 = 2;
#[cfg(target_os = "linux")]
const PRESENT_DENIED: u8 = 4;
/// How often a connected reader looks again at what is there, and how long a read of spacenavd
/// waits for a record before it checks the choice.
#[cfg(target_os = "linux")]
const LOOK_EVERY: Duration = Duration::from_secs(1);
#[cfg(target_os = "linux")]
const SPACENAVD_WAKE: Duration = Duration::from_millis(250);

#[cfg(target_os = "linux")]
impl Drivers {
    fn choice(&self) -> SpaceMouseDriver {
        SpaceMouseDriver::from_u8(self.choice.load(Ordering::Relaxed))
    }

    fn present(&self) -> DriversPresent {
        let found = self.found.load(Ordering::Relaxed);
        DriversPresent { spacenavd: found & PRESENT_SPACENAVD != 0, direct: found & PRESENT_DIRECT != 0, denied: found & PRESENT_DENIED != 0 }
    }

    fn publish(&self, present: DriversPresent) {
        let found = (u8::from(present.spacenavd) * PRESENT_SPACENAVD)
            | (u8::from(present.direct) * PRESENT_DIRECT)
            | (u8::from(present.denied) * PRESENT_DENIED);
        self.found.store(found, Ordering::Relaxed);
    }
}

/// The reader that was picked.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Spacenavd,
    Direct,
}

/// How long a direct device that failed to open stays out of the choice before it is tried again.
#[cfg(target_os = "linux")]
const UNREADABLE_RETRY: Duration = Duration::from_secs(30);

/// When the direct device last failed to open, and which node has had its failure said out loud: a
/// device that is there but unreadable (no permission) does not keep the choice from a daemon that
/// works, and the warning about the udev rule is made once per node, not every retry.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct Unreadable {
    at: Option<Instant>,
    said: Option<std::path::PathBuf>,
}

#[cfg(target_os = "linux")]
impl Unreadable {
    /// `path` could not be opened. Whether to say so: not if this node's failure was already said.
    fn failed(&mut self, path: &std::path::Path) -> bool {
        self.at = Some(Instant::now());
        let new = self.said.as_deref() != Some(path);
        if new {
            self.said = Some(path.to_owned());
        }
        new
    }

    /// A device opened: nothing is held against it, and another node's failure is news again.
    fn opened(&mut self) {
        *self = Self::default();
    }
}

/// Whether the direct reader can be chosen: a device was found, and it did not just fail to open
/// (`since_failed`, how long ago, if it did): without that a device that is there but unreadable
/// (no permission) would keep winning and a working spacenavd would never be tried.
#[cfg(target_os = "linux")]
fn direct_usable(found: bool, since_failed: Option<Duration>, retry: Duration) -> bool {
    found && since_failed.is_none_or(|ago| ago >= retry)
}

/// Whether a read of the daemon should end because the direct device would be the choice now: it was
/// away and is back, or it failed to open, the wait (`retry`) is over (`failed`, how long since) and
/// the caller has found that it opens. A read of anything else is left alone: a device that works is
/// not given up for a daemon that has appeared.
#[cfg(target_os = "linux")]
fn switch_back(wanted: SpaceMouseDriver, pick: Option<Pick>, present: DriversPresent, failed: Option<Duration>, retry: Duration) -> bool {
    pick == Some(Pick::Spacenavd) && choose(wanted, present.spacenavd, direct_usable(present.direct, failed, retry)) == Some(Pick::Direct)
}

/// Which reader to use: the one asked for when it is there, else the other, else none. Automatic
/// asks for the device itself (`/dev/hidraw`) and falls back to spacenavd when no device opens (none
/// there, no permission, still backing off after a failed open): spacenavd has been a known source of
/// trouble, and a device read directly keeps its own settings (key `hid VVVV:PPPP`) across a replug,
/// where the daemon's key is shared by whatever it reads. Direct and Spacenavd mean what they say, with
/// the other as the fallback.
#[cfg(target_os = "linux")]
fn choose(wanted: SpaceMouseDriver, spacenavd: bool, direct: bool) -> Option<Pick> {
    let first = match wanted {
        SpaceMouseDriver::Automatic | SpaceMouseDriver::Direct => Pick::Direct,
        SpaceMouseDriver::Spacenavd => Pick::Spacenavd,
    };
    let there = |pick| match pick {
        Pick::Spacenavd => spacenavd,
        Pick::Direct => direct,
    };
    let other = if first == Pick::Spacenavd { Pick::Direct } else { Pick::Spacenavd };
    [first, other].into_iter().find(|&pick| there(pick))
}

/// What the reader looks at, so that a test can stand in for the machine.
#[cfg(target_os = "linux")]
trait System {
    fn find(&self) -> Option<hidraw::Device>;
    fn daemon_exists(&self) -> bool;
    fn daemon_connect(&self) -> Option<std::os::unix::net::UnixStream>;
    fn open(&self, path: &std::path::Path) -> std::io::Result<Box<dyn std::io::Read>>;
    fn times(&self) -> Times;
}

/// The waits of the reader.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct Times {
    /// How often a read looks again at what is there.
    look_every: Duration,
    /// How long before the next try when nothing could be read.
    retry_after: Duration,
    /// How long a device that failed to open stays out of the choice.
    unreadable_retry: Duration,
    /// How long a read of the daemon waits for a record before it checks for a change.
    wake: Duration,
}

/// The machine itself.
#[cfg(target_os = "linux")]
struct Machine;

#[cfg(target_os = "linux")]
impl System for Machine {
    fn find(&self) -> Option<hidraw::Device> {
        hidraw::find()
    }

    fn daemon_exists(&self) -> bool {
        spacenavd::exists()
    }

    fn daemon_connect(&self) -> Option<std::os::unix::net::UnixStream> {
        spacenavd::connect()
    }

    fn open(&self, path: &std::path::Path) -> std::io::Result<Box<dyn std::io::Read>> {
        Ok(Box::new(hidraw::open(path)?))
    }

    fn times(&self) -> Times {
        Times { look_every: LOOK_EVERY, retry_after: RETRY_AFTER, unreadable_retry: UNREADABLE_RETRY, wake: SPACENAVD_WAKE }
    }
}

/// A change of driver, or the device being back, is looked at when a read goes idle (250 ms of quiet)
/// and, for the daemon's stream, at least every 250 ms while records keep arriving, so the wait is
/// bounded whatever the cap is doing.
/// Reads the 3D mouse through spacenavd or `/dev/hidrawN`, as `Drivers::choice` says (Automatic
/// is the device itself when it opens, else spacenavd, whose `spnavcfg` settings, swapped and
/// inverted axes, sensitivity, dead zones, apply then as in other programs). Both reads wake up when idle to see whether the choice changed, which then
/// moves to the other reader without a restart. hidraw is opened non-blocking and ends with an
/// error when the device is unplugged. Opening needs permission: most distributions grant it to
/// the logged-in user for devices that carry the `uaccess` udev tag, which a rule for the 3D mouse
/// has to add. Ends when `Drivers::quit` is set.
#[cfg(target_os = "linux")]
fn reader<S: System>(ctx: Context, shared: Arc<Mutex<Reading>>, drivers: Arc<Drivers>, system: S) {
    let times = system.times();
    let mut unreadable = Unreadable::default();
    let mut told = None;
    while !drivers.quit.load(Ordering::Relaxed) {
        let device = system.find();
        let present =
            DriversPresent { spacenavd: system.daemon_exists(), direct: device.is_some(), denied: unreadable.at.is_some() && device.is_some() };
        drivers.publish(present);
        let wanted = drivers.choice();
        // The wait after a failed open is for a daemon that works to be read meanwhile; with none there
        // is nothing else to read, so the device is tried again at the usual pace.
        let since_failed = unreadable.at.filter(|_| present.spacenavd).map(|at| at.elapsed());
        let direct = direct_usable(present.direct, since_failed, times.unreadable_retry);
        let mut pick = choose(wanted, present.spacenavd, direct);
        // A socket file with no daemon behind it is no spacenavd, and then the device is tried again too.
        let daemon = if pick == Some(Pick::Spacenavd) { system.daemon_connect() } else { None };
        if pick == Some(Pick::Spacenavd) && daemon.is_none() {
            pick = choose(wanted, false, present.direct);
        }
        let asked_for = match wanted {
            SpaceMouseDriver::Automatic => None,
            SpaceMouseDriver::Spacenavd => Some(Pick::Spacenavd),
            SpaceMouseDriver::Direct => Some(Pick::Direct),
        };
        if let (Some(asked), Some(pick)) = (asked_for, pick)
            && asked != pick
            && told != Some((wanted, pick))
        {
            log::info!("3D mouse: {asked:?} is not available, reading {pick:?} instead");
            told = Some((wanted, pick));
        }
        // Asked at every idle moment of a read: has the choice changed, and what is there now.
        let mut looked = Instant::now();
        let mut failed_at = unreadable.at;
        let mut stop = || {
            if drivers.quit.load(Ordering::Relaxed) {
                return true;
            }
            if looked.elapsed() >= times.look_every {
                looked = Instant::now();
                let now_device = system.find();
                let now_present = DriversPresent {
                    spacenavd: system.daemon_exists(),
                    direct: now_device.is_some(),
                    // Not while the device itself is what is being read: it opened.
                    denied: pick != Some(Pick::Direct) && failed_at.is_some() && now_device.is_some(),
                };
                drivers.publish(now_present);
                // A device that failed to open is tried again once its wait is over, by opening it, so a
                // daemon that works is only left for a device that now works, and a device that still
                // cannot be read is not tried again for another wait.
                if let (Some(at), Some(device), Some(Pick::Spacenavd)) = (failed_at, &now_device, pick)
                    && at.elapsed() >= times.unreadable_retry
                    && system.open(&device.path).is_err()
                {
                    failed_at = Some(Instant::now());
                }
                // The device came back (replugged), or became readable, while the daemon was read.
                if switch_back(wanted, pick, now_present, failed_at.map(|at| at.elapsed()), times.unreadable_retry) {
                    return true;
                }
            }
            drivers.choice() != wanted
        };
        match (pick, daemon, device) {
            (Some(Pick::Spacenavd), Some(daemon), _) => {
                log::info!("3D mouse: reading spacenavd, which applies the spnavcfg settings");
                if let Err(error) = daemon.set_read_timeout(Some(times.wake)) {
                    log::warn!("3D mouse: spacenavd reads cannot time out ({error}); a change of driver waits for the next event");
                }
                connected(&ctx, &shared, Source::Spacenavd);
                spacenavd::read(
                    daemon,
                    |axes| publish_held(&ctx, &shared, axes),
                    |button, down| publish_button(&ctx, &shared, button, down),
                    &mut stop,
                    times.wake,
                );
                reset(&ctx, &shared);
                // What the read found out about the device (it was tried again, and still would not
                // open) is kept for the next choice.
                unreadable.at = failed_at;
            }
            (Some(Pick::Direct), _, Some(device)) => match system.open(&device.path) {
                Ok(file) => {
                    unreadable.opened();
                    log::info!("3D mouse: reading {}", device.path.display());
                    connected(&ctx, &shared, Source::Hid(device.ids));
                    hidraw::read_reports(file, |report| publish(&ctx, &shared, device.layout, report), &mut stop);
                    reset(&ctx, &shared);
                }
                Err(error) => {
                    if unreadable.failed(&device.path) {
                        log::warn!(
                            "3D mouse: cannot open {}: {error}. A udev rule granting the logged-in user access is needed.",
                            device.path.display()
                        );
                    }
                }
            },
            _ => {}
        }
        // A new choice is taken up at once; anything else waits to try again.
        if drivers.choice() == wanted {
            std::thread::sleep(times.retry_after);
        }
    }
}

/// macOS: the 3Dconnexion driver gives the device to its own clients only, so SolidCraft registers
/// as one. Without the driver (or with its helper not running) the HID reader above takes over.
#[cfg(target_os = "macos")]
fn start_driver_client(ctx: &Context, shared: &Arc<Mutex<Reading>>) {
    use solidcraft_3dx_client::Event;
    let (ctx, shared) = (ctx.clone(), Arc::clone(shared));
    let started = solidcraft_3dx_client::start(
        "SolidCraft",
        move |event| match event {
            Event::DeviceAdded => {
                DRIVER_CONNECTED.store(true, Ordering::Relaxed);
                connected(&ctx, &shared, Source::Driver);
            }
            Event::DeviceRemoved => {
                DRIVER_CONNECTED.store(false, Ordering::Relaxed);
                reset(&ctx, &shared);
            }
            Event::Axes { translation, rotation } => {
                if let Ok(mut reading) = shared.lock() {
                    let now = Instant::now();
                    reading.axes = Axes { translation, rotation };
                    reading.at = [now; 6];
                    reading.arrived = true;
                    ctx.request_repaint_after(REPAINT_EVERY);
                }
            }
            Event::Buttons(buttons) => {
                if let Ok(mut reading) = shared.lock() {
                    reading.set_buttons(buttons);
                    ctx.request_repaint_after(REPAINT_EVERY);
                }
            }
        },
        |connected| match connected {
            Ok(()) => DRIVER_REGISTERED.store(true, Ordering::Relaxed),
            Err(error) => log::info!("3D mouse: no driver client: {error}"),
        },
    );
    if let Err(error) = started {
        log::info!("3D mouse: no driver client: {error}");
    }
}

pub struct SpaceMouse {
    shared: Arc<Mutex<Reading>>,
    #[cfg(target_os = "linux")]
    drivers: Arc<Drivers>,
    last_frame: Instant,
    driver_mapping: DriverMapping,
    mapping_checked_at: Instant,
    /// Drops 3DxWare's emulated wheel while the cap moves (#1380).
    wheel: WheelGuard,
    started: Instant,
    /// The calibration as of the last frame and the device it is for, for the wheel filter, which
    /// runs before the app sees the frame.
    calibration: Option<(Source, Calibration)>,
}

impl SpaceMouse {
    /// Starts the reader. A device that `bridge` says its own app reads is left to that app.
    pub fn start(ctx: &Context, bridge: crate::nav_bridge::Claim) -> Self {
        let now = Instant::now();
        let shared = Arc::new(Mutex::new(Reading::new(now)));
        let worker_state = Arc::clone(&shared);
        let worker_ctx = ctx.clone();
        #[cfg(target_os = "linux")]
        let drivers = Arc::new(Drivers::default());
        #[cfg(target_os = "linux")]
        let worker_drivers = Arc::clone(&drivers);
        // hidraw names no device's strings to compare (`Claim`).
        #[cfg(target_os = "linux")]
        let _ = bridge;
        let spawned = std::thread::Builder::new().name("solidcraft-3d-mouse".into()).spawn(move || {
            #[cfg(target_os = "linux")]
            reader(worker_ctx, worker_state, worker_drivers, Machine);
            #[cfg(not(target_os = "linux"))]
            reader(worker_ctx, worker_state, bridge);
        });
        if let Err(error) = spawned {
            log::warn!("3D mouse: reader could not start: {error}");
        }
        #[cfg(target_os = "macos")]
        start_driver_client(ctx, &shared);
        Self {
            shared,
            #[cfg(target_os = "linux")]
            drivers,
            last_frame: now,
            driver_mapping: load_driver_mapping().unwrap_or_else(without_profile),
            mapping_checked_at: now,
            wheel: WheelGuard::default(),
            started: now,
            calibration: None,
        }
    }

    /// Before the app's own hook (which adds the control channel's events): with navigation on,
    /// the wheel events 3DxWare makes from the cap's motion are dropped.
    pub fn raw_input_hook(&mut self, enabled: bool, raw: &mut egui::RawInput) {
        self.raw_input_hook_at(enabled, raw, Instant::now());
    }

    fn raw_input_hook_at(&mut self, enabled: bool, raw: &mut egui::RawInput, now: Instant) {
        if !enabled {
            return;
        }
        let moving = self
            .shared
            .lock()
            .map(|reading| {
                // Only the calibration of the device being read: another's would misjudge it.
                let calibration = self.calibration.filter(|(source, _)| Some(*source) == reading.device).map(|(_, calibration)| calibration);
                calibrated(reading.current_with(now, at_rest(calibration), settle_band(calibration)), calibration).moving()
            })
            .unwrap_or(false);
        self.wheel.filter(moving, now.duration_since(self.started).as_secs_f64(), &mut raw.events);
    }

    /// The device being read, the buttons down on it (bit N for button N + 1) and those that went
    /// down since the last call, all from one look at the reading, so a button never gets a
    /// device it did not come from.
    pub fn buttons(&self) -> solidcraft_ui_egui::space_mouse::Buttons {
        let Ok(mut reading) = self.shared.lock() else { return Default::default() };
        let taps = std::mem::take(&mut reading.taps);
        solidcraft_ui_egui::space_mouse::Buttons {
            device: reading.device.map(Source::key),
            down: reading.buttons,
            taps,
            pending: !reading.buttons_known,
            generation: reading.generation,
        }
    }

    pub fn tick(&mut self, app: &mut SolveApp, ctx: &Context) {
        self.tick_at(app, ctx, Instant::now());
    }

    /// The frame at `now`. Tests pass their own clock, so a slow runner can't age a reading.
    fn tick_at(&mut self, app: &mut SolveApp, ctx: &Context, now: Instant) {
        let seconds = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        if now.duration_since(self.mapping_checked_at) >= RETRY_AFTER {
            let updated = load_driver_mapping().unwrap_or_else(without_profile);
            if updated != self.driver_mapping {
                log::info!("3D mouse: applying updated 3DxWare axis profile");
                self.driver_mapping = updated;
            }
            self.mapping_checked_at = now;
        }
        #[cfg(target_os = "linux")]
        {
            self.drivers.choice.store(app.preferences.space_mouse_driver.to_u8(), Ordering::Relaxed);
            app.space_mouse.drivers = Some(self.drivers.present());
        }
        // What the reader has published, and whether anything arrived since the last frame (taken, so
        // the next frame sees only what comes after).
        let reading = {
            let Ok(mut shared) = self.shared.lock() else { return };
            let seen = *shared;
            shared.arrived = false;
            seen
        };
        app.space_mouse.device = reading.device.map(Source::key);
        // One line per connect or reconnect: which device, which read path, and what is saved for it.
        if app.space_mouse.logged_device != app.space_mouse.device {
            if let (Some(key), Some(source)) = (&app.space_mouse.device, reading.device) {
                log::info!(
                    "3D mouse: {key} read through {} (driver setting {:?}); saved calibration: {}; calibration offered before: {}",
                    source.path(),
                    app.preferences.space_mouse_driver,
                    app.preferences.space_mouse_calibrations.contains_key(key),
                    app.preferences.space_mouse_offered.contains(key),
                );
            } else {
                log::info!("3D mouse: no device");
            }
            app.space_mouse.logged_device = app.space_mouse.device.clone();
        }
        let calibration = calibration(app);
        self.calibration = reading.device.zip(calibration);
        // While this device is being calibrated, an axis with nothing fresh reads 0, as a HID
        // device's silence means: the old calibration's rest isn't fed back into the new one. A
        // calibration of another device doesn't count; it ends in `navigate_frame`.
        let calibrating = app.space_mouse.calibrating.as_ref().is_some_and(|(device, _)| app.space_mouse.device.as_ref() == Some(device));
        let rest = if calibrating { Axes::default() } else { at_rest(calibration) };
        app.space_mouse.retained = !reading.arrived;
        navigate_frame(app, ctx, self.driver_mapping, reading.current_with(now, rest, settle_band(calibration)), seconds);
    }
}

/// A stream that answers reads from a script, for the timeout paths.
#[cfg(all(test, target_os = "linux"))]
mod fake {
    use std::collections::VecDeque;
    use std::io::{Error, ErrorKind, Read, Result};

    enum Step {
        Data(Vec<u8>),
        Fail(ErrorKind),
        End,
    }

    #[derive(Default)]
    pub struct Script(VecDeque<Step>);

    impl Script {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn data(&mut self, bytes: &[u8]) -> &mut Self {
            self.0.push_back(Step::Data(bytes.to_vec()));
            self
        }

        pub fn timeout(&mut self, kind: ErrorKind) -> &mut Self {
            self.fail(kind)
        }

        pub fn fail(&mut self, kind: ErrorKind) -> &mut Self {
            self.0.push_back(Step::Fail(kind));
            self
        }

        pub fn end(&mut self) -> &mut Self {
            self.0.push_back(Step::End);
            self
        }
    }

    /// Past the script's last step the stream has ended.
    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            match self.0.pop_front() {
                Some(Step::Data(bytes)) => {
                    let size = bytes.len().min(buf.len());
                    buf[..size].copy_from_slice(&bytes[..size]);
                    Ok(size)
                }
                Some(Step::Fail(kind)) => Err(Error::from(kind)),
                Some(Step::End) | None => Ok(0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_ui_egui::space_mouse::{DeviceMotion, Motion};

    #[cfg(target_os = "linux")]
    #[test]
    fn the_reader_asked_for_is_used_when_it_is_there_and_the_other_when_it_is_not() {
        use Pick::{Direct, Spacenavd};
        use SpaceMouseDriver::{Automatic, Direct as AskDirect, Spacenavd as AskSpacenavd};
        // (asked, spacenavd there, device there) and what is read.
        let cases = [
            (Automatic, true, true, Some(Direct)),
            (Automatic, true, false, Some(Spacenavd)),
            (Automatic, false, true, Some(Direct)),
            (Automatic, false, false, None),
            (AskSpacenavd, true, true, Some(Spacenavd)),
            (AskSpacenavd, true, false, Some(Spacenavd)),
            (AskSpacenavd, false, true, Some(Direct)),
            (AskSpacenavd, false, false, None),
            (AskDirect, true, true, Some(Direct)),
            (AskDirect, true, false, Some(Spacenavd)),
            (AskDirect, false, true, Some(Direct)),
            (AskDirect, false, false, None),
        ];
        for (asked, spacenavd, direct, expected) in cases {
            assert_eq!(choose(asked, spacenavd, direct), expected, "{asked:?}, spacenavd {spacenavd}, device {direct}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn what_the_reader_found_and_the_choice_pass_between_threads() {
        let drivers = Drivers::default();
        assert_eq!(drivers.present(), DriversPresent::default());
        for (spacenavd, direct) in [(true, false), (false, true), (true, true), (false, false)] {
            drivers.publish(DriversPresent { spacenavd, direct, denied: false });
            assert_eq!(drivers.present(), DriversPresent { spacenavd, direct, denied: false });
        }
        assert_eq!(drivers.choice(), SpaceMouseDriver::Automatic);
        for choice in SpaceMouseDriver::ALL {
            drivers.choice.store(choice.to_u8(), Ordering::Relaxed);
            assert_eq!(drivers.choice(), choice);
        }
    }

    /// The logical axes a reading reaches. Logical Rz rolls the view and logical Ry turns it about
    /// its vertical (`navigation::device_camera` in solidcraft-ui-egui, tested there).
    fn rotation(mapping: DriverMapping, rotation: [i16; 3]) -> [f64; 3] {
        mapping.apply(Axes { translation: [0; 3], rotation }).rotation
    }

    #[test]
    fn each_axis_keeps_its_own_age_so_stale_motion_stops() {
        // Report layouts are tested in solidcraft-space-mouse; here, what the reader keeps.
        let now = Instant::now();
        let mut reading = Reading::new(now);
        let layout = Layout::default();
        assert!(!decode_report(&[1, 1, 2], layout, &mut reading, now));
        assert_eq!(reading.current(now, Axes::default()), Axes::default());
        assert!(decode_report(&[1, 0x9c, 0xff, 0x2c, 0x01, 0, 0], layout, &mut reading, now));
        assert!(decode_report(&[2, 0, 0, 1, 0, 0xff, 0xff], layout, &mut reading, now));
        assert_eq!(reading.current(now, Axes::default()), Axes { translation: [-100, 300, 0], rotation: [0, 1, -1] });
        // Only rotation arrives again: translation goes stale, rotation stays current.
        let later = now + STALE_AFTER + Duration::from_millis(1);
        assert!(decode_report(&[2, 4, 0, 5, 0, 6, 0], layout, &mut reading, later));
        assert_eq!(reading.current(later, Axes::default()), Axes { translation: [0; 3], rotation: [4, 5, 6] });
        // A device sending all six in one report updates both at once.
        let combined = solidcraft_space_mouse::Descriptor::parse(&[
            0x05, 0x01, 0x09, 0x08, 0xA1, 0x01, 0x85, 0x01, 0x16, 0xA2, 0xFE, 0x26, 0x5E, 0x01, 0x75, 0x10, 0x95, 0x06, 0x19, 0x30, 0x29, 0x35, 0x81,
            0x02, 0xC0,
        ])
        .layout()
        .expect("six axes declared");
        let mut both = Reading::new(now);
        assert!(decode_report(&[1, 1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0], combined, &mut both, now));
        assert_eq!(both.current(now, Axes::default()), Axes { translation: [1, 2, 3], rotation: [4, 5, 6] });
    }

    #[test]
    fn a_group_spread_over_reports_keeps_the_axes_it_already_has() {
        // X in report 1, Y in report 2 (16-bit, -350..350): X arriving mustn't zero Y, and Y
        // arriving mustn't zero X.
        let split_x_y = solidcraft_space_mouse::Descriptor::parse(&[
            0x05, 0x01, 0x09, 0x08, 0xA1, 0x01, 0x16, 0xA2, 0xFE, 0x26, 0x5E, 0x01, 0x75, 0x10, 0x95, 0x01, // 1 x 16 bit, -350..350
            0x85, 0x01, 0x09, 0x30, 0x81, 0x02, // report 1: X
            0x85, 0x02, 0x09, 0x31, 0x81, 0x02, // report 2: Y
            0xC0,
        ])
        .layout()
        .expect("X and Y declared");
        let now = Instant::now();
        let mut reading = Reading::new(now);
        assert!(decode_report(&[1, 100, 0], split_x_y, &mut reading, now));
        assert!(decode_report(&[2, 200, 0], split_x_y, &mut reading, now));
        assert_eq!(reading.current(now, Axes::default()).translation, [100, 200, 0]);
        // Then only X keeps coming: Y goes stale on its own.
        let later = now + STALE_AFTER + Duration::from_millis(1);
        assert!(decode_report(&[1, 50, 0], split_x_y, &mut reading, later));
        assert_eq!(reading.current(later, Axes::default()).translation, [50, 0, 0]);
    }

    #[test]
    fn only_the_axis_itself_keeps_it_alive() {
        // 3Dconnexion's split layout, plus buttons in report 3 (one byte of bits).
        let mut layout = Layout::default();
        layout.buttons[0] =
            Some(solidcraft_space_mouse::Field { report_id: 3, bit_offset: 0, bits: 1, logical_min: 0, logical_max: 1, relative: false });
        let now = Instant::now();
        let mut reading = Reading::new(now);
        assert!(decode_report(&[1, 100, 0, 0, 0, 0, 0], layout, &mut reading, now));
        // Just before the timeout X is still there; at exactly the timeout it's stale.
        assert_eq!(reading.current(now + STALE_AFTER - Duration::from_millis(1), Axes::default()).translation, [100, 0, 0]);
        assert_eq!(reading.current(now + STALE_AFTER, Axes::default()).translation, [0; 3]);
        // Button reports and reports with an id the layout doesn't know keep coming, but neither
        // refreshes X: it goes stale on time all the same.
        for step in 1..=10 {
            let at = now + STALE_AFTER * step / 10;
            assert!(decode_report(&[3, 1], layout, &mut reading, at));
            assert!(!decode_report(&[0x17, 80], layout, &mut reading, at));
        }
        assert_eq!(reading.current(now + STALE_AFTER, Axes::default()).translation, [0; 3]);
    }

    /// A device that repeats its reading at rest asks for no frame; a reading that changes does.
    #[test]
    fn a_repeated_report_at_rest_asks_for_no_frame() {
        let ctx = Context::default();
        let shared = Mutex::new(Reading::new(Instant::now()));
        let layout = Layout::default();
        let frame = || {
            let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
            out.textures_delta.clear();
        };
        let asked = || {
            frame();
            ctx.repaint_causes().iter().any(|c| c.file.ends_with("space_mouse.rs"))
        };
        frame();
        // Report 1 carries translation (see the split-layout test).
        let report = [1, 0x9c, 0xff, 0x2c, 0x01, 0, 0];
        publish(&ctx, &shared, layout, &report);
        assert!(asked(), "a report that moves the cap asks for a frame");
        publish(&ctx, &shared, layout, &report);
        assert!(!asked(), "the same reading at rest is no news");
        publish(&ctx, &shared, layout, &[1, 0x9d, 0xff, 0x2c, 0x01, 0, 0]);
        assert!(asked(), "a changed reading asks for a frame");
    }

    /// A button press asks for a frame even though no axis moved, and a repeated report still counts
    /// as arrived (the calibration window needs that) without asking for one.
    #[test]
    fn a_button_report_asks_for_a_frame_and_a_repeat_only_marks_arrival() {
        let mut layout = Layout::default();
        layout.buttons[0] =
            Some(solidcraft_space_mouse::Field { report_id: 3, bit_offset: 0, bits: 1, logical_min: 0, logical_max: 1, relative: false });
        let ctx = Context::default();
        let shared = Mutex::new(Reading::new(Instant::now()));
        let asked = || {
            let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
            out.textures_delta.clear();
            ctx.repaint_causes().iter().any(|c| c.file.ends_with("space_mouse.rs"))
        };
        asked();
        publish(&ctx, &shared, layout, &[3, 1]);
        assert!(asked(), "a button going down asks for a frame");
        shared.lock().unwrap().arrived = false;
        publish(&ctx, &shared, layout, &[3, 1]);
        assert!(!asked(), "the same buttons again are no news");
        assert!(shared.lock().unwrap().arrived, "but the report did arrive");
    }

    /// A simulated 3D-mouse interface. Each read takes the next scripted step, a report or a
    /// timeout (empty), and sets the shared clock to the step's time. Once the script runs out,
    /// the device is unplugged.
    struct Simulated<'a> {
        clock: &'a std::cell::Cell<Instant>,
        start: Instant,
        steps: std::cell::RefCell<std::collections::VecDeque<(u64, Vec<u8>)>>,
    }

    impl ReportSource for Simulated<'_> {
        fn read_report(&self, report: &mut [u8], _timeout_ms: i32) -> Result<usize, String> {
            let (ms, bytes) = self.steps.borrow_mut().pop_front().ok_or_else(|| "unplugged".to_string())?;
            self.clock.set(self.start + Duration::from_millis(ms));
            report.get_mut(..bytes.len()).ok_or_else(|| "report too long".to_string())?.copy_from_slice(&bytes);
            Ok(bytes.len())
        }
    }

    #[derive(Debug, PartialEq)]
    enum Seen {
        /// The product id now read.
        Device(u16),
        /// The buttons it had reported down before it was read, its baseline.
        Baseline(u32),
        /// A report published, the product id read when it was, and whether it showed use.
        Report(u16, Vec<u8>, bool),
        Gone,
    }

    /// Follows simulated devices, given as (product id, scripted steps in ms), and returns what
    /// the reader was told.
    fn follow_simulated(devices: &[(u16, &[(u64, &[u8])])]) -> Vec<Seen> {
        follow_simulated_with(Layout::default(), devices)
    }

    /// As `follow_simulated`, with every device's reports laid out as `layout`.
    fn follow_simulated_with(layout: Layout, devices: &[(u16, &[(u64, &[u8])])]) -> Vec<Seen> {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let candidates = devices
            .iter()
            .map(|(product, steps)| Candidate {
                device: Simulated {
                    clock: &clock,
                    start,
                    steps: std::cell::RefCell::new(steps.iter().map(|(ms, bytes)| (*ms, bytes.to_vec())).collect()),
                },
                layout,
                ids: (0x256f, *product),
                strings: Vec::new(),
            })
            .collect();
        let (mut seen, mut read) = (Vec::new(), 0);
        follow(
            candidates,
            || clock.get(),
            |event| match event {
                Followed::Device((_, product), buttons) => {
                    read = product;
                    seen.push(Seen::Device(product));
                    seen.extend(buttons.map(Seen::Baseline));
                }
                Followed::Report(layout, bytes) => seen.push(Seen::Report(read, bytes.to_vec(), shows_use(*layout, bytes))),
                Followed::Gone => seen.push(Seen::Gone),
            },
        );
        seen
    }

    const RECEIVER: u16 = 0xc652;
    const SPACEMOUSE: u16 = 0xc63a;
    const PUSH_X: &[u8] = &[1, 100, 0, 0, 0, 0, 0];
    const AT_REST: &[u8] = &[1, 0, 0, 0, 0, 0, 0];
    const TILT: &[u8] = &[2, 0, 0, 50, 0, 0, 0];

    /// #3988: a Universal Receiver with nothing paired is found first and never fails, and a wired
    /// SpaceMouse next to it was never read. The receiver is followed only until the SpaceMouse
    /// moves; the SpaceMouse's reports, its zeros included, are read from then on.
    #[test]
    fn an_idle_receiver_does_not_keep_a_spacemouse_from_being_read() {
        let receiver: &[(u64, &[u8])] = &[(0, &[]), (10, &[]), (20, &[]), (30, &[])];
        let spacemouse: &[(u64, &[u8])] = &[(5, &[]), (15, PUSH_X), (25, AT_REST), (35, TILT)];
        assert_eq!(
            follow_simulated(&[(RECEIVER, receiver), (SPACEMOUSE, spacemouse)]),
            [
                Seen::Device(RECEIVER),
                Seen::Device(SPACEMOUSE),
                Seen::Report(SPACEMOUSE, PUSH_X.to_vec(), true),
                Seen::Report(SPACEMOUSE, AT_REST.to_vec(), false),
                Seen::Report(SPACEMOUSE, TILT.to_vec(), true),
                // The receiver is unplugged first, then the SpaceMouse, the one being read.
                Seen::Gone,
            ]
        );
    }

    /// Two devices in use at once don't trade the reading: the other takes over only after the
    /// followed one has shown no use for `STALE_AFTER`. A report at rest or with an id the layout
    /// doesn't know shows no use, and only the followed device's reports are published.
    #[test]
    fn another_device_takes_over_only_once_the_followed_one_is_still() {
        let unknown_id: &[u8] = &[0x17, 80];
        let first: &[(u64, &[u8])] = &[(0, PUSH_X), (100, AT_REST), (200, &[])];
        let second: &[(u64, &[u8])] = &[(50, PUSH_X), (150, unknown_id), (250, TILT)];
        assert_eq!(
            follow_simulated(&[(0xc635, first), (SPACEMOUSE, second)]),
            [
                Seen::Device(0xc635),
                Seen::Report(0xc635, PUSH_X.to_vec(), true),
                // The second moves 50 ms after the first did: not yet. The first's own rest
                // report is read, and the second's unknown report is not.
                Seen::Report(0xc635, AT_REST.to_vec(), false),
                // 250 ms after the first last moved, the second takes over.
                Seen::Device(SPACEMOUSE),
                Seen::Report(SPACEMOUSE, TILT.to_vec(), true),
                Seen::Gone,
            ]
        );
    }

    /// One device is read as before: every report, until it fails. When the followed one of
    /// several fails, the reading is reset and the next one is read.
    #[test]
    fn a_device_that_fails_hands_the_reading_to_the_next() {
        let only: &[(u64, &[u8])] = &[(0, &[]), (1, AT_REST), (2, &[0x17, 80])];
        assert_eq!(
            follow_simulated(&[(SPACEMOUSE, only)]),
            [
                Seen::Device(SPACEMOUSE),
                Seen::Report(SPACEMOUSE, AT_REST.to_vec(), false),
                Seen::Report(SPACEMOUSE, vec![0x17, 80], false),
                Seen::Gone
            ]
        );
        let unplugged: &[(u64, &[u8])] = &[];
        let next: &[(u64, &[u8])] = &[(1, PUSH_X)];
        assert_eq!(
            follow_simulated(&[(SPACEMOUSE, unplugged), (RECEIVER, next)]),
            [Seen::Device(SPACEMOUSE), Seen::Gone, Seen::Device(RECEIVER), Seen::Report(RECEIVER, PUSH_X.to_vec(), true), Seen::Gone]
        );
        assert_eq!(follow_simulated(&[]), []);
    }

    /// A button press that makes a device the one in use is a press. The buttons it reported while
    /// another was read are its baseline; the reader keeps them, so the press isn't taken for a
    /// button already held, as a device's first button report is.
    #[test]
    fn a_press_that_takes_over_counts_as_a_press() {
        let mut layout = Layout::default();
        layout.buttons[0] =
            Some(solidcraft_space_mouse::Field { report_id: 3, bit_offset: 0, bits: 1, logical_min: 0, logical_max: 1, relative: false });
        let (none_down, first_down): (&[u8], &[u8]) = (&[3, 0], &[3, 1]);
        let receiver: &[(u64, &[u8])] = &[(0, &[]), (10, &[]), (20, &[])];
        let spacemouse: &[(u64, &[u8])] = &[(5, none_down), (15, first_down), (25, &[])];
        assert_eq!(
            follow_simulated_with(layout, &[(RECEIVER, receiver), (SPACEMOUSE, spacemouse)]),
            [Seen::Device(RECEIVER), Seen::Device(SPACEMOUSE), Seen::Baseline(0), Seen::Report(SPACEMOUSE, first_down.to_vec(), true), Seen::Gone,]
        );
        // What the reader does with that: the press after the baseline is a tap.
        let mut reading = Reading { buttons_known: false, ..Reading::new(Instant::now()) };
        reading.set_buttons(0);
        assert!(decode_report(first_down, layout, &mut reading, Instant::now()));
        assert_eq!((reading.buttons, reading.taps), (1, 1));
        // Without one, the same report is the baseline: held, not pressed.
        let mut unseen = Reading { buttons_known: false, ..Reading::new(Instant::now()) };
        assert!(decode_report(first_down, layout, &mut unseen, Instant::now()));
        assert_eq!((unseen.buttons, unseen.taps), (1, 0));
    }

    #[test]
    fn a_held_reading_stays_until_the_next_one_and_a_hid_reading_goes_stale() {
        let now = Instant::now();
        let axes = Axes { translation: [0, 200, 0], rotation: [0; 3] };
        let later = now + STALE_AFTER * 20;
        // spacenavd sends nothing while the values stay the same: the push lasts.
        let held = Reading { axes, held: true, ..Reading::new(now) };
        assert_eq!(held.current(later, Axes::default()), axes);
        // A HID device repeats its report: silence means the cap, or the device, is gone.
        let hid = Reading { axes, ..Reading::new(now) };
        assert_eq!(hid.current(later, Axes::default()), Axes::default());
    }

    #[test]
    fn a_held_reading_near_rest_settles_after_the_timeout() {
        // The last records of seven releases from tangency-labs/solidcraft#3523: spacenavd sends
        // nothing after them.
        let leftovers = [
            [15, -60, 13, 59, 34, 129],
            [-12, -69, 4, -26, 7, -6],
            [0, -45, -3, 16, 8, -60],
            [-31, 27, 22, -5, 6, -10],
            [0, -30, -3, 3, -6, 0],
            [5, 32, -2, 43, -8, 12],
            [3, 2, -22, 3, 6, 5],
        ];
        let now = Instant::now();
        for [x, y, z, rx, ry, rz] in leftovers {
            let axes = Axes { translation: [x, y, z], rotation: [rx, ry, rz] };
            let held = Reading { axes, held: true, ..Reading::new(now) };
            assert_eq!(held.current(now + HELD_SETTLE_AFTER - Duration::from_millis(1), Axes::default()), axes);
            assert_eq!(held.current(now + HELD_SETTLE_AFTER, Axes::default()), Axes::default());
        }
    }

    /// Unplug and replug under a driver choice of Direct: the daemon is read while the device is
    /// away, and the read ends when the device is back, so the device's own key, and with it the saved
    /// calibration, applies again (it stayed on the daemon, whose key has no calibration, before).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_replugged_device_takes_the_reading_back_from_the_daemon_it_fell_back_to() {
        use SpaceMouseDriver::{Direct, Spacenavd};
        let both = DriversPresent { spacenavd: true, direct: true, denied: false };
        let only_daemon = DriversPresent { spacenavd: true, direct: false, denied: false };
        // Direct asked for, unplugged: the daemon is read. It comes back: switch.
        let pick = choose(Direct, true, false);
        assert_eq!(pick, Some(Pick::Spacenavd));
        assert!(switch_back(Direct, pick, both, None, UNREADABLE_RETRY));
        assert!(!switch_back(Direct, pick, only_daemon, None, UNREADABLE_RETRY), "still away");
        // A device that was there at the start (but could not be opened) does not make the read flap.
        assert!(!switch_back(Direct, pick, both, Some(Duration::ZERO), UNREADABLE_RETRY));
        // Within the backoff after a failed open, not yet.
        assert!(!switch_back(Direct, pick, both, Some(Duration::from_secs(1)), UNREADABLE_RETRY));
        // Spacenavd, asked for, keeps the daemon whatever else appears.
        assert!(!switch_back(Spacenavd, Some(Pick::Spacenavd), both, None, UNREADABLE_RETRY));
        // The key a replugged device gets is its ids, whatever hidraw node it comes up on.
        assert_eq!(Source::Hid(Some((0x046d, 0xc62b))).key(), "hid 046d:c62b");
    }

    /// Automatic reads the device whenever it opens, calibrated or not, and the daemon only when no device
    /// does; Direct and Spacenavd keep meaning what they say.
    #[cfg(target_os = "linux")]
    #[test]
    fn automatic_reads_the_device_when_it_opens_and_otherwise_the_daemon() {
        use SpaceMouseDriver::{Automatic, Direct, Spacenavd};
        // Both live: the device.
        assert_eq!(choose(Automatic, true, true), Some(Pick::Direct));
        // The device does not open (no permission, backing off): the daemon, under its own key.
        let unopenable = direct_usable(true, Some(Duration::from_secs(1)), UNREADABLE_RETRY);
        assert_eq!(choose(Automatic, true, unopenable), Some(Pick::Spacenavd));
        // Only one source is there: that one. Neither: nothing.
        assert_eq!(choose(Automatic, false, true), Some(Pick::Direct));
        assert_eq!(choose(Automatic, true, false), Some(Pick::Spacenavd));
        assert_eq!(choose(Automatic, false, false), None);
        // Asked for explicitly, they mean what they say.
        assert_eq!(choose(Spacenavd, true, true), Some(Pick::Spacenavd));
        assert_eq!(choose(Direct, true, true), Some(Pick::Direct));
        // The replug path: unplugged -> daemon, replugged -> the read ends and the device is taken.
        let (both, only_daemon) =
            (DriversPresent { spacenavd: true, direct: true, denied: false }, DriversPresent { spacenavd: true, direct: false, denied: false });
        let pick = choose(Automatic, true, false);
        assert_eq!(pick, Some(Pick::Spacenavd));
        assert!(!switch_back(Automatic, pick, only_daemon, None, UNREADABLE_RETRY), "still away");
        assert!(switch_back(Automatic, pick, both, None, UNREADABLE_RETRY), "back");
        assert!(!switch_back(Automatic, pick, both, Some(Duration::from_secs(1)), UNREADABLE_RETRY), "backing off after a failed open");
        // Already reading the device: nothing to switch.
        assert!(!switch_back(Automatic, Some(Pick::Direct), both, None, UNREADABLE_RETRY));
        // Spacenavd, asked for, is never left for the device.
        assert!(!switch_back(Spacenavd, Some(Pick::Spacenavd), both, None, UNREADABLE_RETRY));
    }

    /// A device that failed to open and is readable now ends the read of the daemon once the wait is over,
    /// and not before; a device that works is never given up for a daemon.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_device_that_failed_to_open_ends_the_daemon_read_when_its_wait_is_over() {
        use SpaceMouseDriver::{Automatic, Direct, Spacenavd};
        let both = DriversPresent { spacenavd: true, direct: true, denied: false };
        let wait = UNREADABLE_RETRY;
        for wanted in [Direct, Automatic] {
            assert!(!switch_back(wanted, Some(Pick::Spacenavd), both, Some(Duration::from_secs(1)), wait), "{wanted:?}: inside the wait");
            assert!(switch_back(wanted, Some(Pick::Spacenavd), both, Some(wait), wait), "{wanted:?}: the wait is over");
            assert!(!switch_back(wanted, Some(Pick::Direct), both, Some(wait), wait), "{wanted:?}: already reading the device");
        }
        // Spacenavd, asked for, stays on the daemon whatever the wait.
        assert!(!switch_back(Spacenavd, Some(Pick::Spacenavd), both, Some(wait), wait));
        // A device read directly because there was no daemon is not given up when one appears.
        assert!(!switch_back(Automatic, Some(Pick::Direct), both, None, wait));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_direct_device_that_cannot_be_opened_gives_way_to_a_working_daemon_for_a_while() {
        use SpaceMouseDriver::{Automatic, Direct, Spacenavd};
        for wanted in [Automatic, Spacenavd, Direct] {
            // Found, but the open just failed: the daemon is used, whichever was asked for.
            let ok = direct_usable(true, Some(Duration::from_secs(1)), UNREADABLE_RETRY);
            assert!(!ok);
            assert_eq!(choose(wanted, true, ok), Some(Pick::Spacenavd), "{wanted:?}");
        }
        // Found and never failed: Direct wins when asked for. After the wait it is tried again.
        assert_eq!(choose(Direct, true, direct_usable(true, None, UNREADABLE_RETRY)), Some(Pick::Direct));
        assert_eq!(choose(Direct, true, direct_usable(true, Some(UNREADABLE_RETRY), UNREADABLE_RETRY)), Some(Pick::Direct));
        // Not found is not usable however long ago.
        assert!(!direct_usable(false, None, UNREADABLE_RETRY));
        // With no daemon either, nothing is read rather than a loop on the device that fails.
        assert_eq!(choose(Direct, false, false), None);
    }

    #[test]
    fn the_held_settle_band_follows_the_calibrated_travel() {
        let axis = |source, full: [u16; 2]| solidcraft_space_mouse::AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full };
        let calibration =
            |full| Calibration { dead_zone_share: 0.0, kind: Default::default(), axes: std::array::from_fn(|source| axis(source, [full; 2])) };
        assert_eq!(settle_band(None), Some(HELD_SETTLE_BAND));
        assert_eq!(settle_band(Some(calibration(350))), Some(HELD_SETTLE_BAND));
        assert_eq!(settle_band(Some(calibration(3500))), Some(HELD_SETTLE_BAND));
        assert_eq!(settle_band(Some(calibration(50))), Some(21));
        // A sustained full push on a cap whose full travel is 50 stays a full push.
        let now = Instant::now();
        let push = Axes { translation: [50, 0, 0], rotation: [0; 3] };
        let held = Reading { axes: push, held: true, ..Reading::new(now) };
        let later = now + HELD_SETTLE_AFTER * 4;
        assert_eq!(held.current_with(later, Axes::default(), settle_band(Some(calibration(50)))), push);
        // While the same reading on an ordinary cap (full 350) is a leftover and settles.
        assert_eq!(held.current_with(later, Axes::default(), settle_band(Some(calibration(350)))), Axes::default());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_reachable_daemon_is_not_a_device_until_it_sends_a_record() {
        let ctx = Context::default();
        let shared = Mutex::new(Reading::new(Instant::now()));
        connected(&ctx, &shared, Source::Spacenavd);
        assert_eq!(shared.lock().unwrap().device, None, "a socket that answers is no evidence");
        publish_held(&ctx, &shared, Axes::default());
        assert_eq!(shared.lock().unwrap().device, Some(Source::Spacenavd), "the first record is");
        // A button record is evidence too.
        connected(&ctx, &shared, Source::Spacenavd);
        assert_eq!(shared.lock().unwrap().device, None);
        publish_button(&ctx, &shared, 2, true);
        assert_eq!(shared.lock().unwrap().device, Some(Source::Spacenavd));
        // An opened device is evidence at once.
        connected(&ctx, &shared, Source::Hid(Some((0x046d, 0xc62b))));
        assert_eq!(shared.lock().unwrap().device, Some(Source::Hid(Some((0x046d, 0xc62b)))));
        // Gone, and nothing again.
        reset(&ctx, &shared);
        assert_eq!(shared.lock().unwrap().device, None);
    }

    #[test]
    fn a_tap_between_two_frames_is_still_a_press_and_ends_with_nothing_held() {
        let mut reading = Reading::new(Instant::now());
        // Down and up before the UI looks: the mask is back to zero, the press is not lost.
        reading.set_buttons(0b100);
        reading.set_buttons(0);
        assert_eq!((reading.buttons, reading.taps), (0, 0b100));
        // Holding, then more taps, accumulate until taken; held buttons are not new taps.
        reading.set_buttons(0b001);
        reading.set_buttons(0b001);
        assert_eq!(reading.taps, 0b101);
    }

    #[test]
    fn a_trackballs_held_reading_never_settles_but_a_caps_and_an_uncalibrated_devices_do() {
        let now = Instant::now();
        let axes = Axes { translation: [40, 0, 0], rotation: [0; 3] };
        let held = Reading { axes, held: true, ..Reading::new(now) };
        let later = now + HELD_SETTLE_AFTER * 4;
        let calibration = |kind| {
            let axis = |source| solidcraft_space_mouse::AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] };
            Calibration { dead_zone_share: 0.0, kind, axes: std::array::from_fn(axis) }
        };
        for (calibration, settles_to_rest) in [
            (None, true),
            (Some(calibration(solidcraft_space_mouse::Kind::ReturnToCenter)), true),
            (Some(calibration(solidcraft_space_mouse::Kind::Trackball)), false),
        ] {
            let read = held.current_with(later, Axes::default(), settle_band(calibration));
            assert_eq!(read == Axes::default(), settles_to_rest, "{calibration:?}");
        }
    }

    #[test]
    fn a_settled_held_reading_reads_as_the_devices_own_rest() {
        let now = Instant::now();
        let rest = Axes { translation: [10, -8, 0], rotation: [0, 5, 0] };
        // 40 from zero on X, but only 30 from this device's rest.
        let held = Reading { axes: Axes { translation: [40, -8, 0], rotation: [0, 5, 0] }, held: true, ..Reading::new(now) };
        assert_eq!(held.current(now + HELD_SETTLE_AFTER, rest), rest);
    }

    #[test]
    fn a_firm_held_push_stays_however_long_it_lasts() {
        let now = Instant::now();
        let later = now + Duration::from_secs(10);
        let push = Axes { translation: [0, 350, 0], rotation: [0; 3] };
        let held = Reading { axes: push, held: true, ..Reading::new(now) };
        assert_eq!(held.current(later, Axes::default()), push);
        // One firm axis keeps the small ones with it.
        let mixed = Axes { translation: [5, 300, -10], rotation: [3, 0, 20] };
        let held = Reading { axes: mixed, held: true, ..Reading::new(now) };
        assert_eq!(held.current(later, Axes::default()), mixed);
        // The band is per reading, edge included: at it is a leftover, one past it is a hold.
        let band = HELD_SETTLE_BAND as i16;
        let twist = |value| Reading { axes: Axes { translation: [0; 3], rotation: [0, 0, value] }, held: true, ..Reading::new(now) };
        assert_eq!(twist(band).current(later, Axes::default()), Axes::default());
        assert_eq!(twist(band + 1).current(later, Axes::default()).rotation, [0, 0, band + 1]);
    }

    #[test]
    fn applies_3dxware_axis_swaps_and_reversals() {
        let profile = br#"
            <AppCfg><Devices><Device><AxisBank>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_X</ActionID></Input><Output><ActionID>HIDMultiAxis_X</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Y</ActionID></Input><Output><ActionID>HIDMultiAxis_Z</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Z</ActionID></Input><Output><ActionID>HIDMultiAxis_Y</ActionID><Reversed>true</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rx</ActionID></Input><Output><ActionID>HIDMultiAxis_Rx</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Ry</ActionID></Input><Output><ActionID>HIDMultiAxis_Rz</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rz</ActionID></Input><Output><ActionID>HIDMultiAxis_Ry</ActionID><Reversed>true</Reversed></Output></Axis>
            </AxisBank></Device></Devices></AppCfg>
        "#;
        let mapping = parse_driver_mapping(profile).expect("valid 3DxWare profile");
        let motion = mapping.apply(Axes { translation: [350, 0, 350], rotation: [350, 0, 350] });
        assert_eq!(motion.translation, [1.0, -1.0, 0.0]);
        assert_eq!(motion.rotation, [1.0, -1.0, 0.0]);
    }

    #[test]
    fn without_a_profile_the_drivers_object_mode_axes_are_used() {
        // 3DxWare's object-mode mapping, as a profile (#1172: confirmed on a SpaceMouse Compact).
        let object_mode = br#"
            <AppCfg><Devices><Device><AxisBank>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_X</ActionID></Input><Output><ActionID>HIDMultiAxis_X</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Y</ActionID></Input><Output><ActionID>HIDMultiAxis_Y</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Z</ActionID></Input><Output><ActionID>HIDMultiAxis_Z</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rx</ActionID></Input><Output><ActionID>HIDMultiAxis_Rx</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Ry</ActionID></Input><Output><ActionID>HIDMultiAxis_Rz</ActionID><Reversed>true</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rz</ActionID></Input><Output><ActionID>HIDMultiAxis_Ry</ActionID><Reversed>true</Reversed></Output></Axis>
            </AxisBank></Device></Devices></AppCfg>
        "#;
        let profile = parse_driver_mapping(object_mode).expect("valid 3DxWare profile");
        // On every platform (#1172 on Linux, #1380 on Windows).
        assert_eq!(without_profile(), profile);
        // Tilting the cap rolls the view and twisting it spins it, as with the driver.
        assert_eq!(rotation(without_profile(), [0, 350, 0]), [0.0, 0.0, -1.0]);
        assert_eq!(rotation(without_profile(), [0, 0, 350]), [0.0, -1.0, 0.0]);
    }

    #[test]
    fn mapped_roll_and_spin_reach_the_roll_and_turn_axes() {
        let profile = br#"
            <AppCfg><Devices><Device><AxisBank>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Ry</ActionID></Input><Output><ActionID>HIDMultiAxis_Rz</ActionID><Reversed>false</Reversed></Output></Axis>
              <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rz</ActionID></Input><Output><ActionID>HIDMultiAxis_Ry</ActionID><Reversed>true</Reversed></Output></Axis>
            </AxisBank></Device></Devices></AppCfg>
        "#;
        let mapping = parse_driver_mapping(profile).expect("valid 3DxWare profile");
        // The physical roll reaches the roll axis only, and the physical spin the turn only.
        assert_eq!(rotation(mapping, [0, 350, 0]), [0.0, 0.0, 1.0]);
        assert_eq!(rotation(mapping, [0, 0, 350]), [0.0, -1.0, 0.0]);
    }

    #[test]
    fn each_frame_hands_the_view_its_motion_through_the_drivers_mapping() {
        // The reader's latest tilt, and a profile that makes tilt roll: the frame's motion rolls.
        // The frame runs at the reading's own time, so a slow runner can't let the reading go stale.
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
        app.home.open = false;
        let now = Instant::now();
        let mut reading = Reading::new(now);
        reading.axes.rotation = [0, 350, 0];
        let mut mouse = SpaceMouse {
            shared: Arc::new(Mutex::new(reading)),
            #[cfg(target_os = "linux")]
            drivers: Arc::default(),
            last_frame: now - Duration::from_millis(20),
            driver_mapping: DriverMapping::object_mode(),
            mapping_checked_at: now,
            wheel: WheelGuard::default(),
            started: now,
            calibration: None,
        };
        // A window that has run a frame with focus, as the app's always has by now.
        let ctx = Context::default();
        // Clear the frame's texture changes, or dropping them panics in debug builds (as in #60).
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        mouse.tick_at(&mut app, &ctx, now);
        let DeviceMotion { motion, seconds } = app.viewport.device_motion.expect("the frame hands the page its motion");
        assert_eq!(motion, Motion { translation: [0.0; 3], rotation: [0.0, 0.0, -1.0] });
        assert!(seconds > 0.0);
    }

    fn wheel() -> egui::Event {
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            modifiers: Default::default(),
            phase: egui::TouchPhase::Move,
        }
    }

    fn mouse(reading: Reading, now: Instant) -> SpaceMouse {
        SpaceMouse {
            shared: Arc::new(Mutex::new(reading)),
            #[cfg(target_os = "linux")]
            drivers: Arc::default(),
            last_frame: now,
            driver_mapping: DriverMapping::object_mode(),
            mapping_checked_at: now,
            wheel: WheelGuard::default(),
            started: now,
            calibration: None,
        }
    }

    const DEVICE: Source = Source::Hid(Some((0x1234, 0x5678)));

    /// A device whose raw axes all rest at 100, calibrated as such.
    fn resting_at_100() -> Calibration {
        Calibration {
            dead_zone_share: 0.0,
            kind: Default::default(),
            axes: std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration {
                source,
                reversed: false,
                rest: 100,
                dead_zone: 0,
                full: [350; 2],
            }),
        }
    }

    #[test]
    fn a_calibrated_device_resting_off_zero_leaves_the_wheel_alone() {
        // Read raw, X at 100 is the cap pushed past the dead zone: the wheel is dropped.
        let now = Instant::now();
        let mut resting = Reading::new(now);
        resting.device = Some(DEVICE);
        resting.axes = Axes { translation: [100; 3], rotation: [100; 3] };
        let wheel_kept = |calibration| {
            let mut mouse = mouse(resting, now);
            mouse.calibration = calibration;
            let mut raw = egui::RawInput { events: vec![wheel()], ..Default::default() };
            mouse.raw_input_hook_at(true, &mut raw, now);
            raw.events.len() == 1
        };
        assert!(!wheel_kept(None));
        // Calibrated, it's at rest, so the wheel is the user's.
        assert!(wheel_kept(Some((DEVICE, resting_at_100()))));
        // Another device's calibration, kept from the last frame, isn't used for this one.
        assert!(!wheel_kept(Some((Source::Hid(Some((0x256f, 0xc63a))), resting_at_100()))));
    }

    #[test]
    fn a_calibrated_device_resting_off_zero_doesnt_drift_when_nothing_fresh_arrives() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
        app.home.open = false;
        app.preferences.space_mouse_calibrations.insert(DEVICE.key(), resting_at_100());
        // A window that has run a frame with focus, as the app's always has by now.
        let ctx = Context::default();
        let mut frame = ctx.run_ui(egui::RawInput::default(), |_| {});
        frame.textures_delta.clear();
        let motion = |app: &mut SolveApp, reading: Reading, now: Instant| {
            let mut mouse = mouse(reading, now);
            mouse.last_frame = now - Duration::from_millis(20);
            app.viewport.device_motion = None;
            mouse.tick_at(app, &ctx, now);
            app.viewport.device_motion.is_some()
        };
        // Connected, and nothing reported yet.
        let shared = Mutex::new(Reading::new(Instant::now()));
        connected(&Context::default(), &shared, DEVICE);
        let fresh = *shared.lock().unwrap();
        let now = Instant::now();
        assert!(!motion(&mut app, fresh, now), "connected, nothing reported yet");
        // The same, while another device's calibration window was open: it ends, and this one
        // doesn't move on the first frame either.
        app.space_mouse.calibrating = Some(("hid 256f:c63a".into(), solidcraft_space_mouse::Wizard::default()));
        assert!(!motion(&mut app, fresh, now), "connected mid-calibration of another device");
        assert!(app.space_mouse.calibrating.is_none());
        // Translation reported at rest, rotation not for a while.
        let old = now - STALE_AFTER - Duration::from_millis(1);
        let partly = Reading { axes: Axes { translation: [100; 3], rotation: [0; 3] }, at: [now, now, now, old, old, old], ..fresh };
        assert!(!motion(&mut app, partly, now), "translation at rest, rotation stale");
        // Pushed, then the reports stopped.
        let pushed = Reading { axes: Axes { translation: [450, 100, 100], rotation: [100; 3] }, at: [old; 6], ..fresh };
        assert!(!motion(&mut app, pushed, now), "the push timed out");
        assert!(motion(&mut app, Reading { at: [now; 6], ..pushed }, now), "a fresh push moves the view");
    }

    #[test]
    fn the_wheel_3dxware_makes_from_the_cap_is_dropped() {
        // The cap tilted forward: 3DxWare also sends wheel turns (#1380). They are dropped.
        let now = Instant::now();
        let mut tilted = Reading::new(now);
        tilted.axes.rotation = [-200, 0, 0];
        let mut moving = mouse(tilted, now);
        let mut raw = egui::RawInput { events: vec![wheel(), egui::Event::Zoom(1.1), egui::Event::Text("a".into())], ..Default::default() };
        moving.raw_input_hook_at(true, &mut raw, now);
        assert_eq!(raw.events, [egui::Event::Text("a".into())]);
        // With navigation off, the wheel is the cap's only way to move the view: kept.
        let mut raw = egui::RawInput { events: vec![wheel()], ..Default::default() };
        moving.raw_input_hook_at(false, &mut raw, now);
        assert_eq!(raw.events.len(), 1);
        // The cap at rest: the wheel is the user's.
        let mut resting = mouse(Reading::new(now), now);
        let mut raw = egui::RawInput { events: vec![wheel()], ..Default::default() };
        resting.raw_input_hook_at(true, &mut raw, now);
        assert_eq!(raw.events.len(), 1);
    }

    #[test]
    fn the_wheel_is_the_users_again_once_the_cap_has_been_still_a_moment() {
        let now = Instant::now();
        let mut tilted = Reading::new(now);
        tilted.axes.rotation = [-200, 0, 0];
        let mut cap = mouse(tilted, now);
        let mut raw = egui::RawInput { events: vec![wheel()], ..Default::default() };
        cap.raw_input_hook_at(true, &mut raw, now);
        assert!(raw.events.is_empty(), "dropped while the cap moves");
        // Let go, then wait out the quiet time.
        let later = now + Duration::from_secs_f64(solidcraft_ui_egui::space_mouse::WHEEL_QUIET_S) + Duration::from_millis(60);
        if let Ok(mut reading) = cap.shared.lock() {
            *reading = Reading::new(later);
        }
        let mut raw = egui::RawInput { events: vec![wheel()], ..Default::default() };
        cap.raw_input_hook_at(true, &mut raw, later);
        assert_eq!(raw.events.len(), 1, "kept once the cap has been still");
    }

    /// A stand-in for the machine: a device that can be plugged, unplugged and made unreadable, and a
    /// daemon that can be there or not. Every read is a socket pair, so what the reader reads is
    /// whatever the test writes, and a device is unplugged by closing its end.
    #[cfg(target_os = "linux")]
    mod machine {
        use super::super::*;
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use std::path::{Path, PathBuf};

        pub const IDS: (u16, u16) = (0x046d, 0xc62b);

        #[derive(Default)]
        pub struct World {
            pub device: bool,
            pub daemon: bool,
            pub openable: bool,
            pub device_end: Option<UnixStream>,
            pub daemon_end: Option<UnixStream>,
            pub opens: u32,
            pub connects: u32,
        }

        #[derive(Clone, Default)]
        pub struct Fake(pub Arc<Mutex<World>>);

        impl Fake {
            pub fn world(&self) -> std::sync::MutexGuard<'_, World> {
                self.0.lock().unwrap()
            }

            pub fn unplug(&self) {
                let mut world = self.world();
                world.device = false;
                world.device_end = None;
            }

            /// The daemon sends a record of its own (kind, then seven numbers).
            pub fn daemon_sends(&self, data: [i32; 8]) {
                let mut bytes = [0_u8; 32];
                for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(data) {
                    *chunk = value.to_ne_bytes();
                }
                if let Some(end) = self.world().daemon_end.as_mut() {
                    end.write_all(&bytes).ok();
                }
            }

            /// The daemon sends a motion record (all zeros: the daemon's rest), which is what shows
            /// a device behind it.
            pub fn daemon_sends_a_record(&self) {
                if let Some(end) = self.world().daemon_end.as_mut() {
                    end.write_all(&[0_u8; 32]).ok();
                }
            }
        }

        impl System for Fake {
            fn find(&self) -> Option<hidraw::Device> {
                self.world().device.then(|| hidraw::Device { path: PathBuf::from("/dev/hidraw-fake"), layout: Layout::default(), ids: Some(IDS) })
            }

            fn daemon_exists(&self) -> bool {
                self.world().daemon
            }

            fn daemon_connect(&self) -> Option<UnixStream> {
                let mut world = self.world();
                if !world.daemon {
                    return None;
                }
                let (ours, theirs) = UnixStream::pair().unwrap();
                world.daemon_end = Some(theirs);
                world.connects += 1;
                Some(ours)
            }

            fn open(&self, _: &Path) -> std::io::Result<Box<dyn std::io::Read>> {
                let mut world = self.world();
                world.opens += 1;
                if !world.openable {
                    return Err(std::io::ErrorKind::PermissionDenied.into());
                }
                let (ours, theirs) = UnixStream::pair().unwrap();
                ours.set_nonblocking(true).unwrap();
                world.device_end = Some(theirs);
                Ok(Box::new(ours))
            }

            fn times(&self) -> Times {
                Times {
                    look_every: Duration::from_millis(10),
                    retry_after: Duration::from_millis(10),
                    unreadable_retry: Duration::from_millis(300),
                    wake: Duration::from_millis(5),
                }
            }
        }

        pub struct Running {
            pub fake: Fake,
            pub shared: Arc<Mutex<Reading>>,
            pub drivers: Arc<Drivers>,
            thread: Option<std::thread::JoinHandle<()>>,
        }

        impl Running {
            pub fn start(driver: SpaceMouseDriver, fake: Fake) -> Self {
                let shared = Arc::new(Mutex::new(Reading::new(Instant::now())));
                let drivers = Arc::new(Drivers::default());
                drivers.choice.store(driver.to_u8(), Ordering::Relaxed);
                let (ctx, state, quit, system) = (Context::default(), Arc::clone(&shared), Arc::clone(&drivers), fake.clone());
                let thread = Some(std::thread::spawn(move || reader(ctx, state, quit, system)));
                Self { fake, shared, drivers, thread }
            }

            /// The key of the device being read, if any.
            pub fn key(&self) -> Option<String> {
                self.shared.lock().unwrap().device.map(Source::key)
            }

            /// Waits (up to three seconds) until the device being read has this key.
            pub fn reads(&self, key: &str) {
                let until = Instant::now() + Duration::from_secs(3);
                while self.key().as_deref() != Some(key) {
                    assert!(Instant::now() < until, "still reading {:?}, not {key}", self.key());
                    // Only while the daemon is what is wanted: a daemon that streams without a pause is not
                    // left for the device until it pauses (a read looks for a change when it goes idle), so
                    // sending to it while the device is awaited would keep the test from ever seeing the switch.
                    if key == "spacenavd" {
                        self.fake.daemon_sends_a_record();
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        impl Drop for Running {
            fn drop(&mut self) {
                self.drivers.quit.store(true, Ordering::Relaxed);
                if let Some(thread) = self.thread.take() {
                    thread.join().ok();
                }
            }
        }
    }

    /// The reader itself, on a stand-in machine: Automatic reads the device; unplugged, the daemon;
    /// replugged, the device again (so its own key, and the calibration saved under it, applies), and
    /// the saved driver setting is never touched.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_reader_follows_a_device_through_an_unplug_and_a_replug() {
        use machine::{Fake, IDS, Running};
        let device_key = Source::Hid(Some(IDS)).key();
        let fake = Fake::default();
        *fake.world() = machine::World { device: true, daemon: true, openable: true, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        reader.reads(&device_key);
        fake.unplug();
        reader.reads("spacenavd");
        fake.world().device = true;
        reader.reads(&device_key);
        assert_eq!(reader.drivers.choice(), SpaceMouseDriver::Automatic, "the saved choice is not changed by any of it");
    }

    /// Spacenavd, asked for, is read even with the device there and openable, and the device is not opened.
    #[cfg(target_os = "linux")]
    #[test]
    fn spacenavd_asked_for_is_read_with_the_device_there() {
        use machine::{Fake, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: true, daemon: true, openable: true, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Spacenavd, fake.clone());
        reader.reads("spacenavd");
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(reader.key().as_deref(), Some("spacenavd"));
        assert_eq!(fake.world().opens, 0, "the device was never opened");
    }

    /// The failure of a device to open is said once per node, and forgotten when one opens.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_open_is_said_once_per_node_and_forgotten_after_a_success() {
        let (zero, one) = (std::path::Path::new("/dev/hidraw0"), std::path::Path::new("/dev/hidraw1"));
        let mut unreadable = Unreadable::default();
        assert!(unreadable.failed(zero), "the first failure is said");
        assert!(!unreadable.failed(zero), "not again for the same node");
        assert!(unreadable.failed(one), "another node is news");
        assert!(unreadable.at.is_some());
        // The permission was fixed: it opens. A later failure of any node is said again.
        unreadable.opened();
        assert!(unreadable.at.is_none());
        assert!(unreadable.failed(one), "after a success the next failure is said, though this node was before");
        assert!(unreadable.at.is_some(), "and the wait starts again from it");
        assert!(unreadable.failed(zero));
    }

    /// The daemon read keeps what it learned of the device: a restart of the daemon, a while after the
    /// device was last found unopenable, goes back to the daemon without trying the device again first.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_daemon_restart_does_not_retry_a_device_the_read_just_found_unopenable() {
        use machine::{Fake, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: true, daemon: true, openable: false, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        reader.reads("spacenavd");
        // Wait for a probe by the read (the second open), which refreshes the failure.
        let until = Instant::now() + Duration::from_secs(3);
        while fake.world().opens < 2 {
            assert!(Instant::now() < until, "the read never tried the device again");
            std::thread::sleep(Duration::from_millis(2));
        }
        let (opens, connects) = {
            let world = fake.world();
            (world.opens, world.connects)
        };
        fake.world().daemon_end = None;
        while fake.world().connects == connects {
            assert!(Instant::now() < until, "the daemon was never read again");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(fake.world().opens, opens, "the daemon was read again without opening the device first");
    }

    /// The same restart while the daemon is streaming without a pause: the read is ended by the timer's
    /// look, not by silence, and it still goes back to the daemon without trying the device first.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_daemon_restart_during_a_continuous_stream_does_not_retry_the_unopenable_device_either() {
        use machine::{Fake, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: true, daemon: true, openable: false, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        reader.reads("spacenavd");
        let until = Instant::now() + Duration::from_secs(3);
        // Records keep coming every 2 ms until the read has probed the device a second time.
        while fake.world().opens < 2 {
            assert!(Instant::now() < until, "the busy read never tried the device again");
            fake.daemon_sends_a_record();
            std::thread::sleep(Duration::from_millis(2));
        }
        let (opens, connects) = {
            let world = fake.world();
            (world.opens, world.connects)
        };
        fake.world().daemon_end = None;
        while fake.world().connects == connects {
            assert!(Instant::now() < until, "the daemon was never read again");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(fake.world().opens, opens, "read again without opening the device first");
    }

    /// With no daemon at all, a device that failed to open is tried again at the usual pace, not after the
    /// 30 s wait that is for a daemon to be read meanwhile.
    #[cfg(target_os = "linux")]
    #[test]
    fn without_a_daemon_a_device_that_becomes_readable_is_taken_at_the_usual_pace() {
        use machine::{Fake, IDS, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: true, daemon: false, openable: false, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        let until = Instant::now() + Duration::from_secs(3);
        while fake.world().opens < 1 {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        // Found but not allowed, and nothing else to read: the reader says so.
        let denied = Instant::now() + Duration::from_secs(1);
        while !reader.drivers.present().denied {
            assert!(Instant::now() < denied, "denied is never published");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(reader.drivers.present().direct && !reader.drivers.present().spacenavd);
        fake.world().openable = true;
        let key = Source::Hid(Some(IDS)).key();
        // The wait would be 300 ms here; the usual pace is 10 ms.
        let soon = Instant::now() + Duration::from_millis(200);
        while reader.key().as_deref() != Some(key.as_str()) {
            assert!(Instant::now() < soon, "still not reading the device 200 ms after it became readable");
            std::thread::sleep(Duration::from_millis(2));
        }
        let cleared = Instant::now() + Duration::from_secs(1);
        while reader.drivers.present().denied {
            assert!(Instant::now() < cleared, "denied stays published after the device opened");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// A button held down on one reader is not a press, nor held, on the other when the reader swaps.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_button_held_while_the_reader_swaps_from_the_daemon_to_the_device_is_not_a_press() {
        use machine::{Fake, IDS, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: false, daemon: true, openable: true, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        reader.reads("spacenavd");
        let now = Instant::now();
        let mouse = SpaceMouse {
            shared: Arc::clone(&reader.shared),
            drivers: Arc::clone(&reader.drivers),
            last_frame: now,
            driver_mapping: DriverMapping::object_mode(),
            mapping_checked_at: now,
            wheel: WheelGuard::default(),
            started: now,
            calibration: None,
        };
        // Button 1 goes down on the daemon and stays down.
        fake.daemon_sends([1, 0, 0, 0, 0, 0, 0, 0]);
        let until = Instant::now() + Duration::from_secs(3);
        let on_daemon = loop {
            let seen = mouse.buttons();
            if seen.down != 0 {
                break seen;
            }
            assert!(Instant::now() < until, "the daemon's button never showed");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!((on_daemon.device.as_deref(), on_daemon.down), (Some("spacenavd"), 1));
        // The device comes up and the reader moves to it.
        fake.world().device = true;
        let key = Source::Hid(Some(IDS)).key();
        while reader.key().as_deref() != Some(key.as_str()) {
            assert!(Instant::now() < until, "never moved to the device");
            std::thread::sleep(Duration::from_millis(2));
        }
        // A record that arrived on the daemon does not become fresh again on the device.
        assert!(!reader.shared.lock().unwrap().arrived, "nothing has arrived on the device yet");
        let on_device = mouse.buttons();
        assert_eq!((on_device.device.as_deref(), on_device.down, on_device.taps), (Some(key.as_str()), 0, 0), "nothing held, nothing pressed");
    }

    /// The same ball on the daemon's path, through the reader and the tick: newly arriving records are
    /// fresh even when they decode to the same value (spacenavd truncates 16 and 17 alike), and a last value
    /// kept with nothing arriving is not.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_decelerating_ball_on_the_daemon_is_rolling_while_records_arrive_and_stopped_when_they_do_not() {
        use machine::{Fake, Running};
        use solidcraft_space_mouse::{Kind, Stage, Wizard};
        // (what arrives after the ball slows, whether the step ends): different values that decode alike,
        // the same value over and over, or one record and then silence.
        for (mode, step_done) in [("alternating", false), ("identical", false), ("silence", true)] {
            let fake = Fake::default();
            *fake.world() = machine::World { device: false, daemon: true, openable: true, ..Default::default() };
            let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
            reader.reads("spacenavd");
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
            app.home.open = false;
            let start = Instant::now();
            let mut mouse = SpaceMouse {
                shared: Arc::clone(&reader.shared),
                drivers: Arc::clone(&reader.drivers),
                last_frame: start,
                driver_mapping: DriverMapping::default(),
                mapping_checked_at: start,
                wheel: WheelGuard::default(),
                started: start,
                calibration: None,
            };
            let ctx = Context::default();
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            // One frame of 20 ms of real time (readings are stamped with the real clock): the daemon sends
            // `record` (or nothing), the reader publishes it, the tick feeds the wizard.
            let mut next = start;
            let mut tick = |app: &mut SolveApp, record: Option<i32>| {
                if let Some(x) = record {
                    reader.shared.lock().unwrap().arrived = false;
                    fake.daemon_sends([0, x, 0, 0, 0, 0, 0, 8]);
                    let until = Instant::now() + Duration::from_secs(3);
                    while !reader.shared.lock().unwrap().arrived {
                        assert!(Instant::now() < until, "the record never arrived");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                next += Duration::from_millis(20);
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
                mouse.tick_at(app, &ctx, Instant::now());
            };
            // The first record shows the daemon's device; calibrate it as a trackball.
            tick(&mut app, Some(0));
            app.space_mouse.calibrating = Some((app.space_mouse.device.clone().expect("the daemon is a device now"), Wizard::new(Kind::Trackball)));
            let stage = |app: &SolveApp| app.space_mouse.calibrating.as_ref().map(|(_, wizard)| wizard.stage());
            for _ in 0..400 {
                if stage(&app) == Some(Stage::Step(0)) {
                    break;
                }
                tick(&mut app, Some(0));
            }
            assert_eq!(stage(&app), Some(Stage::Step(0)), "the rest stage is over");
            // The ball at X = 58 on the wire (40 after the daemon's scaling) for 0.6 s.
            for _ in 0..30 {
                tick(&mut app, Some(58));
            }
            // Then 16 and 17, which both decode to 11, for 0.7 s: arriving, or one last record and silence.
            for i in 0..35 {
                let record = match mode {
                    "alternating" => Some(if i % 2 == 0 { 16 } else { 17 }),
                    "identical" => Some(16),
                    _ => (i == 0).then_some(16),
                };
                tick(&mut app, record);
            }
            assert_eq!(stage(&app) == Some(Stage::Step(1)), step_done, "{mode}: {:?}", stage(&app));
        }
    }

    /// A button held while the reader moves from the daemon to the device: the device's first report still
    /// shows it down, and that is a baseline, not a press. Mapped to Esc, nothing is sent at the switch;
    /// let go and pressed again, exactly one Esc; and a quick tap between two looks is still a press.
    /// In each order the UI can meet it in: a frame between connecting and the first report, the first
    /// report before any frame, and a whole disconnect and reconnect that no frame saw.
    #[test]
    fn a_button_held_across_a_switch_to_the_device_does_not_fire_until_pressed_again() {
        for order in ["a frame first", "the report first", "a reconnect no frame saw"] {
            held_across_a_switch(order);
        }
    }

    fn held_across_a_switch(order: &str) {
        use solidcraft_space_mouse::{Field, Layout};
        let ids = Some((0x046d, 0xc62b));
        let device = Source::Hid(ids).key();
        let mut layout = Layout::default();
        layout.buttons[0] = Some(Field { report_id: 3, bit_offset: 0, bits: 1, logical_min: 0, logical_max: 1, relative: false });
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
        app.home.open = false;
        app.space_mouse.device = Some(device.clone());
        app.preferences
            .space_mouse_buttons
            .insert(device.clone(), [(1, solidcraft_ui_egui::space_mouse::ButtonJob::Key(solidcraft_ui_egui::space_mouse::ButtonKey::Esc))].into());
        let ctx = Context::default();
        ctx.run_ui(egui::RawInput { focused: true, ..Default::default() }, |_| {}).textures_delta.clear();
        let shared = Arc::new(Mutex::new(Reading::new(Instant::now())));
        let mouse = SpaceMouse {
            shared: Arc::clone(&shared),
            #[cfg(target_os = "linux")]
            drivers: Arc::default(),
            last_frame: Instant::now(),
            driver_mapping: DriverMapping::object_mode(),
            mapping_checked_at: Instant::now(),
            wheel: WheelGuard::default(),
            started: Instant::now(),
            calibration: None,
        };
        let escapes = |app: &mut SolveApp, mouse: &SpaceMouse| {
            let mut raw = egui::RawInput { focused: true, ..Default::default() };
            solidcraft_ui_egui::space_mouse::buttons_hook(app, &ctx, mouse.buttons(), &mut raw);
            ctx.run_ui(raw.clone(), |_| {}).textures_delta.clear();
            raw.events.iter().filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. })).count()
        };
        let report = |down: bool| publish(&ctx, &shared, layout, &[3, u8::from(down)]);
        // The reader has just connected to the device, with the button still held.
        connected(&ctx, &shared, Source::Hid(ids));
        match order {
            // A frame is drawn before the device has reported anything (the repaint connecting asks for)...
            "a frame first" => assert_eq!(escapes(&mut app, &mouse), 0),
            // ...or the first report comes first...
            "the report first" => {}
            // ...or the UI has seen this device with nothing down, then the reader disconnects and connects
            // again and the first report shows the button held, all between two frames.
            _ => {
                report(false);
                assert_eq!(escapes(&mut app, &mouse), 0, "a look with nothing down");
                reset(&ctx, &shared);
                connected(&ctx, &shared, Source::Hid(ids));
            }
        }
        // ...and then the first report shows the button still down.
        report(true);
        assert_eq!(escapes(&mut app, &mouse), 0, "{order}: the switch sends no Esc");
        report(true);
        assert_eq!(escapes(&mut app, &mouse), 0, "{order}: and holding is not pressing");
        report(false);
        assert_eq!(escapes(&mut app, &mouse), 0, "{order}: letting go is not a press");
        report(true);
        assert_eq!(escapes(&mut app, &mouse), 1, "{order}: a fresh press sends one");
        report(true);
        assert_eq!(escapes(&mut app, &mouse), 0);
        // A tap that is down and up again before the UI looks is still a press.
        report(false);
        report(true);
        report(false);
        assert_eq!(escapes(&mut app, &mouse), 1, "{order}: a quick tap between two looks");
    }

    /// A daemon that sends a record every couple of milliseconds, never going quiet, is still left for
    /// the device when it returns (Automatic prefers the device).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_busy_daemon_is_left_for_the_device_that_returns() {
        use machine::{Fake, IDS, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: false, daemon: true, openable: true, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        let until = Instant::now() + Duration::from_secs(3);
        // The daemon streams without a pause from the start.
        while reader.key().as_deref() != Some("spacenavd") {
            assert!(Instant::now() < until, "the daemon is never read");
            fake.daemon_sends_a_record();
            std::thread::sleep(Duration::from_millis(2));
        }
        fake.world().device = true;
        let key = Source::Hid(Some(IDS)).key();
        while reader.key().as_deref() != Some(key.as_str()) {
            assert!(Instant::now() < until, "still reading {:?} with the device back", reader.key());
            fake.daemon_sends_a_record();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// With the device and the daemon both there and Automatic, the device is read from the start, with no
    /// calibration saved for it.
    #[cfg(target_os = "linux")]
    #[test]
    fn automatic_reads_a_device_with_nothing_saved_for_it() {
        use machine::{Fake, IDS, Running};
        let fake = Fake::default();
        *fake.world() = machine::World { device: true, daemon: true, openable: true, ..Default::default() };
        let reader = Running::start(SpaceMouseDriver::Automatic, fake.clone());
        reader.reads(&Source::Hid(Some(IDS)).key());
        assert_eq!(fake.world().connects, 0, "the daemon was never connected");
    }

    /// A device that cannot be opened (no permission) gives way to the daemon; once the permission is
    /// fixed it is taken within the wait, without the read of the daemon flapping before that, and the
    /// saved driver choice is untouched.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_device_that_becomes_readable_is_taken_from_the_daemon_within_the_retry_wait() {
        use machine::{Fake, IDS, Running};
        for driver in [SpaceMouseDriver::Direct, SpaceMouseDriver::Automatic] {
            let fake = Fake::default();
            *fake.world() = machine::World { device: true, daemon: true, openable: false, ..Default::default() };
            let reader = Running::start(driver, fake.clone());
            reader.reads("spacenavd");
            // Still not readable, over two waits: the daemon read goes on, one connection, a few probes.
            std::thread::sleep(Duration::from_millis(700));
            assert_eq!(reader.key().as_deref(), Some("spacenavd"), "{driver:?}");
            assert_eq!(fake.world().connects, 1, "{driver:?}: the daemon read was not restarted for a device that still cannot be read");
            let opens = fake.world().opens;
            assert!(opens <= 4, "{driver:?}: {opens} tries in 700 ms with a 300 ms wait");
            // The permission is fixed.
            fake.world().openable = true;
            reader.reads(&Source::Hid(Some(IDS)).key());
            assert_eq!(reader.drivers.choice(), driver, "the saved choice is not changed");
        }
    }

    /// After the section's Restore (which puts the driver back to Automatic) the calibration that applies is
    /// the one for the reader that is read now: the device's own when Automatic reads the device (it does
    /// whenever it opens, so from Direct or from Spacenavd the device's calibration applies), and none when
    /// the device cannot be opened and the daemon is read under its own key. Real reader, stand-in machine,
    /// the app's own tick.
    #[cfg(target_os = "linux")]
    #[test]
    fn after_a_restore_the_calibration_that_applies_is_the_one_of_the_reader_being_read() {
        use machine::{Fake, IDS, Running};
        let key = Source::Hid(Some(IDS)).key();
        // (driver before, device opens, calibrated, key read after the restore, calibration applies after)
        let cases = [
            (SpaceMouseDriver::Direct, true, true, key.as_str(), true),
            (SpaceMouseDriver::Direct, true, false, key.as_str(), false),
            (SpaceMouseDriver::Spacenavd, true, true, key.as_str(), true),
            (SpaceMouseDriver::Spacenavd, true, false, key.as_str(), false),
            (SpaceMouseDriver::Direct, false, true, "spacenavd", false),
        ];
        for (before, openable, calibrated, expected, applies) in cases {
            let fake = Fake::default();
            *fake.world() = machine::World { device: true, daemon: true, openable, ..Default::default() };
            let reader = Running::start(before, fake.clone());
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
            app.home.open = false;
            app.preferences.space_mouse_driver = before;
            if calibrated {
                app.preferences.space_mouse_calibrations.insert(key.clone(), resting_at_100());
            }
            let now = Instant::now();
            let mut mouse = SpaceMouse {
                shared: Arc::clone(&reader.shared),
                drivers: Arc::clone(&reader.drivers),
                last_frame: now,
                driver_mapping: DriverMapping::object_mode(),
                mapping_checked_at: now,
                wheel: WheelGuard::default(),
                started: now,
                calibration: None,
            };
            let ctx = Context::default();
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            let settle = |mouse: &mut SpaceMouse, app: &mut SolveApp, want: &str| {
                let until = Instant::now() + Duration::from_secs(3);
                loop {
                    // A daemon that streams without a pause is not left until it pauses, so it is only fed
                    // while it is the reader that is awaited.
                    if want == "spacenavd" {
                        fake.daemon_sends_a_record();
                    }
                    mouse.tick_at(app, &ctx, Instant::now());
                    if app.space_mouse.device.as_deref() == Some(want) {
                        return;
                    }
                    assert!(Instant::now() < until, "{before:?}/{openable}: reading {:?}, not {want}", app.space_mouse.device);
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            // The reader as the setting has it before the restore.
            let first = if before == SpaceMouseDriver::Direct && openable { key.as_str() } else { "spacenavd" };
            settle(&mut mouse, &mut app, first);
            solidcraft_ui_egui::prefs::restore_space_mouse_defaults(&mut app);
            assert_eq!(app.preferences.space_mouse_driver, SpaceMouseDriver::Automatic);
            settle(&mut mouse, &mut app, expected);
            assert_eq!(calibration(&app).is_some(), applies, "{before:?}/{openable}/{calibrated}: the calibration that applies");
            if applies {
                assert_eq!(app.preferences.space_mouse_calibrations.len(), 1, "nothing was copied between keys");
            }
        }
    }
}
