//! Pictures of a [`Scene`] drawn offscreen with the viewport's pipelines ([`crate::view`]): the
//! same shading, edges, silhouettes, section cap and surface analysis the viewport shows, read
//! back as RGBA. `solidcraft-cli snapshot`, MCP screenshots, `ui.render` and thumbnails draw
//! through it, on the application's device ([`Capturer::new`]) or on a headless one
//! ([`Capturer::headless`], [`shared`]). Without a qualifying adapter, the `auto` policy falls
//! to the last tier, the CPU rasterizer ([`headless_rgba`]), and says so: every picture names the
//! renderer that drew it (`gpu`, `gpu-software` or `cpu`); `hardware` and `software` fail with an
//! error that names the policy.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use solidcraft_geom::Vec3;
use solidcraft_render::{AnalysisMode, Camera, Rgb, SECTION_CAP, Scene};
use wgpu::util::DeviceExt;

use crate::view::{AccessMap, DrawStats, Frame, GpuScene, GpuTarget, Renderer, analysis_colors, analysis_uniform};
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
    device: wgpu::Device,
    queue: wgpu::Queue,
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
    renderer: Renderer,
    background: wgpu::RenderPipeline,
    /// The last picture's scene key and accessibility map version (each picture uploads anew).
    key: u64,
    access_version: u64,
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
        let renderer = Renderer::new(&device, GpuTarget { format: FORMAT, depth: Some(depth), samples, tess: false, oit: false }, None);
        let background = background_pipeline(&device, samples, depth);
        caught(scopes).map_or(Ok(()), |e| Err(GpuError::Pipeline(e)))?;
        let max_side = device.limits().max_texture_dimension_2d.min(MAX_SIDE);
        Ok(Capturer {
            adapter: adapter.get_info().name,
            software: adapter.get_info().device_type == wgpu::DeviceType::Cpu,
            samples,
            depth,
            max_side,
            state: Mutex::new(State { renderer, background, key: 0, access_version: 0 }),
            device,
            queue,
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

    /// [`Capturer::is_lost`], asking the driver first: an empty submission meets a loss the
    /// driver reports and marks it, where the picture's first upload would panic inside wgpu.
    fn probe_lost(&self) -> bool {
        if self.is_lost() {
            return true;
        }
        self.queue.submit([]);
        self.is_lost()
    }

    /// A device of its own, without a window (native only): the adapter the `SOLIDCRAFT_GPU`
    /// policy picks ([`crate::select`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless() -> Result<Self> {
        let (adapter, device, queue) = crate::open_headless()?;
        // Errors are caught per picture (error scopes); anything else is logged, never a panic.
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| log::error!("GPU: {e}")));
        let lost = watch_lost(&device);
        Ok(Self::new(&adapter, device, queue)?.watching(lost))
    }

    /// As [`Capturer::headless`], drawing with the depth format `depth` ([`Capturer::with_depth`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless_with_depth(depth: wgpu::TextureFormat) -> Result<Self> {
        let (adapter, device, queue) = crate::open_headless()?;
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| log::error!("GPU: {e}")));
        let lost = watch_lost(&device);
        Ok(Self::with_depth(&adapter, device, queue, depth)?.watching(lost))
    }

    /// As [`Capturer::headless`], without multisampling: one sample per pixel, as the viewport
    /// draws where 4× isn't available (some browsers and devices, or MSAA turned off).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless_single_sample() -> Result<Self> {
        let (adapter, device, queue) = crate::open_headless()?;
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
        self.draw(scene, highlight, cam, width, height).map(|(rgba, _)| rgba)
    }

    /// [`Capturer::rgba`] and what the picture drew ([`DrawStats`]). Both come from the same
    /// turn on the device, so another thread's picture can't put its counts in their place.
    pub fn rgba_with_stats(&self, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<(Vec<u8>, DrawStats)> {
        self.draw(scene, None, cam, width, height)
    }

    /// The picture and what it drew, both read while this call has the device to itself.
    fn draw(&self, scene: &Scene, highlight: Option<GpuScene>, cam: &Camera, width: u32, height: u32) -> Result<(Vec<u8>, DrawStats)> {
        if !(1..=self.max_side).contains(&width) || !(1..=self.max_side).contains(&height) {
            return Err(GpuError::Render(format!("a picture is 1 to {} pixels a side, not {width} × {height}", self.max_side)));
        }
        if self.probe_lost() {
            return Err(GpuError::DeviceLost);
        }
        let mut state = self.state.lock().map_err(|_| GpuError::Render("an earlier picture failed on this device".into()))?;
        let state = &mut *state;
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
        // Always a highlight scene of the picture's own, empty without one: the renderer keeps
        // its last highlight scene while the slot is empty (as the viewport does until the next
        // is built), so the previous picture's highlight would show in this one.
        frame.hl_key = state.key;
        frame.hl_slot = Arc::new(Mutex::new(Some(highlight.unwrap_or_default())));
        self.picture(state, &frame, scene.background.unwrap_or(BACKGROUND), [width, height])
    }

    /// `frame` drawn over the `background` gradient, read back as straight RGBA, and what it
    /// drew (read while the caller holds `state`).
    fn picture(&self, state: &mut State, frame: &Frame, (top, bottom): (Rgb, Rgb), [width, height]: [u32; 2]) -> Result<(Vec<u8>, DrawStats)> {
        let (device, queue) = (&self.device, &self.queue);
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
        let background = background_vertices(device, top, bottom);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_capture") });
        state.renderer.prepare(device, queue, &mut encoder, frame);
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
            pass.set_pipeline(&state.background);
            pass.set_vertex_buffer(0, background.slice(..));
            pass.draw(0..6, 0..1);
            state.renderer.paint(frame, &mut pass, [0.0, 0.0, width as f32, height as f32]);
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
        let stats = state.renderer.stats().lock().map(|s| *s).unwrap_or_default();
        Ok((rgba, stats))
    }

    /// [`Capturer::rgba`] as PNG bytes.
    pub fn png(&self, scene: &Scene, cam: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        let rgba = self.rgba(scene, cam, width, height)?;
        solidcraft_render::png(width as usize, height as usize, rgba).ok_or_else(|| GpuError::Render("PNG encoding failed".into()))
    }
}

/// The process's headless capturer (native only), opened on first use under the `SOLIDCRAFT_GPU`
/// policy and kept; the error when no adapter qualifies or the device fails (not retried).
#[cfg(not(target_arch = "wasm32"))]
pub fn shared() -> std::result::Result<&'static Capturer, &'static GpuError> {
    static SHARED: std::sync::OnceLock<Result<Capturer>> = std::sync::OnceLock::new();
    SHARED.get_or_init(Capturer::headless).as_ref()
}

/// What draws this process's headless pictures (native only): the shared headless device, or the
/// CPU rasterizer where the `SOLIDCRAFT_GPU` policy lands there (`cpu`, or `auto` with no
/// qualifying adapter).
#[cfg(not(target_arch = "wasm32"))]
pub enum Headless {
    Gpu(&'static Capturer),
    Cpu,
}

/// [`Headless`] under the `SOLIDCRAFT_GPU` policy; an error when the policy is `hardware` or
/// `software` and no adapter of that tier opens (or the variable names no policy).
#[cfg(not(target_arch = "wasm32"))]
pub fn headless() -> std::result::Result<Headless, String> {
    use crate::select::Policy;
    let policy = Policy::from_env()?;
    if policy == Policy::Cpu {
        return Ok(Headless::Cpu);
    }
    match shared() {
        Ok(c) => Ok(Headless::Gpu(c)),
        Err(e) if policy == Policy::Auto => {
            log::warn!("pictures are drawn by the CPU rasterizer: {e}");
            Ok(Headless::Cpu)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// `scene` drawn by [`headless`]: straight RGBA and the renderer that drew it (`gpu`,
/// `gpu-software` or `cpu`). The CPU rasterizer draws no surface analysis, highlights or legends.
#[cfg(not(target_arch = "wasm32"))]
pub fn headless_rgba(scene: &Scene, cam: &Camera, width: u32, height: u32) -> std::result::Result<(Vec<u8>, &'static str), String> {
    match headless()? {
        Headless::Gpu(c) => c.rgba(scene, cam, width, height).map(|rgba| (rgba, c.renderer())).map_err(|e| e.to_string()),
        Headless::Cpu => {
            if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
                return Err(format!("a picture is 1 to {MAX_SIDE} pixels a side, not {width} × {height}"));
            }
            let canvas = solidcraft_render::render(scene, cam, width as usize, height as usize);
            Ok((canvas.rgba, "cpu"))
        }
    }
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
        view_proj: cam.view_proj(f64::from(width) / f64::from(height.max(1)), scene.radius.max(1.0)).to_f32(),
        back: cam.back().to_f32(),
        size_px: [width as f32, height as f32],
        clip: scene.clip.map_or([0.0; 4], |(n, d)| [n.x as f32, n.y as f32, n.z as f32, d as f32]),
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
        sky: [0.0; 4],
        ssaa: 1.0,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        moving: None,
    }
}

/// A scene's meshes and lines as the viewport's model scene: faces in their colours (smooth
/// normals; a face normal where a vertex has none), lines as wide lines (over everything when
/// `on_top`), the grid under everything. A body whose appearance is see-through
/// ([`solidcraft_render::SceneMesh::glass`]) goes into the scene's glass, drawn as the viewport
/// draws glass: far sides first, then near sides, so what lies inside it shows through.
pub fn scene_geometry(scene: &Scene) -> GpuScene {
    let mut sc = GpuScene::default();
    for m in &scene.meshes {
        let alpha = (m.alpha() * 255.0).round().clamp(0.0, 255.0) as u8;
        let c4 = |c: Rgb| [c.0, c.1, c.2, alpha];
        let faces: HashMap<u32, Rgb> = m.face_colors.iter().copied().collect();
        let mesh = &m.mesh;
        for (ti, t) in mesh.triangles.iter().enumerate() {
            let col = sc.color(c4(mesh.tri_face.get(ti).and_then(|f| faces.get(f)).copied().unwrap_or(m.color)));
            let [Some(a), Some(b), Some(c)] = t.map(|k| mesh.positions.get(k as usize).copied()) else { continue };
            let flat = (b - a).cross(c - a).normalized().unwrap_or(Vec3::Z);
            for (k, p) in t.iter().zip([a, b, c]) {
                let n = mesh.normals.get(*k as usize).copied().filter(|n| n.is_finite() && n.len() > 0.5).unwrap_or(flat);
                if m.glass() {
                    sc.glass_tri(p.to_f32(), n.to_f32(), col);
                } else {
                    sc.tri(p.to_f32(), n.to_f32(), col);
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

fn background_vertices(device: &wgpu::Device, top: Rgb, bottom: Rgb) -> wgpu::Buffer {
    let mut bytes = Vec::with_capacity(6 * BACKGROUND_VERTEX as usize);
    let corner = |x: f32, y: f32| (x, y, if y > 0.0 { top } else { bottom });
    for (x, y, c) in [corner(-1.0, 1.0), corner(1.0, 1.0), corner(1.0, -1.0), corner(-1.0, 1.0), corner(1.0, -1.0), corner(-1.0, -1.0)] {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
        bytes.extend_from_slice(&[c.0, c.1, c.2, 255]);
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sc_capture_background"),
        contents: &bytes,
        usage: wgpu::BufferUsages::VERTEX,
    })
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
    use crate::view::TRI_SIZE;
    use solidcraft_geom::Mesh;
    use solidcraft_render::SceneMesh;
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
        let index = bytes.get(TRI_SIZE - 4..TRI_SIZE)?.try_into().ok().map(u32::from_le_bytes)?;
        scene.palette.get(crate::view::PaletteIndex(index))
    }

    /// A body the design draws see-through goes into the scene's glass, at its own alpha, and
    /// not among the solid triangles: that is how the viewport draws glass, and it is why a
    /// headless picture of a translucent target shows the tool inside it.
    #[test]
    fn a_see_through_body_is_glass_at_its_own_alpha_and_a_solid_one_is_not() {
        let mesh = |opacity: f32| SceneMesh { mesh: sheet(), color: Rgb(200, 30, 30), face_colors: Vec::new(), opacity };
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

    /// Bodies dragged as a layer of their own ([`crate::view::MovingLayer`]) are drawn moved by
    /// its transform: the picture is the one of the bodies built where they were moved to, and
    /// the bodies go back to the model scene when the drag ends (on this machine's GPU, skipped
    /// without one).
    #[test]
    fn a_moving_layer_is_drawn_moved_by_its_transform() {
        let Ok(capturer) = Capturer::headless_single_sample() else {
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
            Scene {
                meshes: vec![SceneMesh { mesh: Arc::new(mesh), color: Rgb(200, 30, 30), face_colors: Vec::new(), opacity: 1.0 }],
                ..Default::default()
            }
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

    /// A dragged component's parts drawn as instances (one copy of a part's triangles, placed
    /// many times) move with its layer too, lit flat or smooth: the layer built where the drag
    /// found them and moved by its transform draws what the model scene draws with the
    /// placements moved.
    #[test]
    fn a_moving_layers_instances_are_drawn_moved_by_its_transform() {
        let Ok(capturer) = Capturer::headless_single_sample() else {
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

    /// A face with a colour of its own is drawn in that colour, and takes its body's opacity:
    /// painting one face does not make the body solid again.
    #[test]
    fn a_painted_face_keeps_its_bodys_opacity() {
        let mesh = SceneMesh { mesh: sheet(), color: Rgb(200, 30, 30), face_colors: vec![(0, Rgb(0, 255, 0))], opacity: 0.5 };
        let sc = scene_geometry(&Scene { meshes: vec![mesh], ..Default::default() });
        assert_eq!(sc.tris.len(), 0);
        assert_eq!(color(&sc, &sc.glass), Some([0, 255, 0, 128]), "the face's own colour at the body's alpha");
    }
}
