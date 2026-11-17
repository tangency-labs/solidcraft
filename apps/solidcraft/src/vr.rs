//! VR (#2172, docs/vr.md): the design as a thing in the room, held, turned and scaled with the
//! controllers. Asked for with `--vr`, `SOLIDCRAFT_VR=1` or Preferences › XR ([`wanted`]),
//! because the runtime creates the Vulkan device the window then draws with. Off, none of it runs.
//!
//! The window on the desktop goes on as ever, so whoever sits at it sees and drives the same
//! design.
//!
//! How a frame is put together: the model is drawn for each eye from the scenes the window's
//! design view uploaded this frame (`Renderer::draw_second_view`), over a backdrop and a floor
//! fixed in the room, and the controllers' marks over that. The pictures go to the runtime as the
//! room's layer.

use eframe::egui_wgpu::{self, wgpu};
use solidcraft_ui_egui::SolveApp;
use solidcraft_ui_egui::stereo::Source;
use solidcraft_viewport_gpu::view::{Renderer, ViewTarget};
use solidcraft_vr::grab::WorldGrab;
use solidcraft_vr::marks::{self, Style, Vertex};
use solidcraft_vr::room::{self, Transform};
use solidcraft_xr::input::{Button, Hand, HandInput, Pulse};
use solidcraft_xr::view::{self, Eye, Pose};

/// Connect to the VR runtime, open the session with the controllers, and change `options` so
/// that the window opens on the device the runtime made: the same features and limits the window
/// would have asked of its own device, on Vulkan. On an error `options` is as it was.
pub fn open(options: &mut egui_wgpu::WgpuConfiguration) -> Result<Vr, solidcraft_xr::Error> {
    let egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_setup else {
        return Err(solidcraft_xr::Error::Unsupported("the window already has a graphics device".into()));
    };
    let describe = setup.device_descriptor.clone();
    let runtime = solidcraft_xr::Runtime::connect()?;
    let (name, headset) = (runtime.name.clone(), runtime.headset.clone());
    let (mut session, gpu) = runtime.open(&|adapter| describe(adapter))?;
    // Without controllers the model is still shown; the log says why they are missing.
    if let Err(e) = session.attach_controls() {
        log::warn!("VR: no controllers: {e}");
        solidcraft_engine::note!("SolidCraft VR: no controllers: {e}");
    }
    options.wgpu_setup = egui_wgpu::WgpuSetup::Existing(egui_wgpu::WgpuSetupExisting {
        instance: gpu.instance,
        adapter: gpu.adapter,
        device: gpu.device,
        queue: gpu.queue,
    });
    // The headset's frames set the pace; waiting for the monitor's too would halve it.
    options.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    Ok(Vr { session: Some(session), runtime: name, headset, ..Vr::blank(Status::Waiting) })
}

/// VR for one run.
pub struct Vr {
    /// The session, until it ends. Dropped before the window's device (`App` goes first).
    session: Option<solidcraft_xr::Session>,
    /// What draws the session's pictures, made once the viewport's pipelines are in use.
    gpu: Option<Gpu>,
    runtime: String,
    headset: String,
    status: Status,
    /// Frames were shown in the headset at some point of this run.
    ran: bool,
    /// The status last put in Preferences and on the status line, and the language it was put
    /// in (Preferences shows it again in another one when the language changes).
    told: Option<(Status, String)>,
    /// The last design view the window drew: the headset goes on showing it while the window is
    /// hidden (minimized, covered), when the window draws no UI.
    source: Option<Source>,
    /// A stick click asked for the model to be put back: kept until it can be (a frame with a
    /// design view and a tracked head).
    put_back: bool,
    /// Where the model is in the room: `None` until there is a model and a tracked head to place
    /// it in front of.
    transform: Option<Transform>,
    grab: WorldGrab,
    /// Each hand's grip, trigger and stick click, left first.
    buttons: [[Button; 3]; 2],
    /// The model was at real size last frame (a tick is felt when a grab snaps it there).
    real_size: bool,
    /// The design the model was placed for (`Session::uid`): another one opened or switched to is
    /// put in front again.
    design: Option<u64>,
}

impl Vr {
    /// VR that did not start: it only says why, in Preferences and once on the status line.
    pub fn failed(reason: Reason) -> Vr {
        Vr::blank(Status::Failed(reason))
    }

    /// VR not asked for: Preferences says whether OpenXR is there to turn it on with.
    pub fn off(found: solidcraft_xr::Found) -> Vr {
        Vr::blank(Status::Off(found))
    }

    /// VR with no session, nothing placed and nothing held, at `status`.
    fn blank(status: Status) -> Vr {
        Vr {
            session: None,
            gpu: None,
            runtime: String::new(),
            headset: String::new(),
            status,
            ran: false,
            told: None,
            source: None,
            put_back: false,
            transform: None,
            grab: WorldGrab::default(),
            buttons: Default::default(),
            real_size: false,
            design: None,
        }
    }

    /// Whether a session is open: VR asked for and started (it may still wait for the headset).
    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    /// What VR is doing.
    #[cfg(test)]
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The status in the user's language.
    fn status_text(&self) -> String {
        use solidcraft_ui_text::{format, tr};
        let names = [("runtime", self.runtime.as_str()), ("headset", self.headset.as_str())];
        match &self.status {
            Status::Waiting => format("vr.status_waiting", &names),
            Status::Running => format("vr.status_running", &names),
            Status::Ended => tr("vr.status_ended").to_string(),
            Status::Off(found) => off_text(*found),
            Status::Failed(reason) if self.ran => format("vr.status_failed_draw", &[("reason", &reason.text())]),
            Status::Failed(reason) => format("vr.status_failed", &[("reason", &reason.text())]),
        }
    }

    /// One frame of the headset, at the headset's pace, from the window's logic (which runs also
    /// while the window is hidden; `ui_ran`: the window drew its UI since the last call, so this
    /// frame's design view is the one to show). Never fails: a session that does ends, says so,
    /// and leaves the window drawing as it always has.
    pub fn frame(&mut self, app: &mut SolveApp, ctx: &egui::Context, rs: &egui_wgpu::RenderState, ui_ran: bool) {
        if self.session.is_some() {
            if let Err(error) = self.step(app, ctx, rs, ui_ran) {
                log::warn!("VR: {error}");
                solidcraft_engine::note!("SolidCraft VR: stopped: {error}");
                self.status = Status::Failed(Reason::of(&error));
                self.close(app);
            }
        } else {
            app.viewport.stereo.on = false;
        }
        let told = (self.status.clone(), app.preferences.language.clone());
        if self.told.as_ref() != Some(&told) {
            let _language = app.prepare_text(ctx);
            let text = self.status_text();
            // The status line once, when the status changes; Preferences in every language.
            if self.told.as_ref().is_none_or(|(status, _)| *status != self.status) && matches!(self.status, Status::Failed(_) | Status::Ended) {
                app.set_status(text.clone(), matches!(self.status, Status::Failed(_)));
            }
            app.services.vr_status = Some(text);
            self.told = Some(told);
        }
    }

    /// Let go of the session and everything made for it; the window draws on as ever.
    fn close(&mut self, app: &mut SolveApp) {
        app.viewport.stereo.on = false;
        self.gpu = None;
        self.session = None;
        self.source = None;
        self.grab.release();
        if self.status == Status::Running || self.status == Status::Waiting {
            self.status = Status::Ended;
        }
    }

    /// The app is closing: the session is asked to end (for at most half a second, so a runtime
    /// that does not answer cannot hold the app open), then goes before the window's device does.
    pub fn shut_down(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.request_exit();
            let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while std::time::Instant::now() < until {
                match session.poll() {
                    Ok(solidcraft_xr::Status::Closed) | Err(_) => break,
                    Ok(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            }
        }
        self.gpu = None;
        self.session = None;
    }

    /// One frame of the session, or why it cannot go on.
    fn step(&mut self, app: &mut SolveApp, ctx: &egui::Context, rs: &egui_wgpu::RenderState, ui_ran: bool) -> Result<(), solidcraft_xr::Error> {
        let Some(session) = self.session.as_mut() else { return Ok(()) };
        // The viewport's pipelines could not be built: there is nothing to show the model with.
        if app.viewport.gpu_unavailable {
            return Err(solidcraft_xr::Error::Unsupported("the 3D view could not be set up on this graphics card".into()));
        }
        // Not before the viewport's pipelines are in use: they compile on a thread of their own,
        // and the runtime shares the device's queue from its first frame.
        if app.viewport.gpu.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return Ok(());
        }
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(rs, session.format()));
        match session.poll()? {
            solidcraft_xr::Status::Closed => {
                solidcraft_engine::note!("SolidCraft VR: the session ended");
                self.status = Status::Ended;
                self.close(app);
                return Ok(());
            }
            solidcraft_xr::Status::Waiting => {
                app.viewport.stereo.on = false;
                self.status = Status::Waiting;
                self.grab.release();
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return Ok(());
            }
            solidcraft_xr::Status::Running => {}
        }
        self.status = Status::Running;
        self.ran = true;
        app.viewport.stereo.on = true;
        // This frame's design view; while the window is hidden it draws none, and the last one
        // stays. Once the window draws again, a frame without one (the start page) has none.
        if ui_ran {
            self.source = app.viewport.stereo.take();
        }
        let source = self.source.as_ref();
        // Waits for the headset's next frame: this is what paces the window while VR runs.
        let Some(frame) = session.begin_frame()? else { return Ok(()) };
        let hands = session.hands(&frame);
        for hand in Hand::BOTH {
            let [grip, trigger, click] = &mut self.buttons[hand.index()];
            let input = &hands[hand.index()];
            *grip = grip.next(input.squeeze);
            *trigger = trigger.next(input.trigger);
            *click = click.next(if input.stick_click { 1.0 } else { 0.0 });
            if grip.pressed || click.pressed {
                session.pulse(hand, Pulse::GRAB);
            }
        }
        // A stick click puts the model back in front, at its first size; so do the first frame
        // with a model and a tracked head, another design opened or switched to, and the VR
        // software recentring the room. Asked for, it waits for a frame where it can be done.
        self.put_back |= self.buttons.iter().any(|[_, _, click]| click.pressed) || session.recentred();
        let design = app.session.uid;
        if (self.transform.is_none() || self.put_back || self.design != Some(design))
            && let (Some(source), Some(eyes)) = (source, &frame.eyes)
            && let Some(placed) = first_placement(source, eyes, model_box(app))
        {
            self.grab.release();
            self.transform = Some(placed);
            self.design = Some(design);
            self.put_back = false;
        }
        if let Some(current) = &self.transform {
            let grips = Hand::BOTH.map(|hand| {
                let i = hand.index();
                hands[i].grip.filter(|_| self.buttons[i][0].down)
            });
            self.transform = Some(self.grab.update(current, grips));
        }
        // Snapped to real size by a grab: a tick in both hands.
        let real_size = self.transform.is_some_and(|t| t.scale == room::REAL_SIZE);
        if real_size && !self.real_size && self.grab.holding() {
            for hand in Hand::BOTH {
                session.pulse(hand, Pulse::TICK);
            }
        }
        self.real_size = real_size;
        let held = self.buttons.map(|[grip, trigger, _]| [grip.down, trigger.down]);
        let floor = session.floor_height(&frame);
        let scene = Scene { source, transform: self.transform.as_ref(), hands: &hands, held, floor };
        let drawn = if frame.should_render { gpu.draw(session, rs, &frame, &scene) } else { Ok(false) };
        // A begun frame is ended whatever happened in it.
        let ended = session.end_frame(&frame, *drawn.as_ref().unwrap_or(&false));
        ctx.request_repaint();
        drawn.map(|_| ()).and(ended)
    }
}

/// The room direction `room` (a unit vector) in the model's space, as `transform` turns it:
/// the light comes from there, so faces toward the head are lit whichever way the model is held.
fn model_direction(transform: &Transform, room: [f64; 3]) -> [f32; 3] {
    let dot = |a: [f64; 3]| a[0] * room[0] + a[1] * room[1] + a[2] * room[2];
    transform.axes.map(|axis| dot(axis) as f32)
}

/// The model's box (least and greatest corner, model units), or `None` for an empty design.
fn model_box(app: &SolveApp) -> Option<[[f64; 3]; 2]> {
    let model = solidcraft_ui_egui::viewport::model_bounds(app);
    (!model.is_empty()).then_some([[model.min.x, model.min.y, model.min.z], [model.max.x, model.max.y, model.max.z]])
}

/// Where the model first appears: in front of the head, upright, the side the window's camera
/// shows toward the user, `bounds` (the model's box, model units) across. `None` while there is
/// no model to go by (an empty design) or the head is not where a pose can be.
fn first_placement(source: &Source, eyes: &[Eye; 2], bounds: Option<[[f64; 3]; 2]>) -> Option<Transform> {
    let bounds = bounds?;
    let [left, right] = eyes;
    let position = [0, 1, 2].map(|k| (left.pose.position[k] + right.pose.position[k]) * 0.5);
    let head = Pose { position, orientation: left.pose.orientation };
    let up = source.camera.up_dir();
    let back = source.camera.back();
    room::first_transform(Some(bounds), [up.x, up.y, up.z], [back.x, back.y, back.z], &head)
}

/// What a frame shows in the headset.
#[derive(Clone, Copy)]
struct Scene<'a> {
    /// This frame's design view, if one was drawn.
    source: Option<&'a Source>,
    /// Where the model is in the room, once placed.
    transform: Option<&'a Transform>,
    hands: &'a [HandInput; 2],
    /// Each hand's grip and trigger held, left first.
    held: [[bool; 2]; 2],
    /// The floor's height in the room, where the runtime knows it: a grid is drawn there.
    floor: Option<f64>,
}

/// An eye's backdrop and the model over it, and the controllers' marks. The model's picture is
/// in the window's format, where colours may be stored as the screen shows them; the runtime's
/// pictures take linear light.
const SHADER: &str = r#"
struct U {
    top: vec4<f32>,
    bottom: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    back: vec4<f32>,
    // The tangents of the eye's field of view: left, right, up, down.
    tangents: vec4<f32>,
    // x: the model's colours are stored as the screen shows them (1) or as linear light (0).
    // y: there is a model picture (1) or only the backdrop (0).
    // z: the floor's height is known (1): the floor is drawn.
    flags: vec4<f32>,
    // The eye in the room (xyz) and the floor's height (w).
    eye: vec4<f32>,
};
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: U;

struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> O {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: O;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// The floor under the user, fixed in the room: a grid of half-metre squares fading with the
// distance, over `background` (a colour as the screen shows it) where `ray` looks down at it.
fn on_floor(background: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    let height = u.eye.y - u.eye.w;
    let down = min(ray.y, -1e-4);
    let distance = height / -down;
    let at = (u.eye.xyz + ray * distance).xz / 0.5;
    let width = max(fwidth(at), vec2<f32>(1e-4));
    let cell = abs(fract(at - 0.5) - 0.5) / width;
    let line = 1.0 - min(min(cell.x, cell.y), 1.0);
    let seen = select(0.0, 1.0, ray.y < -1e-3 && height > 0.0 && u.flags.z > 0.5);
    let fade = exp(-distance * 0.12) * seen;
    let ground = mix(background, u.bottom.rgb * 0.75, 0.6 * fade);
    return mix(ground, u.top.rgb, line * 0.5 * fade);
}

// The room's backdrop, fixed in the room (the view's background colours from below to above),
// the floor, and the model's picture (colours times alpha) over them.
@fragment
fn fs_eye(i: O) -> @location(0) vec4<f32> {
    let tx = mix(u.tangents.x, u.tangents.y, i.uv.x);
    let ty = mix(u.tangents.z, u.tangents.w, i.uv.y);
    let ray = normalize(u.right.xyz * tx + u.up.xyz * ty - u.back.xyz);
    let background = on_floor(mix(u.bottom.rgb, u.top.rgb, smoothstep(-0.6, 0.6, ray.y)), ray);
    var model = vec4<f32>(0.0);
    if (u.flags.y > 0.5) {
        model = textureSampleLevel(src, smp, i.uv, 0.0);
    }
    if (u.flags.x > 0.5) {
        return vec4<f32>(to_linear(model.rgb + background * (1.0 - model.a)), 1.0);
    }
    return vec4<f32>(model.rgb + to_linear(background) * (1.0 - model.a), 1.0);
}

// The controllers' marks: triangles in the room, seen by this eye.
struct M { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> m: M;

struct P { @builtin(position) pos: vec4<f32>, @location(0) colour: vec4<f32> };

@vertex
fn vs_mark(@location(0) at: vec3<f32>, @location(1) colour: vec4<f32>) -> P {
    var o: P;
    o.pos = m.view_proj * vec4<f32>(at, 1.0);
    o.colour = colour;
    return o;
}

@fragment
fn fs_mark(i: P) -> @location(0) vec4<f32> {
    return i.colour;
}
"#;

/// The eye uniform's size in bytes: eight `vec4<f32>`.
const UNIFORM_SIZE: u64 = 8 * 16;

/// The marks' uniform: one `mat4x4<f32>`.
const MARK_UNIFORM_SIZE: u64 = 64;

/// A mark vertex: its place (3 × f32), then its colour (4 × f32).
const MARK_VERTEX: u64 = 28;

/// What draws the session's pictures on the window's device.
struct Gpu {
    eye: wgpu::RenderPipeline,
    marks: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    mark_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    mark_uniform: wgpu::Buffer,
    /// Room for [`marks::MAX_VERTICES`].
    mark_vertices: wgpu::Buffer,
    /// A stand-in to bind while there is no model picture.
    blank: wgpu::TextureView,
    /// The model as one eye sees it, drawn into for each eye in turn.
    model: Option<ViewTarget>,
    /// The GPU refused the model's picture: it is asked again after this many more tries.
    model_wait: u32,
    /// The window's format stores colours as the screen shows them (not an sRGB format).
    encoded: bool,
}

/// After the GPU refused the model's picture, this many tries go by before it is asked again.
const RETRY_AFTER: u32 = 240;

impl Gpu {
    /// The pipelines for pictures in the session's `format`. Pictures of the model off the
    /// window's thread (thumbnails, file previews) are drawn by the CPU from here on: the runtime
    /// shares the device's queue during a frame, and Vulkan lets only one thread at a time use it.
    fn new(rs: &egui_wgpu::RenderState, format: wgpu::TextureFormat) -> Gpu {
        let device = &rs.device;
        solidcraft_ui_egui::gpu::install_cpu_picture_renderer();
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_vr"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let uniform_entry = |binding, visibility, size| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_vr"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT, UNIFORM_SIZE),
            ],
        });
        let mark_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_vr_marks"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX, MARK_UNIFORM_SIZE)],
        });
        let eye_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_vr"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let marks_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_vr_marks"),
            bind_group_layouts: &[Some(&mark_layout)],
            immediate_size: 0,
        });
        let target = [Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let eye = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_vr_eye"),
            layout: Some(&eye_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_eye"),
                targets: &target,
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        const MARK_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let marks = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_vr_marks"),
            layout: Some(&marks_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_mark"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: MARK_VERTEX,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &MARK_ATTRIBUTES,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_mark"),
                targets: &target,
                compilation_options: Default::default(),
            }),
            // Each mark is a box wound counter-clockwise from outside: its near faces show
            // without a depth buffer.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), front_face: wgpu::FrontFace::Ccw, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_vr"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let buffer =
            |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        let uniform = buffer("sc_vr", UNIFORM_SIZE, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let mark_uniform = buffer("sc_vr_marks", MARK_UNIFORM_SIZE, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let mark_vertices =
            buffer("sc_vr_marks", marks::MAX_VERTICES as u64 * MARK_VERTEX, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
        let mark_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_vr_marks"),
            layout: &mark_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: mark_uniform.as_entire_binding() }],
        });
        let blank = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_vr_blank"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        Gpu {
            eye,
            marks,
            layout,
            mark_bind,
            sampler,
            uniform,
            mark_uniform,
            mark_vertices,
            blank,
            model: None,
            model_wait: 0,
            encoded: !rs.target_format.is_srgb(),
        }
    }

    /// Draw both eyes' pictures for this frame. `true` when both were drawn (only a pair is
    /// shown: one eye without the other is worse than nothing). Each picture is submitted before
    /// this returns.
    fn draw(
        &mut self,
        session: &mut solidcraft_xr::Session,
        rs: &egui_wgpu::RenderState,
        frame: &solidcraft_xr::Frame,
        scene: &Scene<'_>,
    ) -> Result<bool, solidcraft_xr::Error> {
        let Some(eyes) = &frame.eyes else { return Ok(false) };
        let t = solidcraft_theme::Tokens::get();
        let linear = |c: egui::Color32| {
            let l = egui::Rgba::from(c);
            [l.r(), l.g(), l.b()]
        };
        let style = Style { body: linear(t.app_bar), body_held: linear(t.form_primary), ray: linear(t.form_primary), ray_pressed: linear(t.accent) };
        let vertices = marks::marks(scene.hands, scene.held, &style);
        let count = vertices.len().min(marks::MAX_VERTICES);
        if count > 0 {
            rs.queue.write_buffer(&self.mark_vertices, 0, &mark_bytes(&vertices[..count]));
        }
        let background = scene.source.map_or([t.viewport_top, t.viewport_bottom], |s| s.background);
        let placement = scene.transform.map(Transform::placement);
        // The light and the glass order follow the head (one direction for both eyes, so they
        // agree), as the window's follow its camera.
        let back = scene.transform.zip(eyes[0].pose.axes()).map(|(t, [_, _, back])| model_direction(t, back));
        let mut pictures = 0;
        for (index, eye) in eyes.iter().enumerate() {
            let size = session.eye_size(index);
            let model = match (scene.source, placement.as_ref(), back) {
                (Some(source), Some(placement), Some(back)) => self.draw_placed(rs, source, eye, placement, back, size),
                _ => false,
            };
            let (Some(axes), Some(room)) = (eye.pose.axes(), view::room_view_proj(eye)) else { continue };
            let picture = session.eye_picture(index)?;
            let fov = [eye.fov.left, eye.fov.right, eye.fov.up, eye.fov.down];
            let uniform = eye_uniform(background, axes, fov, self.encoded, model, eye.pose.position, scene.floor);
            rs.queue.write_buffer(&self.uniform, 0, &uniform);
            rs.queue.write_buffer(&self.mark_uniform, 0, &matrix_bytes(&view::to_f32(&room)));
            let model_picture = self.model.as_ref().filter(|_| model).map_or(&self.blank, ViewTarget::picture);
            let bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sc_vr"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(model_picture) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.uniform.as_entire_binding() },
                ],
            });
            let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_vr") });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_vr"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &picture.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, picture.size[0] as f32, picture.size[1] as f32, 0.0, 1.0);
                pass.set_pipeline(&self.eye);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
                if count > 0 {
                    pass.set_pipeline(&self.marks);
                    pass.set_bind_group(0, &self.mark_bind, &[]);
                    pass.set_vertex_buffer(0, self.mark_vertices.slice(..));
                    pass.draw(0..count as u32, 0..1);
                }
            }
            rs.queue.submit([encoder.finish()]);
            pictures += 1;
        }
        Ok(pictures == eyes.len())
    }

    /// Draw the model placed in the room as `eye` sees it into [`Gpu::model`], lit from `back`
    /// (toward the head, in the model's space), at the `size` of the eye's picture. `false` when
    /// nothing was drawn.
    fn draw_placed(
        &mut self,
        rs: &egui_wgpu::RenderState,
        source: &Source,
        eye: &Eye,
        placement: &view::Placement,
        back: [f32; 3],
        size: [u32; 2],
    ) -> bool {
        let Some(seen) = view::eye_view(eye, placement) else { return false };
        let mut frame = source.frame.clone();
        frame.back = back;
        frame.view_proj = view::to_f32(&seen.view_proj);
        frame.turned_depth = Some(view::turned_depth_f32(&seen.view_proj));
        frame.eye = [seen.eye[0] as f32, seen.eye[1] as f32, seen.eye[2] as f32, 1.0];
        frame.size_px = [size[0] as f32, size[1] as f32];
        if self.model_wait > 0 {
            self.model_wait -= 1;
            return false;
        }
        let mut renderer = rs.renderer.write();
        let Some(view) = renderer.callback_resources.get_mut::<Renderer>() else { return false };
        // Without the design's own grid and ground shadow: those move with the model, and the
        // room has its floor.
        let drawn = view.draw_second_view(&rs.device, &rs.queue, &frame, size, [0, 0, size[0], size[1]], false, &mut self.model);
        // No picture to draw into: the GPU refused it (the renderer has said so in the log).
        if !drawn && self.model.is_none() {
            self.model_wait = RETRY_AFTER;
        }
        drawn
    }
}

/// The eye shader's uniform: the backdrop's colours (as the screen shows them), the eye's right,
/// up and back directions in the room, its field of view's tangents, the flags, and where the eye
/// and the floor are (`floor`: none where the runtime does not know; a height that is not a
/// number is none too).
fn eye_uniform(
    background: [egui::Color32; 2],
    axes: [[f64; 3]; 3],
    fov: [f64; 4],
    encoded: bool,
    model: bool,
    eye: [f64; 3],
    floor: Option<f64>,
) -> Vec<u8> {
    let floor = floor.filter(|h| h.is_finite());
    let colour = |c: egui::Color32| {
        let [r, g, b, _] = c.to_normalized_gamma_f32();
        [r, g, b]
    };
    let [top, bottom] = background.map(colour);
    let direction = |a: [f64; 3]| [a[0] as f32, a[1] as f32, a[2] as f32, 0.0];
    let tangents = fov.map(|angle| angle.tan() as f32);
    let rows: [[f32; 4]; 8] = [
        [top[0], top[1], top[2], 1.0],
        [bottom[0], bottom[1], bottom[2], 1.0],
        direction(axes[0]),
        direction(axes[1]),
        direction(axes[2]),
        // Left, right, up, down; the picture's top row is the up tangent.
        [tangents[0], tangents[1], tangents[2], tangents[3]],
        [if encoded { 1.0 } else { 0.0 }, if model { 1.0 } else { 0.0 }, if floor.is_some() { 1.0 } else { 0.0 }, 0.0],
        [eye[0] as f32, eye[1] as f32, eye[2] as f32, floor.unwrap_or(0.0) as f32],
    ];
    rows.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// A matrix as the shader takes it: its columns one after another.
fn matrix_bytes(m: &[[f32; 4]; 4]) -> Vec<u8> {
    m.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// The marks' vertices as the shader takes them.
fn mark_bytes(vertices: &[Vertex]) -> Vec<u8> {
    vertices.iter().flat_map(|v| v.position.into_iter().chain(v.colour)).flat_map(|v| v.to_le_bytes()).collect()
}

/// The environment variable that asks for VR for one run (`1`, `true`, `on`, `yes`).
pub const VR_ENV: &str = "SOLIDCRAFT_VR";

/// Whether a start should try VR: the command line's `--vr`, else `SOLIDCRAFT_VR`, else
/// `graphics.json`'s `vr` (Preferences › XR › Start in VR). The environment can also say no (`0`, `off`)
/// to a saved yes. Anything else it holds is ignored, and the saved choice stands.
pub fn wanted(flag: bool, env: Option<&str>, saved: Option<&serde_json::Value>) -> bool {
    if flag {
        return true;
    }
    match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("1" | "true" | "on" | "yes") => return true,
        Some("0" | "false" | "off" | "no") => return false,
        _ => {}
    }
    saved.and_then(|v| v.get("vr")).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

/// What VR is doing, for Preferences › XR and the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not asked for in this run: what was found of OpenXR, without starting any VR software.
    Off(solidcraft_xr::Found),
    /// The session is open and the runtime is not showing it yet (the headset is not worn).
    Waiting,
    /// Frames are shown in the headset.
    Running,
    /// The session is over for this run.
    Ended,
    /// It never started, or a frame failed: why.
    Failed(Reason),
}

/// What Preferences says in a run without VR, in the user's language.
fn off_text(found: solidcraft_xr::Found) -> String {
    use solidcraft_ui_text::{format, tr};
    match off_words(found) {
        (text, None) => tr(text).to_string(),
        (text, Some(reason)) => format(text, &[("reason", tr(reason))]),
    }
}

/// The catalogue entries for a run without VR: the sentence, and what is missing in it.
fn off_words(found: solidcraft_xr::Found) -> (&'static str, Option<&'static str>) {
    use solidcraft_xr::Found;
    match found {
        Found::Ready => ("vr.status_off_found", None),
        Found::NoLoader => ("vr.status_not_found", Some("vr.reason_no_loader")),
        Found::NoRuntime => ("vr.status_not_found", Some("vr.reason_not_installed")),
    }
}

/// Why there is no VR view, kept as what happened rather than as words: the words are looked up
/// when they are shown, in the language the settings then name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Safe mode, the software renderer, or a start that is recovering from a failed one.
    Software,
    /// The OpenXR loader is not beside the program or on the system.
    NoLoader,
    /// No VR software is registered or running.
    NoRuntime,
    /// The VR software runs but has no headset.
    NoHeadset,
    /// The VR software cannot share pictures with a Vulkan app.
    NoVulkan,
    /// Anything else, in the runtime's or the device's own words.
    Other(String),
}

impl Reason {
    /// What `error` is to the user.
    pub fn of(error: &solidcraft_xr::Error) -> Reason {
        use solidcraft_xr::Error;
        match error {
            Error::NoLoader(_) => Reason::NoLoader,
            Error::NoRuntime(_) => Reason::NoRuntime,
            Error::NoHeadset(_) => Reason::NoHeadset,
            Error::NoVulkan => Reason::NoVulkan,
            other => Reason::Other(other.to_string()),
        }
    }

    /// The reason in the user's language: what to start or plug in where the user can do
    /// something about it, the runtime's own words otherwise.
    pub fn text(&self) -> String {
        use solidcraft_ui_text::tr;
        match self {
            Reason::Software => tr("vr.reason_software").to_string(),
            Reason::NoLoader => tr("vr.reason_no_loader").to_string(),
            Reason::NoRuntime => tr("vr.reason_no_runtime").to_string(),
            Reason::NoHeadset => tr("vr.reason_no_headset").to_string(),
            Reason::NoVulkan => tr("vr.reason_no_vulkan").to_string(),
            Reason::Other(words) => words.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Off unless asked: by the flag, the environment or the saved setting, in that order; the
    /// environment can also turn a saved yes off for a run.
    #[test]
    fn the_vr_view_is_tried_only_when_asked_for() {
        assert!(!wanted(false, None, None), "nothing asked: the app starts as ever");
        assert!(!wanted(false, None, Some(&json!({"msaa": 4}))), "an older graphics.json has no choice");
        assert!(!wanted(false, None, Some(&json!({"vr": false}))));
        assert!(wanted(false, None, Some(&json!({"vr": true}))));
        assert!(wanted(true, None, Some(&json!({"vr": false}))), "--vr");
        assert!(wanted(true, Some("0"), None), "--vr over the environment");
        for yes in ["1", "true", "ON", " yes "] {
            assert!(wanted(false, Some(yes), None), "{yes}");
        }
        for no in ["0", "false", "Off", "no"] {
            assert!(!wanted(false, Some(no), Some(&json!({"vr": true}))), "{no} turns a saved yes off");
        }
    }

    /// A saved or environment value that is not a choice is not taken for one.
    #[test]
    fn a_value_that_is_not_a_choice_changes_nothing() {
        for junk in ["", "2", "maybe", "\u{0}", "yes please"] {
            assert!(!wanted(false, Some(junk), None), "{junk:?}");
            assert!(wanted(false, Some(junk), Some(&json!({"vr": true}))), "{junk:?} leaves the saved choice");
        }
        for saved in [json!({"vr": "true"}), json!({"vr": 1}), json!({"vr": null}), json!({"vr": [true]}), json!("vr"), json!(null)] {
            assert!(!wanted(false, None, Some(&saved)), "{saved}");
        }
    }

    /// A start that failed says what to do about it where the user can do something, and passes
    /// the runtime's own words on otherwise.
    #[test]
    fn a_failed_start_says_what_to_do() {
        use solidcraft_xr::Error;
        assert_eq!(Reason::of(&Error::NoRuntime("XR_ERROR_RUNTIME_UNAVAILABLE".into())), Reason::NoRuntime);
        assert_eq!(Reason::of(&Error::NoHeadset("XR_ERROR_FORM_FACTOR_UNAVAILABLE".into())), Reason::NoHeadset);
        assert_eq!(Reason::of(&Error::NoLoader("openxr_loader.dll".into())), Reason::NoLoader);
        assert_eq!(Reason::of(&Error::NoVulkan), Reason::NoVulkan);
        let other = Reason::of(&Error::NoFormat(vec![37, 44]));
        assert!(matches!(&other, Reason::Other(words) if words.contains("[37, 44]")), "{other:?}");
        assert!(other.text().contains("[37, 44]"), "the runtime's own words are passed on");
        // Each known reason has words of its own in the catalog.
        let known = [Reason::Software, Reason::NoLoader, Reason::NoRuntime, Reason::NoHeadset, Reason::NoVulkan];
        let texts: std::collections::BTreeSet<String> = known.iter().map(Reason::text).collect();
        assert_eq!(texts.len(), known.len(), "{texts:?}");
    }

    /// The buffers' sizes are what the shader reads: seven vectors for the eye, one matrix for
    /// the marks, seven numbers a vertex.
    #[test]
    fn uniforms_and_vertices_have_the_shaders_layout() {
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let u = eye_uniform([egui::Color32::WHITE, egui::Color32::BLACK], axes, [-1.0, 1.0, 1.0, -1.0], true, false, [0.0, 1.6, 0.0], Some(-0.1));
        assert_eq!(u.len() as u64, UNIFORM_SIZE);
        let float = |at: usize| f32::from_le_bytes([u[at], u[at + 1], u[at + 2], u[at + 3]]);
        assert_eq!((float(6 * 16 + 8), float(7 * 16 + 4), float(7 * 16 + 12)), (1.0, 1.6, -0.1), "floor known, eye and floor height");
        let none = eye_uniform([egui::Color32::WHITE; 2], axes, [-1.0, 1.0, 1.0, -1.0], true, false, [0.0; 3], Some(f64::NAN));
        assert_eq!(f32::from_le_bytes([none[104], none[105], none[106], none[107]]), 0.0, "a floor at no height is not drawn");
        assert_eq!(matrix_bytes(&[[0.0; 4]; 4]).len() as u64, MARK_UNIFORM_SIZE);
        let v = Vertex { position: [1.0, 2.0, 3.0], colour: [0.1, 0.2, 0.3, 1.0] };
        let bytes = mark_bytes(&[v, v]);
        assert_eq!(bytes.len() as u64, 2 * MARK_VERTEX);
        assert_eq!(f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 2.0);
    }

    /// The light's direction turns with the model: toward the head in the room is the model's
    /// own direction toward it, whichever way the model is held.
    #[test]
    fn the_light_follows_the_head_into_the_model() {
        let turned = Transform { axes: solidcraft_vr::room::turn_about_vertical(std::f64::consts::FRAC_PI_2), ..Transform::MILLIMETRES };
        // The model's X points down the room's −Z: a head behind −Z sees the model's +X side.
        let back = model_direction(&turned, [0.0, 0.0, -1.0]);
        assert!((back[0] - 1.0).abs() < 1e-6 && back[1].abs() < 1e-6 && back[2].abs() < 1e-6, "{back:?}");
        assert_eq!(model_direction(&Transform::MILLIMETRES, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
    }

    /// The shader parses and validates, with the entry points the pipelines name: the backdrop,
    /// the floor and the model, and the controllers' marks.
    #[test]
    fn the_shader_is_valid() {
        let module = wgpu::naga::front::wgsl::parse_str(SHADER).expect("the shader parses");
        let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["vs", "fs_eye", "vs_mark", "fs_mark"]);
        let flags = wgpu::naga::valid::ValidationFlags::all();
        let capabilities = wgpu::naga::valid::Capabilities::empty();
        wgpu::naga::valid::Validator::new(flags, capabilities).validate(&module).expect("the shader is valid");
    }

    /// VR that did not start says why, and keeps saying it.
    #[test]
    fn a_failed_start_keeps_saying_why() {
        let vr = Vr::failed(Reason::NoRuntime);
        assert_eq!(vr.status(), &Status::Failed(Reason::NoRuntime));
        assert!(vr.session.is_none() && vr.transform.is_none());
    }

    /// A run without VR opens no session and says whether OpenXR is there, each finding in words
    /// of its own: "not found" names what is missing, the loader before the VR software.
    #[test]
    fn a_run_without_vr_says_whether_openxr_is_installed() {
        use solidcraft_xr::Found;
        let vr = Vr::off(Found::NoRuntime);
        assert_eq!(vr.status(), &Status::Off(Found::NoRuntime));
        assert!(!vr.has_session());
        let words = [Found::Ready, Found::NoLoader, Found::NoRuntime].map(off_words);
        assert_eq!(words[0], ("vr.status_off_found", None));
        assert_eq!(words[1], ("vr.status_not_found", Some("vr.reason_no_loader")), "the loader is the one the failed start names too");
        assert_eq!(words[2], ("vr.status_not_found", Some("vr.reason_not_installed")));
        // Every entry is in the catalogue, and "not found" has the place for what is missing.
        let catalogue: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/locales/pending/vr-viewer.json")).expect("the VR texts parse");
        for (text, reason) in words {
            let sentence = catalogue["en"][text].as_str().unwrap_or_default();
            assert!(!sentence.is_empty(), "{text}");
            assert_eq!(sentence.contains("{reason}"), reason.is_some(), "{text}: {sentence}");
            assert!(reason.is_none_or(|reason| catalogue["en"][reason].is_string()), "{reason:?}");
        }
        assert!(!off_text(Found::NoRuntime).is_empty());
    }
}
