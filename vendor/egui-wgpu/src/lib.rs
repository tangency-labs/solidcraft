//! This crates provides bindings between [`egui`](https://github.com/emilk/egui) and [wgpu](https://crates.io/crates/wgpu).
//!
//! If you're targeting WebGL you also need to turn on the
//! `webgl` feature of the `wgpu` crate:
//!
//! ```toml
//! # Enable both WebGL and WebGPU backends on web.
//! wgpu = { version = "*", features = ["webgpu", "webgl"] }
//! ```
//!
//! You can control whether WebGL or WebGPU will be picked at runtime by configuring
//! [`WgpuConfiguration::wgpu_setup`].
//! The default is to prefer WebGPU and fall back on WebGL.
//!
//! ## Feature flags
#![doc = document_features::document_features!()]
//!

pub use wgpu;

/// Low-level painting of [`egui`](https://github.com/emilk/egui) on [`wgpu`].
mod renderer;

mod setup;

pub use renderer::*;
pub use setup::{
    EguiDisplayHandle, NativeAdapterSelectorMethod, WgpuSetup, WgpuSetupCreateNew,
    WgpuSetupExisting,
};

/// Helpers for capturing screenshots of the UI.
#[cfg(feature = "capture")]
pub mod capture;

/// Module for painting [`egui`](https://github.com/emilk/egui) with [`wgpu`] on [`winit`].
#[cfg(feature = "winit")]
pub mod winit;

use std::sync::Arc;

use epaint::mutex::RwLock;

/// SolidCraft: what [`set_startup_hook`] is told before each step of getting the graphics
/// device and the window's surface up, so a crash inside the driver (with no Rust panic) can
/// still say which step it died in. `detail` is empty except for [`STEP_ADAPTER_CHOSEN`].
pub type StartupHook = fn(step: &'static str, detail: &str);

static STARTUP_HOOK: std::sync::OnceLock<StartupHook> = std::sync::OnceLock::new();

/// SolidCraft: the steps [`StartupHook`] is told about, in order.
pub const STEP_INSTANCE: &str = "graphics instance";
pub const STEP_SURFACE: &str = "window surface";
pub const STEP_ADAPTER: &str = "adapter request";
/// The adapter was picked; `detail` names it, its backend and driver.
pub const STEP_ADAPTER_CHOSEN: &str = "adapter chosen";
pub const STEP_DEVICE: &str = "device creation";
pub const STEP_RENDERER: &str = "egui renderer (shader compile)";
pub const STEP_SURFACE_CONFIGURE: &str = "surface configure";
pub const STEP_FIRST_PRESENT: &str = "first frame (present)";

/// SolidCraft: install the startup hook (once; later calls are ignored).
pub fn set_startup_hook(hook: StartupHook) {
    let _ = STARTUP_HOOK.set(hook);
}

pub(crate) fn startup_step(step: &'static str, detail: &str) {
    if let Some(hook) = STARTUP_HOOK.get() {
        hook(step, detail);
    }
}

/// An error produced by egui-wgpu.
#[derive(thiserror::Error, Debug)]
pub enum WgpuError {
    #[error(transparent)]
    RequestAdapterError(#[from] wgpu::RequestAdapterError),

    #[error("Adapter selection failed: {0}")]
    CustomNativeAdapterSelectionError(String),

    #[error("There was no valid format for the surface at all.")]
    NoSurfaceFormatsAvailable,

    #[error(transparent)]
    RequestDeviceError(#[from] wgpu::RequestDeviceError),

    /// SolidCraft: pipeline validation must return to the startup retry.
    #[error("Could not create the egui renderer: {0}")]
    CreateRendererError(wgpu::Error),

    #[error(transparent)]
    CreateSurfaceError(#[from] wgpu::CreateSurfaceError),

    #[cfg(feature = "winit")]
    #[error(transparent)]
    HandleError(#[from] ::winit::raw_window_handle::HandleError),
}

/// Runtime-mutable subset of [`WgpuConfiguration`].
///
/// Edit any field to have the surface reconfigured on the next paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceConfig {
    /// Present mode used for the primary surface.
    pub present_mode: wgpu::PresentMode,

    /// Desired maximum number of frames that the presentation engine should queue in advance.
    ///
    /// Use `1` for low-latency, and `2` for high-throughput.
    ///
    /// See [`wgpu::SurfaceConfiguration::desired_maximum_frame_latency`] for details.
    ///
    /// `None` => Let `wgpu` pick a default (currently `2`).
    pub desired_maximum_frame_latency: Option<u32>,
}

impl SurfaceConfig {
    /// Good default for GUIs with very little (or no) extra GPU work.
    pub const LOW_LATENCY: Self = Self {
        present_mode: wgpu::PresentMode::AutoVsync,

        desired_maximum_frame_latency: if cfg!(target_os = "ios") {
            None // The default is good on iOS, while `Some(1)` cuts FPS in half
        } else {
            Some(1)
        },
    };

    /// Good default for GUIs with a lot of extra GPU work,
    /// or that want to prioritize smoothness over latency.
    pub const HIGH_THROUGHPUT: Self = Self {
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: Some(2), // High-throughput.
    };
}

/// SolidCraft: whether a device was lost (the driver reset it, Windows' TDR among others, #4172).
///
/// A lost device can't be used again. wgpu only learns of the loss when a call into the driver
/// fails: from then on every call on the device fails quietly, and the device's lost callback
/// marks this. But a queue write that is the first call to meet the loss panics inside wgpu
/// (`PendingWrites::activate`), so the painter first probes the device ([`DeviceLost::probe`]):
/// the probe meets the loss instead, and the frame's uploads are skipped.
#[derive(Clone, Debug, Default)]
pub struct DeviceLost {
    lost: Arc<std::sync::atomic::AtomicBool>,
    reason: Arc<std::sync::Mutex<Option<String>>>,
}

impl DeviceLost {
    /// Watch `device`: its lost callback marks the loss. A device has one lost callback, so this
    /// replaces any set before.
    pub fn watch(device: &wgpu::Device) -> Self {
        let lost = Self::default();
        let seen = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            seen.mark(&format!("{reason:?}: {message}"));
        });
        lost
    }

    /// Mark the device lost, for `reason` (the first reason is kept).
    pub fn mark(&self, reason: &str) {
        if let Ok(mut kept) = self.reason.lock() {
            kept.get_or_insert_with(|| reason.to_owned());
        }
        self.lost.store(true, std::sync::atomic::Ordering::Release);
    }

    /// The device was lost.
    pub fn is_lost(&self) -> bool {
        self.lost.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Why the device was lost, as wgpu said it.
    pub fn reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|r| r.clone())
    }

    /// The flag [`DeviceLost::is_lost`] reads, for others drawing on the same device.
    pub fn flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.lost.clone()
    }

    /// Whether the device is lost, asking the driver first, so a loss it reports is marked here
    /// rather than met by the next upload.
    ///
    /// The probe opens a command list, as the upload would: a queue write's first call into the
    /// driver is `begin_encoding`, and on D3D12 that is where a reset device says so. An empty
    /// submission only signals a fence: #6528 met a reset D3D12 device in the upload right after
    /// one. wgpu handles a failed `begin_encoding` in a command encoder's `finish` (it loses the
    /// device, and the lost callback marks it); only its queue writes unwrap it.
    pub fn probe(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> bool {
        if self.is_lost() {
            return true;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("device lost probe"),
        });
        encoder.insert_debug_marker("device lost probe");
        queue.submit([encoder.finish()]);
        self.is_lost()
    }
}

/// SolidCraft: a frame acquired from a window's surface, which gives its swapchain image back
/// however the frame ends: presented, dropped unpresented, or dropped while a panic unwinds
/// (#7148).
///
/// wgpu presents or discards a frame only on a valid device. On a lost one both return early, and
/// the surface keeps the frame's image, which holds one of the swapchain's acquire semaphores.
/// Dropping or reconfiguring that surface later (the window closing as the app restarts after the
/// loss) then panics in wgpu-hal: "Trying to destroy a SwapchainAcquireSemaphore that is still in
/// use by a SurfaceTexture". So the frame keeps a second handle to its texture, and once the frame
/// is gone that handle is destroyed ([`FreeSurfaceTexture`]), which takes the image (and its
/// semaphores) back from the surface, lost device or not. After a present or discard that worked,
/// the image is already back and the destroy does nothing.
///
/// The order is the struct's: Rust drops fields in declaration order, so [`Self::frame`] always
/// goes before the texture handle is destroyed, and [`Self::present`] presents before it.
pub struct AcquiredFrame<F, T: FreeSurfaceTexture> {
    /// The frame; dropped (discarded) before the texture is freed.
    pub frame: F,
    texture: FreeOnDrop<T>,
}

impl<F, T: FreeSurfaceTexture> AcquiredFrame<F, T> {
    /// `texture` is a second handle to `frame`'s texture.
    pub fn new(frame: F, texture: T) -> Self {
        Self {
            frame,
            texture: FreeOnDrop(texture),
        }
    }

    /// Present the frame with `present`, then free the texture.
    pub fn present(self, present: impl FnOnce(F)) {
        let Self { frame, texture } = self;
        present(frame);
        drop(texture);
    }
}

/// SolidCraft: what [`AcquiredFrame`] does with its texture handle once the frame is gone.
pub trait FreeSurfaceTexture {
    /// Give the surface's image back, whatever the device's state; nothing once it is back.
    fn free(&self);
}

impl FreeSurfaceTexture for wgpu::Texture {
    /// wgpu-core's destroy of a surface texture takes its image and drops it without checking the
    /// device, and does nothing when a present or discard took it already.
    fn free(&self) {
        self.destroy();
    }
}

/// Frees its texture when dropped, also while a panic unwinds.
struct FreeOnDrop<T: FreeSurfaceTexture>(T);

impl<T: FreeSurfaceTexture> Drop for FreeOnDrop<T> {
    fn drop(&mut self) {
        self.0.free();
    }
}

/// Access to the render state for egui.
#[derive(Clone)]
pub struct RenderState {
    /// Wgpu adapter used for rendering.
    pub adapter: wgpu::Adapter,

    /// All the available adapters.
    ///
    /// This is not available on web.
    /// On web, we always select WebGPU is available, then fall back to WebGL if not.
    #[cfg(not(target_arch = "wasm32"))]
    pub available_adapters: Vec<wgpu::Adapter>,

    /// Wgpu instance used for creating surfaces and adapters.
    pub instance: wgpu::Instance,

    /// Wgpu device used for rendering, created from the adapter.
    pub device: wgpu::Device,

    /// Wgpu queue used for rendering, created from the adapter.
    pub queue: wgpu::Queue,

    /// The target texture format used for presenting to the window.
    pub target_format: wgpu::TextureFormat,

    /// Effective sample count after backend compatibility handling.
    /// Custom paint callbacks must use this, rather than the requested setting.
    pub msaa_samples: u32,

    /// SolidCraft: the MSAA sample counts this device can render the target (and depth) format
    /// with, ascending, always starting with 1.
    pub msaa_supported: Vec<u32>,

    /// Egui renderer responsible for drawing the UI.
    pub renderer: Arc<RwLock<Renderer>>,

    /// Runtime-mutable subset of the wgpu configuration.
    ///
    /// Update this to have the surface reconfigured on the next paint.
    pub surface_config: SurfaceConfig,

    /// SolidCraft: the device was lost; the painter draws nothing more (#4172).
    pub device_lost: DeviceLost,
}

async fn request_adapter(
    instance: &wgpu::Instance,
    power_preference: wgpu::PowerPreference,
    compatible_surface: Option<&wgpu::Surface<'_>>,
    available_adapters: &[wgpu::Adapter],
) -> Result<wgpu::Adapter, WgpuError> {
    profiling::function_scope!();

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface,
            // We don't expose this as an option right now since it's fairly rarely useful:
            // * only has an effect on native
            // * fails if there's no software rasterizer available
            // * can achieve the same with `native_adapter_selector`
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .inspect_err(|_err| {
            if cfg!(target_arch = "wasm32") {
                // Nothing to add here
            } else if available_adapters.is_empty() {
                if std::env::var("DYLD_LIBRARY_PATH").is_ok() {
                    // DYLD_LIBRARY_PATH can sometimes lead to loading dylibs that cause
                    // us to find zero adapters. Very strange.
                    // I don't want to debug this again.
                    // See https://github.com/rerun-io/rerun/issues/11351 for more
                    log::warn!(
                        "No wgpu adapter found. This could be because DYLD_LIBRARY_PATH causes dylibs to be loaded that interfere with Metal device creation. Try restarting with DYLD_LIBRARY_PATH=''"
                    );
                } else {
                    log::info!("No wgpu adapter found");
                }
            } else if available_adapters.len() == 1 {
                log::info!(
                    "The only available wgpu adapter was not suitable: {}",
                    adapter_info_summary(&available_adapters[0].get_info())
                );
            } else {
                log::info!(
                    "No suitable wgpu adapter found out of the {} available ones: {}",
                    available_adapters.len(),
                    describe_adapters(available_adapters)
                );
            }
        })?;

    if 1 < available_adapters.len() {
        log::info!(
            "There are {} available wgpu adapters: {}",
            available_adapters.len(),
            describe_adapters(available_adapters)
        );
    }

    Ok(adapter)
}

impl RenderState {
    /// Creates a new [`RenderState`], containing everything needed for drawing egui with wgpu.
    ///
    /// # Errors
    /// Wgpu initialization may fail due to incompatible hardware or driver for a given config.
    pub async fn create(
        config: &WgpuConfiguration,
        instance: &wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'static>>,
        options: RendererOptions,
    ) -> Result<Self, WgpuError> {
        profiling::scope!("RenderState::create"); // async yield give bad names using `profile_function`

        // This is always an empty list on web.
        #[cfg(not(target_arch = "wasm32"))]
        let available_adapters = {
            let backends = if let WgpuSetup::CreateNew(create_new) = &config.wgpu_setup {
                create_new.instance_descriptor.backends
            } else {
                wgpu::Backends::all()
            };

            startup_step(STEP_ADAPTER, ""); // SolidCraft
            instance.enumerate_adapters(backends).await
        };

        let (instance, adapter, device, queue) = match config.wgpu_setup.clone() {
            WgpuSetup::CreateNew(WgpuSetupCreateNew {
                instance_descriptor: _,
                display_handle: _,
                power_preference,
                native_adapter_selector: _native_adapter_selector,
                device_descriptor,
            }) => {
                let adapter = {
                    #[cfg(target_arch = "wasm32")]
                    {
                        request_adapter(instance, power_preference, compatible_surface, &[]).await
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let selected = _native_adapter_selector
                            .map(|select| select(&available_adapters, compatible_surface))
                            .transpose()
                            .map_err(WgpuError::CustomNativeAdapterSelectionError)?
                            .flatten();
                        if let Some(adapter) = selected {
                            Ok(adapter)
                        } else {
                            request_adapter(
                                instance,
                                power_preference,
                                compatible_surface,
                                &available_adapters,
                            )
                            .await
                        }
                    }
                }?;

                // SolidCraft: name the adapter before the driver is asked for a device.
                let info = adapter.get_info();
                startup_step(
                    STEP_ADAPTER_CHOSEN,
                    &format!(
                        "{:?} {} ({:?}, driver {} {})",
                        info.backend, info.name, info.device_type, info.driver, info.driver_info
                    ),
                );
                startup_step(STEP_DEVICE, "");
                let (device, queue) = {
                    profiling::scope!("request_device");
                    adapter
                        .request_device(&(*device_descriptor)(&adapter))
                        .await?
                };

                (instance.clone(), adapter, device, queue)
            }
            WgpuSetup::Existing(WgpuSetupExisting {
                instance,
                adapter,
                device,
                queue,
            }) => (instance, adapter, device, queue),
        };
        // SolidCraft: a device the driver loses stops the painter, not the app (#4172).
        let device_lost = DeviceLost::watch(&device);

        log_adapter_info(&adapter.get_info());

        let surface_formats = {
            profiling::scope!("get_capabilities");
            compatible_surface.map_or_else(
                || vec![wgpu::TextureFormat::Rgba8Unorm],
                |s| s.get_capabilities(&adapter).formats,
            )
        };
        let target_format = crate::preferred_framebuffer_format(&surface_formats)?;

        // SolidCraft: the OpenGL resolve path can present an entirely black frame with MSAA
        // (notably Mesa/llvmpipe). Decide after selecting the real surface adapter,
        // before creating either pipelines or attachments; no second GPU probe.
        // A sample count the device doesn't support for the target or depth format fails
        // pipeline validation (a panic at start): fall back to the largest supported one.
        let msaa_supported =
            supported_msaa_samples(&adapter, &device, target_format, options.depth_stencil_format);
        let wanted = effective_msaa_samples(adapter.get_info().backend, options.msaa_samples);
        let msaa_samples = clamp_msaa_samples(&msaa_supported, wanted);
        if msaa_samples != wanted {
            log::warn!(
                "MSAA {wanted}x is not supported for {target_format:?} on this device (supported: {msaa_supported:?}); using {msaa_samples}x"
            );
        }
        let options = RendererOptions {
            msaa_samples,
            ..options
        };
        startup_step(STEP_RENDERER, ""); // SolidCraft
        // SolidCraft: capture errors on the real surface/device, including driver shader
        // compilation failures, before wgpu's default uncaptured-error handler can panic.
        // Await every scope (also on WebGPU); an invalid renderer is never handed to eframe.
        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let renderer = Renderer::new(&device, target_format, options);
        let mut failed = None;
        for scope in scopes.into_iter().rev() {
            if let Some(error) = scope.pop().await {
                failed.get_or_insert(error);
            }
        }
        if let Some(error) = failed {
            return Err(WgpuError::CreateRendererError(error));
        }

        // On wasm, depending on feature flags, wgpu objects may or may not implement sync.
        // It doesn't make sense to switch to Rc for that special usecase, so simply disable the lint.
        #[allow(clippy::allow_attributes, clippy::arc_with_non_send_sync)] // For wasm
        Ok(Self {
            instance,
            adapter,
            #[cfg(not(target_arch = "wasm32"))]
            available_adapters,
            device,
            queue,
            target_format,
            msaa_samples: options.msaa_samples,
            msaa_supported,
            renderer: Arc::new(RwLock::new(renderer)),
            surface_config: config.surface,
            device_lost,
        })
    }
}

fn describe_adapters(adapters: &[wgpu::Adapter]) -> String {
    if adapters.is_empty() {
        "(none)".to_owned()
    } else if adapters.len() == 1 {
        adapter_info_summary(&adapters[0].get_info())
    } else {
        adapters
            .iter()
            .map(|a| format!("{{{}}}", adapter_info_summary(&a.get_info())))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Specifies which action should be taken as consequence of a surface error.
pub enum SurfaceErrorAction {
    /// Do nothing and skip the current frame.
    SkipFrame,

    /// Reconfigure the existing surface, then skip the current frame.
    ///
    /// Calls [`wgpu::Surface::configure`] on the current surface object.
    /// Use for [`wgpu::CurrentSurfaceTexture::Outdated`].
    Reconfigure,

    /// Drop the surface, create a new one via [`wgpu::Instance::create_surface`], configure it,
    /// then skip the current frame.
    ///
    /// Use for [`wgpu::CurrentSurfaceTexture::Lost`], where reconfiguring the same surface
    /// object cannot recover.
    RecreateSurface,
}

/// Configuration for using wgpu with eframe or the egui-wgpu winit feature.
#[derive(Clone)]
pub struct WgpuConfiguration {
    /// Runtime-mutable configuration for the surface (present mode, frame latency).
    ///
    /// These are the fields exposed via [`RenderState::surface_config`] for live
    /// reconfiguration at runtime.
    pub surface: SurfaceConfig,

    /// How to create the wgpu adapter & device
    pub wgpu_setup: WgpuSetup,

    /// Callback for surface status changes.
    ///
    /// Called with the [`wgpu::CurrentSurfaceTexture`] result whenever acquiring a frame
    /// does not return [`wgpu::CurrentSurfaceTexture::Success`]. For
    /// [`wgpu::CurrentSurfaceTexture::Suboptimal`], egui uses the frame as-is and
    /// defers surface reconfiguration to the next frame — the callback is not invoked
    /// in that case either.
    pub on_surface_status:
        Arc<dyn Fn(&wgpu::CurrentSurfaceTexture) -> SurfaceErrorAction + Send + Sync>,
}

#[test]
fn wgpu_config_impl_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WgpuConfiguration>();
}

impl core::fmt::Debug for WgpuConfiguration {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self {
            surface,
            wgpu_setup,
            on_surface_status: _,
        } = self;
        f.debug_struct("WgpuConfiguration")
            .field("surface", &surface)
            .field("wgpu_setup", &wgpu_setup)
            .finish_non_exhaustive()
    }
}

impl WgpuConfiguration {
    #[inline]
    pub fn with_surface_config(mut self, surface_config: SurfaceConfig) -> Self {
        self.surface = surface_config;
        self
    }
}

impl Default for WgpuConfiguration {
    fn default() -> Self {
        Self {
            surface: SurfaceConfig::HIGH_THROUGHPUT,

            // No display handle available at this point — callers should replace this with
            // `WgpuSetup::from_display_handle(...)` before creating the instance if one is available.
            wgpu_setup: WgpuSetup::without_display_handle(),
            on_surface_status: Arc::new(|status| match status {
                wgpu::CurrentSurfaceTexture::Outdated => {
                    // The compositor changed the surface (resize, scale, output, …). wgpu
                    // requires us to reconfigure before the next acquire. Skipping would mean
                    // we are stuck in `Outdated` forever.
                    log::trace!("Dropped frame with error: {status:?}");
                    SurfaceErrorAction::Reconfigure
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    // The underlying surface is gone and we need a fresh one from the `wgpu::Instance`.
                    log::debug!("Dropped frame with error: {status:?}");
                    SurfaceErrorAction::RecreateSurface
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    // App is hidden (minimized / behind another window). Skip silently.
                    log::trace!("Skipping frame due to occlusion.");
                    SurfaceErrorAction::SkipFrame
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    // SolidCraft: the acquire's error was captured; most often the surface has
                    // no configuration ("Surface is not configured for presentation", #7007).
                    // Skipping alone would meet the same error on every frame after.
                    log::debug!("Dropped frame with error: {status:?}");
                    SurfaceErrorAction::Reconfigure
                }
                _ => {
                    log::warn!("Dropped frame with error: {status:?}");
                    SurfaceErrorAction::SkipFrame
                }
            }),
        }
    }
}

/// Find the framebuffer format that egui prefers
///
/// # Errors
/// Returns [`WgpuError::NoSurfaceFormatsAvailable`] if the given list of formats is empty.
pub fn preferred_framebuffer_format(
    formats: &[wgpu::TextureFormat],
) -> Result<wgpu::TextureFormat, WgpuError> {
    for &format in formats {
        if matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Ok(format);
        }
    }

    formats
        .first()
        .copied()
        .ok_or(WgpuError::NoSurfaceFormatsAvailable)
}

/// Take's epi's depth/stencil bits and returns the corresponding wgpu format.
pub fn depth_format_from_bits(depth_buffer: u8, stencil_buffer: u8) -> Option<wgpu::TextureFormat> {
    match (depth_buffer, stencil_buffer) {
        (0, 8) => Some(wgpu::TextureFormat::Stencil8),
        (16, 0) => Some(wgpu::TextureFormat::Depth16Unorm),
        (24, 0) => Some(wgpu::TextureFormat::Depth24Plus),
        (24, 8) => Some(wgpu::TextureFormat::Depth24PlusStencil8),
        (32, 0) => Some(wgpu::TextureFormat::Depth32Float),
        (32, 8) => Some(wgpu::TextureFormat::Depth32FloatStencil8),
        _ => None,
    }
}

// ---------------------------------------------------------------------------

fn log_adapter_info(info: &wgpu::AdapterInfo) {
    let summary = adapter_info_summary(info);

    let is_test = cfg!(test); // Software rasterizers are expected (and preferred) during testing!

    if info.device_type == wgpu::DeviceType::Cpu && !is_test {
        log::warn!("Software rasterizer detected - loss of performance expected. {summary}");
    } else {
        log::debug!("wgpu adapter: {summary}");
    }
}

/// A human-readable summary about an adapter
pub fn adapter_info_summary(info: &wgpu::AdapterInfo) -> String {
    let wgpu::AdapterInfo {
        name,
        vendor,
        device,
        device_type,
        driver,
        driver_info,
        backend,
        device_pci_bus_id,
        subgroup_min_size,
        subgroup_max_size,
        transient_saves_memory,
        limit_bucket,
    } = &info;

    // Example values:
    // > name: "llvmpipe (LLVM 16.0.6, 256 bits)", device_type: Cpu, backend: Vulkan, driver: "llvmpipe", driver_info: "Mesa 23.1.6-arch1.4 (LLVM 16.0.6)"
    // > name: "Apple M1 Pro", device_type: IntegratedGpu, backend: Metal, driver: "", driver_info: ""
    // > name: "ANGLE (Apple, Apple M1 Pro, OpenGL 4.1)", device_type: IntegratedGpu, backend: Gl, driver: "", driver_info: ""

    use core::fmt::Write as _;

    let mut summary = format!("backend: {backend:?}, device_type: {device_type:?}");

    if !name.is_empty() {
        write!(summary, ", name: {name:?}").ok();
    }
    if !driver.is_empty() {
        write!(summary, ", driver: {driver:?}").ok();
    }
    if !driver_info.is_empty() {
        write!(summary, ", driver_info: {driver_info:?}").ok();
    }
    if *vendor != 0 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            write!(
                summary,
                ", vendor: {} (0x{vendor:04X})",
                parse_vendor_id(*vendor)
            )
            .ok();
        }
        #[cfg(target_arch = "wasm32")]
        {
            write!(summary, ", vendor: 0x{vendor:04X}").ok();
        }
    }
    if *device != 0 {
        write!(summary, ", device: 0x{device:02X}").ok();
    }
    if !device_pci_bus_id.is_empty() {
        write!(summary, ", pci_bus_id: {device_pci_bus_id:?}").ok();
    }
    if *subgroup_min_size != 0 || *subgroup_max_size != 0 {
        write!(
            summary,
            ", subgroup_size: {subgroup_min_size}..={subgroup_max_size}"
        )
        .ok();
    }
    write!(
        summary,
        ", transient_saves_memory: {transient_saves_memory:?}"
    )
    .ok();
    write!(summary, ", limit_bucket: {limit_bucket:?}").ok();

    summary
}

/// Tries to parse the adapter's vendor ID to a human-readable string.
#[cfg(not(target_arch = "wasm32"))]
pub fn parse_vendor_id(vendor_id: u32) -> &'static str {
    match vendor_id {
        wgpu::hal::auxil::db::amd::VENDOR => "AMD",
        wgpu::hal::auxil::db::apple::VENDOR => "Apple",
        wgpu::hal::auxil::db::arm::VENDOR => "ARM",
        wgpu::hal::auxil::db::broadcom::VENDOR => "Broadcom",
        wgpu::hal::auxil::db::imgtec::VENDOR => "Imagination Technologies",
        wgpu::hal::auxil::db::intel::VENDOR => "Intel",
        wgpu::hal::auxil::db::mesa::VENDOR => "Mesa",
        wgpu::hal::auxil::db::nvidia::VENDOR => "NVIDIA",
        wgpu::hal::auxil::db::qualcomm::VENDOR => "Qualcomm",
        _ => "Unknown",
    }
}

/// Selected-adapter policy shared by egui pipelines, attachments and host callbacks.
pub fn effective_msaa_samples(backend: wgpu::Backend, requested: u32) -> u32 {
    if backend == wgpu::Backend::Gl { 1 } else { requested.max(1) }
}

/// SolidCraft: the MSAA sample counts `device` can render `target_format` and
/// `depth_stencil_format` with (ascending, always starting with 1). Without
/// [`wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] the device allows only the counts
/// WebGPU guarantees for a format (1 and 4), whatever the adapter could do.
pub fn supported_msaa_samples(
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    depth_stencil_format: Option<wgpu::TextureFormat>,
) -> Vec<u32> {
    let features = device.features();
    let flags = |format: wgpu::TextureFormat| {
        if features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
            adapter.get_texture_format_features(format).flags
        } else {
            format.guaranteed_format_features(features).flags
        }
    };
    let target = flags(target_format);
    let depth = depth_stencil_format.map(flags);
    msaa_counts(target, depth)
}

/// The sample counts both the colour target and the depth buffer support. A multisampled colour
/// target must also resolve into the single-sampled surface.
fn msaa_counts(
    target: wgpu::TextureFormatFeatureFlags,
    depth: Option<wgpu::TextureFormatFeatureFlags>,
) -> Vec<u32> {
    let resolves = target.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE);
    [1, 2, 4, 8, 16]
        .into_iter()
        .filter(|&n| {
            n == 1
                || (resolves
                    && target.sample_count_supported(n)
                    && depth.is_none_or(|d| d.sample_count_supported(n)))
        })
        .collect()
}

/// SolidCraft: the largest of the `supported` sample counts that is at most `requested` (1 when
/// none is).
pub fn clamp_msaa_samples(supported: &[u32], requested: u32) -> u32 {
    supported
        .iter()
        .copied()
        .filter(|&n| n <= requested.max(1))
        .max()
        .unwrap_or(1)
}

#[cfg(test)]
mod msaa_tests {
    #[test]
    fn gl_saved_multisampling_is_clamped_and_native_backends_keep_it() {
        for requested in [0, 1, 2, 4, 8] {
            assert_eq!(super::effective_msaa_samples(wgpu::Backend::Gl, requested), 1);
            for backend in [wgpu::Backend::Vulkan, wgpu::Backend::Dx12, wgpu::Backend::Metal, wgpu::Backend::BrowserWebGpu] {
                assert_eq!(super::effective_msaa_samples(backend, requested), requested.max(1));
            }
        }
    }

    #[test]
    fn unsupported_sample_counts_fall_back_to_the_largest_supported_one() {
        use wgpu::TextureFormatFeatureFlags as F;
        // WebGPU's guarantee for Bgra8Unorm without adapter-specific features: 1 and 4 (a
        // saved 8x failed pipeline validation at start).
        let guaranteed = F::MULTISAMPLE_X4 | F::MULTISAMPLE_RESOLVE;
        let supported = super::msaa_counts(guaranteed, Some(F::MULTISAMPLE_X4));
        assert_eq!(supported, [1, 4]);
        assert_eq!(super::clamp_msaa_samples(&supported, 8), 4);
        assert_eq!(super::clamp_msaa_samples(&supported, 2), 1);
        assert_eq!(super::clamp_msaa_samples(&supported, 0), 1);
        // The depth buffer limits it too, and a target that can't resolve can't multisample.
        let adapter = F::MULTISAMPLE_X2 | F::MULTISAMPLE_X4 | F::MULTISAMPLE_X8 | F::MULTISAMPLE_RESOLVE;
        assert_eq!(super::msaa_counts(adapter, None), [1, 2, 4, 8]);
        assert_eq!(super::msaa_counts(adapter, Some(F::MULTISAMPLE_X4)), [1, 4]);
        assert_eq!(super::msaa_counts(F::MULTISAMPLE_X4, None), [1]);
        assert_eq!(super::clamp_msaa_samples(&[], 8), 1);
    }
}

/// SolidCraft: run `f` with `device`'s errors captured, and the first error it caused. Uncaptured,
/// an error goes to the device's handler, which panics by default: a surface the driver can't
/// reconfigure (D3D12 "Failed to wait for GPU to come idle", #1935) crashed the app. Native error
/// scopes resolve at once; on the web the scope still keeps the error from the handler.
pub fn capture_errors<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> (T, Option<wgpu::Error>) {
    let scopes = [
        wgpu::ErrorFilter::OutOfMemory,
        wgpu::ErrorFilter::Internal,
        wgpu::ErrorFilter::Validation,
    ]
    .map(|filter| device.push_error_scope(filter));
    let value = f();
    let mut failed = None;
    for scope in scopes.into_iter().rev() {
        let mut popped = std::pin::pin!(scope.pop());
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        if let std::task::Poll::Ready(Some(error)) =
            std::future::Future::poll(popped.as_mut(), &mut cx)
        {
            failed.get_or_insert(error);
        }
    }
    (value, failed)
}

/// SolidCraft: why a surface configure ([`configure_captured`]) left the surface without its
/// configuration.
#[derive(Debug)]
pub enum ConfigureError {
    /// The device refused it, with this error (#1935).
    Refused(wgpu::Error),
    /// The device was lost (#7007).
    DeviceLost,
    /// The adapter doesn't support the surface.
    Unsupported,
}

impl std::fmt::Display for ConfigureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(error) => error.fmt(f),
            Self::DeviceLost => f.write_str("the graphics device was lost"),
            Self::Unsupported => f.write_str("the surface isn't supported by this adapter"),
        }
    }
}

/// SolidCraft: run a surface configure on `device` under [`capture_errors`], and say whether the
/// surface has its configuration. A configure that meets a lost device reports nothing: wgpu
/// passes device-loss errors to no error scope and no handler, only to the lost callback. On
/// D3D12 a driver reset is found that way, when the configure waits for the present queue, after
/// wgpu has dropped the surface's old configuration. Taken as configured, the surface's next
/// acquire failed with "Surface is not configured for presentation", and the device's default
/// error handler panicked (#7007). `lost` is the device's [`DeviceLost`], marked by then.
pub fn configure_captured(
    device: &wgpu::Device,
    lost: &DeviceLost,
    configure: impl FnOnce(),
) -> Result<(), ConfigureError> {
    let ((), failed) = capture_errors(device, configure);
    if let Some(error) = failed {
        return Err(ConfigureError::Refused(error));
    }
    if lost.is_lost() {
        return Err(ConfigureError::DeviceLost);
    }
    Ok(())
}

/// SolidCraft: refused configures in a row after which the surface is recreated (#1935).
pub const SURFACE_RECREATE_AFTER: u32 = 3;

/// SolidCraft: a surface's streak of refused configures (#1935) and when the next try is due.
/// A refusal waits 16 ms, doubling with each refusal in a row up to 1 s, and every
/// [`SURFACE_RECREATE_AFTER`] refusals the surface is recreated before the next try. Until the
/// try is due, no configure or recreate happens however often the window repaints, so a device
/// that keeps refusing costs a try a second instead of one a frame. A configure the device takes
/// ends the streak, a recreate still pending included. The clock is passed in, for tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigureRetry {
    failures: u32,
    due: Option<web_time::Instant>,
    /// The streak asked for a recreate that hasn't happened yet.
    recreate: bool,
}

/// SolidCraft: what follows a refused configure ([`ConfigureRetry::refused`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPlan {
    /// Refusals in a row, this one included.
    pub failures: u32,
    /// The first refusal of the streak: the one worth a warning.
    pub first: bool,
    /// How long until the next try.
    pub delay: std::time::Duration,
    /// This refusal asked for the surface to be recreated before the next try.
    pub recreate: bool,
}

impl ConfigureRetry {
    /// Refusals in a row so far.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// How long until the next try is due at `now`; `None` when a configure may go ahead (no
    /// refusal pending, or its delay has passed).
    pub fn wait(&self, now: web_time::Instant) -> Option<std::time::Duration> {
        self.due
            .and_then(|due| due.checked_duration_since(now))
            .filter(|left| !left.is_zero())
    }

    /// Whether a configure (or recreate) may go ahead at `now`.
    pub fn is_due(&self, now: web_time::Instant) -> bool {
        self.wait(now).is_none()
    }

    /// Whether the streak asked for a recreate that hasn't happened yet.
    pub fn recreate_pending(&self) -> bool {
        self.recreate
    }

    /// Whether the surface should be recreated at `now`: a recreate is pending and its try is due.
    pub fn recreate_due(&self, now: web_time::Instant) -> bool {
        self.recreate && self.is_due(now)
    }

    /// The surface was recreated: no recreate is pending. The streak and its deadline stay.
    pub fn recreated(&mut self) {
        self.recreate = false;
    }

    /// The device refused a configure at `now`: the streak grows and the next try waits.
    pub fn refused(&mut self, now: web_time::Instant) -> RetryPlan {
        self.failures = self.failures.saturating_add(1);
        let doublings = self.failures.saturating_sub(1).min(6);
        let delay = std::time::Duration::from_millis((16u64 << doublings).min(1000));
        // A deadline past the clock's range can't happen in practice; it would make the try due.
        self.due = now.checked_add(delay);
        let recreate = self.failures % SURFACE_RECREATE_AFTER == 0;
        self.recreate |= recreate;
        RetryPlan {
            failures: self.failures,
            first: self.failures == 1,
            delay,
            recreate,
        }
    }

    /// The device took a configure: the streak ends, and a pending recreate with it. Returns how
    /// many refusals it had.
    pub fn took(&mut self) -> u32 {
        std::mem::take(self).failures
    }

    /// Runs `configure` at `now` unless a refused one's retry isn't due yet, and keeps the
    /// streak. The surface's configures all go through here, so an early repaint (input, an
    /// animation) can't bring a retry forward.
    pub fn attempt<E>(
        &mut self,
        now: web_time::Instant,
        configure: impl FnOnce() -> Result<(), E>,
    ) -> Attempt<E> {
        if let Some(left) = self.wait(now) {
            return Attempt::Waiting(left);
        }
        match configure() {
            Ok(()) => Attempt::Took(self.took()),
            Err(error) => Attempt::Refused(self.refused(now), error),
        }
    }
}

/// SolidCraft: what came of [`ConfigureRetry::attempt`].
#[derive(Debug)]
pub enum Attempt<E> {
    /// Not tried: the retry after a refusal is due in this long.
    Waiting(std::time::Duration),
    /// The configure was taken, after this many refusals in a row (0 when none).
    Took(u32),
    /// The configure was refused with this error; the plan says what follows.
    Refused(RetryPlan, E),
}

/// SolidCraft: the present mode a viewport's surface is configured with. The main window keeps
/// `configured`. Another window (the agent chat popped out into its own window) is an immediate
/// viewport: eframe draws and presents it inside the main window's frame, one surface after the
/// other. With vsync on both, every frame waited for the display twice, and on a compositor that
/// sends no frame callbacks to a window it isn't showing (Wayland: on another workspace, scrolled
/// away, covered) the main window stalled behind the other one's present. So another window
/// presents without waiting where its surface can (Mailbox, which doesn't tear, before
/// Immediate); the main window's vsync still paces both. A `configured` mode without vsync is
/// kept everywhere.
pub fn viewport_present_mode(
    root: bool,
    configured: wgpu::PresentMode,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    use wgpu::PresentMode;
    let waits = matches!(
        configured,
        PresentMode::AutoVsync | PresentMode::Fifo | PresentMode::FifoRelaxed
    );
    if root || !waits {
        return configured;
    }
    [PresentMode::Mailbox, PresentMode::Immediate]
        .into_iter()
        .find(|mode| supported.contains(mode))
        .unwrap_or(configured)
}

/// SolidCraft: the surface size for a window of `width` x `height` pixels on a device whose
/// textures are at most `max_side` on a side, and the scale it is drawn at. A window that fits is
/// unchanged (scale 1); a larger one shrinks uniformly, keeping its aspect ratio, so the image
/// the compositor stretches back over the window is not distorted. Never zero on a side.
pub fn clamp_surface_extent(width: u32, height: u32, max_side: u32) -> (u32, u32, f32) {
    let max_side = max_side.max(1);
    let (width, height) = (width.max(1), height.max(1));
    let longest = width.max(height);
    if longest <= max_side {
        return (width, height, 1.0);
    }
    let scale = f64::from(max_side) / f64::from(longest);
    let fit = |side: u32| -> u32 {
        let scaled = (f64::from(side) * scale).floor() as u32;
        scaled.clamp(1, max_side)
    };
    let scale_f32 = scale as f32;
    (fit(width), fit(height), scale_f32)
}

#[cfg(test)]
mod device_lost_tests {
    /// A loss is marked once, with the first reason, and every clone (and the shared flag) sees it.
    #[test]
    fn a_marked_loss_is_seen_by_every_clone() {
        let lost = super::DeviceLost::default();
        let painter = lost.clone();
        let flag = lost.flag();
        assert!(!painter.is_lost() && lost.reason().is_none());
        lost.mark("Unknown: Device lost");
        lost.mark("Destroyed: ");
        assert!(painter.is_lost());
        assert!(flag.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(painter.reason().as_deref(), Some("Unknown: Device lost"));
    }

    /// A window's surface as wgpu-core treats the image of an acquired frame: a present or a
    /// discard gives it back only on a valid device, a destroy of the texture always (#7148).
    #[derive(Default)]
    struct Surface {
        lost: std::cell::Cell<bool>,
        image_held: std::cell::Cell<bool>,
        calls: std::cell::RefCell<Vec<&'static str>>,
    }

    impl Surface {
        fn call(&self, name: &'static str, gives_back: bool) {
            self.calls.borrow_mut().push(name);
            if gives_back {
                self.image_held.set(false);
            }
        }
    }

    /// A frame of [`Surface`]: discarded when dropped unpresented.
    struct Frame<'s> {
        surface: &'s Surface,
        presented: bool,
    }

    impl Drop for Frame<'_> {
        fn drop(&mut self) {
            if !self.presented {
                self.surface.call("discard", !self.surface.lost.get());
            }
        }
    }

    struct Texture<'s>(&'s Surface);

    impl super::FreeSurfaceTexture for Texture<'_> {
        fn free(&self) {
            self.0.call("free", true);
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum End {
        Present,
        Drop,
        Panic,
    }

    /// Acquire a frame from `surface` and end it as `end`.
    fn run_frame(surface: &Surface, end: End) {
        surface.image_held.set(true);
        let frame = super::AcquiredFrame::new(
            Frame {
                surface,
                presented: false,
            },
            Texture(surface),
        );
        match end {
            End::Present => frame.present(|mut frame| {
                frame.presented = true;
                surface.call("present", !surface.lost.get());
            }),
            End::Drop => drop(frame),
            End::Panic => {
                let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let _frame = frame;
                    panic!("a paint callback panics while the frame is held");
                }));
                assert!(unwound.is_err());
            }
        }
    }

    /// However a frame ends, on a valid or a lost device, the surface has its image back by the
    /// time the frame is gone, so dropping or reconfiguring the surface can't meet a semaphore
    /// still in use (#7148). The texture is freed only after the present or discard: freed first,
    /// a valid device's present or discard would find it destroyed.
    #[test]
    fn a_frame_gives_its_image_back_however_it_ends_even_on_a_lost_device() {
        for lost in [false, true] {
            for end in [End::Present, End::Drop, End::Panic] {
                let surface = Surface::default();
                surface.lost.set(lost);
                run_frame(&surface, end);
                let first = if matches!(end, End::Present) {
                    "present"
                } else {
                    "discard"
                };
                assert_eq!(
                    *surface.calls.borrow(),
                    [first, "free"],
                    "lost {lost}, {end:?}"
                );
                assert!(!surface.image_held.get(), "lost {lost}, {end:?}");
            }
        }
    }
}
