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
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// The device being read; none between devices.
    device: Option<Source>,
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
}

impl Reading {
    fn new(now: Instant) -> Self {
        Self { device: None, axes: Axes::default(), at: [now; 6], held: false }
    }

    /// The axes at `now`. One with nothing fresh reads as `rest`: the device's own rest when it
    /// is calibrated, so a device that rests off zero calibrates to no motion rather than drifting.
    fn current(self, now: Instant, rest: Axes) -> Axes {
        let mut axes = self.axes;
        if self.held {
            let values = axes.translation.into_iter().chain(axes.rotation);
            let rests = rest.translation.into_iter().chain(rest.rotation);
            let near_rest = values.zip(rests).all(|(value, rest)| value.abs_diff(rest) <= HELD_SETTLE_BAND);
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
    true
}

/// Publishes one input report from the device to the egui thread.
fn publish(ctx: &Context, shared: &Mutex<Reading>, layout: Layout, report: &[u8]) {
    if DRIVER_CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut reading) = shared.lock()
        && decode_report(report, layout, &mut reading, Instant::now())
    {
        ctx.request_repaint();
    }
}

/// Publishes the cap's position from spacenavd.
#[cfg(target_os = "linux")]
fn publish_held(ctx: &Context, shared: &Mutex<Reading>, axes: Axes) {
    if let Ok(mut reading) = shared.lock() {
        *reading = Reading { device: reading.device, axes, held: true, ..Reading::new(Instant::now()) };
        ctx.request_repaint();
    }
}

/// A device is open: say which, so its calibration applies.
fn connected(ctx: &Context, shared: &Mutex<Reading>, source: Source) {
    if let Ok(mut reading) = shared.lock() {
        // Nothing has arrived from it yet, so every axis starts out stale.
        let now = Instant::now();
        let stale = now.checked_sub(STALE_AFTER).unwrap_or(now);
        *reading = Reading { device: Some(source), at: [stale; 6], ..Reading::new(now) };
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

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn reader(ctx: Context, shared: Arc<Mutex<Reading>>) {
    let Ok(mut api) = hid_api() else {
        log::warn!("3D mouse: HID initialization failed");
        return;
    };
    loop {
        if let Some((device, layout, ids)) = candidates(&mut api).into_iter().find_map(|path| {
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
            Some((device, layout, (info.vendor_id(), info.product_id())))
        }) {
            connected(&ctx, &shared, Source::Hid(Some(ids)));
            let mut report = [0_u8; REPORT_BUFFER_BYTES];
            loop {
                match device.read_timeout(&mut report, 100) {
                    Ok(0) => {}
                    Ok(size) => publish(&ctx, &shared, layout, report.get(..size).unwrap_or_default()),
                    Err(error) => {
                        log::info!("3D mouse disconnected: {error}");
                        break;
                    }
                }
            }
            reset(&ctx, &shared);
        }
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
    /// `PRESENT_SPACENAVD` and `PRESENT_DIRECT`, as of the reader's last look.
    found: AtomicU8,
}

#[cfg(target_os = "linux")]
const PRESENT_SPACENAVD: u8 = 1;
#[cfg(target_os = "linux")]
const PRESENT_DIRECT: u8 = 2;
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
        DriversPresent { spacenavd: found & PRESENT_SPACENAVD != 0, direct: found & PRESENT_DIRECT != 0 }
    }

    fn publish(&self, present: DriversPresent) {
        let found = (u8::from(present.spacenavd) * PRESENT_SPACENAVD) | (u8::from(present.direct) * PRESENT_DIRECT);
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

/// Which reader to use: the one asked for when it is there, else the other, else none. Automatic
/// asks for spacenavd, which is what has always been read first.
#[cfg(target_os = "linux")]
fn choose(wanted: SpaceMouseDriver, spacenavd: bool, direct: bool) -> Option<Pick> {
    let first = match wanted {
        SpaceMouseDriver::Automatic | SpaceMouseDriver::Spacenavd => Pick::Spacenavd,
        SpaceMouseDriver::Direct => Pick::Direct,
    };
    let there = |pick| match pick {
        Pick::Spacenavd => spacenavd,
        Pick::Direct => direct,
    };
    let other = if first == Pick::Spacenavd { Pick::Direct } else { Pick::Spacenavd };
    [first, other].into_iter().find(|&pick| there(pick))
}

/// Reads the 3D mouse through spacenavd or `/dev/hidrawN`, as `Drivers::choice` says (Automatic
/// is spacenavd when it runs, so what `spnavcfg` sets, swapped and inverted axes, sensitivity,
/// dead zones, applies here as in other programs). Both reads wake up when idle to see whether
/// the choice changed, which then moves to the other reader without a restart. hidraw is opened
/// non-blocking and ends with an error when the device is unplugged. Opening needs permission:
/// most distributions grant it to the logged-in user for devices that carry the `uaccess` udev
/// tag, which a rule for the 3D mouse has to add.
#[cfg(target_os = "linux")]
fn reader(ctx: Context, shared: Arc<Mutex<Reading>>, drivers: Arc<Drivers>) {
    use std::os::unix::fs::OpenOptionsExt;
    let look = || DriversPresent { spacenavd: spacenavd::exists(), direct: hidraw::find().is_some() };
    let mut denied = None;
    let mut told = None;
    loop {
        let device = hidraw::find();
        let present = DriversPresent { spacenavd: spacenavd::exists(), direct: device.is_some() };
        drivers.publish(present);
        let wanted = drivers.choice();
        let mut pick = choose(wanted, present.spacenavd, present.direct);
        // A socket file with no daemon behind it is no spacenavd.
        let daemon = if pick == Some(Pick::Spacenavd) { spacenavd::connect() } else { None };
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
        let mut stop = || {
            if looked.elapsed() >= LOOK_EVERY {
                looked = Instant::now();
                drivers.publish(look());
            }
            drivers.choice() != wanted
        };
        match (pick, daemon, device) {
            (Some(Pick::Spacenavd), Some(daemon), _) => {
                log::info!("3D mouse: reading spacenavd, which applies the spnavcfg settings");
                if let Err(error) = daemon.set_read_timeout(Some(SPACENAVD_WAKE)) {
                    log::warn!("3D mouse: spacenavd reads cannot time out ({error}); a change of driver waits for the next event");
                }
                connected(&ctx, &shared, Source::Spacenavd);
                spacenavd::read(daemon, |axes| publish_held(&ctx, &shared, axes), &mut stop);
                reset(&ctx, &shared);
            }
            (Some(Pick::Direct), _, Some(device)) => match std::fs::OpenOptions::new().read(true).custom_flags(hidraw::O_NONBLOCK).open(&device.path)
            {
                Ok(file) => {
                    denied = None;
                    log::info!("3D mouse: reading {}", device.path.display());
                    connected(&ctx, &shared, Source::Hid(device.ids));
                    hidraw::read_reports(file, |report| publish(&ctx, &shared, device.layout, report), &mut stop);
                    reset(&ctx, &shared);
                }
                // Say so once per device node, not every retry.
                Err(error) if denied.as_ref() != Some(&device.path) => {
                    log::warn!("3D mouse: cannot open {}: {error}. A udev rule granting the logged-in user access is needed.", device.path.display());
                    denied = Some(device.path);
                }
                Err(_) => {}
            },
            _ => {}
        }
        // A new choice is taken up at once; anything else waits to try again.
        if drivers.choice() == wanted {
            std::thread::sleep(RETRY_AFTER);
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
                    ctx.request_repaint();
                }
            }
            Event::Buttons(_) => {}
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
    pub fn start(ctx: &Context) -> Self {
        let now = Instant::now();
        let shared = Arc::new(Mutex::new(Reading::new(now)));
        let worker_state = Arc::clone(&shared);
        let worker_ctx = ctx.clone();
        #[cfg(target_os = "linux")]
        let drivers = Arc::new(Drivers::default());
        #[cfg(target_os = "linux")]
        let worker_drivers = Arc::clone(&drivers);
        let spawned = std::thread::Builder::new().name("solidcraft-3d-mouse".into()).spawn(move || {
            #[cfg(target_os = "linux")]
            reader(worker_ctx, worker_state, worker_drivers);
            #[cfg(not(target_os = "linux"))]
            reader(worker_ctx, worker_state);
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
                calibrated(reading.current(now, at_rest(calibration)), calibration).moving()
            })
            .unwrap_or(false);
        self.wheel.filter(moving, now.duration_since(self.started).as_secs_f64(), &mut raw.events);
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
        let Ok(reading) = self.shared.lock().map(|reading| *reading) else { return };
        app.space_mouse.device = reading.device.map(Source::key);
        let calibration = calibration(app);
        self.calibration = reading.device.zip(calibration);
        // While this device is being calibrated, an axis with nothing fresh reads 0, as a HID
        // device's silence means: the old calibration's rest isn't fed back into the new one. A
        // calibration of another device doesn't count; it ends in `navigate_frame`.
        let calibrating = app.space_mouse.calibrating.as_ref().is_some_and(|(device, _)| app.space_mouse.device.as_ref() == Some(device));
        let rest = if calibrating { Axes::default() } else { at_rest(calibration) };
        navigate_frame(app, ctx, self.driver_mapping, reading.current(now, rest), seconds);
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
            (Automatic, true, true, Some(Spacenavd)),
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
            drivers.publish(DriversPresent { spacenavd, direct });
            assert_eq!(drivers.present(), DriversPresent { spacenavd, direct });
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
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
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
}
