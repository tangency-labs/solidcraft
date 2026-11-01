//! Getting the window open, whatever state the settings folder or the graphics device is in
//! (docs/book/src/guide/troubleshooting.md).
//!
//! - **Settings that don't read** (`app.ron`, `graphics.json`): moved aside as
//!   `<name>.bad-<time>` and the app starts with defaults, instead of a newer or damaged file
//!   being dropped silently or stopping the start.
//! - **Launches that die before the window is up**: a marker file counts them; after two in a
//!   row the next launch starts in safe mode by itself.
//! - **Safe mode** (`--safe-mode` or `SOLIDCRAFT_SAFE_MODE=1`): default settings (the saved ones
//!   are left untouched), no recovery offer, the software renderer and CPU meshing.
//! - **A renderer that cannot start**: on Windows, try the remaining graphics APIs in order,
//!   then the software renderer (`SOLIDCRAFT_RENDERER=cpu`). An explicit `WGPU_BACKEND` fails
//!   cleanly without switching. Exhausted retries show a message box with the error.
//! - **A GPU that hangs after the window is up** (Windows, [`HangWatch`]): frames that each wait
//!   out their acquire while the user is there. The app closes, keeping unsaved changes for
//!   recovery, and starts again on the next API (D3D12 → Vulkan → OpenGL → the software
//!   renderer), kept in `graphics.json`.

use std::path::{Path, PathBuf};

/// The environment variable that starts safe mode (any value but `0` or empty).
pub const SAFE_MODE_ENV: &str = "SOLIDCRAFT_SAFE_MODE";
/// `SOLIDCRAFT_RENDERER=cpu` picks the software renderer (WARP, lavapipe, llvmpipe).
pub const RENDERER_ENV: &str = "SOLIDCRAFT_RENDERER";
/// Consecutive launches that died before the window was up, after which safe mode starts.
pub const FAILED_STARTS_FOR_SAFE_MODE: u32 = 2;
const MARKER: &str = "starting";

/// Explicit application-settings isolation for side-by-side builds and native UI
/// acceptance. A supplied invalid path refuses startup rather than falling back
/// to the user's ordinary settings. No directory is read or created here.
pub fn settings_directory(override_path: Option<&std::ffi::OsStr>, default: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    match override_path {
        None => Ok(default),
        Some(raw) => {
            let path = PathBuf::from(raw);
            if raw.is_empty() || !path.is_absolute() {
                return Err("SOLIDCRAFT_SETTINGS_DIR must be a nonempty absolute directory path".into());
            }
            if path.exists() && !path.is_dir() {
                return Err("SOLIDCRAFT_SETTINGS_DIR names a file, not a directory".into());
            }
            Ok(Some(path))
        }
    }
}

/// Whether an environment value switches something on.
pub fn env_on(v: Option<&str>) -> bool {
    v.map(str::trim).is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// Whether `SOLIDCRAFT_RENDERER` asks for the software renderer.
pub fn cpu_renderer_env(v: Option<&str>) -> bool {
    v.map(str::trim).is_some_and(|v| v.eq_ignore_ascii_case("cpu") || v.eq_ignore_ascii_case("software"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Move `path` aside as `<name>.bad-<time>` (kept for a bug report); returns where it went.
pub fn back_up(path: &Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "settings".into());
    let mut to = path.with_file_name(format!("{name}.bad-{}", unix_now()));
    let mut n = 1;
    while to.exists() && n < 100 {
        to = path.with_file_name(format!("{name}.bad-{}-{n}", unix_now()));
        n += 1;
    }
    std::fs::rename(path, &to)?;
    Ok(to)
}

/// Whether eframe's `app.ron` reads, with the preferences inside it.
fn app_ron_reads(text: &str, prefs_keys: &[&str]) -> bool {
    let Ok(kv) = ron::from_str::<std::collections::HashMap<String, String>>(text) else { return false };
    prefs_keys.iter().filter_map(|k| kv.get(*k)).all(|p| serde_json::from_str::<serde_json::Value>(p).is_ok_and(|v| v.is_object()))
}

/// Check the settings files in `dir` before anything reads them: one that doesn't read is moved
/// aside (the app then starts with defaults for it). Returns a line per file moved.
pub fn check_settings(dir: &Path, prefs_keys: &[&str]) -> Vec<String> {
    let mut notes = Vec::new();
    let mut check = |name: &str, reads: &dyn Fn(&str) -> bool| {
        let path = dir.join(name);
        let Ok(bytes) = std::fs::read(&path) else { return };
        let ok = std::str::from_utf8(&bytes).is_ok_and(|t| t.trim().is_empty() || reads(t));
        if ok {
            return;
        }
        match back_up(&path) {
            Ok(to) => {
                log::warn!("settings: {} did not read; moved to {}", path.display(), to.display());
                notes.push(format!("{name} could not be read and was moved to {}; defaults are used.", to.display()));
            }
            Err(e) => log::warn!("settings: {} did not read and could not be moved: {e}", path.display()),
        }
    };
    check("app.ron", &|t| app_ron_reads(t, prefs_keys));
    check("graphics.json", &|t| serde_json::from_str::<serde_json::Value>(t).is_ok_and(|v| v.is_object()));
    notes
}

/// A launch begins: count it in the marker in `dir`. Returns how many launches before this one
/// died before their window was up.
pub fn begin(dir: &Path) -> u32 {
    let marker = dir.join(MARKER);
    let before = std::fs::read_to_string(&marker).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0).min(1000);
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(&marker, (before + 1).to_string());
    }
    before
}

/// The window is up: this launch did not fail to start.
pub fn started(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(MARKER));
}

/// The software adapter among `adapters` (WARP on Windows, lavapipe or llvmpipe elsewhere), else
/// the first one that can draw to `surface`.
pub fn pick_cpu_adapter(adapters: &[eframe::wgpu::Adapter], surface: Option<&eframe::wgpu::Surface<'_>>) -> Result<eframe::wgpu::Adapter, String> {
    let fits = |a: &&eframe::wgpu::Adapter| surface.is_none_or(|s| a.is_surface_supported(s));
    adapters
        .iter()
        .filter(fits)
        .find(|a| a.get_info().device_type == eframe::wgpu::DeviceType::Cpu)
        .or_else(|| adapters.iter().find(fits))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().backend)).collect();
            format!("no graphics adapter can draw the window (found: {})", if names.is_empty() { "none".into() } else { names.join(", ") })
        })
}

/// Adapter properties used to choose a card without opening a device.
#[derive(Clone, Debug)]
pub struct AdapterChoice {
    pub name: String,
    pub compatible: bool,
}

/// Choose an adapter by name. The environment override follows wgpu's case-insensitive
/// substring convention among surface-compatible adapters. A saved preference prefers an
/// exact (then case-insensitive) name among compatible adapters, and if the named card is
/// only listed as incompatible with the surface still picks it (#8341: otherwise the
/// preference is kept in the dialog while wgpu silently uses another GPU).
/// No match leaves selection to the normal wgpu request.
pub fn choose_adapter(adapters: &[AdapterChoice], environment: Option<&str>, preference: Option<&str>) -> Option<usize> {
    // wgpu's environment helper panics on a missing name; an override must remain recoverable.
    let compatible = || adapters.iter().enumerate().filter(|(_, a)| a.compatible);
    if let Some(name) = environment.filter(|s| !s.is_empty()) {
        let name = name.to_lowercase();
        if let Some((index, _)) = compatible().find(|(_, a)| a.name.to_lowercase().contains(&name)) {
            return Some(index);
        }
    }
    let name = preference.filter(|s| !s.is_empty())?;
    let find = |pred: &dyn Fn(&AdapterChoice) -> bool, only_compatible: bool| {
        adapters.iter().enumerate().find(|(_, a)| (!only_compatible || a.compatible) && pred(a)).map(|(i, _)| i)
    };
    find(&|a| a.name == name, true)
        .or_else(|| find(&|a| a.name.eq_ignore_ascii_case(name), true))
        .or_else(|| find(&|a| a.name == name, false))
        .or_else(|| find(&|a| a.name.eq_ignore_ascii_case(name), false))
}

/// Leave Automatic on wgpu's normal request path; named choices may defer to it too.
pub fn configure_adapter(setup: &mut eframe::egui_wgpu::WgpuSetupCreateNew, cpu: bool, environment: Option<String>, preference: Option<String>) {
    if cpu {
        setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, surface| pick_cpu_adapter(adapters, surface).map(Some)));
    } else if environment.as_deref().is_some_and(|s| !s.is_empty()) || preference.as_deref().is_some_and(|s| !s.is_empty()) {
        setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| {
            let choices: Vec<_> = adapters
                .iter()
                .map(|a| AdapterChoice { name: a.get_info().name, compatible: surface.is_none_or(|s| a.is_surface_supported(s)) })
                .collect();
            Ok(choose_adapter(&choices, environment.as_deref(), preference.as_deref()).and_then(|i| adapters.get(i)).cloned())
        }));
    }
}

/// `WGPU_BACKEND`: when set, wgpu picks the backends from it and the order below is not used.
pub const WGPU_BACKEND_ENV: &str = "WGPU_BACKEND";

/// The graphics backends to start with, or `None` to leave it to wgpu (`WGPU_BACKEND`, or all
/// the backends it was built with).
///
/// On Windows, D3D12 comes first, then Vulkan, then OpenGL, and only the first one with a
/// hardware adapter is started at all. wgpu would otherwise start Vulkan, D3D12 and OpenGL
/// together and prefer Vulkan, and starting Vulkan loads every installed Vulkan driver: Intel's
/// (`igvk64.dll`) crashed SolidCraft at startup that way, inside the driver, before anything
/// could be reported. D3D12 is Windows' own graphics API, the one its drivers are tested with.
/// `hardware(backends)` lists those backends' adapters that aren't software (it is only asked
/// about the backends being tried, one at a time).
///
/// One exception: when D3D12's only hardware adapters are AMD integrated GPUs and Vulkan has a
/// hardware adapter, Vulkan. On those (Ryzen 5400U, 5600U, 5700G) D3D12 hung the driver a few
/// seconds into the first frames and Windows reset it (a black screen, then a crash, #1789,
/// #2806); 0.11.0, which preferred Vulkan, worked there, and `WGPU_BACKEND=vulkan` runs it
/// smoothly (#3321).
///
/// - `WGPU_BACKEND` set (`env_set`): `None`, whatever it says wins.
/// - The software renderer (`cpu`, safe mode): D3D12 alone on Windows (WARP); `None` elsewhere.
/// - `preference` (`graphics.json`'s `backend`: `dx12`, `vulkan`, `gl` or a comma list; `auto`
///   or empty for the order above): those backends, on any system.
/// - Elsewhere than Windows: `None`.
/// - No hardware adapter at all: D3D12 (WARP, Windows' software renderer).
pub fn choose_backends(
    windows: bool,
    env_set: bool,
    cpu: bool,
    preference: Option<&str>,
    mut hardware: impl FnMut(eframe::wgpu::Backends) -> Vec<HardwareAdapter>,
) -> Option<eframe::wgpu::Backends> {
    use eframe::wgpu::Backends;
    if env_set {
        return None;
    }
    if cpu {
        return windows.then_some(Backends::DX12);
    }
    if let Some(p) = preference.map(str::trim).filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto")) {
        let chosen = Backends::from_comma_list(p);
        if !chosen.is_empty() {
            return Some(chosen);
        }
        log::warn!("graphics: unknown backend preference `{p}`; using the default order");
    }
    if !windows {
        return None;
    }
    for b in WINDOWS_ORDER {
        let found = hardware(b);
        if found.is_empty() {
            continue;
        }
        if b == Backends::DX12 && found.iter().all(HardwareAdapter::hangs_on_d3d12) && !hardware(Backends::VULKAN).is_empty() {
            log::warn!("graphics: Vulkan rather than D3D12 on an AMD integrated GPU (#3321)");
            return Some(Backends::VULKAN);
        }
        return Some(b);
    }
    Some(Backends::DX12)
}

/// A hardware (not software) adapter a backend offers, as [`choose_backends`] needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareAdapter {
    /// The PCI vendor id.
    pub vendor: u32,
    pub device_type: eframe::wgpu::DeviceType,
}

/// AMD's PCI vendor id.
const AMD: u32 = 0x1002;

impl HardwareAdapter {
    /// An AMD integrated GPU, whose D3D12 driver hung on SolidCraft's first frames (#3321).
    pub fn hangs_on_d3d12(&self) -> bool {
        self.vendor == AMD && self.device_type == eframe::wgpu::DeviceType::IntegratedGpu
    }
}

/// Frames in a row that stalled ([`eframe::egui_wgpu::stalled_frames`]: each waited out wgpu's
/// one-second acquire) after which the GPU is taken as hung.
pub const STALLED_FRAMES_FOR_RESTART: u32 = 5;
/// Seconds the window's surface may refuse its configuration ([`eframe::egui_wgpu::refused_for`])
/// while the user is there before the GPU is taken as hung: after a reset, a surface that stays
/// unsupported leaves a blank window. Refusals that clear up (a resize, a display waking up)
/// take a moment, not this long.
pub const REFUSED_S_FOR_RESTART: f64 = 10.0;
/// Stalled frames count only this many seconds after the user's input: a hung GPU leaves a white
/// window the user clicks at, while a switched-off display can hold frames back with nobody there.
pub const STALL_INPUT_WINDOW_S: f64 = 10.0;
/// Set on the launch that replaces one whose GPU hung: the hung backend, for the notice.
pub const HUNG_ENV: &str = "SOLIDCRAFT_GPU_HUNG";

/// Watches for a hung GPU after the window is up (#3321). The failed-start count misses it: frames
/// still come, a second apart, so the start counts as good.
#[derive(Debug, Default)]
pub struct HangWatch {
    last_input: Option<f64>,
    /// When the user came back: the first input after none for [`STALL_INPUT_WINDOW_S`].
    arrived: f64,
    /// The stalled streak when the user came back. Those frames stalled with nobody there (a
    /// display switched off holds them back too), and the input that wakes the display comes
    /// before the next frame shows whether the GPU is fine, so only stalls after it count.
    stalled_before: u32,
    hung: bool,
}

impl HangWatch {
    /// Per frame, at `now` seconds: whether the user gave input this frame, the frames in a row
    /// that stalled, and the seconds the surface has refused its configuration. True once, the
    /// frame the GPU is taken as hung.
    pub fn hung(&mut self, now: f64, input: bool, stalled: u32, refused_s: Option<f64>) -> bool {
        if stalled < self.stalled_before {
            // The streak broke: every stall since came after the user was back.
            self.stalled_before = 0;
        }
        if input {
            if !self.user_there(now) {
                self.arrived = now;
                self.stalled_before = stalled;
            }
            self.last_input = Some(now);
        }
        if self.hung || !self.user_there(now) {
            return false;
        }
        // A surface refused since before the user came back counts from then.
        let refused = refused_s.map_or(0.0, |r| r.min(now - self.arrived));
        self.hung = stalled.saturating_sub(self.stalled_before) >= STALLED_FRAMES_FOR_RESTART || refused >= REFUSED_S_FOR_RESTART;
        self.hung
    }

    fn user_there(&self, now: f64) -> bool {
        self.last_input.is_some_and(|t| (0.0..=STALL_INPUT_WINDOW_S).contains(&(now - t)))
    }
}

/// The backend a launch whose GPU hung on `backend` starts again on, kept in `graphics.json` for
/// later launches too (`cpu`: the software renderer, which has nothing to fall back to). Each
/// hang moves on to the next of [`WINDOWS_ORDER`], then to the software renderer, so a GPU hangs
/// at most once on each: a safe-mode restart would leave the backend that hung saved for the
/// next normal start. D3D12 goes to Vulkan, which runs the GPUs whose D3D12 driver hung (#1789);
/// it isn't probed here, as loading Vulkan's drivers into this process is what crashed once
/// (`WINDOWS_ORDER`): the new launch finds out, and falls back to software if it has no GPU.
pub fn after_hang(backend: eframe::wgpu::Backend, cpu: bool) -> Option<&'static str> {
    match backend {
        _ if cpu => None,
        eframe::wgpu::Backend::Dx12 => Some("vulkan"),
        eframe::wgpu::Backend::Vulkan => Some("gl"),
        _ => Some(SOFTWARE_BACKEND),
    }
}

/// `graphics.json`'s `backend` for the software renderer, kept after the GPU hung on every API.
pub const SOFTWARE_BACKEND: &str = "cpu";

/// The notice on the launch after the GPU hung on `hung` (a backend's name): this one is in safe
/// mode, or on the `backend` preference.
pub fn hang_note(hung: &str, safe_mode: bool, backend: Option<&str>) -> String {
    let now = match backend {
        _ if safe_mode => "in safe mode".to_string(),
        Some(b) => format!("on {}, which it keeps using (the backend in graphics.json)", api_name(b)),
        None => "with another renderer".to_string(),
    };
    let unsaved = if safe_mode { "the next normal start offers unsaved changes" } else { "unsaved changes are offered for recovery" };
    format!("The graphics driver stopped responding on {}, so SolidCraft restarted {now}; {unsaved}.", api_name(hung))
}

/// A backend's name (`Dx12`, `vulkan`, …) as people know the graphics API.
fn api_name(backend: &str) -> &str {
    match backend.trim().to_ascii_lowercase().as_str() {
        "dx12" => "Direct3D 12",
        "vulkan" => "Vulkan",
        "gl" => "OpenGL",
        "metal" => "Metal",
        "cpu" | "software" => "the software renderer",
        _ => backend,
    }
}

/// Internal, process-local retry choice; never written to the graphics preference.
pub const RETRY_BACKEND_ENV: &str = "SOLIDCRAFT_STARTUP_BACKEND";
/// Why this launch is using a fallback, carried to the new process for status and Preferences.
pub const FALLBACK_NOTE_ENV: &str = "SOLIDCRAFT_GRAPHICS_FALLBACK";

/// A handled startup failure tries each remaining Windows API once, then software rendering.
/// An explicit environment choice is final, including when software rendering was requested.
pub fn fallback_backend(windows: bool, explicit: bool, cpu: bool, failed: eframe::wgpu::Backends) -> Option<&'static str> {
    if explicit || cpu {
        return None;
    }
    if windows {
        let last = WINDOWS_ORDER.iter().rposition(|backend| failed.intersects(*backend));
        if let Some(next) = last.and_then(|i| WINDOWS_ORDER.get(i + 1)) {
            return Some(if *next == eframe::wgpu::Backends::VULKAN { "vulkan" } else { "gl" });
        }
    }
    Some("cpu")
}

/// Keep driver diagnostics on one bounded line, including across the finite retry chain.
pub fn fallback_note(previous: &str, failed: eframe::wgpu::Backends, next: &str, error: &str) -> String {
    let previous: String = previous.chars().take(2048).collect();
    let error: String = error.chars().take(1024).collect();
    let line = format!("{previous} Graphics fallback: {failed:?} failed ({error}); trying {next}.");
    line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(4096).collect()
}

/// The device feature for a 32-bit float depth buffer with a stencil (reversed depth, #1434).
pub const FLOAT_DEPTH_STENCIL: eframe::wgpu::Features = eframe::wgpu::Features::DEPTH32FLOAT_STENCIL8;

/// Whether every adapter `options` could start offers a float depth buffer with a stencil. eframe
/// makes the window's depth buffer from the depth bits before it picks the adapter, so the window
/// asks for a float one only when whichever it picks has it.
pub fn float_depth_everywhere(options: &eframe::egui_wgpu::WgpuConfiguration) -> bool {
    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_setup else { return false };
    let mut desc = solidcraft_gpu::instance_descriptor_from_env();
    desc.backends = setup.instance_descriptor.backends;
    let instance = eframe::wgpu::Instance::new(desc);
    let adapters = pollster::block_on(instance.enumerate_adapters(setup.instance_descriptor.backends));
    let all = !adapters.is_empty() && adapters.iter().all(|a| a.features().contains(FLOAT_DEPTH_STENCIL));
    log::info!("graphics: float depth with stencil on every adapter: {all}");
    all
}

/// The order backends are tried in on Windows.
pub const WINDOWS_ORDER: [eframe::wgpu::Backends; 3] = [eframe::wgpu::Backends::DX12, eframe::wgpu::Backends::VULKAN, eframe::wgpu::Backends::GL];

/// The hardware adapters of `backends` (started on their own, nothing else).
pub fn hardware_adapters(backends: eframe::wgpu::Backends) -> Vec<HardwareAdapter> {
    let mut desc = eframe::wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    desc.flags = eframe::wgpu::InstanceFlags::from_build_config().with_env();
    desc.backend_options.dx12.shader_compiler = solidcraft_gpu::dx12_compiler();
    let instance = eframe::wgpu::Instance::new(desc);
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let found: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().device_type)).collect();
    log::info!("graphics: {backends:?} adapters: {found:?}");
    adapters
        .iter()
        .map(|a| a.get_info())
        .filter(|i| i.device_type != eframe::wgpu::DeviceType::Cpu)
        .map(|i| HardwareAdapter { vendor: i.vendor, device_type: i.device_type })
        .collect()
}

/// The environment variable that switches wgpu's indirect-call validation on or off.
pub const INDIRECT_VALIDATION_ENV: &str = "WGPU_VALIDATION_INDIRECT_CALL";

/// The instance flags for the window. wgpu's indirect-call validation compiles a compute shader
/// when the device is created, and on Intel Macs (macOS 12 with Intel Iris graphics, #3024,
/// #3080, #4605) the Metal compiler refuses it: the device is lost and the window never opens.
/// So on an Intel Mac the validation is left off, unless the user turned it on or off with
/// `WGPU_VALIDATION_INDIRECT_CALL` themselves.
pub fn window_instance_flags(flags: eframe::wgpu::InstanceFlags, intel_mac: bool, env: Option<&str>) -> eframe::wgpu::InstanceFlags {
    if intel_mac && env.map(str::trim).is_none_or(str::is_empty) { flags - eframe::wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL } else { flags }
}

/// The environment variable that makes the macOS compatibility shim report an older OS
/// version to wgpu's Metal backend. On OCLP-patched machines the Metal compiler accepts
/// only up to the MSL standard of the original GPU, not the one the patched OS reports,
/// so wgpu picks a standard that the compiler refuses (`invalid value 'metal3.2'`).
/// Setting this variable before the process starts causes the OS to report the older
/// version and wgpu selects a supported MSL standard (#3080, #4605).
#[cfg(target_os = "macos")]
pub const SYSTEM_VERSION_COMPAT_ENV: &str = "SYSTEM_VERSION_COMPAT";

/// The NVRAM variable that OpenCore Legacy Patcher writes on every OCLP-booted system.
/// Its presence is the primary signal that `SYSTEM_VERSION_COMPAT=1` is needed.
#[cfg(target_os = "macos")]
pub const OCLP_NVRAM_KEY: &str = "4D1FDA02-38C7-4A6A-9CC6-4BCCA8B30102:OCLP-Version";

/// `true` if the OCLP NVRAM key is present on this system.
///
/// Runs `nvram <key>` and treats exit 0 as present. Any error (missing binary,
/// permission denied, key absent) returns `false` — this is a best-effort probe.
#[cfg(target_os = "macos")]
pub fn oclp_nvram_present() -> bool {
    std::process::Command::new("nvram").arg(OCLP_NVRAM_KEY).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Whether to re-launch with `SYSTEM_VERSION_COMPAT=1`: it is not set yet and OCLP shows through
/// the NVRAM key. The probe runs only when it is needed, so a re-launched process (variable
/// set) never starts `nvram`.
#[cfg(target_os = "macos")]
fn compat_wanted(compat_set: bool, nvram: impl FnOnce() -> bool) -> bool {
    !compat_set && nvram()
}

/// If this process is running under OCLP and `SYSTEM_VERSION_COMPAT` has not been set,
/// re-launch the current binary with `SYSTEM_VERSION_COMPAT=1` added to the environment,
/// then exit with the child's status code.
///
/// `SYSTEM_VERSION_COMPAT=1` must be present **before** the process starts: setting it
/// with `std::env::set_var` inside `main` is too late because the macOS frameworks have
/// already queried the OS version by then. Re-launching is the only safe approach that
/// works without `unsafe` (a true `execve` replacement would require it).
///
/// Returns `Ok(false)` immediately when:
/// - `SYSTEM_VERSION_COMPAT` is already set (re-launched process, no loop), or
/// - the OCLP NVRAM key is absent.
///
/// Returns `Err` only when `std::env::current_exe()` fails or the child process cannot
/// be spawned; the caller should log the error and continue normally.
///
/// Never returns `Ok(true)`: a successful re-launch exits the current process.
#[cfg(target_os = "macos")]
pub fn relaunch_with_version_compat() -> Result<bool, String> {
    // A set variable means this is the re-launched process: no loop.
    if !compat_wanted(std::env::var_os(SYSTEM_VERSION_COMPAT_ENV).is_some(), oclp_nvram_present) {
        return Ok(false);
    }
    log::info!("startup: OCLP detected; re-launching with {}=1", SYSTEM_VERSION_COMPAT_ENV);
    eprintln!("SolidCraft: OCLP detected; re-launching with {}=1", SYSTEM_VERSION_COMPAT_ENV);
    let exe = std::env::current_exe().map_err(|e| format!("could not find own executable: {e}"))?;
    let status = std::process::Command::new(&exe)
        .args(std::env::args_os().skip(1))
        .env(SYSTEM_VERSION_COMPAT_ENV, "1")
        .status()
        .map_err(|e| format!("could not re-launch with {}: {e}", SYSTEM_VERSION_COMPAT_ENV))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Windows with OpenGL: the window keeps the system title bar (#2063). The window without OS
/// decorations keeps its caption style while having no frame, and some OpenGL drivers (older
/// Intel HD/UHD) then show the picture a caption's height lower than the window, so it no longer
/// lines up with the pointer. Direct3D 12 and Vulkan draw it in place and keep the application bar
/// as the title bar. `backends` are the ones wgpu starts: OpenGL alone is decided before the
/// window opens.
pub fn system_title_bar(windows: bool, backends: eframe::wgpu::Backends) -> bool {
    use eframe::wgpu::Backends;
    windows && backends.contains(Backends::GL) && !backends.intersects(Backends::DX12 | Backends::VULKAN)
}

/// The `--max-file-size` value (#4086): a size such as `2GB` or `1500MB`, at most 16 GB. It raises
/// the largest STEP/import and design file this run reads, over the saved preference, without
/// saving anything. A missing or bad value stops the start with this error.
pub fn max_file_size(value: Option<&str>) -> Result<u64, String> {
    let value = value.ok_or("--max-file-size needs a size, for example --max-file-size 2GB")?;
    solidcraft_engine::io::limits::parse_flag(value).map_err(|e| format!("--max-file-size: {e}"))
}

/// The message shown when no window could be opened at all.
pub fn no_window_message(error: &str, settings: Option<&Path>) -> String {
    let folder = settings.map(|d| d.display().to_string()).unwrap_or_else(|| "the SolidCraft settings folder".into());
    format!(
        "SolidCraft could not open its window.\n\n{error}\n\n\
         The software renderer was tried too. Things to try:\n\
         - Install or update the graphics driver (on Windows, from the GPU maker's site rather than Windows Update).\n\
         - Over Remote Desktop or in a virtual machine, enable 3D acceleration or run it on the machine itself.\n\
         - Start it once with `solidcraft --safe-mode`.\n\
         - Rename the settings folder: {folder}\n\n\
         More: https://docs.solidcraft.io/guide/troubleshooting.html"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards() -> Vec<AdapterChoice> {
        ["Integrated", "Discrete small", "Discrete large"].into_iter().map(|name| AdapterChoice { name: name.into(), compatible: true }).collect()
    }

    #[test]
    fn max_file_size_parses_sizes_and_refuses_bad_ones() {
        assert_eq!(max_file_size(Some("2GB")), Ok(2 << 30));
        assert_eq!(max_file_size(Some("1500MB")), Ok(1500 << 20));
        for bad in [None, Some(""), Some("2"), Some("17GB"), Some("lots"), Some("-1GB")] {
            let error = max_file_size(bad).unwrap_err();
            assert!(error.contains("--max-file-size"), "{bad:?}: {error}");
        }
    }

    #[test]
    fn adapter_names_environment_override_and_missing_card_fallback() {
        let cards = cards();
        assert_eq!(choose_adapter(&cards, None, Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, Some("LARGE"), Some("Discrete small")), Some(2));
        assert_eq!(choose_adapter(&cards, None, Some("removed")), None);
        assert_eq!(choose_adapter(&cards, Some("missing"), Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, Some(""), Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, None, Some("discrete small")), Some(1), "preference matches without regard to case");
        assert_eq!(choose_adapter(&cards, None, Some("Discrete")), None, "a preference is the full name, not a substring");
    }

    #[test]
    fn automatic_adapter_leaves_selection_to_wgpu() {
        let mut cards = cards();
        assert_eq!(choose_adapter(&cards, None, None), None);
        assert_eq!(choose_adapter(&cards, Some(""), Some("")), None);
        cards[2].compatible = false;
        // Environment still needs a compatible adapter; a preference keeps the named card (#8341).
        assert_eq!(choose_adapter(&cards, Some("large"), Some("Discrete large")), Some(2));
        assert_eq!(choose_adapter(&cards, Some("small"), None), Some(1));
        assert_eq!(choose_adapter(&[], None, None), None);
    }

    #[test]
    fn preference_keeps_a_named_card_even_when_surface_says_incompatible() {
        let mut cards = cards();
        cards[1].compatible = false;
        assert_eq!(choose_adapter(&cards, None, Some("Discrete small")), Some(1));
        assert_eq!(choose_adapter(&cards, None, Some("discrete SMALL")), Some(1));
    }

    #[test]
    fn automatic_adapter_preserves_the_previous_request_configuration() {
        use eframe::egui_wgpu::WgpuSetupCreateNew;
        let previous = WgpuSetupCreateNew::without_display_handle();
        for preference in [None, Some(String::new())] {
            let mut setup = WgpuSetupCreateNew::without_display_handle();
            configure_adapter(&mut setup, false, None, preference);
            assert!(setup.native_adapter_selector.is_none(), "Automatic must use the normal wgpu request");
            assert_eq!(setup.power_preference, previous.power_preference);
            assert_eq!(setup.instance_descriptor.backends, previous.instance_descriptor.backends);
        }
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        configure_adapter(&mut setup, false, Some("missing".into()), Some("removed".into()));
        assert!(setup.native_adapter_selector.unwrap()(&[], None).unwrap().is_none(), "missing names must use the normal request too");
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-startup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn isolated_settings_are_explicit_absolute_and_never_fall_back() {
        let default = std::env::temp_dir().join("ordinary-settings");
        assert_eq!(settings_directory(None, Some(default.clone())).unwrap(), Some(default.clone()));
        let selected = std::env::temp_dir().join("solidcraft-isolated-settings-fixture");
        assert_eq!(settings_directory(Some(selected.as_os_str()), Some(default.clone())).unwrap(), Some(selected));
        for raw in ["", "relative/settings", "../ordinary"] {
            assert!(settings_directory(Some(std::ffi::OsStr::new(raw)), Some(default.clone())).is_err());
        }
        let file = std::env::current_exe().unwrap();
        assert!(settings_directory(Some(file.as_os_str()), Some(default)).is_err());
    }

    #[test]
    fn intel_macs_start_without_indirect_call_validation() {
        use eframe::wgpu::InstanceFlags as F;
        let on = F::VALIDATION | F::VALIDATION_INDIRECT_CALL;
        assert_eq!(window_instance_flags(on, true, None), F::VALIDATION);
        assert_eq!(window_instance_flags(on, true, Some("  ")), F::VALIDATION);
        // The user's own choice stands, on or off.
        assert_eq!(window_instance_flags(on, true, Some("1")), on);
        assert_eq!(window_instance_flags(F::VALIDATION, true, Some("0")), F::VALIDATION);
        // Every other machine keeps what it had.
        assert_eq!(window_instance_flags(on, false, None), on);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn oclp_relaunch_needs_the_signal_and_no_compat_variable() {
        use super::compat_wanted;
        let probed = std::cell::Cell::new(false);
        let probe = |found: bool| {
            let probed = &probed;
            move || {
                probed.set(true);
                found
            }
        };
        // The re-launched process has the variable: no probe, no second re-launch.
        assert!(!compat_wanted(true, probe(true)));
        assert!(!probed.get());
        // The OCLP NVRAM key is the signal.
        assert!(compat_wanted(false, probe(true)));
        // No signal: an ordinary Mac starts as it is.
        assert!(!compat_wanted(false, probe(false)));
    }

    #[test]
    fn switches_read_from_the_environment() {
        assert!(env_on(Some("1")) && env_on(Some("yes")) && !env_on(Some("0")) && !env_on(Some(" ")) && !env_on(None) && !env_on(Some("false")));
        assert!(cpu_renderer_env(Some("cpu")) && cpu_renderer_env(Some("Software")) && !cpu_renderer_env(Some("gpu")) && !cpu_renderer_env(None));
    }

    #[test]
    fn settings_that_do_not_read_are_moved_aside_and_good_ones_stay() {
        let keys = ["solidcraft.prefs", "solvecraft.prefs"];
        let d = scratch("check");
        // Good files (and an empty one) stay where they are.
        std::fs::write(d.join("app.ron"), r#"{"solidcraft.prefs": "{\"grid\":true}", "egui": "(anything)"}"#).unwrap();
        std::fs::write(d.join("graphics.json"), r#"{"msaa":4}"#).unwrap();
        assert!(check_settings(&d, &keys).is_empty());
        assert!(d.join("app.ron").is_file() && d.join("graphics.json").is_file());
        // Damaged RON, preferences that aren't JSON, a truncated graphics.json, non-UTF-8 bytes.
        for (name, text) in [
            ("app.ron", b"{\"solidcraft.prefs\": \"{\\\"gr".as_slice()),
            ("app.ron", br#"{"solvecraft.prefs": "not json"}"#.as_slice()),
            ("graphics.json", br#"{"msaa":"#.as_slice()),
            ("graphics.json", b"\xff\xfe\x00".as_slice()),
        ] {
            std::fs::write(d.join(name), text).unwrap();
            let notes = check_settings(&d, &keys);
            assert_eq!(notes.len(), 1, "{name}: {notes:?}");
            assert!(!d.join(name).exists(), "{name} moved");
            let kept: Vec<_> =
                std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("{name}.bad-"))).collect();
            assert!(kept.iter().any(|e| std::fs::read(e.path()).unwrap() == text), "{name}: the bad file is kept");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn launches_that_never_got_a_window_are_counted_until_one_does() {
        let d = scratch("marker");
        assert_eq!(begin(&d), 0);
        assert_eq!(begin(&d), 1);
        assert_eq!(begin(&d), FAILED_STARTS_FOR_SAFE_MODE);
        started(&d);
        assert_eq!(begin(&d), 0);
        // A damaged marker counts as none.
        std::fs::write(d.join(MARKER), "zz").unwrap();
        assert_eq!(begin(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A discrete NVIDIA GPU, and AMD's integrated one (#3321).
    const NVIDIA: HardwareAdapter = HardwareAdapter { vendor: 0x10de, device_type: eframe::wgpu::DeviceType::DiscreteGpu };
    const AMD_IGPU: HardwareAdapter = HardwareAdapter { vendor: AMD, device_type: eframe::wgpu::DeviceType::IntegratedGpu };

    /// A probe where the backends `with` have `gpus`, and the others none.
    fn probe_with(
        with: &'static [eframe::wgpu::Backends],
        gpus: &'static [HardwareAdapter],
    ) -> impl FnMut(eframe::wgpu::Backends) -> Vec<HardwareAdapter> {
        use eframe::wgpu::Backends;
        move |b: Backends| {
            assert!(b == Backends::DX12 || b == Backends::VULKAN || b == Backends::GL, "one backend at a time: {b:?}");
            if with.contains(&b) { gpus.to_vec() } else { Vec::new() }
        }
    }

    #[test]
    fn windows_starts_d3d12_first_and_vulkan_only_when_d3d12_has_no_gpu() {
        use eframe::wgpu::Backends;
        let probe = |with: &'static [Backends]| probe_with(with, &[NVIDIA]);
        // D3D12 works: Vulkan and OpenGL are never even asked about (starting Vulkan loads
        // its drivers, which is what crashed).
        let mut asked = Vec::new();
        let chosen = choose_backends(true, false, false, None, |b| {
            asked.push(b);
            if b == Backends::DX12 { vec![NVIDIA] } else { Vec::new() }
        });
        assert_eq!(chosen, Some(Backends::DX12));
        assert_eq!(asked, [Backends::DX12]);
        // Everything works: still D3D12 alone.
        assert_eq!(choose_backends(true, false, false, None, probe(&[Backends::DX12, Backends::VULKAN, Backends::GL])), Some(Backends::DX12));
        // No D3D12 hardware adapter: Vulkan, else OpenGL, else D3D12's software adapter (WARP).
        assert_eq!(choose_backends(true, false, false, None, probe(&[Backends::VULKAN, Backends::GL])), Some(Backends::VULKAN));
        assert_eq!(choose_backends(true, false, false, None, probe(&[Backends::GL])), Some(Backends::GL));
        assert_eq!(choose_backends(true, false, false, None, probe(&[])), Some(Backends::DX12));
        // `auto` and blanks are the default order.
        assert_eq!(choose_backends(true, false, false, Some(" auto "), probe(&[Backends::DX12])), Some(Backends::DX12));
        assert_eq!(choose_backends(true, false, false, Some(""), probe(&[Backends::GL])), Some(Backends::GL));
    }

    /// AMD integrated GPUs hung under D3D12 at the first frames, the driver reset and the app
    /// crashed (#1789, #2806); they run on Vulkan (#3321).
    #[test]
    fn an_amd_integrated_gpu_starts_vulkan_rather_than_d3d12() {
        use eframe::wgpu::Backends;
        let all = &[Backends::DX12, Backends::VULKAN, Backends::GL];
        assert_eq!(choose_backends(true, false, false, None, probe_with(all, &[AMD_IGPU])), Some(Backends::VULKAN));
        // Without a Vulkan GPU: D3D12 still, rather than OpenGL or software.
        assert_eq!(choose_backends(true, false, false, None, probe_with(&[Backends::DX12], &[AMD_IGPU])), Some(Backends::DX12));
        // Another GPU beside it (a laptop's discrete one): D3D12, which can pick that one.
        assert_eq!(choose_backends(true, false, false, None, probe_with(all, &[AMD_IGPU, NVIDIA])), Some(Backends::DX12));
        // A discrete AMD GPU keeps D3D12.
        let discrete = &[HardwareAdapter { vendor: AMD, device_type: eframe::wgpu::DeviceType::DiscreteGpu }];
        assert_eq!(choose_backends(true, false, false, None, probe_with(all, discrete)), Some(Backends::DX12));
        // The preference and WGPU_BACKEND still win, and other systems are left to wgpu.
        let never = |b: Backends| -> Vec<HardwareAdapter> { panic!("no probing expected, asked about {b:?}") };
        assert_eq!(choose_backends(true, false, false, Some("dx12"), never), Some(Backends::DX12));
        assert_eq!(choose_backends(true, true, false, None, never), None);
        assert_eq!(choose_backends(false, false, false, None, never), None);
    }

    /// After the window was up, D3D12 hung the GPU: frames came a second apart and never showed,
    /// a white window with no crash (#1789, #3321). Five such frames while the user is there
    /// restart the app, D3D12 on Vulkan and anything else in safe mode.
    #[test]
    fn a_gpu_that_hangs_after_the_window_is_up_restarts_on_another_renderer() {
        use eframe::wgpu::Backend;
        let stalled = STALLED_FRAMES_FOR_RESTART;
        let mut watch = HangWatch::default();
        assert!(!watch.hung(0.0, true, 0, None));
        assert!(!watch.hung(4.0, false, stalled - 1, None), "not yet");
        assert!(watch.hung(5.0, false, stalled, None), "input 5 s ago");
        assert!(!watch.hung(6.0, true, stalled + 1, None), "once");
        // Nobody at the computer (a display switched off holds frames back too): no restart.
        let mut away = HangWatch::default();
        assert!(!away.hung(100.0, false, stalled, None));
        assert!(!away.hung(200.0, false, 100, None));
        // The input that wakes the display comes before a frame shows the GPU is fine: the
        // stalls from before don't count, and the frames on time that follow end the streak.
        let mut woke = HangWatch::default();
        assert!(!woke.hung(0.0, true, 0, None));
        assert!(!woke.hung(STALL_INPUT_WINDOW_S + 1.0, false, 30, None));
        assert!(!woke.hung(STALL_INPUT_WINDOW_S + 2.0, true, 30, None), "back at a healthy app");
        assert!(!woke.hung(STALL_INPUT_WINDOW_S + 3.0, true, 0, None));
        let later: Vec<bool> = (1..=stalled).map(|n| woke.hung(STALL_INPUT_WINDOW_S + 3.0 + f64::from(n), true, n, None)).collect();
        assert_eq!(later.last(), Some(&true), "then it hangs: {later:?}");
        // Back at a GPU that hung while nobody was there: five more stalls.
        let mut back = HangWatch::default();
        assert!(!back.hung(100.0, true, 30, None));
        assert!(!back.hung(103.0, true, 30 + stalled - 1, None));
        assert!(back.hung(104.0, true, 30 + stalled, None));
        // A surface refused for good after a reset never reaches the acquire: a blank window.
        let mut refused = HangWatch::default();
        assert!(!refused.hung(0.0, true, 0, Some(60.0)), "refused since before the user came");
        assert!(!refused.hung(REFUSED_S_FOR_RESTART - 1.0, true, 0, Some(60.0 + REFUSED_S_FOR_RESTART - 1.0)));
        assert!(refused.hung(REFUSED_S_FOR_RESTART, true, 0, Some(60.0 + REFUSED_S_FOR_RESTART)));
        let mut resized = HangWatch::default();
        assert!(!resized.hung(0.0, true, 0, Some(0.5)), "a moment's refusal");

        // Each API hangs at most once: D3D12, then Vulkan, then OpenGL, then software.
        assert_eq!(after_hang(Backend::Dx12, false), Some("vulkan"));
        assert_eq!(after_hang(Backend::Vulkan, false), Some("gl"));
        assert_eq!(after_hang(Backend::Gl, false), Some(SOFTWARE_BACKEND));
        assert_eq!(after_hang(Backend::Dx12, true), None, "software: nothing to fall back to");
        assert!(cpu_renderer_env(Some(SOFTWARE_BACKEND)), "the saved software renderer starts as such");
        let note = hang_note("Dx12", false, Some("vulkan"));
        assert!(note.contains("on Direct3D 12") && note.contains("on Vulkan") && note.contains("recovery"), "{note}");
        assert!(hang_note("Vulkan", true, Some("gl")).contains("in safe mode"));
        assert!(hang_note("Gl", false, Some("cpu")).contains("on the software renderer, which it keeps using"));
    }

    #[test]
    fn failed_windows_backends_fall_back_in_order_and_stop_after_software() {
        use eframe::wgpu::Backends as B;
        assert_eq!(fallback_backend(true, false, false, B::DX12), Some("vulkan"));
        assert_eq!(fallback_backend(true, false, false, B::VULKAN), Some("gl"));
        assert_eq!(fallback_backend(true, false, false, B::GL), Some("cpu"));
        assert_eq!(fallback_backend(true, false, true, B::DX12), None);
        assert_eq!(fallback_backend(true, false, false, B::DX12 | B::VULKAN), Some("gl"));
        assert_eq!(fallback_backend(false, false, false, B::VULKAN), Some("cpu"));
        let note = fallback_note("", B::DX12, "vulkan", "egui_pipeline\n0x80070057");
        assert!(note.contains("egui_pipeline 0x80070057") && note.contains("vulkan") && !note.contains('\n'));
        let long = fallback_note(&"x".repeat(5000), B::VULKAN, "gl", &"y".repeat(5000));
        assert!(long.chars().count() <= 4096 && long.ends_with("trying gl."));
    }

    #[test]
    fn explicit_backend_failure_never_switches_api_or_retries_software() {
        use eframe::wgpu::Backends as B;
        for windows in [false, true] {
            for cpu in [false, true] {
                for backend in [B::DX12, B::VULKAN, B::GL, B::all()] {
                    assert_eq!(fallback_backend(windows, true, cpu, backend), None);
                    let mut probed = false;
                    assert_eq!(
                        choose_backends(windows, true, cpu, Some("vulkan"), |_| {
                            probed = true;
                            Vec::new()
                        }),
                        None
                    );
                    assert!(!probed);
                }
            }
        }
    }

    #[test]
    fn only_opengl_on_windows_gets_the_system_title_bar() {
        use eframe::wgpu::Backends;
        assert!(system_title_bar(true, Backends::GL));
        for b in [Backends::DX12, Backends::VULKAN, Backends::DX12 | Backends::GL, Backends::VULKAN | Backends::GL, Backends::all()] {
            assert!(!system_title_bar(true, b), "{b:?}");
        }
        for b in [Backends::GL, Backends::VULKAN, Backends::METAL] {
            assert!(!system_title_bar(false, b), "{b:?}");
        }
    }

    #[test]
    fn wgpu_backend_the_preference_and_the_software_renderer_override_the_order() {
        use eframe::wgpu::Backends;
        let never = |b: Backends| -> Vec<HardwareAdapter> { panic!("no probing expected, asked about {b:?}") };
        // WGPU_BACKEND: wgpu reads it itself, nothing is probed.
        assert_eq!(choose_backends(true, true, false, Some("gl"), never), None);
        assert_eq!(choose_backends(true, true, true, None, never), None);
        // The software renderer (safe mode, the retry): WARP through D3D12 alone, never Vulkan.
        assert_eq!(choose_backends(true, false, true, Some("vulkan"), never), Some(Backends::DX12));
        assert_eq!(choose_backends(false, false, true, None, never), None);
        // The preference, on any system, without probing.
        assert_eq!(choose_backends(true, false, false, Some("vulkan"), never), Some(Backends::VULKAN));
        assert_eq!(choose_backends(true, false, false, Some("GL"), never), Some(Backends::GL));
        assert_eq!(choose_backends(true, false, false, Some("dx12,vulkan"), never), Some(Backends::DX12 | Backends::VULKAN));
        assert_eq!(choose_backends(false, false, false, Some("vulkan"), never), Some(Backends::VULKAN));
        // An unknown preference is the default order.
        assert_eq!(choose_backends(true, false, false, Some("glide"), probe_with(&[Backends::DX12], &[NVIDIA])), Some(Backends::DX12));
        // Elsewhere: wgpu's own choice.
        assert_eq!(choose_backends(false, false, false, None, never), None);
        assert_eq!(choose_backends(false, false, false, Some("auto"), never), None);
    }

    #[test]
    fn probing_a_backend_alone_never_panics() {
        // Whatever this machine has (often nothing): an answer, not a crash. On Windows the probe
        // goes the way the app's does, stopping at the first backend with a GPU: a machine with
        // D3D12 never loads its OpenGL driver into the test process, and AMD's (`atio6axx.dll`)
        // aborted the process as it exited (#1156).
        if cfg!(target_os = "windows") {
            let _ = choose_backends(true, false, false, None, hardware_adapters);
        } else {
            for b in WINDOWS_ORDER {
                let _ = hardware_adapters(b);
            }
        }
    }

    #[test]
    fn the_message_names_the_settings_folder_and_safe_mode() {
        let m = no_window_message("no adapter", Some(Path::new("/x/SolidCraft")));
        assert!(m.contains("no adapter") && m.contains("/x/SolidCraft") && m.contains("--safe-mode"));
    }

    #[test]
    fn the_software_adapter_is_preferred_and_its_absence_is_an_error_not_a_panic() {
        assert!(pick_cpu_adapter(&[], None).unwrap_err().contains("none"));
        // The adapters the software renderer chooses among: it starts D3D12 alone on Windows
        // (WARP), not every backend, so the test doesn't load the OpenGL driver either (#1156).
        let options = crate::wgpu_options(true, None);
        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_setup else { panic!("the software renderer makes its own device") };
        let backends = setup.instance_descriptor.backends;
        if cfg!(target_os = "windows") && std::env::var_os(WGPU_BACKEND_ENV).is_none() {
            assert_eq!(backends, eframe::wgpu::Backends::DX12);
        }
        let mut desc = solidcraft_gpu::instance_descriptor_from_env();
        desc.backends = backends;
        let instance = eframe::wgpu::Instance::new(desc);
        let adapters = pollster::block_on(instance.enumerate_adapters(backends));
        if adapters.is_empty() {
            return;
        }
        let picked = pick_cpu_adapter(&adapters, None).unwrap();
        if adapters.iter().any(|a| a.get_info().device_type == eframe::wgpu::DeviceType::Cpu) {
            assert_eq!(picked.get_info().device_type, eframe::wgpu::DeviceType::Cpu);
        }
    }
}
