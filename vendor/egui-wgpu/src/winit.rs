#![expect(clippy::missing_errors_doc)]
#![expect(clippy::undocumented_unsafe_blocks)]
#![expect(clippy::unwrap_used)] // TODO(emilk): avoid unwraps
#![expect(unsafe_code)]

use crate::{RenderState, SurfaceConfig, SurfaceErrorAction, WgpuConfiguration, renderer};
use crate::{
    RendererOptions,
    capture::{CaptureReceiver, CaptureSender, CaptureState, capture_channel},
};
use core::num::NonZeroU32;
use egui::{Context, Event, UserData, ViewportId, ViewportIdMap, ViewportIdSet};
use std::sync::Arc;

struct SurfaceState {
    surface: wgpu::Surface<'static>,
    alpha_mode: wgpu::CompositeAlphaMode,
    width: u32,
    height: u32,
    /// SolidCraft: the window's size in pixels, which `width`/`height` may be clamped down from.
    requested: (u32, u32),
    /// SolidCraft: `width / requested.0` (1 unless clamped); egui is drawn at this scale so the
    /// whole window still fits in the smaller surface.
    scale: f32,
    resizing: bool,
    needs_reconfigure: bool,
    needs_recreate: bool,
    /// SolidCraft: configures the device refused in a row and when the next try is due (#1935),
    /// kept across a recreate.
    retry: crate::ConfigureRetry,
}

/// SolidCraft: whether a frame was presented yet (`crate::STEP_FIRST_PRESENT`).
static FIRST_PRESENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Everything you need to paint egui with [`wgpu`] on [`winit`].
///
/// Alternatively you can use [`crate::Renderer`] directly.
///
/// NOTE: all egui viewports share the same painter.
pub struct Painter {
    context: Context,
    config: WgpuConfiguration,
    options: RendererOptions,
    support_transparent_backbuffer: bool,
    screen_capture_state: Option<CaptureState>,

    instance: wgpu::Instance,
    render_state: Option<RenderState>,

    // Per viewport/window:
    depth_texture_view: ViewportIdMap<wgpu::TextureView>,
    msaa_texture_view: ViewportIdMap<wgpu::TextureView>,
    surfaces: ViewportIdMap<SurfaceState>,
    capture_tx: CaptureSender,
    capture_rx: CaptureReceiver,
}

impl Painter {
    /// Manages [`wgpu`] state, including surface state, required to render egui.
    ///
    /// Only the [`wgpu::Instance`] is initialized here. Device selection and the initialization
    /// of render + surface state is deferred until the painter is given its first window target
    /// via [`set_window()`](Self::set_window). (Ensuring that a device that's compatible with the
    /// native window is chosen)
    ///
    /// Before calling [`paint_and_update_textures()`](Self::paint_and_update_textures) a
    /// [`wgpu::Surface`] must be initialized (and corresponding render state) by calling
    /// [`set_window()`](Self::set_window) once you have
    /// a [`winit::window::Window`] with a valid `.raw_window_handle()`
    /// associated.
    pub async fn new(
        context: Context,
        config: WgpuConfiguration,
        support_transparent_backbuffer: bool,
        options: RendererOptions,
    ) -> Self {
        let (capture_tx, capture_rx) = capture_channel();
        crate::startup_step(crate::STEP_INSTANCE, ""); // SolidCraft
        let instance = config.wgpu_setup.new_instance().await;

        Self {
            context,
            config,
            options,
            support_transparent_backbuffer,
            screen_capture_state: None,

            instance,
            render_state: None,

            depth_texture_view: Default::default(),
            surfaces: Default::default(),
            msaa_texture_view: Default::default(),

            capture_tx,
            capture_rx,
        }
    }

    /// Get the [`RenderState`].
    ///
    /// Will return [`None`] if the render state has not been initialized yet.
    pub fn render_state(&self) -> Option<RenderState> {
        self.render_state.clone()
    }

    /// SolidCraft: a configuration the device refuses (D3D12: "Failed to wait for GPU to come
    /// idle before reconfiguring the Surface", #1935) comes back as the error instead of reaching
    /// the device's uncaptured-error handler, which panics; so does a configure that met a lost
    /// device, or a surface the adapter doesn't support, neither of which configured it (#7007).
    /// Called only through [`Self::try_configure`], which says what follows.
    fn configure_surface(
        viewport_id: ViewportId,
        surface_state: &SurfaceState,
        render_state: &RenderState,
        config: &SurfaceConfig,
    ) -> Result<(), crate::ConfigureError> {
        profiling::function_scope!();

        let SurfaceConfig {
            present_mode,
            desired_maximum_frame_latency,
        } = *config;

        // Transaction presentation can hold a drawable during AppKit live resize. Keep the
        // configured low-latency path normally, but use three Metal drawables while resizing.
        #[cfg(all(target_os = "macos", feature = "macos-window-resize-jitter-fix"))]
        let desired_maximum_frame_latency = if surface_state.resizing {
            Some(desired_maximum_frame_latency.unwrap_or(2).max(2))
        } else {
            desired_maximum_frame_latency
        };

        let width = surface_state.width;
        let height = surface_state.height;

        // SolidCraft: an unsupported surface is a refused configure, not a panic.
        let Some(default_config) = surface_state
            .surface
            .get_default_config(&render_state.adapter, width, height)
        else {
            return Err(crate::ConfigureError::Unsupported);
        };
        // SolidCraft: a window other than the main one presents without waiting for the display
        // where its surface can (`crate::viewport_present_mode`).
        let supported = surface_state
            .surface
            .get_capabilities(&render_state.adapter)
            .present_modes;
        let present_mode = crate::viewport_present_mode(
            viewport_id == ViewportId::ROOT,
            present_mode,
            &supported,
        );
        let mut surf_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: render_state.target_format,
            present_mode,
            alpha_mode: surface_state.alpha_mode,
            view_formats: vec![render_state.target_format],
            ..default_config
        };

        if let Some(desired_maximum_frame_latency) = desired_maximum_frame_latency {
            surf_config.desired_maximum_frame_latency = desired_maximum_frame_latency;
        }

        let device = &render_state.device;
        crate::configure_captured(device, &render_state.device_lost, || {
            surface_state.surface.configure(device, &surf_config);
        })
    }

    /// SolidCraft: configures the surface at `now` unless a refused configure's retry isn't due
    /// yet (#1935, [`crate::ConfigureRetry::attempt`]), and says whether the surface has its
    /// configuration. A refusal is a warning once per streak (debug after that); it marks the
    /// surface to be configured again (every few refusals in a row the retry state also asks for
    /// a recreate first) and asks for a repaint when the retry is due. A try that isn't due
    /// configures nothing and asks for that repaint again, since an earlier repaint may have used
    /// up the one asked for before.
    fn try_configure(
        context: &Context,
        viewport_id: ViewportId,
        state: &mut SurfaceState,
        render_state: &RenderState,
        config: &SurfaceConfig,
        now: web_time::Instant,
    ) -> bool {
        // A copy, as the configure borrows the whole surface state.
        let mut retry = state.retry;
        let attempt = retry.attempt(now, || {
            Self::configure_surface(viewport_id, state, render_state, config)
        });
        state.retry = retry;
        match attempt {
            crate::Attempt::Took(refusals) => {
                if refusals > 0 {
                    log::info!("The surface took its configuration after {refusals} refusals");
                }
                state.needs_reconfigure = false;
                true
            }
            crate::Attempt::Waiting(left) => {
                state.needs_reconfigure = true;
                context.request_repaint_after_for(left, viewport_id);
                false
            }
            crate::Attempt::Refused(plan, error) => {
                if plan.first {
                    log::warn!(
                        "The {}x{} surface could not be configured; trying again: {error}",
                        state.width,
                        state.height
                    );
                } else {
                    log::debug!(
                        "The surface refused its configuration again ({} in a row): {error}",
                        plan.failures
                    );
                }
                state.needs_reconfigure = true;
                context.request_repaint_after_for(plan.delay, viewport_id);
                false
            }
        }
    }

    /// Drop the existing [`wgpu::Surface`] for `viewport_id` and create a fresh one for the
    /// given window via [`wgpu::Instance::create_surface`], then configure it.
    ///
    /// Used to recover from [`wgpu::CurrentSurfaceTexture::Lost`], where reconfiguring the
    /// existing surface object cannot recover.
    fn recreate_surface(
        &mut self,
        viewport_id: ViewportId,
        window: &Arc<winit::window::Window>,
    ) -> Result<(), crate::WgpuError> {
        profiling::function_scope!();

        let Some(old_state) = self.surfaces.remove(&viewport_id) else {
            return Ok(());
        };

        let surface = self.instance.create_surface(Arc::clone(window))?;
        // SolidCraft: the refusal streak and its deadline go on with the new surface (#1935); a
        // recreate the streak asked for is done by this one.
        let mut retry = old_state.retry;
        retry.recreated();
        self.install_surface(
            surface,
            viewport_id,
            old_state.requested.0,
            old_state.requested.1,
            old_state.resizing,
            retry,
        );
        Ok(())
    }

    /// Updates (or clears) the [`winit::window::Window`] associated with the [`Painter`]
    ///
    /// This creates a [`wgpu::Surface`] for the given Window (as well as initializing render
    /// state if needed) that is used for egui rendering.
    ///
    /// This must be called before trying to render via
    /// [`paint_and_update_textures`](Self::paint_and_update_textures)
    ///
    /// # Portability
    ///
    /// _In particular it's important to note that on Android a it's only possible to create
    /// a window surface between `Resumed` and `Paused` lifecycle events, and Winit will panic on
    /// attempts to query the raw window handle while paused._
    ///
    /// On Android [`set_window`](Self::set_window) should be called with `Some(window)` for each
    /// `Resumed` event and `None` for each `Paused` event. Currently, on all other platforms
    /// [`set_window`](Self::set_window) may be called with `Some(window)` as soon as you have a
    /// valid [`winit::window::Window`].
    ///
    /// # Errors
    /// If the provided wgpu configuration does not match an available device.
    pub async fn set_window(
        &mut self,
        viewport_id: ViewportId,
        window: Option<Arc<winit::window::Window>>,
    ) -> Result<(), crate::WgpuError> {
        profiling::scope!("Painter::set_window"); // profile_function gives bad names for async functions

        if let Some(window) = window {
            let size = window.inner_size();
            if !self.surfaces.contains_key(&viewport_id) {
                crate::startup_step(crate::STEP_SURFACE, ""); // SolidCraft
                let surface = self.instance.create_surface(window)?;
                self.add_surface(surface, viewport_id, size).await?;
            }
        } else {
            log::warn!("No window - clearing all surfaces");
            self.surfaces.clear();
        }
        Ok(())
    }

    /// Updates (or clears) the [`winit::window::Window`] associated with the [`Painter`] without taking ownership of the window.
    ///
    /// Like [`set_window`](Self::set_window) except:
    ///
    /// # Safety
    /// The user is responsible for ensuring that the window is alive for as long as it is set.
    pub async unsafe fn set_window_unsafe(
        &mut self,
        viewport_id: ViewportId,
        window: Option<&winit::window::Window>,
    ) -> Result<(), crate::WgpuError> {
        profiling::scope!("Painter::set_window_unsafe"); // profile_function gives bad names for async functions

        if let Some(window) = window {
            let size = window.inner_size();
            if !self.surfaces.contains_key(&viewport_id) {
                let surface = unsafe {
                    self.instance
                        .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)?
                };
                self.add_surface(surface, viewport_id, size).await?;
            }
        } else {
            log::warn!("No window - clearing all surfaces");
            self.surfaces.clear();
        }
        Ok(())
    }

    async fn add_surface(
        &mut self,
        surface: wgpu::Surface<'static>,
        viewport_id: ViewportId,
        size: winit::dpi::PhysicalSize<u32>,
    ) -> Result<(), crate::WgpuError> {
        if self.render_state.is_none() {
            let render_state =
                RenderState::create(&self.config, &self.instance, Some(&surface), self.options)
                    .await?;
            // SolidCraft: attachment sample counts must match the backend-adjusted pipelines.
            self.options.msaa_samples = render_state.msaa_samples;
            self.render_state = Some(render_state);
        }
        crate::startup_step(crate::STEP_SURFACE_CONFIGURE, ""); // SolidCraft
        self.install_surface(
            surface,
            viewport_id,
            size.width,
            size.height,
            false,
            crate::ConfigureRetry::default(),
        );
        Ok(())
    }

    /// Inserts a freshly created surface into [`Self::surfaces`] and configures it.
    ///
    /// Render state must already be initialised before calling this.
    // NOTE: The same assumption is already required by `resize_and_generate_depth_texture_view_and_msaa_view`.
    fn install_surface(
        &mut self,
        surface: wgpu::Surface<'static>,
        viewport_id: ViewportId,
        width: u32,
        height: u32,
        resizing: bool,
        retry: crate::ConfigureRetry,
    ) {
        let alpha_mode = {
            // Panic: We use the same failure mode as `resize_and_generate_depth_texture_view_and_msaa_view`
            let render_state = self
                .render_state
                .as_ref()
                .expect("install_surface called before render_state initialization");
            if self.support_transparent_backbuffer {
                let supported_alpha_modes =
                    surface.get_capabilities(&render_state.adapter).alpha_modes;
                // Prefer pre multiplied over post multiplied!
                if supported_alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                    wgpu::CompositeAlphaMode::PreMultiplied
                } else if supported_alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied)
                {
                    wgpu::CompositeAlphaMode::PostMultiplied
                } else {
                    log::warn!(
                        "Transparent window was requested, but the active wgpu surface does not support a `CompositeAlphaMode` with transparency."
                    );
                    wgpu::CompositeAlphaMode::Auto
                }
            } else {
                wgpu::CompositeAlphaMode::Auto
            }
        };
        self.surfaces.insert(
            viewport_id,
            SurfaceState {
                surface,
                width,
                height,
                requested: (width, height),
                scale: 1.0,
                alpha_mode,
                resizing,
                needs_reconfigure: false,
                needs_recreate: false,
                retry,
            },
        );
        let Some(width) = NonZeroU32::new(width) else {
            log::debug!("The window width was zero; skipping generate textures");
            return;
        };
        let Some(height) = NonZeroU32::new(height) else {
            log::debug!("The window height was zero; skipping generate textures");
            return;
        };
        self.resize_and_generate_depth_texture_view_and_msaa_view(viewport_id, width, height);
    }

    /// Returns the maximum texture dimension supported if known
    ///
    /// This API will only return a known dimension after `set_window()` has been called
    /// at least once, since the underlying device and render state are initialized lazily
    /// once we have a window (that may determine the choice of adapter/device).
    pub fn max_texture_side(&self) -> Option<usize> {
        self.render_state
            .as_ref()
            .map(|rs| rs.device.limits().max_texture_dimension_2d as usize)
    }

    fn resize_and_generate_depth_texture_view_and_msaa_view(
        &mut self,
        viewport_id: ViewportId,
        width_in_pixels: NonZeroU32,
        height_in_pixels: NonZeroU32,
    ) {
        profiling::function_scope!();

        // SolidCraft: no unwraps, and a window larger than the device's largest texture (a
        // transient size while a window moves between monitors of different scale, or a window
        // spanning several monitors) gets a surface clamped to it instead of a validation panic.
        let (Some(render_state), Some(surface_state)) = (
            self.render_state.as_ref(),
            self.surfaces.get_mut(&viewport_id),
        ) else {
            log::warn!("Resize before the surface and render state exist; ignored");
            return;
        };

        let max_side = render_state.device.limits().max_texture_dimension_2d;
        let requested = (width_in_pixels.get(), height_in_pixels.get());
        let (width, height, scale) = crate::clamp_surface_extent(requested.0, requested.1, max_side);
        if scale < 1.0 {
            log::warn!(
                "Window of {}x{} pixels is larger than the largest texture ({max_side}); drawing at {width}x{height}",
                requested.0,
                requested.1
            );
        }

        surface_state.width = width;
        surface_state.height = height;
        surface_state.requested = requested;
        surface_state.scale = scale;

        // SolidCraft: while a refused configure waits for its retry (#1935), a resize doesn't
        // try early, since the device refuses, not the size: the new size is kept above and the
        // retry configures it.
        Self::try_configure(
            &self.context,
            viewport_id,
            surface_state,
            render_state,
            &self.config.surface,
            web_time::Instant::now(),
        );

        if let Some(depth_format) = self.options.depth_stencil_format {
            self.depth_texture_view.insert(
                viewport_id,
                render_state
                    .device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("egui_depth_texture"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: self.options.msaa_samples.max(1),
                        dimension: wgpu::TextureDimension::D2,
                        format: depth_format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[depth_format],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            );
        }

        if let Some(render_state) = (self.options.msaa_samples > 1)
            .then_some(self.render_state.as_ref())
            .flatten()
        {
            let texture_format = render_state.target_format;
            self.msaa_texture_view.insert(
                viewport_id,
                render_state
                    .device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("egui_msaa_texture"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: self.options.msaa_samples.max(1),
                        dimension: wgpu::TextureDimension::D2,
                        format: texture_format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[texture_format],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            );
        }
    }

    /// Handles changes of the resizing state.
    ///
    /// Should be called prior to the first [`Painter::on_window_resized`] call and after the last in
    /// the chain. Used to apply platform-specific logic, e.g. OSX Metal window resize jitter fix.
    pub fn on_window_resize_state_change(&mut self, viewport_id: ViewportId, resizing: bool) {
        profiling::function_scope!();

        let Some(state) = self.surfaces.get_mut(&viewport_id) else {
            return;
        };
        if state.resizing == resizing {
            if resizing {
                log::debug!(
                    "Painter::on_window_resize_state_change() redundant call while resizing"
                );
            } else {
                log::debug!(
                    "Painter::on_window_resize_state_change() redundant call after resizing"
                );
            }
            return;
        }

        // Set before reconfiguring so macOS live resize uses the temporary latency bump above.
        state.resizing = resizing;

        // Resizing is a bit tricky on macOS.
        // It requires enabling ["present_with_transaction"](https://developer.apple.com/documentation/quartzcore/cametallayer/presentswithtransaction)
        // flag to avoid jittering during the resize. Even though resize jittering on macOS
        // is common across rendering backends, the solution for wgpu/metal is known.
        //
        // See https://github.com/emilk/egui/issues/903
        #[cfg(all(target_os = "macos", feature = "macos-window-resize-jitter-fix"))]
        {
            // SAFETY: `as_hal::<Metal>()` returns `None` unless this surface is backed by wgpu's
            // Metal backend.
            unsafe {
                if let (Some(render_state), Some(hal_surface)) = (
                    self.render_state.as_ref(),
                    state.surface.as_hal::<wgpu::hal::api::Metal>(),
                ) {
                    hal_surface
                        .render_layer()
                        .lock()
                        .setPresentsWithTransaction(resizing);

                    // SolidCraft: not before a refused configure's retry is due (#1935).
                    Self::try_configure(
                        &self.context,
                        viewport_id,
                        state,
                        render_state,
                        &self.config.surface,
                        web_time::Instant::now(),
                    );
                }
            }
        }
    }

    pub fn on_window_resized(
        &mut self,
        viewport_id: ViewportId,
        width_in_pixels: NonZeroU32,
        height_in_pixels: NonZeroU32,
    ) {
        profiling::function_scope!();

        if self.surfaces.contains_key(&viewport_id) {
            self.resize_and_generate_depth_texture_view_and_msaa_view(
                viewport_id,
                width_in_pixels,
                height_in_pixels,
            );
        } else {
            log::warn!(
                "Ignoring window resize notification with no surface created via Painter::set_window()"
            );
        }
    }

    /// Returns two things:
    ///
    /// The approximate number of seconds spent on vsync-waiting (if any),
    /// and the captures captured screenshot if it was requested.
    ///
    /// If `capture_data` isn't empty, a screenshot will be captured.
    #[expect(clippy::too_many_arguments)]
    pub fn paint_and_update_textures(
        &mut self,
        viewport_id: ViewportId,
        pixels_per_point: f32,
        clear_color: [f32; 4],
        clipped_primitives: &[epaint::ClippedPrimitive],
        textures_delta: &mut epaint::textures::TexturesDelta,
        capture_data: Vec<UserData>,
        window: &Arc<winit::window::Window>,
    ) -> f32 {
        profiling::function_scope!();

        /// Guard to ensure that commands are always submitted to the renderer queue
        /// so that calls to [`write_buffer()`](https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.write_buffer)
        /// are completed even if we take a codepath which doesn't submit commands and avoids
        /// internal buffers growing indefinitely.
        ///
        /// This may happen, for example, if no output frame is resolved.
        /// See <https://github.com/emilk/egui/pull/7928> for full context.
        struct RendererQueueGuard<'q> {
            queue: &'q wgpu::Queue,
            commands_submitted: bool,
        }

        impl Drop for RendererQueueGuard<'_> {
            fn drop(&mut self) {
                // Only submit an empty command buffer array if no commands were
                // explicitly submitted.
                if !self.commands_submitted {
                    self.queue.submit([]);
                }
            }
        }

        let capture = !capture_data.is_empty();
        let mut vsync_sec = 0.0;
        // SolidCraft: one clock reading for this frame's surface retries (#1935).
        let now = web_time::Instant::now();

        // SolidCraft: a lost device draws nothing more; the app sees `RenderState::device_lost`
        // and restarts (#4172).
        if self
            .render_state
            .as_ref()
            .is_some_and(|rs| rs.device_lost.is_lost())
        {
            if let Some(rs) = &self.render_state
                && let Some(effect) = &rs.renderer.read().frame_compositor
            {
                effect.suspend();
            }
            // The app's frame (not this paint) acts on it: keep its frames coming.
            self.context
                .request_repaint_after_for(std::time::Duration::from_millis(250), viewport_id);
            return vsync_sec;
        }

        // If the previous frame produced `CurrentSurfaceTexture::Lost`, the action match
        // below set `needs_recreate`. Recreate the surface now, before re-borrowing
        // `self.render_state` / `self.surfaces` for the rest of the paint.
        // SolidCraft: refused configures also ask for a recreate every few in a row (#1935),
        // done only once their retry is due, whatever repaints before; until then the check
        // below skips the frame. A recreate for `Lost` goes ahead as before.
        if self
            .surfaces
            .get(&viewport_id)
            .is_some_and(|s| s.needs_recreate || s.retry.recreate_due(now))
            && let Err(err) = self.recreate_surface(viewport_id, window)
        {
            log::error!("Failed to recreate surface for {viewport_id:?}: {err}");
            return vsync_sec;
        }

        // Apply any runtime changes requested via `RenderState::surface_config`.
        // We diff against the already-applied values in `self.config.surface`
        // and, if anything differs, mark every surface as needing reconfiguration so
        // the existing `needs_reconfigure` pathway below picks them up.
        if let Some(render_state) = self.render_state.as_ref()
            && render_state.surface_config != self.config.surface
        {
            self.config.surface = render_state.surface_config;
            #[expect(clippy::iter_over_hash_type)]
            for surface in self.surfaces.values_mut() {
                surface.needs_reconfigure = true;
            }
        }

        let Some(render_state) = self.render_state.as_mut() else {
            return vsync_sec;
        };
        if let Some(effect) = &render_state.renderer.read().frame_compositor {
            effect.begin_viewport(viewport_id.0.value());
        }

        let mut render_queue_guard = RendererQueueGuard {
            queue: &render_state.queue,
            commands_submitted: false,
        };
        // SolidCraft: a minimized native window may retain a clamped 1x1 surface.
        if let Some(effect) = &render_state.renderer.read().frame_compositor {
            let extent = window.inner_size();
            if extent.width == 0 || extent.height == 0 {
                effect.suspend();
                return vsync_sec;
            }
        }

        let Some(surface_state) = self.surfaces.get_mut(&viewport_id) else {
            return vsync_sec;
        };

        // SolidCraft: a surface still refusing its configuration, or waiting for the retry of a
        // refused one (to be configured or recreated), skips this frame rather than acquire a
        // texture from it; the retry is scheduled (#1935).
        if (surface_state.needs_reconfigure || surface_state.retry.recreate_pending())
            && !Self::try_configure(
                &self.context,
                viewport_id,
                surface_state,
                render_state,
                &self.config.surface,
                now,
            )
        {
            return vsync_sec;
        }

        let output_frame = {
            profiling::scope!("get_current_texture");
            // This is what vsync-waiting happens on my Mac.
            let start = web_time::Instant::now();
            // SolidCraft: the first frame is the last startup step.
            if !FIRST_PRESENT.swap(true, std::sync::atomic::Ordering::Relaxed) {
                crate::startup_step(crate::STEP_FIRST_PRESENT, "");
            }
            // SolidCraft: an acquire wgpu refuses (a surface left without its configuration,
            // #7007) comes back as `CurrentSurfaceTexture::Validation` instead of reaching the
            // device's uncaptured-error handler, which panics by default.
            let (output_frame, failed) = crate::capture_errors(&render_state.device, || {
                surface_state.surface.get_current_texture()
            });
            if let Some(error) = failed {
                log::warn!("The surface gave no frame: {error}");
            }
            vsync_sec += start.elapsed().as_secs_f32();
            output_frame
        };

        let frame = match output_frame {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                surface_state.needs_reconfigure = true;
                frame
            }
            other => {
                match (*self.config.on_surface_status)(&other) {
                    SurfaceErrorAction::Reconfigure => {
                        // SolidCraft: not before a refused configure's retry is due (#1935).
                        if Self::try_configure(
                            &self.context,
                            viewport_id,
                            surface_state,
                            render_state,
                            &self.config.surface,
                            now,
                        ) {
                            self.context.request_repaint_of(viewport_id);
                        }
                    }
                    SurfaceErrorAction::RecreateSurface => {
                        // Because of ownership, I could not find an easy way to do a full recovery here,
                        // as that would involve dropping the old surface and creating a new one.
                        // For now, we defer the recreation to the beginning of the next frame (which
                        // we ensure to arrive via `request_repaint_of`). A cleaner solution would be
                        // to untangle the ownership of `RenderState`.
                        surface_state.needs_recreate = true;
                        self.context.request_repaint_of(viewport_id);
                    }
                    SurfaceErrorAction::SkipFrame => {}
                }
                return vsync_sec;
            }
        };
        // SolidCraft: however this frame ends (presented, dropped by an early return below, or
        // dropped while a panic unwinds), the surface gets its image back, also on a lost device,
        // where wgpu's present and discard keep it and the surface's drop then panicked (#7148).
        let texture = frame.texture.clone();
        let output_frame = crate::AcquiredFrame::new(frame, texture);

        // SolidCraft: before the uploads, an empty submission asks the driver whether the device
        // is still there. The first queue write to meet a lost device panics inside wgpu (#4172);
        // the submission meets it instead, marks it lost, and the frame is dropped unpresented.
        if render_state.device_lost.probe(&render_state.queue) {
            if let Some(effect) = &render_state.renderer.read().frame_compositor {
                effect.suspend();
            }
            render_queue_guard.commands_submitted = true;
            log::error!(
                "The graphics device was lost: {}",
                render_state.device_lost.reason().unwrap_or_default()
            );
            self.context.request_repaint_of(viewport_id);
            return vsync_sec;
        }

        // SolidCraft: acquire the drawable before preparing callbacks. Preparation can
        // cache GPU resources whose initialization is recorded into `encoder` (such as
        // the viewport's tessellated vertices). If acquisition skips this frame, dropping
        // that encoder leaves the cached resources uninitialized on every later frame.
        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("encoder"),
                });

        // Upload all resources for the GPU.
        // SolidCraft: a clamped surface draws the whole window at a smaller scale.
        let screen_descriptor = renderer::ScreenDescriptor {
            size_in_pixels: [surface_state.width, surface_state.height],
            pixels_per_point: pixels_per_point * surface_state.scale,
        };

        let user_cmd_bufs = {
            let mut renderer = render_state.renderer.write();
            #[expect(clippy::iter_over_hash_type)] // Order doesn't matter here
            for (id, image_deltas) in textures_delta.set.drain() {
                for image_delta in image_deltas {
                    renderer.update_texture(
                        &render_state.device,
                        &render_state.queue,
                        id,
                        &image_delta,
                    );
                }
            }

            renderer.update_buffers(
                &render_state.device,
                &render_state.queue,
                &mut encoder,
                clipped_primitives,
                &screen_descriptor,
            )
        };

        let mut capture_buffer = None;
        {
            let renderer = render_state.renderer.read();

            let target_texture = if capture {
                let capture_state = self.screen_capture_state.get_or_insert_with(|| {
                    CaptureState::new(&render_state.device, &output_frame.frame.texture)
                });
                capture_state.update(&render_state.device, &output_frame.frame.texture);

                &capture_state.texture
            } else {
                &output_frame.frame.texture
            };
            let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

            let (view, resolve_target) = (self.options.msaa_samples > 1)
                .then_some(self.msaa_texture_view.get(&viewport_id))
                .flatten()
                .map_or((&target_view, None), |texture_view| {
                    (texture_view, Some(&target_view))
                });

            // SolidCraft: the pass owner selects offscreen composition once, never in render's loop.
            let composed = self.options.msaa_samples == 1
                && self.options.depth_stencil_format.is_none()
                && renderer.frame_compositor.as_ref().is_some_and(|effect| {
                    effect.render(
                        &renderer,
                        &mut encoder,
                        &target_view,
                        clipped_primitives,
                        &screen_descriptor,
                        clear_color,
                    )
                });
            if !composed {
                let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui_render"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: clear_color[0] as f64,
                                g: clear_color[1] as f64,
                                b: clear_color[2] as f64,
                                a: clear_color[3] as f64,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: self.depth_texture_view.get(&viewport_id).map(
                        |view| {
                            wgpu::RenderPassDepthStencilAttachment {
                                view,
                                depth_ops: self
                                    .options
                                    .depth_stencil_format
                                    .is_some_and(|depth_stencil_format| {
                                        depth_stencil_format.has_depth_aspect()
                                    })
                                    .then_some(wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(1.0),
                                        // It is very unlikely that the depth buffer is needed after egui finished rendering
                                        // so no need to store it. (this can improve performance on tiling GPUs like mobile chips or Apple Silicon)
                                        store: wgpu::StoreOp::Discard,
                                    }),
                                stencil_ops: self
                                    .options
                                    .depth_stencil_format
                                    .is_some_and(|depth_stencil_format| {
                                        depth_stencil_format.has_stencil_aspect()
                                    })
                                    .then_some(wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(0),
                                        store: wgpu::StoreOp::Discard,
                                    }),
                            }
                        },
                    ),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

                // Forgetting the pass' lifetime means that we are no longer compile-time protected from
                // runtime errors caused by accessing the parent encoder before the render pass is dropped.
                // Since we don't pass it on to the renderer, we should be perfectly safe against this mistake here!
                renderer.render(
                    &mut render_pass.forget_lifetime(),
                    clipped_primitives,
                    &screen_descriptor,
                );
            }

            if capture && let Some(capture_state) = &mut self.screen_capture_state {
                capture_buffer = Some(capture_state.copy_textures(
                    &render_state.device,
                    &output_frame.frame,
                    &mut encoder,
                ));
            }
        }

        let encoded = {
            profiling::scope!("CommandEncoder::finish");
            encoder.finish()
        };

        // Submit the commands: both the main buffer and user-defined ones.
        {
            profiling::scope!("Queue::submit");
            // wgpu doesn't document where vsync can happen. Maybe here?
            let start = web_time::Instant::now();
            render_state
                .queue
                .submit(core::iter::chain(user_cmd_bufs, [encoded]));
            vsync_sec += start.elapsed().as_secs_f32();
        };

        // Ensure that the queue guard does not do unnecessary work when dropped
        render_queue_guard.commands_submitted = true;
        // SolidCraft: caches become valid only after the producer encoder was submitted.
        if let Some(effect) = &render_state.renderer.read().frame_compositor {
            effect.submitted();
        }

        // Free textures marked for destruction **after** queue submit since they might still be used in the current frame.
        // Calling `wgpu::Texture::destroy` on a texture that is still in use would invalidate the command buffer(s) it is used in.
        // However, once we called `wgpu::Queue::submit`, it is up for wgpu to determine how long the underlying gpu resource has to live.
        {
            let mut renderer = render_state.renderer.write();
            #[expect(clippy::iter_over_hash_type)] // Order doesn't matter here
            for id in textures_delta.free.drain() {
                renderer.free_texture(&id);
            }
        }

        if let Some(capture_buffer) = capture_buffer
            && let Some(screen_capture_state) = &mut self.screen_capture_state
        {
            screen_capture_state.read_screen_rgba(
                self.context.clone(),
                capture_buffer,
                capture_data,
                self.capture_tx.clone(),
                viewport_id,
            );
        }

        window.pre_present_notify();

        {
            profiling::scope!("present");
            // wgpu doesn't document where vsync can happen. Maybe here?
            let start = web_time::Instant::now();
            output_frame.present(|frame| render_state.queue.present(frame));
            vsync_sec += start.elapsed().as_secs_f32();
        }

        vsync_sec
    }

    /// Call this at the beginning of each frame to receive the requested screenshots.
    pub fn handle_screenshots(&self, events: &mut Vec<Event>) {
        for (viewport_id, user_data, screenshot) in self.capture_rx.try_iter() {
            let screenshot = Arc::new(screenshot);
            for data in user_data {
                events.push(Event::Screenshot {
                    viewport_id,
                    user_data: data,
                    image: Arc::clone(&screenshot),
                });
            }
        }
    }

    pub fn gc_viewports(&mut self, active_viewports: &ViewportIdSet) {
        self.surfaces.retain(|id, _| active_viewports.contains(id));
        self.depth_texture_view
            .retain(|id, _| active_viewports.contains(id));
        self.msaa_texture_view
            .retain(|id, _| active_viewports.contains(id));
    }

    #[expect(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    pub fn destroy(&mut self) {
        // TODO(emilk): something here?
    }
}
