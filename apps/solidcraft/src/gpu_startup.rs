//! How the window's graphics device is brought up: one switch, `SOLIDCRAFT_GPU_STARTUP`, between
//! two strategies behind the same calls, so going back is one line ([`DEFAULT`]) for everyone or
//! one variable for one machine.
//!
//! - [`GpuStartup::Standard`] (the default): one wgpu instance on wgpu's own backends, a hardware
//!   Vulkan adapter first where there is one, else wgpu's own choice (D3D12, then OpenGL, on
//!   Windows). No API is probed beforehand, no crash-guard file names one on disk, and the window
//!   is not restarted on another API when frames stall. A window that can't start falls back to
//!   the software renderer, as safe graphics mode does. Preferences › Graphics' backend choice and
//!   `WGPU_BACKEND` still pick the API.
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
/// - Otherwise `None`: the adapter is picked by [`vulkan_first`].
pub fn standard_backends(windows: bool, env_set: bool, cpu: bool, preference: Option<&str>) -> Option<eframe::wgpu::Backends> {
    use eframe::wgpu::Backends;
    if env_set {
        return None;
    }
    if cpu {
        return windows.then_some(Backends::DX12);
    }
    let preference = preference.map(str::trim).filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto"))?;
    let chosen = Backends::from_comma_list(preference);
    if chosen.is_empty() {
        log::warn!("graphics: unknown backend preference `{preference}`; using wgpu's own choice");
        return None;
    }
    Some(chosen)
}

/// What [`vulkan_first`] needs to know of an adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterKind {
    pub backend: eframe::wgpu::Backend,
    pub device_type: eframe::wgpu::DeviceType,
    /// It can draw to the window's surface.
    pub compatible: bool,
}

impl AdapterKind {
    pub fn of(adapter: &eframe::wgpu::Adapter, surface: Option<&eframe::wgpu::Surface<'_>>) -> Self {
        let info = adapter.get_info();
        Self { backend: info.backend, device_type: info.device_type, compatible: surface.is_none_or(|s| adapter.is_surface_supported(s)) }
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
        assert_eq!(standard_backends(true, false, false, None), None, "wgpu's own set; the adapter is picked Vulkan first");
        assert_eq!(standard_backends(true, false, false, Some("auto")), None);
        assert_eq!(standard_backends(false, false, false, Some(" ")), None);
        assert_eq!(standard_backends(true, false, false, Some("nonsense")), None);
        assert_eq!(standard_backends(true, false, false, Some("dx12")), Some(Backends::DX12), "Preferences › Graphics");
        assert_eq!(standard_backends(false, false, false, Some("vulkan,gl")), Some(Backends::VULKAN | Backends::GL));
        assert_eq!(standard_backends(true, true, false, Some("dx12")), None, "WGPU_BACKEND wins");
        assert_eq!(standard_backends(true, false, true, Some("vulkan")), Some(Backends::DX12), "the software renderer: WARP");
        assert_eq!(standard_backends(false, false, true, None), None);
    }

    fn kind(backend: Backend, device_type: DeviceType) -> AdapterKind {
        AdapterKind { backend, device_type, compatible: true }
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
