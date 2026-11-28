//! How the window's graphics device is brought up: one switch, `SOLIDCRAFT_GPU_STARTUP`, between
//! two strategies behind the same calls, so going back is one line ([`DEFAULT`]) for everyone or
//! one variable for one machine.
//!
//! - [`GpuStartup::Standard`] (the default): one wgpu instance on wgpu's own backends, a hardware
//!   Vulkan adapter first where there is one, else wgpu's own choice (D3D12, then OpenGL, on
//!   Windows). No API is probed beforehand, and the window is not restarted on another API when
//!   frames stall. A crash-guard file names an API on disk only while Intel's Vulkan driver
//!   creates the window's device on Windows ([`guards_device_creation`], #22110); after a crash
//!   there, the launches of that version start the other APIs. On Windows, in Automatic, OpenGL
//!   is named the same way while the instance starts, until the adapter is chosen
//!   ([`guards_instance_creation`], #22917). A window that can't start falls
//!   back to the software renderer, as safe graphics mode does. Preferences › Graphics' backend
//!   choice and `WGPU_BACKEND` still pick the API.
//! - [`GpuStartup::Legacy`]: the startup every release had before: on Windows each API probed for
//!   a hardware adapter in turn (Vulkan, D3D12, OpenGL), the crash guard (#18683), the fallback
//!   through the APIs when a window can't start, and the restart on another API when the GPU hangs
//!   (#3321). It runs exactly as before; it is deleted in the next minor release if nobody needs it.

use std::sync::OnceLock;

/// The variable that picks the strategy: `standard` or `legacy`. Anything else is the default.
pub const ENV: &str = "SOLIDCRAFT_GPU_STARTUP";

/// How the window's graphics device is brought up (the module's docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuStartup {
    /// One instance, Vulkan first, else wgpu's own choice.
    Standard,
    /// The API probe ladder, crash guard and hang restarts every release had before.
    Legacy,
}

/// The strategy without the variable. Going back to the old startup for everyone is this line.
pub const DEFAULT: GpuStartup = GpuStartup::Standard;

impl GpuStartup {
    /// The strategy `value` (the variable's value) names; [`DEFAULT`] when it names none.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("legacy") => Self::Legacy,
            Some("standard") => Self::Standard,
            _ => DEFAULT,
        }
    }

    /// This process's strategy, read once from [`ENV`].
    pub fn current() -> Self {
        static CURRENT: OnceLock<GpuStartup> = OnceLock::new();
        *CURRENT.get_or_init(|| Self::parse(std::env::var(ENV).ok().as_deref()))
    }

    /// The strategy's name, as the variable takes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Legacy => "legacy",
        }
    }

    /// Whether this is the old startup, with its probes, crash guard and restarts.
    pub fn legacy(self) -> bool {
        self == Self::Legacy
    }
}

/// [`GpuStartup::Standard`]'s instance backends, or `None` for wgpu's own set (`WGPU_BACKEND`
/// when set, else every backend wgpu was built with):
/// - `WGPU_BACKEND` set (`env_set`): `None`, whatever it says wins.
/// - The software renderer (`cpu`): D3D12 alone on Windows, whose software adapter is WARP;
///   `None` elsewhere (lavapipe, llvmpipe).
/// - A backend `preference` (Preferences › Graphics, `dx12`, `vulkan`, `gl` or a comma list;
///   `auto` or empty for none): those backends.
/// - An API whose driver crashed the process while its device was created on this version
///   (`crashed`, recorded only under [`guards_device_creation`]'s condition): every other API this
///   system has, so that driver is not loaded again (#22110).
/// - Otherwise `None`: the adapter is picked by [`vulkan_first`].
pub fn standard_backends(
    windows: bool,
    env_set: bool,
    cpu: bool,
    preference: Option<&str>,
    crashed: eframe::wgpu::Backends,
) -> Option<eframe::wgpu::Backends> {
    use eframe::wgpu::Backends;
    if env_set {
        return None;
    }
    if cpu {
        return windows.then_some(Backends::DX12);
    }
    if let Some(chosen) = chosen_backends(preference) {
        return Some(chosen);
    }
    if crashed.is_empty() {
        return None;
    }
    let platform = if windows { Backends::VULKAN | Backends::DX12 | Backends::GL } else { Backends::VULKAN | Backends::GL | Backends::METAL };
    Some(crate::startup::without_crashed(platform, crashed))
}

/// The backends a Preferences › Graphics choice names (`dx12`, `vulkan`, `gl` or a comma list);
/// `None` for Automatic (`auto` or empty), the software renderer's `cpu`, or a name wgpu doesn't
/// know.
pub fn chosen_backends(preference: Option<&str>) -> Option<eframe::wgpu::Backends> {
    let preference = preference.map(str::trim).filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto"))?;
    if crate::startup::cpu_renderer_env(Some(preference)) {
        return None;
    }
    let chosen = eframe::wgpu::Backends::from_comma_list(preference);
    if chosen.is_empty() {
        log::warn!("graphics: unknown backend preference `{preference}`; using wgpu's own choice");
        return None;
    }
    Some(chosen)
}

/// Intel's PCI vendor id.
const INTEL_VENDOR: u32 = 0x8086;

/// Whether the window's device on `backend` and `vendor`'s adapter is named on disk while it is
/// created, so a crash inside the driver is remembered (`startup::BackendGuard`) and the next
/// launches of this version start another API. Only Intel's Vulkan driver on Windows
/// (`igvk64.dll`), which crashes with an access violation while creating the device on HD, UHD,
/// Iris Plus and Iris Xe graphics (#22110, #18683); every other adapter starts as before, with
/// nothing written.
pub fn guards_device_creation(windows: bool, backend: eframe::wgpu::Backend, vendor: u32) -> bool {
    windows && backend == eframe::wgpu::Backend::Vulkan && vendor == INTEL_VENDOR
}

/// Whether OpenGL is named on disk while the window's instance starts, so a crash inside its
/// driver then is remembered (`startup::BackendGuard`) and the next launches of this version
/// leave OpenGL out ([`standard_backends`]'s `crashed`). On Windows wgpu loads the OpenGL driver
/// when the instance is created, before any adapter or vendor is known, and AMD's
/// (`atio6axx.dll`) crashes there on Radeon 780M laptops when Vulkan starts in the same instance
/// (#22917). Only when Automatic picked the `backends` (`automatic`: no Preferences choice, no
/// `WGPU_BACKEND`, not the software renderer) and they hold OpenGL next to Vulkan or D3D12: a
/// chosen API is never second-guessed, and an instance of OpenGL alone has nothing to fall back
/// to. Nothing is named on other systems.
pub fn guards_instance_creation(windows: bool, backends: eframe::wgpu::Backends, automatic: bool) -> bool {
    use eframe::wgpu::Backends;
    windows && automatic && backends.contains(Backends::GL) && backends.intersects(Backends::VULKAN | Backends::DX12)
}

/// The software renderer's backends in safe graphics mode: D3D12 alone on Windows, whose
/// software adapter (WARP) is the one it picks, as outside safe graphics ([`standard_backends`]),
/// so no Vulkan or OpenGL driver is loaded beside it (#22917: AMD's OpenGL driver crashed
/// `--safe-mode`). `None` (wgpu's own set) elsewhere, or once D3D12 crashed on this version.
pub fn safe_software_backends(windows: bool, crashed: eframe::wgpu::Backends) -> Option<eframe::wgpu::Backends> {
    use eframe::wgpu::Backends;
    (windows && !crashed.contains(Backends::DX12)).then_some(Backends::DX12)
}

/// What [`vulkan_first`] needs to know of an adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterKind {
    pub backend: eframe::wgpu::Backend,
    pub device_type: eframe::wgpu::DeviceType,
    /// The PCI vendor id ([`guards_device_creation`]).
    pub vendor: u32,
    /// It can draw to the window's surface.
    pub compatible: bool,
}

impl AdapterKind {
    pub fn of(adapter: &eframe::wgpu::Adapter, surface: Option<&eframe::wgpu::Surface<'_>>) -> Self {
        let info = adapter.get_info();
        Self {
            backend: info.backend,
            device_type: info.device_type,
            vendor: info.vendor,
            compatible: surface.is_none_or(|s| adapter.is_surface_supported(s)),
        }
    }
}

/// [`GpuStartup::Standard`]'s adapter among `adapters`: the hardware Vulkan adapter that can draw
/// to the window, a discrete GPU before an integrated one, as wgpu's high-performance preference
/// orders them; `None` when there is none, and wgpu picks among all its backends' adapters. A
/// software Vulkan adapter (lavapipe) is never preferred over another API's hardware one.
pub fn vulkan_first(adapters: &[AdapterKind]) -> Option<usize> {
    use eframe::wgpu::DeviceType;
    let rank = |t: DeviceType| match t {
        DeviceType::DiscreteGpu => 3,
        DeviceType::IntegratedGpu => 2,
        DeviceType::VirtualGpu => 1,
        DeviceType::Other | DeviceType::Cpu => 0,
    };
    adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.backend == eframe::wgpu::Backend::Vulkan && a.compatible && rank(a.device_type) > 0)
        // The first of the best: wgpu lists adapters in the driver's order.
        .fold(None, |best: Option<(usize, i32)>, (i, a)| match best {
            Some((_, r)) if r >= rank(a.device_type) => best,
            _ => Some((i, rank(a.device_type))),
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::wgpu::{Backend, Backends, DeviceType};

    #[test]
    fn the_variable_picks_the_strategy_and_anything_else_is_the_default() {
        assert_eq!(GpuStartup::parse(Some("legacy")), GpuStartup::Legacy);
        assert_eq!(GpuStartup::parse(Some(" Legacy \n")), GpuStartup::Legacy);
        assert_eq!(GpuStartup::parse(Some("standard")), GpuStartup::Standard);
        for other in [None, Some(""), Some("ladder"), Some("1")] {
            assert_eq!(GpuStartup::parse(other), DEFAULT, "{other:?}");
        }
        assert_eq!(DEFAULT, GpuStartup::Standard);
        assert!(GpuStartup::Legacy.legacy() && !GpuStartup::Standard.legacy());
        for s in [GpuStartup::Standard, GpuStartup::Legacy] {
            assert_eq!(GpuStartup::parse(Some(s.name())), s);
        }
    }

    #[test]
    fn standard_starts_wgpus_backends_unless_something_names_one() {
        let none = Backends::empty();
        assert_eq!(standard_backends(true, false, false, None, none), None, "wgpu's own set; the adapter is picked Vulkan first");
        assert_eq!(standard_backends(true, false, false, Some("auto"), none), None);
        assert_eq!(standard_backends(false, false, false, Some(" "), none), None);
        assert_eq!(standard_backends(true, false, false, Some("nonsense"), none), None);
        assert_eq!(standard_backends(true, false, false, Some("dx12"), none), Some(Backends::DX12), "Preferences › Graphics");
        assert_eq!(standard_backends(false, false, false, Some("vulkan,gl"), none), Some(Backends::VULKAN | Backends::GL));
        assert_eq!(standard_backends(true, true, false, Some("dx12"), none), None, "WGPU_BACKEND wins");
        assert_eq!(standard_backends(true, false, true, Some("vulkan"), none), Some(Backends::DX12), "the software renderer: WARP");
        assert_eq!(standard_backends(false, false, true, None, none), None);
    }

    /// #22110: after Intel's Vulkan driver crashed the process while the window's device was
    /// created, Automatic starts the other APIs only, so Vulkan's driver is never loaded again on
    /// this version; a choice in Preferences and `WGPU_BACKEND` still win.
    #[test]
    fn a_crashed_api_is_left_out_of_automatic_and_a_choice_still_wins() {
        let vk = Backends::VULKAN;
        assert_eq!(standard_backends(true, false, false, None, vk), Some(Backends::DX12 | Backends::GL));
        assert_eq!(standard_backends(true, false, false, Some("auto"), vk), Some(Backends::DX12 | Backends::GL));
        assert_eq!(standard_backends(false, false, false, None, vk), Some(Backends::GL | Backends::METAL));
        assert_eq!(standard_backends(true, false, false, Some("gl"), vk), Some(Backends::GL), "the user's choice");
        assert_eq!(standard_backends(true, false, false, Some("vulkan"), vk), Some(vk), "an explicit Vulkan choice");
        assert_eq!(standard_backends(true, true, false, None, vk), None, "WGPU_BACKEND wins");
        assert_eq!(standard_backends(true, false, true, None, vk), Some(Backends::DX12), "the software renderer: WARP");
        let all = Backends::VULKAN | Backends::DX12 | Backends::GL;
        assert_eq!(standard_backends(true, false, false, None, all), Some(all), "everything crashed: all are tried again");
    }

    #[test]
    fn a_choice_names_backends_and_automatic_or_software_names_none() {
        assert_eq!(chosen_backends(Some("gl")), Some(Backends::GL));
        assert_eq!(chosen_backends(Some(" DX12 ")), Some(Backends::DX12));
        for none in [None, Some(""), Some("auto"), Some("Auto"), Some("cpu"), Some("nonsense")] {
            assert_eq!(chosen_backends(none), None, "{none:?}");
        }
    }

    /// Only Intel's Vulkan driver on Windows is named on disk while the device is created: every
    /// other adapter, and Intel on another API or system, starts exactly as before.
    #[test]
    fn only_intels_vulkan_device_on_windows_is_guarded() {
        const NVIDIA: u32 = 0x10de;
        const AMD: u32 = 0x1002;
        assert!(guards_device_creation(true, Backend::Vulkan, INTEL_VENDOR));
        for (windows, backend, vendor) in [
            (false, Backend::Vulkan, INTEL_VENDOR),
            (true, Backend::Dx12, INTEL_VENDOR),
            (true, Backend::Gl, INTEL_VENDOR),
            (true, Backend::Vulkan, NVIDIA),
            (true, Backend::Vulkan, AMD),
            (true, Backend::Vulkan, 0),
        ] {
            assert!(!guards_device_creation(windows, backend, vendor), "{windows} {backend:?} {vendor:#x}");
        }
    }

    /// #22917: OpenGL is named on disk while the instance starts only on Windows, in Automatic,
    /// and only next to another API; a chosen API, the software renderer and other systems name
    /// nothing.
    #[test]
    fn only_automatics_opengl_instance_on_windows_is_guarded() {
        let all = Backends::VULKAN | Backends::DX12 | Backends::GL;
        let egui_default = Backends::PRIMARY | Backends::GL;
        assert!(guards_instance_creation(true, all, true));
        assert!(guards_instance_creation(true, egui_default, true), "wgpu's own set");
        assert!(guards_instance_creation(true, Backends::DX12 | Backends::GL, true), "after a Vulkan crash");
        for (windows, backends, automatic) in [
            (false, all, true),
            (false, egui_default, true),
            (true, all, false),
            (true, Backends::GL, true),
            (true, Backends::GL, false),
            (true, Backends::VULKAN | Backends::DX12, true),
            (true, Backends::DX12, true),
            (true, Backends::VULKAN, true),
        ] {
            assert!(!guards_instance_creation(windows, backends, automatic), "{windows} {backends:?} {automatic}");
        }
    }

    /// #22917: after AMD's OpenGL driver crashed the process while the instance started,
    /// Automatic starts Vulkan and D3D12 only on Windows; a choice and `WGPU_BACKEND` still win.
    /// A chosen D3D12 or Vulkan never includes OpenGL, whatever crashed.
    #[test]
    fn a_crashed_opengl_is_left_out_of_automatic_and_a_chosen_api_never_starts_it() {
        let gl = Backends::GL;
        assert_eq!(standard_backends(true, false, false, None, gl), Some(Backends::VULKAN | Backends::DX12));
        assert_eq!(standard_backends(true, false, false, Some("auto"), gl), Some(Backends::VULKAN | Backends::DX12));
        assert_eq!(standard_backends(true, false, false, Some("gl"), gl), Some(gl), "an explicit OpenGL choice");
        assert_eq!(standard_backends(true, true, false, None, gl), None, "WGPU_BACKEND wins");
        assert_eq!(standard_backends(true, false, true, None, gl), Some(Backends::DX12), "the software renderer: WARP");
        for crashed in [Backends::empty(), gl, Backends::VULKAN, Backends::DX12] {
            for (choice, backends) in [("dx12", Backends::DX12), ("vulkan", Backends::VULKAN)] {
                let started = standard_backends(true, false, false, Some(choice), crashed);
                assert_eq!(started, Some(backends), "{choice} after {crashed:?}");
                assert!(!started.unwrap().contains(gl));
            }
        }
        // Nothing recorded: wgpu's own set, exactly as before.
        assert_eq!(standard_backends(true, false, false, None, Backends::empty()), None);
    }

    #[test]
    fn safe_software_renderer_starts_d3d12_alone_on_windows() {
        assert_eq!(safe_software_backends(true, Backends::empty()), Some(Backends::DX12));
        assert_eq!(safe_software_backends(true, Backends::GL), Some(Backends::DX12));
        assert_eq!(safe_software_backends(true, Backends::VULKAN | Backends::GL), Some(Backends::DX12));
        assert_eq!(safe_software_backends(true, Backends::DX12), None, "WARP's API crashed: wgpu's own set less it");
        assert_eq!(safe_software_backends(false, Backends::empty()), None, "lavapipe, llvmpipe: as before");
    }

    fn kind(backend: Backend, device_type: DeviceType) -> AdapterKind {
        AdapterKind { backend, device_type, vendor: 0, compatible: true }
    }

    #[test]
    fn a_hardware_vulkan_adapter_comes_first_and_nothing_else_is_picked() {
        let (vk, dx, gl) = (Backend::Vulkan, Backend::Dx12, Backend::Gl);
        // Windows with one GPU: D3D12 and OpenGL list it too, Vulkan's wins.
        let windows =
            [kind(dx, DeviceType::IntegratedGpu), kind(vk, DeviceType::IntegratedGpu), kind(gl, DeviceType::Other), kind(dx, DeviceType::Cpu)];
        assert_eq!(vulkan_first(&windows), Some(1));
        // Integrated and discrete: the discrete GPU, as wgpu's high-performance preference does.
        let both = [kind(vk, DeviceType::IntegratedGpu), kind(vk, DeviceType::DiscreteGpu), kind(vk, DeviceType::DiscreteGpu)];
        assert_eq!(vulkan_first(&both), Some(1));
        // No Vulkan driver (older Intel graphics, virtual machines, Remote Desktop): wgpu picks.
        assert_eq!(vulkan_first(&[kind(dx, DeviceType::IntegratedGpu), kind(dx, DeviceType::Cpu)]), None);
        // Only lavapipe on Vulkan: never over another API's hardware adapter.
        assert_eq!(vulkan_first(&[kind(vk, DeviceType::Cpu), kind(gl, DeviceType::IntegratedGpu)]), None);
        // A Vulkan adapter that can't draw to the window is passed over.
        let unusable = AdapterKind { compatible: false, ..kind(vk, DeviceType::DiscreteGpu) };
        assert_eq!(vulkan_first(&[unusable, kind(vk, DeviceType::IntegratedGpu)]), Some(1));
        assert_eq!(vulkan_first(&[]), None);
    }
}
