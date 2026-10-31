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
//! - **Graphics failure during startup** (setup, or a panic in the first ten seconds after the
//!   first frame): on Windows, try the remaining graphics APIs in order, then safe mode on the
//!   software renderer (`SOLIDCRAFT_RENDERER=cpu`). An explicit `WGPU_BACKEND` fails cleanly
//!   without switching. Exhausted retries show a message box with the error.

use std::path::{Path, PathBuf};

/// A queued presentation can return before a driver reset is reported (#1789). Keep
/// recovery available for ten seconds after it, unless the user starts interacting.
pub const STARTUP_SETTLE_TIME: std::time::Duration = std::time::Duration::from_secs(10);

/// Shared with the event-loop caller so unwinding the app can veto a relaunch if it
/// contains unsaved work. Once startup has settled or user activity begins, never rearm.
#[derive(Clone)]
pub struct Recovery(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Default for Recovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Recovery {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)))
    }

    pub fn eligible(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn observe(&self, since_present: Option<std::time::Duration>, user_activity: bool) {
        if user_activity || since_present.is_some_and(|elapsed| elapsed >= STARTUP_SETTLE_TIME) {
            self.0.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

/// Start eframe under the graphics recovery boundary, including its app-creation callback.
pub fn run_native(name: &str, options: eframe::NativeOptions, create_app: eframe::AppCreator<'_>) -> eframe::Result {
    run_graphics(|| eframe::run_native(name, options, create_app))
}

/// Keep graphics setup and the first paint inside the same recovery boundary. In particular,
/// wgpu's surface acquisition can panic after eframe has already constructed the app (#1789).
/// The installed panic hook still records the original failure before it is caught here.
pub fn run_graphics(run: impl FnOnce() -> eframe::Result) -> eframe::Result {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(result) => result,
        Err(payload) => {
            let message = payload.downcast_ref::<String>().map(String::as_str).or_else(|| payload.downcast_ref::<&str>().copied());
            let message: String = message.unwrap_or("non-text panic").chars().take(2000).collect();
            Err(eframe::Error::AppCreation(std::io::Error::other(format!("graphics event loop panicked: {message}")).into()))
        }
    }
}

/// A fresh process avoids reusing a failed device or winit event loop. Safe mode also keeps
/// saved settings untouched, disables GPU meshing, and ignores a saved backend preference.
pub fn software_retry(exe: &Path, args: impl IntoIterator<Item = std::ffi::OsString>) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.args(args).arg("--safe-mode").env(RENDERER_ENV, "cpu").env_remove(WGPU_BACKEND_ENV);
    command
}

/// Hand startup over to `command`, a retry. The failed process must be free to exit:
/// on Windows its dropped windows can still be waiting for event-loop destruction messages.
pub fn hand_off(mut command: std::process::Command) -> std::io::Result<()> {
    use std::process::Stdio;
    // Do not retain a caller's console or redirected pipes after the parent exits.
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0200); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    }
    // Dropping Child leaves the replacement running. Waiting here strands the
    // failed window because its event loop is no longer dispatching messages.
    command.spawn().map(drop)
}

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

/// Choose only surface-compatible adapters. Names saved in preferences are exact; the
/// environment override follows wgpu's case-insensitive substring convention.
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
    if let Some(name) = preference.filter(|s| !s.is_empty())
        && let Some((index, _)) = compatible().find(|(_, a)| a.name == name)
    {
        return Some(index);
    }
    None
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
/// `has_hardware(backends)` says whether those backends have an adapter that isn't software
/// (it is only asked about the backends being tried, one at a time).
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
    mut has_hardware: impl FnMut(eframe::wgpu::Backends) -> bool,
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
    Some(WINDOWS_ORDER.into_iter().find(|b| has_hardware(*b)).unwrap_or(Backends::DX12))
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

/// Whether `backends` (started on their own, nothing else) have a hardware adapter.
pub fn has_hardware_adapter(backends: eframe::wgpu::Backends) -> bool {
    let mut desc = eframe::wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    desc.flags = eframe::wgpu::InstanceFlags::from_build_config().with_env();
    desc.backend_options.dx12.shader_compiler = solidcraft_gpu::dx12_compiler();
    let instance = eframe::wgpu::Instance::new(desc);
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let found: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().device_type)).collect();
    log::info!("graphics: {backends:?} adapters: {found:?}");
    adapters.iter().any(|a| a.get_info().device_type != eframe::wgpu::DeviceType::Cpu)
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
        assert_eq!(choose_adapter(&cards, None, Some("Discrete")), None, "saved names are exact");
    }

    #[test]
    fn automatic_adapter_leaves_selection_to_wgpu() {
        let mut cards = cards();
        assert_eq!(choose_adapter(&cards, None, None), None);
        assert_eq!(choose_adapter(&cards, Some(""), Some("")), None);
        cards[2].compatible = false;
        assert_eq!(choose_adapter(&cards, Some("large"), Some("Discrete large")), None);
        assert_eq!(choose_adapter(&cards, Some("small"), None), Some(1));
        assert_eq!(choose_adapter(&[], None, None), None);
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

    #[test]
    fn a_non_text_graphics_panic_is_an_error_too() {
        let error = run_graphics(|| std::panic::panic_any(17usize)).unwrap_err();
        assert!(error.to_string().contains("non-text panic"));
    }

    #[test]
    fn a_post_present_failure_can_retry_but_interaction_or_unsaved_work_vetoes_it() {
        use std::time::Duration;
        let recovery = Recovery::new();
        let app = recovery.clone();
        app.observe(None, false);
        assert!(recovery.eligible());
        // The reporter's normal launch fails at 7 s and safe mode at 2 s, after
        // Queue::present has returned. Neither should be mistaken for stable startup.
        for seconds in [2, 7] {
            app.observe(Some(Duration::from_secs(seconds)), false);
            assert!(recovery.eligible());
        }
        let error = run_graphics(|| panic!("Surface is not configured for presentation"));
        assert!(error.is_err() && recovery.eligible());
        // App::drop observes dirty documents even if a paint panics before the next logic.
        app.observe(None, true);
        assert!(!recovery.eligible());
        app.observe(Some(Duration::ZERO), false);
        assert!(!recovery.eligible(), "saving or becoming idle must not rearm recovery");

        let recovery = Recovery::new();
        recovery.observe(Some(STARTUP_SETTLE_TIME), false);
        assert!(!recovery.eligible(), "later runtime failures must not relaunch");
    }

    #[test]
    fn software_retry_preserves_files_and_control_args_but_clears_the_backend_override() {
        let args = ["--control", "47812", "a design.solidcraft"].map(std::ffi::OsString::from);
        let command = software_retry(Path::new("solidcraft.exe"), args);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["--control", "47812", "a design.solidcraft", "--safe-mode"]);
        let env: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(env.get(std::ffi::OsStr::new(RENDERER_ENV)), Some(&Some(std::ffi::OsStr::new("cpu"))));
        assert_eq!(env.get(std::ffi::OsStr::new(WGPU_BACKEND_ENV)), Some(&None));
    }

    #[test]
    fn recovery_handoff_exits_the_parent_while_the_child_is_running() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        const STAGE: &str = "SOLIDCRAFT_TEST_HANDOFF_STAGE";
        const ROOT: &str = "SOLIDCRAFT_TEST_HANDOFF_ROOT";
        let exe = std::env::current_exe().unwrap();
        let args = ["--exact", "startup::tests::recovery_handoff_exits_the_parent_while_the_child_is_running", "--nocapture", "--"];
        if let Some(stage) = std::env::var_os(STAGE) {
            let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
            if stage == "parent" && !std::env::args().any(|arg| arg == "--safe-mode") {
                // The test process inherits this value into the replacement, without
                // mutating the environment of the outer, parallel test runner.
                hand_off(software_retry(&exe, args.map(std::ffi::OsString::from))).unwrap();
                return;
            }
            assert!(std::env::args().any(|arg| arg == "--safe-mode"));
            assert_eq!(std::env::var(RENDERER_ENV).unwrap(), "cpu");
            assert!(std::env::var_os(WGPU_BACKEND_ENV).is_none());
            std::fs::write(root.join("child-started"), "ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !root.join("release-child").exists() {
                assert!(Instant::now() < deadline, "outer test did not release replacement");
                std::thread::sleep(Duration::from_millis(20));
            }
            std::fs::write(root.join("child-finished"), "done").unwrap();
            return;
        }
        let root = scratch("handoff");
        struct Probe {
            parent: std::process::Child,
            root: PathBuf,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = std::fs::write(self.root.join("release-child"), "done");
                let _ = self.parent.kill();
                let _ = self.parent.wait();
            }
        }
        // A second invocation is the failing app's parent; its retry is the third.
        // Identify that third process by the --safe-mode flag supplied by production code.
        let mut probe = Probe {
            parent: Command::new(&exe)
                .args(args)
                .env(STAGE, "parent")
                .env(ROOT, &root)
                .env(WGPU_BACKEND_ENV, "vulkan")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            root,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = probe.parent.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "failed parent is still waiting for the replacement");
            std::thread::sleep(Duration::from_millis(20));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !probe.root.join("child-started").exists() {
            assert!(Instant::now() < deadline, "replacement did not start after parent exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!probe.root.join("child-finished").exists());
        std::fs::write(probe.root.join("release-child"), "done").unwrap();
        while !probe.root.join("child-finished").exists() {
            assert!(Instant::now() < deadline, "replacement did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// #1789's exact validation panic, on WARP, without a driver reset. A failed
    /// reconfiguration can remove wgpu's presentation state while leaving a surface object.
    #[cfg(target_os = "windows")]
    #[test]
    fn an_unconfigured_surface_after_setup_reaches_graphics_recovery() {
        use eframe::wgpu;
        use winit::{event_loop::EventLoop, platform::windows::EventLoopBuilderExtWindows, window::Window};

        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        let event_loop = builder.build().unwrap();
        #[allow(deprecated)]
        let window = std::sync::Arc::new(event_loop.create_window(Window::default_attributes().with_visible(false)).unwrap());
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance.create_surface(window).unwrap();
        let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
            .into_iter()
            .find(|a| a.get_info().device_type == wgpu::DeviceType::Cpu && a.is_surface_supported(&surface));
        let Some(adapter) = adapter else {
            assert!(std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "WARP is required for the surface regression");
            eprintln!("skipping the surface regression: no WARP adapter");
            return;
        };
        // The reporter's second failure is a surface reconfigure racing thumbnails on
        // the very same queue. Exercise the actual painter resize and capture paths.
        #[allow(deprecated)]
        let capture_window = std::sync::Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_visible(false).with_inner_size(winit::dpi::PhysicalSize::new(64, 64)))
                .unwrap(),
        );
        captures_during_surface_resize(capture_window);
        let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let config = surface.get_default_config(&adapter, 64, 64).unwrap();
        surface.configure(&device, &config);
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            other => panic!("WARP must acquire the initial frame: {other:?}"),
        };
        // Capturing this deliberate configuration failure lets the next acquire reproduce
        // the report's secondary panic. Drop the old frame inside the same error scope.
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &config);
        drop(frame);
        assert!(pollster::block_on(validation.pop()).is_some());
        let result = run_graphics(|| {
            let _frame = surface.get_current_texture();
            Ok(())
        });
        assert!(result.unwrap_err().to_string().contains("Surface is not configured for presentation"));
    }

    #[cfg(target_os = "windows")]
    fn captures_during_surface_resize(window: std::sync::Arc<winit::window::Window>) {
        use eframe::egui_wgpu;
        use std::num::NonZeroU32;
        use std::sync::Arc;
        let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(
            egui::Context::default(),
            crate::wgpu_options(true, None),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(egui::ViewportId::ROOT, Some(window))).unwrap();
        let rs = painter.render_state().unwrap();
        assert_eq!(rs.adapter.get_info().device_type, eframe::wgpu::DeviceType::Cpu);
        let capturer = solidcraft_gpu::capture::Capturer::new(&rs.adapter, rs.device.clone(), rs.queue.clone())
            .unwrap()
            .with_queue_access(rs.queue_access.clone());
        let barrier = Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let capture_barrier = barrier.clone();
            scope.spawn(move || {
                let scene = solidcraft_engine::render::Scene::default();
                let camera = solidcraft_engine::render::Camera::default();
                capture_barrier.wait();
                for _ in 0..64 {
                    let rgba = capturer.rgba(&scene, &camera, 64, 64).unwrap();
                    assert_eq!(rgba.len(), 64 * 64 * 4);
                }
            });
            barrier.wait();
            for i in 0..256 {
                painter.on_window_resized(egui::ViewportId::ROOT, NonZeroU32::new(64 + i % 2).unwrap(), NonZeroU32::new(64).unwrap());
            }
        });
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

    #[test]
    fn windows_starts_d3d12_first_and_vulkan_only_when_d3d12_has_no_gpu() {
        use eframe::wgpu::Backends;
        let probe = |with: &'static [Backends]| {
            move |b: Backends| {
                assert!(b == Backends::DX12 || b == Backends::VULKAN || b == Backends::GL, "one backend at a time: {b:?}");
                with.contains(&b)
            }
        };
        // D3D12 works: Vulkan and OpenGL are never even asked about (starting Vulkan loads
        // its drivers, which is what crashed).
        let mut asked = Vec::new();
        let chosen = choose_backends(true, false, false, None, |b| {
            asked.push(b);
            b == Backends::DX12
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
                            false
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
        let never = |b: Backends| -> bool { panic!("no probing expected, asked about {b:?}") };
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
        assert_eq!(choose_backends(true, false, false, Some("glide"), |b| b == Backends::DX12), Some(Backends::DX12));
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
            let _ = choose_backends(true, false, false, None, has_hardware_adapter);
        } else {
            for b in WINDOWS_ORDER {
                let _ = has_hardware_adapter(b);
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
