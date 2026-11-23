//! Pictures of a [`Scene`] drawn offscreen with the viewport's pipelines ([`crate::view`]): the
//! same shading, edges, silhouettes, section cap and surface analysis the viewport shows, read
//! back as RGBA. A host's command-line snapshots, screenshots and thumbnails draw through it, on
//! the application's device ([`Capturer::new`]) or on a headless one
//! ([`Capturer::headless`], [`shared`]). Without a qualifying adapter, the `auto` policy falls
//! to the last tier, the CPU rasterizer ([`headless_rgba`]), and says so: every picture names the
//! renderer that drew it (`gpu`, `gpu-software` or `cpu`); `hardware` and `software` fail with an
//! error that names the policy.

use solidcraft_viewport::vec::Vec3Ops;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use solidcore_record::Vec3;
use solidcraft_viewport::hatch::Hatch;
use solidcraft_viewport::{AnalysisMode, Camera, Rgb, SECTION_CAP, Scene, SceneMesh};

use crate::view::{AccessMap, DrawStats, Frame, GpuScene, GpuTarget, Palette, PaletteIndex, Renderer, analysis_colors, analysis_uniform};
use crate::{GpuError, Result, caught, error_scopes};

/// The colour format pictures are drawn in: sRGB bytes as they are, like the viewport's usual
/// (non-sRGB) target.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Samples per pixel (MSAA), as the desktop viewport's default; 1 where the device has no 4×.
pub const SAMPLES: u32 = 4;
/// The largest picture side (pixels); the device's texture limit may lower it.
pub const MAX_SIDE: u32 = 8192;
/// The background of a scene without one of its own: the light theme's viewport gradient.
const BACKGROUND: (Rgb, Rgb) = (Rgb(236, 240, 245), Rgb(205, 212, 222));
/// How long a picture may take on the GPU before it is given up.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The viewport's pipelines on one device, drawing pictures offscreen. Pictures are drawn one at
/// a time (callers on several threads take turns).
pub struct Capturer {
    adapter: String,
    /// The adapter is a GPU driver running on the CPU (llvmpipe, lavapipe, WARP).
    software: bool,
    samples: u32,
    /// The depth format: 32-bit float (reversed depth) where the device has it ([`crate::depth_format`]).
    depth: wgpu::TextureFormat,
    max_side: u32,
    state: Mutex<State>,
    /// The device was lost (#4172): set by the device's lost callback, which whoever made the
    /// device registered ([`Capturer::watching`]).
    lost: Arc<AtomicBool>,
}

struct State {
    /// The device and what draws on it; `None` once closed ([`Capturer::close`]).
    gpu: Option<Gpu>,
    /// The last picture's scene key and accessibility map version (each picture uploads anew).
    key: u64,
    access_version: u64,
    /// The resolve textures cleared before their first resolve (tests count them).
    #[cfg(test)]
    resolve_clears: u32,
}

/// A capturer's device, with its queue and the pipelines built on it.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    background: wgpu::RenderPipeline,
}

impl Capturer {
    /// The pipelines on an existing device (the application's).
    pub fn new(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Result<Self> {
        let depth = crate::depth_format(&device, true);
        Self::with_depth(adapter, device, queue, depth)
    }

    /// As [`Capturer::new`], drawing with the depth format `depth` (with a stencil): the fallback
    /// `Depth24PlusStencil8` on a device that has a float one too, to compare the two.
    pub fn with_depth(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue, depth: wgpu::TextureFormat) -> Result<Self> {
        Self::with_samples(adapter, device, queue, depth, SAMPLES)
    }

    /// As [`Capturer::with_depth`], with `samples` per pixel (1: no multisampling, as a device or
    /// browser without 4× draws); 1 where the device lacks that many.
    fn with_samples(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue, depth: wgpu::TextureFormat, samples: u32) -> Result<Self> {
        let flags = |f: wgpu::TextureFormat| adapter.get_texture_format_features(f).flags;
        let samples =
            if samples > 1 && flags(FORMAT).sample_count_supported(samples) && flags(depth).sample_count_supported(samples) { samples } else { 1 };
        let scopes = error_scopes(&device);
        let renderer = Renderer::new(&device, GpuTarget { format: FORMAT, depth: Some(depth), samples, tess: false, oit: false, edges: false }, None);
        let background = background_pipeline(&device, samples, depth);
        caught(scopes).map_or(Ok(()), |e| Err(GpuError::Pipeline(e)))?;
        let max_side = device.limits().max_texture_dimension_2d.min(MAX_SIDE);
        Ok(Capturer {
            adapter: adapter.get_info().name,
            software: adapter.get_info().device_type == wgpu::DeviceType::Cpu,
            samples,
            depth,
            max_side,
            state: Mutex::new(State {
                gpu: Some(Gpu { device, queue, renderer, background }),
                key: 0,
                access_version: 0,
                #[cfg(test)]
                resolve_clears: 0,
            }),
            lost: Arc::default(),
        })
    }

    /// Stop drawing once `lost` is set: the flag the device's lost callback sets (the app's
    /// window device has one, `egui_wgpu::DeviceLost`). A lost device can't draw again; its
    /// pictures fail with [`GpuError::DeviceLost`] without touching it (#4172).
    pub fn watching(mut self, lost: Arc<AtomicBool>) -> Self {
        self.lost = lost;
        self
    }

    /// The device was lost: pictures fail with [`GpuError::DeviceLost`].
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// [`Capturer::is_lost`], asking the driver first: a submission that opens a command list
    /// meets a loss the driver reports and marks it, where the picture's first upload would
    /// panic inside wgpu. As `egui_wgpu::DeviceLost::probe`: an empty submission only signals a
    /// fence, which a reset D3D12 device let through (#6528).
    fn probe_lost(&self, gpu: &Gpu) -> bool {
        if self.is_lost() {
            return true;
        }
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("device lost probe") });
        encoder.insert_debug_marker("device lost probe");
        gpu.queue.submit([encoder.finish()]);
        self.is_lost()
    }

    /// Close the device: it, its queue and its pipelines are dropped, once a picture being drawn
    /// has ended. Pictures asked for afterwards fail and say so.
    pub fn close(&self) {
        // Also after a picture panicked: the device closes whatever that picture left.
        let gpu = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).gpu.take();
        drop(gpu);
    }

    /// A device of its own, without a window (native only): the adapter `policy` picks
    /// ([`crate::select`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless(policy: crate::select::Policy) -> Result<Self> {
        let (adapter, device, queue) = crate::open_headless(policy)?;
        // Errors are caught per picture (error scopes); anything else is logged, never a panic.
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| log::error!("GPU: {e}")));
        let lost = watch_lost(&device);
        Ok(Self::new(&adapter, device, queue)?.watching(lost))
    }

    /// As [`Capturer::headless`], drawing with the depth format `depth` ([`Capturer::with_depth`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless_with_depth(policy: crate::select::Policy, depth: wgpu::TextureFormat) -> Result<Self> {
        let (adapter, device, queue) = crate::open_headless(policy)?;
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| log::error!("GPU: {e}")));
        let lost = watch_lost(&device);
        Ok(Self::with_depth(&adapter, device, queue, depth)?.watching(lost))
    }

    /// As [`Capturer::headless`], without multisampling: one sample per pixel, as the viewport
    /// draws where 4× isn't available (some browsers and devices, or MSAA turned off).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless_single_sample(policy: crate::select::Policy) -> Result<Self> {
        let (adapter, device, queue) = crate::open_headless(policy)?;
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| log::error!("GPU: {e}")));
        let depth = crate::depth_format(&device, true);
        let lost = watch_lost(&device);
        Ok(Self::with_samples(&adapter, device, queue, depth, 1)?.watching(lost))
    }

    /// The depth format pictures are drawn with; float formats are drawn with reversed depth.
    pub fn depth_format(&self) -> wgpu::TextureFormat {
        self.depth
    }

    /// The adapter's name.
    pub fn adapter_name(&self) -> &str {
        &self.adapter
    }

    /// What a picture's reply names as its renderer: `gpu`, or `gpu-software` on a software
    /// adapter.
    pub fn renderer(&self) -> &'static str {
        if self.software { "gpu-software" } else { "gpu" }
    }

    /// Samples per pixel the pictures are drawn with.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// `scene` seen by `cam`, `width` × `height` pixels: straight RGBA, rows top to bottom.
    pub fn rgba(&self, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        self.rgba_highlighted(scene, None, cam, width, height)
    }

    /// [`Capturer::rgba`] with `highlight` drawn as the viewport draws its highlight scene
    /// (selection and hover feedback), over the model scene.
    pub fn rgba_highlighted(&self, scene: &Scene, highlight: Option<GpuScene>, cam: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        self.draw(scene, highlight, cam, [width, height], 0.0).map(|(rgba, _)| rgba)
    }

    /// [`Capturer::rgba`] with ambient occlusion of `radius` (world units), as the viewport
    /// shades with it on.
    pub fn rgba_occluded(&self, scene: &Scene, cam: &Camera, width: u32, height: u32, radius: f32) -> Result<Vec<u8>> {
        self.draw(scene, None, cam, [width, height], radius).map(|(rgba, _)| rgba)
    }

    /// [`Capturer::rgba`] and what the picture drew ([`DrawStats`]). Both come from the same
    /// turn on the device, so another thread's picture can't put its counts in their place.
    pub fn rgba_with_stats(&self, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<(Vec<u8>, DrawStats)> {
        self.draw(scene, None, cam, [width, height], 0.0)
    }

    /// The picture and what it drew, both read while this call has the device to itself.
    fn draw(
        &self,
        scene: &Scene,
        highlight: Option<GpuScene>,
        cam: &Camera,
        [width, height]: [u32; 2],
        ao_radius: f32,
    ) -> Result<(Vec<u8>, DrawStats)> {
        if !(1..=self.max_side).contains(&width) || !(1..=self.max_side).contains(&height) {
            return Err(GpuError::Render(format!("a picture is 1 to {} pixels a side, not {width} × {height}", self.max_side)));
        }
        let mut state = self.state.lock().map_err(|_| GpuError::Render("an earlier picture failed on this device".into()))?;
        let state = &mut *state;
        if self.probe_lost(state.gpu.as_ref().ok_or_else(closed)?) {
            return Err(GpuError::DeviceLost);
        }
        state.key = state.key.wrapping_add(1);
        let mode = scene.analysis.map(|a| a.mode);
        let access = match mode {
            Some(AnalysisMode::Access { dir }) => {
                state.access_version = state.access_version.wrapping_add(1);
                AccessMap::build(&scene.meshes.iter().map(|m| m.mesh.as_ref()).collect::<Vec<_>>(), dir, state.access_version)
            }
            _ => None,
        };
        let mut frame = frame(scene, cam, [width, height], state.key, access);
        frame.ao_radius = ao_radius;
        // Always a highlight scene of the picture's own, empty without one: the renderer keeps
        // its last highlight scene while the slot is empty (as the viewport does until the next
        // is built), so the previous picture's highlight would show in this one.
        frame.hl_key = state.key;
        frame.hl_slot = Arc::new(Mutex::new(Some(highlight.unwrap_or_default())));
        let result = self.picture(state, &frame, scene.background.unwrap_or(BACKGROUND), [width, height]);
        // Each capture uploads its own scene. Release the picture's buffers after readback or
        // error so a long-lived capturer keeps its pipelines without holding the last mesh.
        if let Some(gpu) = &mut state.gpu {
            gpu.renderer.retain_views(&[]);
        }
        result
    }

    /// `frame` drawn over the `background` gradient, read back as straight RGBA, and what it
    /// drew (read while the caller holds `state`).
    fn picture(&self, state: &mut State, frame: &Frame, (top, bottom): (Rgb, Rgb), [width, height]: [u32; 2]) -> Result<(Vec<u8>, DrawStats)> {
        let Gpu { device, queue, renderer, background: background_pipeline } = state.gpu.as_mut().ok_or_else(closed)?;
        let (device, queue) = (&*device, &*queue);
        let scopes = error_scopes(device);
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = |label: &str, format: wgpu::TextureFormat, samples: u32, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let resolved = texture("sc_capture", FORMAT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let multisampled = (self.samples > 1).then(|| texture("sc_capture_ms", FORMAT, self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let depth = texture("sc_capture_depth", self.depth, self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let (resolved_view, depth_view) = (resolved.create_view(&Default::default()), depth.create_view(&Default::default()));
        let multisampled_view = multisampled.as_ref().map(|t| t.create_view(&Default::default()));
        let background = match background_vertices(device, top, bottom) {
            Ok(b) => b,
            Err(e) => {
                let _ = caught(scopes);
                return Err(e);
            }
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_capture") });
        // Multisampled, the picture's texture is written only by the resolve: cleared first, or
        // some AMD Radeon Direct3D 12 drivers read it back black or as garbage (#10140), as the
        // viewport's resolve targets showed nothing until they were cleared (#6828).
        let _cleared = crate::view::clear_resolve_targets(&mut encoder, multisampled_view.is_some().then_some(&resolved_view));
        #[cfg(test)]
        {
            state.resolve_clears += _cleared;
        }
        renderer.prepare(device, queue, &mut encoder, frame);
        if let Some(reason) = renderer.take_refused_upload() {
            let _ = caught(scopes);
            return Err(GpuError::Render(reason));
        }
        {
            let (view, resolve_target, store) = match &multisampled_view {
                Some(ms) => (ms, Some(&resolved_view), wgpu::StoreOp::Discard),
                None => (&resolved_view, None, wgpu::StoreOp::Store),
            };
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_capture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(crate::view::far_depth(crate::view::reversed_depth(Some(self.depth)))),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            // The background first, as the viewport paints it before the model.
            pass.set_pipeline(background_pipeline);
            pass.set_vertex_buffer(0, background.slice(..));
            pass.draw(0..6, 0..1);
            renderer.paint(frame, &mut pass, [0.0, 0.0, width as f32, height as f32]);
        }
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_capture_read"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            resolved.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
            },
            size,
        );
        let submission = queue.submit([encoder.finish()]);
        // Waiting on a device the submission found lost would panic inside wgpu.
        if self.is_lost() {
            return Err(GpuError::DeviceLost);
        }
        if let Some(e) = caught(scopes) {
            return Err(GpuError::Render(e));
        }
        let slice = read.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(TIMEOUT) })
            .map_err(|e| GpuError::Render(format!("waiting for the picture: {e}")))?;
        read_back(&rx)?;
        let data = slice.get_mapped_range().map_err(|e| GpuError::Render(format!("reading the picture back: {e}")))?;
        let mut rgba = Vec::with_capacity((row as usize) * (height as usize));
        for r in data.chunks(padded as usize).take(height as usize) {
            rgba.extend_from_slice(r.get(..row as usize).unwrap_or_default());
        }
        if rgba.len() != (row as usize) * (height as usize) {
            return Err(GpuError::Render("the picture read back short".into()));
        }
        let stats = renderer.stats().lock().map(|s| *s).unwrap_or_default();
        Ok((rgba, stats))
    }

    /// [`Capturer::rgba`] as PNG bytes.
    pub fn png(&self, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        let rgba = self.rgba(scene, cam, width, height)?;
        solidcraft_viewport::png(width as usize, height as usize, rgba).ok_or_else(|| GpuError::Render("PNG encoding failed".into()))
    }
}

/// What a picture asked of a closed capturer fails with ([`Capturer::close`]).
fn closed() -> GpuError {
    GpuError::Render("the GPU device for pictures is closed".into())
}

#[cfg(not(target_arch = "wasm32"))]
static SHARED: std::sync::OnceLock<Result<Capturer>> = std::sync::OnceLock::new();

/// The process's headless capturer (native only), opened on first use on the adapter `policy`
/// picks and kept, whatever later calls pass; the error when no adapter qualifies or the device
/// fails (not retried). A host passes the same policy every time.
#[cfg(not(target_arch = "wasm32"))]
pub fn shared(policy: crate::select::Policy) -> std::result::Result<&'static Capturer, &'static GpuError> {
    SHARED.get_or_init(|| Capturer::headless(policy)).as_ref()
}

/// For a process about to exit on Linux with NVIDIA's own driver: close the device of [`shared`],
/// if one was opened ([`Capturer::close`]). Without a display that driver's Vulkan runs on its EGL
/// library, whose exit handler unloads `libnvidia-eglcore` while an open device's threads still
/// run in it, and the process then ends with a segmentation fault after its work is done (#19431).
/// Every other system and driver exits with the device open, as before.
#[cfg(not(target_arch = "wasm32"))]
pub fn close_shared_before_exit() {
    if crate::select::nvidia_linux_driver()
        && let Some(Ok(capturer)) = SHARED.get()
    {
        capturer.close();
    }
}

/// What draws this process's headless pictures (native only): the shared headless device, or the
/// CPU rasterizer where the policy lands there (`cpu`, or `auto` with no qualifying adapter).
#[cfg(not(target_arch = "wasm32"))]
pub enum Headless {
    Gpu(&'static Capturer),
    Cpu,
}

/// [`Headless`] under `policy`; the error when the policy is `hardware` or `software` and no
/// adapter of that tier opens.
#[cfg(not(target_arch = "wasm32"))]
pub fn headless(policy: crate::select::Policy) -> std::result::Result<Headless, &'static GpuError> {
    use crate::select::Policy;
    if policy == Policy::Cpu {
        return Ok(Headless::Cpu);
    }
    match shared(policy) {
        Ok(c) => Ok(Headless::Gpu(c)),
        Err(e) if policy == Policy::Auto => {
            log::warn!("pictures are drawn by the CPU rasterizer: {e}");
            Ok(Headless::Cpu)
        }
        Err(e) => Err(e),
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Headless {
    /// `scene` drawn: straight RGBA and the renderer that drew it (`gpu`, `gpu-software` or
    /// `cpu`). The CPU rasterizer draws no surface analysis, highlights or legends.
    pub fn rgba(&self, scene: &Scene, cam: &Camera, width: u32, height: u32) -> std::result::Result<(Vec<u8>, &'static str), String> {
        match self {
            Headless::Gpu(c) => c.rgba(scene, cam, width, height).map(|rgba| (rgba, c.renderer())).map_err(|e| e.to_string()),
            Headless::Cpu => {
                if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
                    return Err(format!("a picture is 1 to {MAX_SIDE} pixels a side, not {width} × {height}"));
                }
                let canvas = solidcraft_viewport::render(scene, cam, width as usize, height as usize);
                Ok((canvas.rgba, "cpu"))
            }
        }
    }
}

/// `scene` drawn by [`headless`] under `policy` ([`Headless::rgba`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn headless_rgba(
    policy: crate::select::Policy,
    scene: &Scene,
    cam: &Camera,
    width: u32,
    height: u32,
) -> std::result::Result<(Vec<u8>, &'static str), String> {
    headless(policy).map_err(|e| e.to_string())?.rgba(scene, cam, width, height)
}

/// The frame parameters of a picture: the scene's own geometry as the model scene (key `key`),
/// the camera, the section, the analysis, and the silhouette outlines of the default visual style.
fn frame(scene: &Scene, cam: &Camera, [width, height]: [u32; 2], key: u64, access: Option<AccessMap>) -> Frame {
    let empty = || Arc::new(Mutex::new(None));
    let eye = cam.eye().to_f32();
    let cap = scene.cap.unwrap_or(SECTION_CAP);
    let mode = scene.analysis.map(|a| a.mode);
    Frame {
        product_lights: None,
        product: false,
        upload_error: None,
        key,
        slot: Arc::new(Mutex::new(Some(scene_geometry(scene)))),
        sketch_key: 0,
        sketch_slot: empty(),
        hl_key: 0,
        hl_slot: empty(),
        pv_key: 0,
        pv_slot: empty(),
        pv_hide: false,
        preview_bounds: None,
        view_proj: cam.view_proj(f64::from(width) / f64::from(height.max(1)), scene.radius.max(1.0)).to_f32(),
        turned_depth: None,
        back: cam.back().to_f32(),
        size_px: [width as f32, height as f32],
        clip: crate::view::clip_planes(&scene.clip),
        clip_hatch: crate::view::clip_hatches(&(0..scene.clip.len()).map(|k| scene.clip_hatch.get(k).copied().unwrap_or(k)).collect::<Vec<_>>()),
        cap: [f32::from(cap.0) / 255.0, f32::from(cap.1) / 255.0, f32::from(cap.2) / 255.0, if scene.plain_cap { 0.0 } else { 1.0 }],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis: analysis_uniform(mode, access.as_ref()),
        analysis_colors: scene.analysis.map_or([0.0; 36], |a| analysis_colors(&a.colors)),
        access,
        outline: true,
        eye: [eye[0], eye[1], eye[2], if cam.fov > 0.0 { 1.0 } else { 0.0 }],
        ao_radius: 0.0,
        ao_samples: crate::view::AO_SAMPLES,
        edge_px: crate::view::EDGE_PX,
        sky: [0.0; 4],
        ssaa: 1.0,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: scene.part_hatch.map_or_else(Default::default, |c| c.map(|Rgb(r, g, b)| [r, g, b].map(|x| f32::from(x) / 255.0))),
        moving: None,
        motions: Default::default(),
        ground: None,
    }
}

/// A scene's meshes and lines as the viewport's model scene: faces in their colours (smooth
/// normals; a face normal where a vertex has none), lines as wide lines (over everything when
/// `on_top`), the grid under everything. A body whose appearance is see-through
/// ([`solidcraft_viewport::SceneMesh::glass`]) goes into the scene's glass, drawn as the viewport
/// draws glass: far sides first, then near sides, so what lies inside it shows through. When the
/// scene hatches its cut by part ([`Scene::part_hatch`]), each body's colours are palette entries
/// of its own, carrying its hatch.
pub fn scene_geometry(scene: &Scene) -> GpuScene {
    let by_part = scene.part_hatch.is_some() && !scene.clip.is_empty();
    // Per body, its colours' stable entries (by part), and the stable palette they make.
    let mut own: Vec<HashMap<[u8; 4], PaletteIndex>> = Vec::new();
    let mut stable: Vec<[u8; 4]> = Vec::new();
    let mut hatches: Vec<(PaletteIndex, Hatch)> = Vec::new();
    let c4 = |m: &SceneMesh, c: Rgb| [c.0, c.1, c.2, (m.alpha() * 255.0).round().clamp(0.0, 255.0) as u8];
    if by_part {
        for m in &scene.meshes {
            let mut map = HashMap::new();
            for c in std::iter::once(m.color).chain(m.face_colors.iter().map(|(_, c)| *c)) {
                map.entry(c4(m, c)).or_insert_with(|| {
                    let i = PaletteIndex(u32::try_from(stable.len()).unwrap_or(0));
                    stable.push(c4(m, c));
                    hatches.push((i, m.hatch));
                    i
                });
            }
            own.push(map);
        }
    }
    let mut palette = Palette::with_stable(stable);
    for (i, h) in hatches {
        palette.set_hatch(i, h);
    }
    let mut sc = GpuScene::with_palette(palette);
    for (mi, m) in scene.meshes.iter().enumerate() {
        let faces: HashMap<u32, Rgb> = m.face_colors.iter().copied().collect();
        let mesh = &m.mesh;
        for (ti, t) in mesh.triangles.iter().enumerate() {
            let rgba = c4(m, mesh.tri_face.get(ti).and_then(|f| faces.get(f)).copied().unwrap_or(m.color));
            let col = match own.get(mi).and_then(|o| o.get(&rgba)) {
                Some(i) => *i,
                None => sc.color(rgba),
            };
            let [Some(a), Some(b), Some(c)] = t.map(|k| mesh.positions.get(k as usize).copied()) else { continue };
            let flat = (b - a).cross(c - a).normalized().unwrap_or(Vec3::Z);
            for (k, p) in t.iter().zip([a, b, c]) {
                let n = mesh.normals.get(*k as usize).copied().filter(|n| n.is_finite() && n.len() > 0.5).unwrap_or(flat);
                // A mesh with values gives each vertex its own ([`SceneMesh::values`]).
                match m.values.as_ref().and_then(|v| v.get(*k as usize)).copied() {
                    Some(value) if m.glass() => sc.glass_tri_value(p.to_f32(), n.to_f32(), col, value),
                    Some(value) if m.closed => sc.tri_value(p.to_f32(), n.to_f32(), col, value),
                    Some(value) => sc.open_tri_value(p.to_f32(), n.to_f32(), col, value),
                    None if m.glass() => sc.glass_tri(p.to_f32(), n.to_f32(), col),
                    None if m.closed => sc.tri(p.to_f32(), n.to_f32(), col),
                    None => sc.open_tri(p.to_f32(), n.to_f32(), col),
                }
            }
        }
    }
    let alpha = |a: f32| (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    for l in &scene.grid {
        let col = sc.color([l.color.0, l.color.1, l.color.2, alpha(l.alpha)]);
        for w in l.points.windows(2) {
            sc.under_line(w[0].to_f32(), w[1].to_f32(), col, l.width as f32);
        }
    }
    for l in &scene.lines {
        let col = sc.color([l.color.0, l.color.1, l.color.2, alpha(l.alpha)]);
        for w in l.points.windows(2) {
            sc.line(w[0].to_f32(), w[1].to_f32(), col, l.width as f32, l.on_top);
        }
    }
    sc
}

/// A flag `device`'s lost callback sets (a device of this crate's own: a device has one lost
/// callback, and the app's window device has egui-wgpu's).
#[cfg(not(target_arch = "wasm32"))]
fn watch_lost(device: &wgpu::Device) -> Arc<AtomicBool> {
    let lost = Arc::new(AtomicBool::new(false));
    let seen = lost.clone();
    device.set_device_lost_callback(move |reason, message| {
        log::error!("GPU: the device was lost ({reason:?}): {message}");
        seen.store(true, Ordering::Release);
    });
    lost
}

/// The answer to a picture's read-back, once the wait for its submission is over. The read-back
/// is ready by then, but its answer may still be on its way: any other submit or poll on the
/// device (the window's frames, another picture's device check) can pick it up and answer it on
/// its own thread once it lets go of the device. So a native picture waits for the answer; a
/// browser answers later, on its own, and a picture can't wait for it there.
fn read_back(answer: &std::sync::mpsc::Receiver<std::result::Result<(), wgpu::BufferAsyncError>>) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let answer = answer.recv_timeout(TIMEOUT).map_err(|e| GpuError::Render(format!("reading the picture back: {e}")))?;
    #[cfg(target_arch = "wasm32")]
    let answer = answer.try_recv().map_err(|_| GpuError::Unsupported("reading a picture back without waiting for the browser".into()))?;
    answer.map_err(|e| GpuError::Render(format!("reading the picture back: {e}")))
}

/// The background gradient: the viewport's top colour at the top, its bottom one at the bottom,
/// mixed in sRGB as the window paints it.
const BACKGROUND_SHADER: &str = r#"
struct O { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };
@vertex
fn vs(@location(0) p: vec2<f32>, @location(1) c: vec4<f32>) -> O {
    var o: O;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.color = c;
    return o;
}
@fragment
fn fs(i: O) -> @location(0) vec4<f32> {
    return i.color;
}
"#;

/// Background vertex: NDC position (2 × f32) and sRGB colour (4 × u8).
const BACKGROUND_VERTEX: u64 = 12;

fn background_vertices(device: &wgpu::Device, top: Rgb, bottom: Rgb) -> Result<wgpu::Buffer> {
    let mut bytes = Vec::with_capacity(6 * BACKGROUND_VERTEX as usize);
    let corner = |x: f32, y: f32| (x, y, if y > 0.0 { top } else { bottom });
    for (x, y, c) in [corner(-1.0, 1.0), corner(1.0, 1.0), corner(1.0, -1.0), corner(-1.0, 1.0), corner(1.0, -1.0), corner(-1.0, -1.0)] {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
        bytes.extend_from_slice(&[c.0, c.1, c.2, 255]);
    }
    crate::buffer_init(device, "sc_capture_background", &bytes, wgpu::BufferUsages::VERTEX)
}

fn background_pipeline(device: &wgpu::Device, samples: u32, depth: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sc_capture_background"),
        source: wgpu::ShaderSource::Wgsl(BACKGROUND_SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sc_capture_background"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    const A: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sc_capture_background"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[Some(wgpu::VertexBufferLayout { array_stride: BACKGROUND_VERTEX, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A })],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{TRI_COLOR_AT, TRI_SIZE};
    use solidcore_tess::Mesh;
    use std::sync::Arc;

    /// One triangle facing the camera.
    fn sheet() -> Arc<Mesh> {
        Arc::new(Mesh {
            positions: vec![Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0)],
            normals: vec![Vec3::new(0.0, -1.0, 0.0); 3],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![0],
            ..Default::default()
        })
    }

    /// The colour of a triangle `scene_geometry` wrote (its first vertex's palette entry).
    fn color(scene: &GpuScene, bytes: &[u8]) -> Option<[u8; 4]> {
        let index = bytes.get(TRI_COLOR_AT..TRI_COLOR_AT + 4)?.try_into().ok().map(u32::from_le_bytes)?;
        scene.palette.get(crate::view::PaletteIndex(index))
    }

    /// A body the design draws see-through goes into the scene's glass, at its own alpha, and
    /// not among the solid triangles: that is how the viewport draws glass, and it is why a
    /// headless picture of a translucent target shows the tool inside it.
    #[test]
    fn a_see_through_body_is_glass_at_its_own_alpha_and_a_solid_one_is_not() {
        let mesh = |opacity: f32| SceneMesh {
            mesh: sheet(),
            color: Rgb(200, 30, 30),
            face_colors: Vec::new(),
            opacity,
            closed: true,
            hatch: Default::default(),
            values: None,
        };
        let geometry = |opacity: f32| scene_geometry(&Scene { meshes: vec![mesh(opacity)], ..Default::default() });

        let solid = geometry(1.0);
        assert_eq!((solid.tris.len(), solid.glass.len()), (3 * TRI_SIZE, 0));
        assert_eq!(color(&solid, &solid.tris), Some([200, 30, 30, 255]), "a solid body is drawn opaque");

        let glass = geometry(0.3);
        assert_eq!((glass.tris.len(), glass.glass.len()), (0, 3 * TRI_SIZE), "a see-through body is glass, not a solid triangle");
        assert_eq!(color(&glass, &glass.glass).map(|c| c[3]), Some(77), "0.3 of 255");

        // The threshold and the floor are the viewport's: an appearance a hair under 1 stays
        // solid, and opacity 0 still leaves a trace rather than vanishing.
        let almost = geometry(0.9995);
        assert_eq!((almost.tris.len(), almost.glass.len()), (3 * TRI_SIZE, 0), "an appearance a hair under 1 is still solid");
        assert_eq!(color(&almost, &almost.tris).map(|c| c[3]), Some(255));
        let trace = geometry(0.0);
        assert_eq!(color(&trace, &trace.glass).map(|c| c[3]), Some(13), "0.05 of 255");
    }

    /// A multisampled picture is resolved into a new texture nothing else writes. On some AMD
    /// Radeon Direct3D 12 drivers an uncleared one read back black or as garbage, so `ui.render`
    /// and the MCP screenshot came back black while the window showed the model (#10140). Each
    /// picture clears its new resolve texture once, before the resolve, and shows its background.
    #[test]
    fn a_multisampled_picture_clears_its_resolve_texture_before_the_resolve() {
        let Ok(capturer) = Capturer::headless(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let clears = || capturer.state.lock().map(|s| s.resolve_clears).unwrap_or(u32::MAX);
        let rgba = capturer.rgba(&Scene::default(), &Camera::default(), 32, 24).unwrap();
        // The light background gradient, opaque, in every corner: not black, not transparent.
        for at in [0, 31, 23 * 32, 23 * 32 + 31] {
            let p = rgba.get(at * 4..at * 4 + 4).unwrap();
            assert!(p[0] > 150 && p[1] > 150 && p[2] > 150 && p[3] == 255, "pixel {at} shows the background: {p:?}");
        }
        let expected = u32::from(capturer.samples() > 1);
        assert_eq!(clears(), expected, "the multisampled picture's resolve texture is cleared first; one sample resolves nothing");
        capturer.rgba(&Scene::default(), &Camera::default(), 32, 24).unwrap();
        assert_eq!(clears(), 2 * expected, "every picture has a new resolve texture, cleared again");
        // One sample: the picture's own pass clears the texture it draws into.
        if let Ok(single) = Capturer::headless_single_sample(crate::test_env::policy()) {
            single.rgba(&Scene::default(), &Camera::default(), 32, 24).unwrap();
            assert_eq!(single.state.lock().map(|s| s.resolve_clears).ok(), Some(0));
        }
    }

    #[test]
    fn a_capture_releases_its_scene_uploads_after_readback() {
        let capturer = match Capturer::headless(crate::test_env::policy()) {
            Ok(capturer) => capturer,
            Err(error) if crate::test_env::skip_unless_required(crate::test_env::required(), &error) => return,
            Err(error) => panic!("required GPU unavailable: {error}"),
        };
        let scene = Scene { meshes: vec![SceneMesh::solid(sheet(), Rgb(200, 30, 30))], ..Default::default() };
        let cam = Camera::default();
        let first = capturer.rgba_with_stats(&scene, &cam, 32, 24).unwrap();
        assert!(first.1.triangles > 0, "the capture drew the mesh");
        assert_eq!(capturer.rgba_with_stats(&scene, &cam, 32, 24).unwrap(), first);
        let state = capturer.state.lock().unwrap();
        let renderer = &state.gpu.as_ref().unwrap().renderer;
        assert_eq!(renderer.uploads().geometry, 0, "the renderer released the model scene after readback");
    }

    /// A closed capturer has dropped its device and everything it drew with, and a picture asked
    /// of it afterwards fails and says so. A host on NVIDIA's Linux driver closes the shared
    /// capturer before it exits ([`close_shared_before_exit`]).
    #[test]
    fn a_closed_capturer_drops_its_device_and_draws_no_more_pictures() {
        let Ok(capturer) = Capturer::headless(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        capturer.rgba(&Scene::default(), &Camera::default(), 32, 24).unwrap();
        capturer.close();
        assert!(capturer.state.lock().unwrap().gpu.is_none(), "the device, its queue and its pipelines are dropped");
        let after = capturer.rgba(&Scene::default(), &Camera::default(), 32, 24);
        assert!(matches!(&after, Err(GpuError::Render(why)) if why.contains("closed")), "{:?}", after.err());
        // Closing again changes nothing.
        capturer.close();
        assert!(capturer.rgba(&Scene::default(), &Camera::default(), 32, 24).is_err());
    }

    /// Bodies dragged as a layer of their own ([`crate::view::MovingLayer`]) are drawn moved by
    /// its transform: the picture is the one of the bodies built where they were moved to, and
    /// the bodies go back to the model scene when the drag ends (on this machine's GPU, skipped
    /// without one).
    #[test]
    fn a_moving_layer_is_drawn_moved_by_its_transform() {
        let Ok(capturer) = Capturer::headless_single_sample(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let wide = Arc::new(Mesh {
            positions: vec![Vec3::new(-1.0, 0.0, -0.5), Vec3::new(0.2, 0.0, -0.5), Vec3::new(-0.4, 0.0, 0.6)],
            ..(*sheet()).clone()
        });
        let shift = Vec3::new(0.6, 0.0, 0.3);
        let at = |offset: Vec3| {
            let mesh = Mesh { positions: wide.positions.iter().map(|p| *p + offset).collect(), ..(*wide).clone() };
            Scene { meshes: vec![SceneMesh::solid(Arc::new(mesh), Rgb(200, 30, 30))], ..Default::default() }
        };
        // Two and a half units of the view's height either side.
        let cam = Camera { distance: 6.0, ..Camera::default() };
        let size = [64, 64];
        let slot = |sc: GpuScene| Arc::new(Mutex::new(Some(sc)));
        let Ok(mut state) = capturer.state.lock() else { panic!("capturer state") };
        // Drawn where it was moved to.
        let moved = frame(&at(shift), &cam, size, 1, None);
        let expected = capturer.picture(&mut state, &moved, BACKGROUND, size).unwrap().0;
        assert!(expected.chunks(4).filter(|p| p[0] > p[1].saturating_add(60)).count() > 50, "the body shows");
        // Drawn where it was, moved by the layer; the model scene empty.
        let mut layered = frame(&Scene::default(), &cam, size, 2, None);
        let transform = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [shift.x as f32, shift.y as f32, shift.z as f32, 1.0]];
        layered.moving = Some(crate::view::MovingLayer { key: 3, slot: slot(scene_geometry(&at(Vec3::ZERO))), transform });
        let drawn = capturer.picture(&mut state, &layered, BACKGROUND, size).unwrap().0;
        let differ = expected.chunks(4).zip(drawn.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
        assert!(differ <= 4, "{differ} pixels differ from the body drawn where it was moved to");
        // The drag over: the model scene draws it, and nothing is drawn twice or left behind.
        let after = frame(&at(shift), &cam, size, 4, None);
        let back = capturer.picture(&mut state, &after, BACKGROUND, size).unwrap().0;
        assert_eq!(back, expected);
    }

    /// A frame without a moving layer lets the layer's scene go: the same key handed in again
    /// with nothing in its slot draws nothing, and a new key takes the scene waiting in its slot.
    /// So who leaves its layer out of a frame (a preview hidden for a pick) hands the scene over
    /// again under a new key (on this machine's GPU, skipped without one).
    #[test]
    fn a_moving_layer_left_out_of_a_frame_is_drawn_again_only_under_a_new_key() {
        let Ok(capturer) = Capturer::headless_single_sample(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let wide = Mesh { positions: vec![Vec3::new(-1.0, 0.0, -0.5), Vec3::new(0.2, 0.0, -0.5), Vec3::new(-0.4, 0.0, 0.6)], ..(*sheet()).clone() };
        let body = scene_geometry(&Scene { meshes: vec![SceneMesh::solid(Arc::new(wide), Rgb(200, 30, 30))], ..Default::default() });
        let cam = Camera { distance: 6.0, ..Camera::default() };
        let size = [64, 64];
        let transform = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let Ok(mut state) = capturer.state.lock() else { panic!("capturer state") };
        // The picture of an empty model scene with the layer `moving`, and how much of it is red.
        let mut picture = |model: u64, moving: Option<(u64, Option<GpuScene>)>| {
            let mut f = frame(&Scene::default(), &cam, size, model, None);
            f.moving = moving.map(|(key, scene)| crate::view::MovingLayer { key, slot: Arc::new(Mutex::new(scene)), transform });
            let drawn = capturer.picture(&mut state, &f, BACKGROUND, size).unwrap().0;
            let red = drawn.chunks(4).filter(|p| p[0] > p[1].saturating_add(60)).count();
            (drawn, red)
        };
        let (shown, red) = picture(1, Some((7, Some(body.clone()))));
        assert!(red > 50, "the layer's body shows");
        assert_eq!(picture(2, Some((7, None))).0, shown, "the same key again: the scene taken stays");
        assert_eq!(picture(3, None).1, 0, "a frame without the layer");
        assert_eq!(picture(4, Some((7, None))).1, 0, "the scene went with that frame: its key alone brings nothing back");
        assert_eq!(picture(5, Some((8, Some(body)))).0, shown, "handed over again under a new key");
    }

    /// A dragged component's parts drawn as instances (one copy of a part's triangles, placed
    /// many times) move with its layer too, lit flat or smooth: the layer built where the drag
    /// found them and moved by its transform draws what the model scene draws with the
    /// placements moved.
    #[test]
    fn a_moving_layers_instances_are_drawn_moved_by_its_transform() {
        let Ok(capturer) = Capturer::headless_single_sample(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let flat = Mesh { positions: vec![Vec3::new(-1.0, 0.0, -0.5), Vec3::new(0.2, 0.0, -0.5), Vec3::new(-0.4, 0.0, 0.6)], ..(*sheet()).clone() };
        let smooth = Mesh { normals: vec![Vec3::new(-0.2, -1.0, 0.0), Vec3::new(0.2, -1.0, 0.0), Vec3::new(0.0, -1.0, 0.2)], ..flat.clone() };
        let shift = [0.6_f32, 0.0, 0.3];
        let cam = Camera { distance: 6.0, ..Camera::default() };
        let size = [64, 64];
        let Ok(mut state) = capturer.state.lock() else { panic!("capturer state") };
        for (k, mesh) in [flat, smooth].iter().enumerate() {
            let tris = Arc::new(crate::view::SharedTris::new(mesh));
            assert_eq!(tris.flat, k == 0);
            let placed_at = |[x, y, z]: [f32; 3]| {
                let mut inst = crate::view::InstancedMesh {
                    key: 1,
                    tris: tris.clone(),
                    instances: Vec::new(),
                    look: crate::view::InstanceLook::Opaque,
                    levels: Vec::new(),
                    closed: true,
                    edges: None,
                };
                let mut sc = GpuScene::default();
                let red = sc.color([200, 30, 30, 255]);
                inst.place([[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]], red);
                sc.instanced = vec![inst];
                Arc::new(Mutex::new(Some(sc)))
            };
            let key = 10 * k as u64;
            // The model scene with the placement where it was moved to.
            let mut moved = frame(&Scene::default(), &cam, size, key + 1, None);
            moved.slot = placed_at(shift);
            let expected = capturer.picture(&mut state, &moved, BACKGROUND, size).unwrap().0;
            assert!(expected.chunks(4).filter(|p| p[0] > p[1].saturating_add(60)).count() > 50, "the part shows");
            // Placed where it was, in the layer moved by the drag; the model scene empty.
            let mut layered = frame(&Scene::default(), &cam, size, key + 2, None);
            let transform = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [shift[0], shift[1], shift[2], 1.0]];
            layered.moving = Some(crate::view::MovingLayer { key: key + 3, slot: placed_at([0.0; 3]), transform });
            let drawn = capturer.picture(&mut state, &layered, BACKGROUND, size).unwrap().0;
            let differ = expected.chunks(4).zip(drawn.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
            assert!(differ <= 4, "flat {}: {differ} pixels differ from the part placed where it was moved to", k == 0);
        }
    }

    /// A model scene of one block: for each of `bodies` (where it is built, its colour, its
    /// motion slot) a triangle facing the camera with an edge line along its base, every vertex
    /// put by `put` (its place and its normal turned alike). Its palette entries name the slot.
    fn slotted(
        bodies: &[([f32; 3], [u8; 4], u32)],
        put: &dyn Fn(usize, [f32; 3]) -> [f32; 3],
        turn: &dyn Fn(usize, [f32; 3]) -> [f32; 3],
    ) -> crate::view::SceneSlot {
        use crate::view::{BlockBuilder, SceneBlock, TriClass};
        let mut sc = GpuScene::default();
        let mut block = BlockBuilder::default();
        for (k, (at, rgba, slot)) in bodies.iter().enumerate() {
            let palette = Arc::make_mut(&mut sc.palette);
            let (fill, edge) = (palette.intern_moved(*rgba, *slot), palette.intern_moved([20, 20, 20, 255], *slot));
            let corners =
                [[-1.0, 0.0, -0.5], [0.2, 0.0, -0.5], [-0.4, 0.0, 0.6]].map(|p: [f32; 3]| put(k, [p[0] + at[0], p[1] + at[1], p[2] + at[2]]));
            block.begin_body();
            block.moved_by(*slot);
            block.triangle(corners, [turn(k, [0.0, -1.0, 0.0]); 3], fill, TriClass::Opaque);
            block.line(corners[0], corners[1], edge, 3.0);
            block.end_body();
        }
        sc.blocks.push(SceneBlock { key: 77, bytes: block.freeze() });
        Arc::new(Mutex::new(Some(sc)))
    }

    /// Bodies whose palette entries name a slot of the motion table are drawn moved by the
    /// motion a frame brings for that slot ([`crate::view::Motions`]), with nothing built again:
    /// the picture is the one of the bodies built where each was moved to, lit as they are
    /// turned, their edge lines with them. A body built outside the view and moved into it is
    /// drawn (its box is tested where it is drawn), a frame that brings a motion for one slot
    /// leaves the other bodies where they were built, and a frame that brings none draws every
    /// body where it was built (on this machine's GPU, skipped without one).
    #[test]
    fn bodies_are_drawn_where_their_motions_move_them() {
        let Ok(capturer) = Capturer::headless_single_sample(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let (red, blue) = ([200, 30, 30, 255], [30, 30, 200, 255]);
        // The red body is built far outside the view; the blue one below the middle of it.
        let built = [([-40.0, 0.0, 0.0], red, 3), ([0.3, 0.0, -1.0], blue, 5)];
        // Red: tilted 30 degrees about X, its face turned up toward the light, then shifted into the upper half of the
        // view. Blue: shifted a little.
        let (c, s) = ((-30f32).to_radians().cos(), (-30f32).to_radians().sin());
        let tilt = |p: [f32; 3]| [p[0], c * p[1] - s * p[2], s * p[1] + c * p[2]];
        let red_moves = |p: [f32; 3]| {
            let t = tilt(p);
            [t[0] + 40.4, t[1], t[2] + 0.9]
        };
        let red_rows: crate::view::MotionRows = [[1.0, 0.0, 0.0, 40.4], [0.0, c, -s, 0.0], [0.0, s, c, 0.9]];
        let blue_rows: crate::view::MotionRows = [[1.0, 0.0, 0.0, -0.5], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.1]];
        let blue_moves = |p: [f32; 3]| [p[0] - 0.5, p[1], p[2] + 0.1];
        let still = |_: usize, p: [f32; 3]| p;
        let cam = Camera { distance: 6.0, ..Camera::default() };
        let size = [64, 64];
        let reds = |rgba: &[u8]| rgba.chunks(4).filter(|p| p[0] > p[2].saturating_add(60)).count();
        let blues = |rgba: &[u8]| rgba.chunks(4).filter(|p| p[2] > p[0].saturating_add(60)).count();
        let differ =
            |a: &[u8], b: &[u8]| a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
        let Ok(mut state) = capturer.state.lock() else { panic!("capturer state") };
        let mut picture = |key: u64, slot: crate::view::SceneSlot, motions: crate::view::Motions| {
            let mut f = frame(&Scene::default(), &cam, size, key, None);
            f.slot = slot;
            f.motions = motions;
            capturer.picture(&mut state, &f, BACKGROUND, size).unwrap().0
        };
        let none = crate::view::Motions::default;
        // Both built where they are moved to, in no slot.
        let unslotted = [(built[0].0, red, 0), (built[1].0, blue, 0)];
        let expected = picture(
            1,
            slotted(&unslotted, &|k, p| if k == 0 { red_moves(p) } else { blue_moves(p) }, &|k, n| if k == 0 { tilt(n) } else { n }),
            none(),
        );
        assert!(reds(&expected) > 50 && blues(&expected) > 50, "both bodies show where they are moved to");
        // Built where they are, and moved by their slots' motions.
        let both = crate::view::Motions::new([(3, red_rows), (5, blue_rows)]);
        let moved = picture(2, slotted(&built, &still, &still), both);
        let off = differ(&expected, &moved);
        assert!(off <= 8, "{off} pixels differ from the bodies built where their motions move them");
        // The same scene, a motion for blue alone: red is where it was built, outside the view.
        let only_blue = picture(2, slotted(&built, &still, &still), crate::view::Motions::new([(5, blue_rows)]));
        assert!(reds(&only_blue) == 0 && blues(&only_blue) > 50, "a slot without a motion leaves its body where it was built");
        // No motions: blue where it was built too.
        let unmoved = picture(2, slotted(&built, &still, &still), none());
        let as_built = picture(3, slotted(&unslotted, &still, &still), none());
        assert!(reds(&unmoved) == 0 && blues(&unmoved) > 50);
        assert_eq!(differ(&as_built, &unmoved), 0, "a frame without motions draws every body where it was built");
        assert!(differ(&unmoved, &only_blue) > 20, "the motion shows");
    }

    /// A part drawn as placements of shared triangles moves with the motion its palette entry
    /// names, flat or smooth: placed outside the view, then tilted and moved into it, it is drawn
    /// (and lit) as the model scene draws it with the placement itself tilted and moved.
    #[test]
    fn a_placement_is_drawn_where_its_bodys_motion_moves_it() {
        let Ok(capturer) = Capturer::headless_single_sample(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let flat = Mesh { positions: vec![Vec3::new(-1.0, 0.0, -0.5), Vec3::new(0.2, 0.0, -0.5), Vec3::new(-0.4, 0.0, 0.6)], ..(*sheet()).clone() };
        let smooth = Mesh { normals: vec![Vec3::new(-0.2, -1.0, 0.0), Vec3::new(0.2, -1.0, 0.0), Vec3::new(0.0, -1.0, 0.2)], ..flat.clone() };
        let cam = Camera { distance: 6.0, ..Camera::default() };
        let size = [64, 64];
        let Ok(mut state) = capturer.state.lock() else { panic!("capturer state") };
        for (k, mesh) in [flat, smooth].iter().enumerate() {
            let tris = Arc::new(crate::view::SharedTris::new(mesh));
            // Tilted 30 degrees about X, its face turned up toward the light: a normal not turned would light it otherwise.
            let (c, s) = ((-30f32).to_radians().cos(), (-30f32).to_radians().sin());
            let placed = |rows: [[f32; 4]; 3], slot: u32| {
                let mut inst = crate::view::InstancedMesh {
                    key: 1,
                    tris: tris.clone(),
                    instances: Vec::new(),
                    look: crate::view::InstanceLook::Opaque,
                    levels: Vec::new(),
                    closed: true,
                    edges: None,
                };
                let mut sc = GpuScene::default();
                let red = Arc::make_mut(&mut sc.palette).intern_moved([200, 30, 30, 255], slot);
                inst.place(rows, red);
                sc.instanced = vec![inst];
                Arc::new(Mutex::new(Some(sc)))
            };
            let key = 10 * k as u64;
            let mut there = frame(&Scene::default(), &cam, size, key + 1, None);
            there.slot = placed([[1.0, 0.0, 0.0, 0.6], [0.0, c, -s, 0.0], [0.0, s, c, 0.3]], 0);
            let expected = capturer.picture(&mut state, &there, BACKGROUND, size).unwrap().0;
            assert!(expected.chunks(4).filter(|p| p[0] > p[1].saturating_add(60)).count() > 50, "the part shows");
            // Placed far outside the view, and moved into it by its slot's motion.
            let mut moved = frame(&Scene::default(), &cam, size, key + 2, None);
            moved.slot = placed([[1.0, 0.0, 0.0, -40.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]], 7);
            moved.motions = crate::view::Motions::new([(7, [[1.0, 0.0, 0.0, 40.6], [0.0, c, -s, 0.0], [0.0, s, c, 0.3]])]);
            let drawn = capturer.picture(&mut state, &moved, BACKGROUND, size).unwrap().0;
            let differ = expected.chunks(4).zip(drawn.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
            assert!(differ <= 4, "flat {}: {differ} pixels differ from the part placed where its motion moves it", k == 0);
        }
    }

    /// Copies of a part share their palette entry (#18857) and each placement names its own
    /// motion: only the copy whose own motion moves it into the view is drawn there, culled where
    /// that motion puts it, not where the shared entry's motion would. The other two copies, one
    /// moved by the entry's motion and one by a motion of its own, stay outside it.
    #[test]
    fn each_copy_of_a_part_is_drawn_where_its_own_motion_moves_it() {
        let Ok(capturer) = Capturer::headless_single_sample(crate::test_env::policy()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let mesh = Mesh { positions: vec![Vec3::new(-1.0, 0.0, -0.5), Vec3::new(0.2, 0.0, -0.5), Vec3::new(-0.4, 0.0, 0.6)], ..(*sheet()).clone() };
        let tris = Arc::new(crate::view::SharedTris::new(&mesh));
        let cam = Camera { distance: 6.0, ..Camera::default() };
        let size = [64, 64];
        let Ok(mut state) = capturer.state.lock() else { panic!("capturer state") };
        let (c, s) = ((-30f32).to_radians().cos(), (-30f32).to_radians().sin());
        let at = |x: f32| [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let scene = |placements: &[([[f32; 4]; 3], u32)]| {
            let mut inst = crate::view::InstancedMesh {
                key: 1,
                tris: tris.clone(),
                instances: Vec::new(),
                look: crate::view::InstanceLook::Opaque,
                levels: Vec::new(),
                closed: true,
                edges: None,
            };
            let mut sc = GpuScene::default();
            // The entry every copy shares, which names motion 9.
            let red = Arc::make_mut(&mut sc.palette).intern_moved([200, 30, 30, 255], 9);
            for (rows, slot) in placements {
                inst.place_moved(*rows, red, crate::view::PaletteIndex(0), crate::view::PaletteIndex(0), *slot);
            }
            sc.instanced = vec![inst];
            Arc::new(Mutex::new(Some(sc)))
        };
        let into_view = [[1.0, 0.0, 0.0, 40.6], [0.0, c, -s, 0.0], [0.0, s, c, 0.3]];
        // One copy where the moved one ends, the shared entry's motion leaving it as it is.
        let mut there = frame(&Scene::default(), &cam, size, 1, None);
        there.slot = scene(&[([[1.0, 0.0, 0.0, 0.6], [0.0, c, -s, 0.0], [0.0, s, c, 0.3]], crate::view::UNMOVED_SLOT)]);
        there.motions = crate::view::Motions::new([(9, crate::view::UNMOVED)]);
        let expected = capturer.picture(&mut state, &there, BACKGROUND, size).unwrap().0;
        assert!(expected.chunks(4).filter(|p| p[0] > p[1].saturating_add(60)).count() > 50, "the part shows");
        // Three copies far outside the view: the first moved into it by its own motion 7, the
        // second by the entry's motion 9 and the third by its own motion 8, both left outside.
        let mut moved = frame(&Scene::default(), &cam, size, 2, None);
        moved.slot = scene(&[(at(-40.0), 7), (at(40.0), crate::view::UNMOVED_SLOT), (at(-40.0), 8)]);
        moved.motions = crate::view::Motions::new([(7, into_view), (8, crate::view::UNMOVED), (9, crate::view::UNMOVED)]);
        let drawn = capturer.picture(&mut state, &moved, BACKGROUND, size).unwrap().0;
        let differ = expected.chunks(4).zip(drawn.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
        assert!(differ <= 4, "{differ} pixels differ from the one copy placed where its own motion moves it");
    }

    /// A face with a colour of its own is drawn in that colour, and takes its body's opacity:
    /// painting one face does not make the body solid again.
    #[test]
    fn a_painted_face_keeps_its_bodys_opacity() {
        let mesh = SceneMesh {
            mesh: sheet(),
            color: Rgb(200, 30, 30),
            face_colors: vec![(0, Rgb(0, 255, 0))],
            opacity: 0.5,
            closed: true,
            hatch: Default::default(),
            values: None,
        };
        let sc = scene_geometry(&Scene { meshes: vec![mesh], ..Default::default() });
        assert_eq!(sc.tris.len(), 0);
        assert_eq!(color(&sc, &sc.glass), Some([0, 255, 0, 128]), "the face's own colour at the body's alpha");
    }
}
