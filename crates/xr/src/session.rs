//! The OpenXR session: the runtime and headset it runs on, the Vulkan device it shares with the
//! window, the pictures the runtime shows, the controllers, and one frame at a time.
//!
//! A frame goes: [`Session::poll`] (what the runtime wants), [`Session::begin_frame`] (waits for
//! the headset's next frame and says where the eyes will be), [`Session::hands`] (the
//! controllers then), [`Session::eye_picture`] (the pictures to draw into; the caller draws and
//! submits), then [`Session::end_frame`] (hands them in). Every step is a `Result`: a session that fails leaves
//! the desktop app as it was.

mod controls;
mod interop;

use openxr as xr;

use crate::input::{Hand, HandInput, Pulse};
use crate::plan::{self, ColorFormat, Lifecycle, State, Step};
use crate::view::{Eye, Fov, Pose};

/// Why there is no VR view.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The OpenXR loader library is not beside the program or on the system: ours to ship.
    #[error("the OpenXR loader was not found ({0})")]
    NoLoader(String),
    /// The loader is there, but no VR software is registered or running.
    #[error("no VR software is running ({0})")]
    NoRuntime(String),
    /// The VR software runs, but has no headset.
    #[error("the VR software found no headset ({0})")]
    NoHeadset(String),
    /// The VR software cannot share pictures with a Vulkan app.
    #[error("this VR software does not support Vulkan (XR_KHR_vulkan_enable2)")]
    NoVulkan,
    /// Vulkan, or the graphics card the headset is on, cannot do what the view needs.
    #[error("{0}")]
    Vulkan(String),
    /// The runtime offers no picture format the view can be drawn in.
    #[error("the VR software offers no sRGB picture format (it offers {0:?})")]
    NoFormat(Vec<i64>),
    /// The runtime describes a headset this view cannot draw for.
    #[error("{0}")]
    Unsupported(String),
    /// The runtime refused a call.
    #[error("{what}: {code}")]
    Xr { what: &'static str, code: String },
}

impl Error {
    /// The runtime refused `what` with `code`. The VR software gone (it was closed or crashed)
    /// or the headset gone is said as such, not in OpenXR's words.
    fn xr(what: &'static str, code: xr::sys::Result) -> Error {
        match code {
            xr::sys::Result::ERROR_INSTANCE_LOST => Error::NoRuntime(format!("{what}: {code:?}")),
            xr::sys::Result::ERROR_SESSION_LOST => Error::NoHeadset(format!("{what}: {code:?}")),
            _ => Error::Xr { what, code: code.to_string() },
        }
    }
}

/// The wgpu device the runtime's Vulkan device became: the window draws with it too.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// A VR runtime with a headset, before any graphics.
pub struct Runtime {
    instance: xr::Instance,
    system: xr::SystemId,
    /// The runtime's name and version, for the log.
    pub name: String,
    /// The headset's name, for the log.
    pub headset: String,
    /// The runtime knows where the floor is under the user (`XR_EXT_local_floor`).
    local_floor: bool,
}

/// The extensions asked for: `required`, and each extra the runtime lists in `available`. The
/// floor (`XR_EXT_local_floor`; the play area stands in otherwise), tracked hands as controllers
/// (a pinch selects, a closed hand grabs), and the models of the controllers actually held, which
/// take both `XR_EXT_render_model` and `XR_EXT_interaction_render_model`. Their ids are
/// `XR_EXT_uuid`'s type, asked for where the runtime lists it but never required for them: a
/// runtime that wants it and does not list it refuses the extras, and VR starts without them.
fn optional_extensions(available: &xr::ExtensionSet, required: &xr::ExtensionSet) -> xr::ExtensionSet {
    let mut enabled = required.clone();
    enabled.ext_local_floor = available.ext_local_floor;
    enabled.ext_hand_interaction = available.ext_hand_interaction;
    enabled.ext_render_model = available.ext_render_model && available.ext_interaction_render_model;
    enabled.ext_interaction_render_model = enabled.ext_render_model;
    enabled.ext_uuid = enabled.ext_render_model && available.ext_uuid;
    enabled
}

/// What to ask for when the runtime refuses `enabled`, in order: the same without the controller
/// models (the extensions most likely to need another the runtime does not list), then `required`
/// alone. A step that asks for nothing less than the one before is left out.
fn fallbacks(enabled: &xr::ExtensionSet, required: &xr::ExtensionSet) -> Vec<xr::ExtensionSet> {
    let mut steps = Vec::new();
    if enabled.ext_render_model || enabled.ext_interaction_render_model || enabled.ext_uuid {
        let mut without_models = enabled.clone();
        without_models.ext_render_model = false;
        without_models.ext_interaction_render_model = false;
        without_models.ext_uuid = required.ext_uuid;
        steps.push(without_models);
    }
    let last = steps.last().unwrap_or(enabled);
    let more = |a: bool, b: bool| a && !b;
    if more(last.ext_local_floor, required.ext_local_floor) || more(last.ext_hand_interaction, required.ext_hand_interaction) {
        steps.push(required.clone());
    }
    steps
}

impl Runtime {
    /// Find the VR software and its headset. Each way this can fail is its own [`Error`], so the
    /// app can say what to start or plug in.
    pub fn connect() -> Result<Runtime, Error> {
        let entry = interop::load_entry()?;
        let available = entry.enumerate_extensions().map_err(|e| Error::NoRuntime(e.to_string()))?;
        if !available.khr_vulkan_enable2 {
            return Err(Error::NoVulkan);
        }
        let mut required = xr::ExtensionSet::default();
        required.khr_vulkan_enable2 = true;
        let mut enabled = optional_extensions(&available, &required);
        log::info!(
            "VR: controller models offered: XR_EXT_render_model {}, XR_EXT_interaction_render_model {}, XR_EXT_uuid {}",
            available.ext_render_model,
            available.ext_interaction_render_model,
            available.ext_uuid
        );
        let app = xr::ApplicationInfo {
            application_name: "SolidCraft",
            application_version: 1,
            engine_name: "SolidCraft",
            engine_version: 1,
            api_version: xr::Version::new(1, 0, 0),
        };
        // The extras are never worth not starting for: a runtime that refuses them (one needs
        // another the runtime does not say) gets asked without the controller models, then for
        // the required ones alone, so the floor and tracked hands stay where only the models fail.
        let mut steps = fallbacks(&enabled, &required).into_iter();
        let instance = loop {
            match entry.create_instance(&app, &enabled, &[], &()) {
                Ok(instance) => break instance,
                Err(e) => match steps.next() {
                    Some(next) => {
                        log::warn!("VR: the VR software refused the optional extensions ({e}); asking for fewer");
                        enabled = next;
                    }
                    None => return Err(Error::NoRuntime(e.to_string())),
                },
            }
        };
        let name = instance.properties().map(|p| format!("{} {}", p.runtime_name, p.runtime_version)).unwrap_or_default();
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY).map_err(|e| Error::NoHeadset(e.to_string()))?;
        let headset = instance.system_properties(system).map(|p| p.system_name).unwrap_or_default();
        Ok(Runtime { instance, system, name, headset, local_floor: enabled.ext_local_floor })
    }

    /// Open the graphics device and the session on it. `describe` gives the features and limits
    /// the window's device is opened with, for the adapter the headset is on; the window then
    /// draws with the returned [`Gpu`]. Keep the [`Session`] no longer than that device's users.
    pub fn open(self, describe: &dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>) -> Result<(Session, Gpu), Error> {
        let Runtime { instance, system, local_floor, .. } = self;
        let (gpu, raw) = interop::open_graphics(&instance, system, describe)?;
        let (session, waiter, stream) = interop::create_session(&instance, system, raw)?;
        let kind = xr::ViewConfigurationType::PRIMARY_STEREO;
        let views = instance.enumerate_view_configuration_views(system, kind).map_err(|e| Error::xr("reading the headset's views", e))?;
        let [left, right] = views.as_slice() else {
            return Err(Error::Unsupported(format!("the headset has {} views, not two eyes", views.len())));
        };
        let max_side = gpu.device.limits().max_texture_dimension_2d;
        let size_of = |v: &xr::ViewConfigurationView| {
            plan::eye_size(
                [v.recommended_image_rect_width, v.recommended_image_rect_height],
                [v.max_image_rect_width, v.max_image_rect_height],
                max_side,
            )
        };
        let (Some(left), Some(right)) = (size_of(left), size_of(right)) else {
            return Err(Error::Unsupported("the headset recommends no picture size".into()));
        };
        let offered: Vec<i64> =
            session.enumerate_swapchain_formats().map_err(|e| Error::xr("reading the picture formats", e))?.into_iter().map(i64::from).collect();
        let format = plan::pick_format(&offered).ok_or(Error::NoFormat(offered))?;
        let blends = instance.enumerate_environment_blend_modes(system, kind).unwrap_or_default();
        let blend = blends.first().copied().unwrap_or(xr::EnvironmentBlendMode::OPAQUE);
        // The real room around the model: the alpha-blended view, where the VR software has one.
        let see_through = blends.contains(&xr::EnvironmentBlendMode::ALPHA_BLEND);
        log::info!("VR: the VR software offers the blend modes {blends:?}; the real room (passthrough): {see_through}");
        let space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .map_err(|e| Error::xr("creating the room's space", e))?;
        // The floor under the user: the floor below the room's origin where the runtime knows it,
        // else the play area's (on the floor, where one was set up), else none.
        let floor_kinds = [local_floor.then_some(xr::ReferenceSpaceType::LOCAL_FLOOR), Some(xr::ReferenceSpaceType::STAGE)];
        let floor = floor_kinds.into_iter().flatten().find_map(|kind| session.create_reference_space(kind, xr::Posef::IDENTITY).ok());
        let session = Session {
            eyes: None,
            models: Vec::new(),
            models_ask: ModelAsk::default(),
            controls: None,
            floor,
            space,
            stream,
            waiter,
            session,
            instance,
            events: xr::EventDataBuffer::new(),
            life: Lifecycle::NEW,
            recentred: false,
            in_frame: false,
            blend,
            see_through,
            passthrough: false,
            format,
            eye_sizes: [left, right],
            device: gpu.device.clone(),
        };
        Ok((session, gpu))
    }
}

/// A set of pictures the runtime shows one of each frame, wrapped for wgpu.
struct Chain {
    /// One view per picture, in the runtime's order. Before `swapchain`: they go first.
    views: Vec<wgpu::TextureView>,
    _textures: Vec<wgpu::Texture>,
    swapchain: xr::Swapchain<xr::Vulkan>,
    size: [u32; 2],
    /// A picture is acquired and waited for: it must be released before the frame ends.
    held: bool,
}

impl Chain {
    /// Pictures of `size` in `format`, or why the runtime or the device made none.
    fn new(session: &xr::Session<xr::Vulkan>, device: &wgpu::Device, format: ColorFormat, size: [u32; 2]) -> Result<Chain, Error> {
        let info = xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::SAMPLED,
            format: u32::try_from(format.vulkan()).unwrap_or_default(),
            sample_count: 1,
            width: size[0],
            height: size[1],
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        };
        let swapchain = session.create_swapchain(&info).map_err(|e| Error::xr("creating the pictures", e))?;
        let images = swapchain.enumerate_images().map_err(|e| Error::xr("reading the pictures", e))?;
        let textures: Vec<wgpu::Texture> = images
            .into_iter()
            .map(|image| interop::wrap_image(device, image, texture_format(format), size))
            .collect::<Option<_>>()
            .ok_or_else(|| Error::Vulkan("the session's device is not a Vulkan one".into()))?;
        if textures.is_empty() {
            return Err(Error::Unsupported("the VR software gave no pictures to draw into".into()));
        }
        let views = textures.iter().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default())).collect();
        Ok(Chain { views, _textures: textures, swapchain, size, held: false })
    }

    /// The picture to draw into this frame: acquired and waited for, to be released by
    /// [`Chain::release`] once the drawing is submitted.
    fn acquire(&mut self) -> Result<Picture, Error> {
        if self.held {
            return Err(Error::Xr { what: "acquiring a picture", code: "the last one was not handed in".into() });
        }
        let index = self.swapchain.acquire_image().map_err(|e| Error::xr("acquiring a picture", e))?;
        // Held from here: an acquired picture must be waited for and released, whatever follows.
        self.held = true;
        self.swapchain.wait_image(xr::Duration::INFINITE).map_err(|e| Error::xr("waiting for a picture", e))?;
        let view = self.views.get(index as usize).cloned().ok_or(Error::Xr { what: "acquiring a picture", code: format!("no picture {index}") })?;
        Ok(Picture { view, size: self.size })
    }

    /// Hand the acquired picture back, if one is held.
    fn release(&mut self) -> Result<(), Error> {
        if !std::mem::take(&mut self.held) {
            return Ok(());
        }
        self.swapchain.release_image().map_err(|e| Error::xr("handing a picture in", e))
    }
}

/// The renderer's name for a runtime picture format.
fn texture_format(format: ColorFormat) -> wgpu::TextureFormat {
    match format {
        ColorFormat::Bgra8Srgb => wgpu::TextureFormat::Bgra8UnormSrgb,
        ColorFormat::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
    }
}

/// A picture to draw into this frame.
pub struct Picture {
    pub view: wgpu::TextureView,
    /// Its size in pixels.
    pub size: [u32; 2],
}

/// One frame of the headset.
pub struct Frame {
    time: xr::Time,
    views: Vec<xr::View>,
    /// The runtime shows what is drawn this frame. `false`: nothing needs drawing.
    pub should_render: bool,
    /// Where the two eyes will be when the frame is shown; `None` while the headset is not
    /// tracked, when nothing drawn for a pose would be right.
    pub eyes: Option<[Eye; 2]>,
}

/// A controller's model as the runtime hands it out ([`Session::new_controller_models`]).
#[derive(Clone, Debug)]
pub struct ModelAsset {
    /// The asset's id: models with the same id look the same.
    pub cache: [u8; 16],
    /// The hand holding it, where it is a hand's controller.
    pub hand: Option<Hand>,
    /// The glTF asset (GLB, core glTF: no extensions asked for).
    pub glb: Vec<u8>,
    /// The names of its moving parts (glTF node names), in the order [`ModelPose::nodes`] has them.
    pub nodes: Vec<String>,
}

/// Where a controller's model is in a frame ([`Session::controller_models`]).
#[derive(Clone, Debug)]
pub struct ModelPose {
    pub cache: [u8; 16],
    pub hand: Option<Hand>,
    /// Where the asset's origin is in the room; `None` while it is not tracked.
    pub pose: Option<Pose>,
    /// Each moving part's pose relative to its parent node (it replaces the asset's own) and
    /// whether it shows; empty when the runtime did not say this frame.
    pub nodes: Vec<(Pose, bool)>,
}

/// How often asking for the controller models is tried in all after the runtime says they
/// changed: the first ask and the tries after it failed or found none.
const MODEL_TRIES: u32 = 5;

/// The frames let pass before the first try again after a failed ask; each later one waits
/// twice as long (half a second, then 1, 2 and 4 at 90 frames a second).
const MODEL_RETRY_FRAMES: u32 = 45;

/// When the controller models are asked for ([`Session::new_controller_models`]): on the frame
/// after the runtime says they changed, and again after an ask failed, later each time and at
/// most [`MODEL_TRIES`] times in all, so a runtime whose list keeps failing is not asked every
/// frame. What it depends on: the runtime's news ([`ModelAsk::changed`] starts afresh) and how
/// each ask went ([`ModelAsk::failed`]); nothing is remembered past the next change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ModelAsk {
    /// The models are to be asked for.
    stale: bool,
    /// The asks that failed since the runtime last said the models changed.
    failures: u32,
    /// The frames still to let pass before asking.
    wait: u32,
}

impl ModelAsk {
    /// The runtime says the models changed (or the controls were just attached): ask on the
    /// next frame, with every try again.
    fn changed(&mut self) {
        *self = ModelAsk { stale: true, failures: 0, wait: 0 };
    }

    /// Whether to ask this frame (asked once a frame); asking takes the request.
    fn due(&mut self) -> bool {
        if !self.stale {
            return false;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return false;
        }
        self.stale = false;
        true
    }

    /// The ask just made failed: ask again after a wait twice as long as the last, unless it
    /// has been tried [`MODEL_TRIES`] times; then only the runtime's next news asks again.
    fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
        if self.failures < MODEL_TRIES {
            self.stale = true;
            self.wait = MODEL_RETRY_FRAMES << (self.failures - 1).min(8);
        }
    }
}

/// What [`Session::poll`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The runtime is not showing the app (the headset is off, or it is still starting).
    Waiting,
    /// Frames are running.
    Running,
    /// The session is over: the user left VR, or the runtime went away.
    Closed,
}

/// A running OpenXR session on the window's device.
pub struct Session {
    // Dropped in this order: the pictures, the room, the frame handles and the session all go
    // before the device they were made on.
    eyes: Option<[Chain; 2]>,
    /// The models of the controllers held (`XR_EXT_render_model`), where the runtime has them.
    /// Dropped before the controls: the runtime made them from the bound controllers.
    models: Vec<interop::RawModel>,
    /// When the models are asked for again: the controllers changed (or were just attached), or
    /// asking failed.
    models_ask: ModelAsk,
    /// The controllers, once [`Session::attach_controls`] made them.
    controls: Option<controls::Controls>,
    /// A space on the floor under the user, where the runtime has one.
    floor: Option<xr::Space>,
    space: xr::Space,
    stream: xr::FrameStream<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    session: xr::Session<xr::Vulkan>,
    instance: xr::Instance,
    events: xr::EventDataBuffer,
    life: Lifecycle,
    /// The VR software recentred the room since [`Session::recentred`] last asked.
    recentred: bool,
    /// Between a begun and an ended frame.
    in_frame: bool,
    blend: xr::EnvironmentBlendMode,
    /// The VR software can show the real room around what is drawn (`ALPHA_BLEND`).
    see_through: bool,
    /// The real room is shown where nothing is drawn ([`Session::set_passthrough`]).
    passthrough: bool,
    format: ColorFormat,
    eye_sizes: [[u32; 2]; 2],
    device: wgpu::Device,
}

/// A runtime's session state as [`plan::State`].
fn state_of(state: xr::SessionState) -> State {
    match state {
        xr::SessionState::IDLE => State::Idle,
        xr::SessionState::READY => State::Ready,
        xr::SessionState::SYNCHRONIZED => State::Synchronized,
        xr::SessionState::VISIBLE => State::Visible,
        xr::SessionState::FOCUSED => State::Focused,
        xr::SessionState::STOPPING => State::Stopping,
        xr::SessionState::LOSS_PENDING => State::LossPending,
        xr::SessionState::EXITING => State::Exiting,
        _ => State::Other,
    }
}

/// The first `size` pixels of a picture, from its top left corner, as the runtime takes it.
fn pixel_rect(size: [u32; 2]) -> xr::Rect2Di {
    xr::Rect2Di {
        offset: xr::Offset2Di { x: 0, y: 0 },
        extent: xr::Extent2Di { width: i32::try_from(size[0]).unwrap_or(i32::MAX), height: i32::try_from(size[1]).unwrap_or(i32::MAX) },
    }
}

/// One eye of the room's layer: the whole of `chain`'s picture, seen as `view` was located.
fn eye_layer<'a>(chain: &'a Chain, view: &xr::View) -> xr::CompositionLayerProjectionView<'a, xr::Vulkan> {
    xr::CompositionLayerProjectionView::new()
        .pose(view.pose)
        .fov(view.fov)
        .sub_image(xr::SwapchainSubImage::new().swapchain(&chain.swapchain).image_array_index(0).image_rect(pixel_rect(chain.size)))
}

/// A runtime's pose as [`Pose`].
fn pose_of(pose: xr::Posef) -> Pose {
    let (p, q) = (pose.position, pose.orientation);
    Pose { position: [p.x, p.y, p.z].map(f64::from), orientation: [q.x, q.y, q.z, q.w].map(f64::from) }
}

impl Session {
    /// The format of every picture this session hands out.
    pub fn format(&self) -> wgpu::TextureFormat {
        texture_format(self.format)
    }

    /// The size of an eye's picture (pixels).
    pub fn eye_size(&self, eye: usize) -> [u32; 2] {
        self.eye_sizes.get(eye).copied().unwrap_or(self.eye_sizes[0])
    }

    /// Take the runtime's news: begin the frames when it is ready for them, end them when it
    /// asks, and say when the session is over.
    pub fn poll(&mut self) -> Result<Status, Error> {
        // At most this many a call: a runtime that never runs out of events must not hold the
        // window's frame.
        for _ in 0..64 {
            let state = match self.instance.poll_event(&mut self.events).map_err(|e| Error::xr("reading the VR software's events", e))? {
                None => break,
                Some(xr::Event::SessionStateChanged(e)) => state_of(e.state()),
                Some(xr::Event::InstanceLossPending(_)) => State::LossPending,
                Some(xr::Event::ReferenceSpaceChangePending(_)) => {
                    self.recentred = true;
                    continue;
                }
                Some(xr::Event::InteractionRenderModelsChangedEXT) => {
                    self.models_ask.changed();
                    continue;
                }
                Some(_) => continue,
            };
            let (step, life) = self.life.on(state);
            match step {
                Step::Stay | Step::Close => {}
                Step::Begin => {
                    self.session.begin(xr::ViewConfigurationType::PRIMARY_STEREO).map_err(|e| Error::xr("beginning the session", e))?;
                }
                Step::End => {
                    self.session.end().map_err(|e| Error::xr("ending the session", e))?;
                }
            }
            self.life = life;
        }
        Ok(if self.life.closed {
            Status::Closed
        } else if self.life.running {
            Status::Running
        } else {
            Status::Waiting
        })
    }

    /// Ask the runtime to end the session (the app leaves VR): [`Session::poll`] reports
    /// [`Status::Closed`] once it has.
    pub fn request_exit(&mut self) {
        if self.life.running {
            if let Err(e) = self.session.request_exit() {
                log::warn!("VR: the session did not take the request to end: {e}");
                self.life = Lifecycle { running: false, closed: true };
            }
        } else {
            self.life = Lifecycle { running: false, closed: true };
        }
    }

    /// Wait for the headset's next frame and begin it. `None` while frames are not running. A
    /// begun frame must be ended with [`Session::end_frame`].
    pub fn begin_frame(&mut self) -> Result<Option<Frame>, Error> {
        if !self.life.running || self.in_frame {
            return Ok(None);
        }
        let state = self.waiter.wait().map_err(|e| Error::xr("waiting for the headset's frame", e))?;
        self.stream.begin().map_err(|e| Error::xr("beginning the frame", e))?;
        self.in_frame = true;
        let located = self.session.locate_views(xr::ViewConfigurationType::PRIMARY_STEREO, state.predicted_display_time, &self.space);
        let (views, eyes) = match located {
            Ok((flags, views)) if flags.contains(xr::ViewStateFlags::ORIENTATION_VALID | xr::ViewStateFlags::POSITION_VALID) => {
                let eye = |v: &xr::View| Eye {
                    pose: pose_of(v.pose),
                    fov: Fov {
                        left: f64::from(v.fov.angle_left),
                        right: f64::from(v.fov.angle_right),
                        up: f64::from(v.fov.angle_up),
                        down: f64::from(v.fov.angle_down),
                    },
                };
                let eyes = match views.as_slice() {
                    [left, right] => Some([eye(left), eye(right)]),
                    _ => None,
                };
                (views, eyes)
            }
            Ok(_) => (Vec::new(), None),
            Err(e) => {
                log::debug!("VR: the eyes were not located: {e}");
                (Vec::new(), None)
            }
        };
        Ok(Some(Frame { time: state.predicted_display_time, views, should_render: state.should_render, eyes }))
    }

    /// Read the controllers from now on ([`Session::hands`]): make the actions, suggest the
    /// bindings of [`crate::input::PROFILES`] and attach them. Once a session; a second call
    /// changes nothing. A session without it never asks the runtime for the controllers, so
    /// they stay with whatever else uses them.
    pub fn attach_controls(&mut self) -> Result<(), Error> {
        if self.controls.is_none() {
            let hands_tracked = self.instance.exts().ext_hand_interaction.is_some();
            self.controls = Some(controls::Controls::new(&self.instance, &self.session, hands_tracked)?);
            self.models_ask.changed();
        }
        Ok(())
    }

    /// The models of the controllers held, when they changed since the last call (the user
    /// picked up other controllers, or the runtime found them): every model now, each with its
    /// asset. `None` when nothing changed, and always where the runtime does not hand out models
    /// (`XR_EXT_render_model`); the app then draws its own marks. Asking that failed or found none
    /// is tried again a little later, a few times ([`ModelAsk`]); meanwhile there are no models.
    pub fn new_controller_models(&mut self) -> Option<Vec<ModelAsset>> {
        if !self.models_ask.due() || self.controls.is_none() || self.instance.exts().ext_render_model.is_none() {
            return None;
        }
        self.models.clear();
        let path = |p: &str| self.instance.string_to_path(p).ok();
        let (Some(left), Some(right)) = (path(Hand::Left.path()), path(Hand::Right.path())) else {
            self.models_ask.failed();
            return None;
        };
        match interop::controller_models(&self.instance, &self.session, [left, right]) {
            Ok(models) => {
                // None yet: the runtime may not have bound the controllers this soon after the
                // controls were attached, and need not say so when it has; asked again a little
                // later, as after a failure.
                if models.is_empty() {
                    self.models_ask.failed();
                }
                self.models = models;
            }
            Err(e) => {
                log::info!("VR: no controller models: {e}");
                self.models_ask.failed();
            }
        }
        Some(
            self.models
                .iter_mut()
                .map(|m| ModelAsset { cache: m.cache, hand: m.hand.map(|i| Hand::BOTH[i]), glb: std::mem::take(&mut m.glb), nodes: m.nodes.clone() })
                .collect(),
        )
    }

    /// Where each controller model (in the order [`Session::new_controller_models`] gave them)
    /// is when `frame` is shown, and how its moving parts stand.
    pub fn controller_models(&self, frame: &Frame) -> Vec<ModelPose> {
        let valid = xr::SpaceLocationFlags::ORIENTATION_VALID | xr::SpaceLocationFlags::POSITION_VALID;
        self.models
            .iter()
            .map(|m| {
                let pose = m
                    .space
                    .as_ref()
                    .and_then(|s| s.locate(&self.space, frame.time).ok())
                    .filter(|l| l.location_flags.contains(valid))
                    .map(|l| pose_of(l.pose))
                    .filter(|p| p.axes().is_some());
                let nodes = m.state(frame.time).map(|states| states.into_iter().map(|(p, v)| (pose_of(p), v)).collect()).unwrap_or_default();
                ModelPose { cache: m.cache, hand: m.hand.map(|i| Hand::BOTH[i]), pose, nodes }
            })
            .collect()
    }

    /// Both hands' controllers as they will be when `frame` is shown, left first: all
    /// [`HandInput::NONE`] before [`Session::attach_controls`], and while the session does not
    /// have the input focus (the runtime's own menu is open).
    pub fn hands(&self, frame: &Frame) -> [HandInput; 2] {
        match &self.controls {
            Some(controls) if self.life.running => controls.read(&self.session, &self.space, frame.time),
            _ => [HandInput::NONE; 2],
        }
    }

    /// The floor's height in the room (metres, the room's Y) when `frame` is shown, or `None`
    /// where the runtime does not know where the floor is.
    pub fn floor_height(&self, frame: &Frame) -> Option<f64> {
        let location = self.floor.as_ref()?.locate(&self.space, frame.time).ok()?;
        let y = f64::from(location.pose.position.y);
        (location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID) && y.is_finite()).then_some(y)
    }

    /// Whether the VR software recentred the room (the user reset the view) since the last ask:
    /// what was placed in front of the user is not any more.
    pub fn recentred(&mut self) -> bool {
        std::mem::take(&mut self.recentred)
    }

    /// Show the real room where nothing is drawn (a picture's alpha below one lets it through),
    /// or not; where the VR software cannot (`false` back), the room stays as drawn.
    pub fn set_passthrough(&mut self, on: bool) -> bool {
        self.passthrough = on && self.see_through;
        self.see_through
    }

    /// Whether the real room shows where nothing is drawn: the pictures' alpha counts.
    pub fn passthrough(&self) -> bool {
        self.passthrough
    }

    /// Vibrate `hand`'s controller (nothing before [`Session::attach_controls`]).
    pub fn pulse(&self, hand: Hand, pulse: Pulse) {
        if let Some(controls) = &self.controls {
            controls.pulse(&self.session, hand, pulse);
        }
    }

    /// The picture to draw `eye` (0 left, 1 right) into this frame. Draw into it and submit
    /// before [`Session::end_frame`].
    pub fn eye_picture(&mut self, eye: usize) -> Result<Picture, Error> {
        if self.eyes.is_none() {
            let make = |size| Chain::new(&self.session, &self.device, self.format, size);
            self.eyes = Some([make(self.eye_sizes[0])?, make(self.eye_sizes[1])?]);
        }
        let chain =
            self.eyes.as_mut().and_then(|eyes| eyes.get_mut(eye)).ok_or(Error::Xr { what: "acquiring a picture", code: format!("no eye {eye}") })?;
        chain.acquire()
    }

    /// Hand the frame in: the eyes' pictures as the view of the room when `eyes` (both were drawn
    /// this frame), else nothing. Everything drawn into this frame's pictures must have been
    /// submitted.
    pub fn end_frame(&mut self, frame: &Frame, eyes: bool) -> Result<(), Error> {
        if !std::mem::take(&mut self.in_frame) {
            return Ok(());
        }
        let mut released = Ok(());
        for chain in self.eyes.iter_mut().flatten() {
            released = released.and(chain.release());
        }
        let projection_views: Option<[xr::CompositionLayerProjectionView<'_, xr::Vulkan>; 2]> = match (&self.eyes, frame.views.as_slice()) {
            (Some([left, right]), [left_view, right_view]) if eyes && released.is_ok() && frame.eyes.is_some() => {
                Some([eye_layer(left, left_view), eye_layer(right, right_view)])
            }
            _ => None,
        };
        // With the real room shown, the pictures' alpha says where it shows through.
        let flags = if self.passthrough { xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA } else { xr::CompositionLayerFlags::EMPTY };
        let projection =
            projection_views.as_ref().map(|views| xr::CompositionLayerProjection::new().layer_flags(flags).space(&self.space).views(views));
        let mut layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> = Vec::with_capacity(1);
        if let Some(projection) = &projection {
            layers.push(projection);
        }
        let blend = if self.passthrough { xr::EnvironmentBlendMode::ALPHA_BLEND } else { self.blend };
        let ended = self.stream.end(frame.time, blend, &layers).map_err(|e| Error::xr("handing the frame in", e));
        released.and(ended)
    }
}

/// Let go of `chains`: the device frees its views of their pictures (once the drawing that used
/// them is done) before the runtime frees the pictures themselves.
fn retire(device: &wgpu::Device, chains: impl IntoIterator<Item = Chain>) {
    let swapchains: Vec<xr::Swapchain<xr::Vulkan>> = chains
        .into_iter()
        .map(|chain| {
            let Chain { views, _textures, swapchain, .. } = chain;
            drop(views);
            drop(_textures);
            swapchain
        })
        .collect();
    if swapchains.is_empty() {
        return;
    }
    if let Err(e) = device.poll(wgpu::PollType::wait_indefinitely()) {
        log::debug!("VR: the device was not idle as pictures were let go: {e}");
    }
    drop(swapchains);
}

impl Drop for Session {
    /// The pictures go the way [`retire`] lets them go; the fields then drop in their order.
    fn drop(&mut self) {
        let chains: Vec<Chain> = self.eyes.take().into_iter().flatten().collect();
        retire(&self.device, chains);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The controller models are asked for where the runtime lists both of their extensions,
    /// with `XR_EXT_uuid` only where it is listed too; with either of the two missing, none of
    /// them is, and the required ones always are.
    #[test]
    fn controller_models_do_not_wait_for_the_uuid_extension() {
        let mut required = xr::ExtensionSet::default();
        required.khr_vulkan_enable2 = true;
        let mut available = required.clone();
        available.ext_render_model = true;
        available.ext_interaction_render_model = true;
        let enabled = optional_extensions(&available, &required);
        assert!(enabled.khr_vulkan_enable2 && enabled.ext_render_model && enabled.ext_interaction_render_model);
        assert!(!enabled.ext_uuid, "not listed, not asked for");
        available.ext_uuid = true;
        assert!(optional_extensions(&available, &required).ext_uuid);
        available.ext_interaction_render_model = false;
        let enabled = optional_extensions(&available, &required);
        assert!(!enabled.ext_render_model && !enabled.ext_interaction_render_model && !enabled.ext_uuid);
        assert!(enabled.khr_vulkan_enable2);
    }

    /// A refused start asks again without the controller models first, keeping the floor and
    /// tracked hands, and only then for the required extensions alone; with nothing optional
    /// asked for, there is nothing to fall back to.
    #[test]
    fn a_refusal_drops_the_controller_models_before_the_floor_and_hands() {
        let mut required = xr::ExtensionSet::default();
        required.khr_vulkan_enable2 = true;
        let mut available = required.clone();
        available.ext_local_floor = true;
        available.ext_hand_interaction = true;
        available.ext_render_model = true;
        available.ext_interaction_render_model = true;
        let enabled = optional_extensions(&available, &required);
        let steps = fallbacks(&enabled, &required);
        assert_eq!(steps.len(), 2);
        assert!(steps[0].ext_local_floor && steps[0].ext_hand_interaction && steps[0].khr_vulkan_enable2);
        assert!(!steps[0].ext_render_model && !steps[0].ext_interaction_render_model && !steps[0].ext_uuid);
        assert!(!steps[1].ext_local_floor && !steps[1].ext_hand_interaction && steps[1].khr_vulkan_enable2);
        // Only the models offered: one step, straight to the required ones.
        let mut models_only = required.clone();
        models_only.ext_render_model = true;
        models_only.ext_interaction_render_model = true;
        let steps = fallbacks(&optional_extensions(&models_only, &required), &required);
        assert_eq!(steps.len(), 1);
        assert!(!steps[0].ext_render_model && steps[0].khr_vulkan_enable2);
        // Nothing optional: the first refusal is the answer.
        assert!(fallbacks(&required, &required).is_empty());
    }

    /// The frames on which `ask` asks within `frames` frames, each ask failing when `fails`.
    fn asks(ask: &mut ModelAsk, frames: u32, fails: bool) -> Vec<u32> {
        let mut out = Vec::new();
        for frame in 0..frames {
            if ask.due() {
                out.push(frame);
                if fails {
                    ask.failed();
                }
            }
        }
        out
    }

    /// Nothing is asked until the runtime says the models changed; then once, on the next
    /// frame, and not again while the ask worked.
    #[test]
    fn the_models_are_asked_for_once_after_each_change() {
        let mut ask = ModelAsk::default();
        assert!(asks(&mut ask, 100, false).is_empty());
        ask.changed();
        assert_eq!(asks(&mut ask, 100, false), [0]);
    }

    /// An ask that failed is tried again later, each wait twice the last, at most
    /// [`MODEL_TRIES`] times in all, never every frame; the runtime's next news starts afresh.
    #[test]
    fn a_failed_ask_is_tried_again_later_a_few_times() {
        let mut ask = ModelAsk::default();
        ask.changed();
        let r = MODEL_RETRY_FRAMES;
        let tried = asks(&mut ask, 100_000, true);
        assert_eq!(tried, [0, r + 1, 3 * r + 2, 7 * r + 3, 15 * r + 4]);
        assert_eq!(tried.len() as u32, MODEL_TRIES);
        assert!(asks(&mut ask, 100_000, true).is_empty(), "given up until the runtime has news");
        ask.changed();
        assert_eq!(asks(&mut ask, 10, true), [0], "news: asked at once again");
        // A failed ask, then news before the try again: asked on the next frame.
        ask.changed();
        assert_eq!(asks(&mut ask, 1, true), [0]);
        ask.changed();
        assert_eq!(asks(&mut ask, 1, false), [0]);
    }

    /// The VR software closed or crashed under a running session (Monado stopped, seen on
    /// Linux): that is said as no VR software running, the headset unplugged as no headset;
    /// any other refusal keeps the runtime's words.
    #[test]
    fn a_lost_runtime_or_headset_is_said_plainly() {
        let lost = Error::xr("reading the VR software's events", xr::sys::Result::ERROR_INSTANCE_LOST);
        assert!(matches!(lost, Error::NoRuntime(_)), "{lost:?}");
        let unplugged = Error::xr("waiting for the frame", xr::sys::Result::ERROR_SESSION_LOST);
        assert!(matches!(unplugged, Error::NoHeadset(_)), "{unplugged:?}");
        let other = Error::xr("ending the frame", xr::sys::Result::ERROR_VALIDATION_FAILURE);
        assert!(matches!(other, Error::Xr { what: "ending the frame", .. }), "{other:?}");
    }
}
