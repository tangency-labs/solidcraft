//! The OpenXR session: the runtime and headset it runs on, the Vulkan device it shares with the
//! window, the pictures the runtime shows, and one frame at a time.
//!
//! A frame goes: [`Session::poll`] (what the runtime wants), [`Session::begin_frame`] (waits for
//! the headset's next frame and says where the eyes will be), [`Session::eye_picture`] and
//! [`Session::window_picture`] (the pictures to draw into; the caller draws and submits), then
//! [`Session::end_frame`] (hands them in). Every step is a `Result`: a session that fails leaves
//! the desktop app as it was.

mod interop;

use openxr as xr;

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
    /// The runtime refused `what` with `code`.
    fn xr(what: &'static str, code: xr::sys::Result) -> Error {
        Error::Xr { what, code: code.to_string() }
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
        let mut enabled = xr::ExtensionSet::default();
        enabled.khr_vulkan_enable2 = true;
        let app = xr::ApplicationInfo {
            application_name: "SolidCraft",
            application_version: 1,
            engine_name: "SolidCraft",
            engine_version: 1,
            api_version: xr::Version::new(1, 0, 0),
        };
        let instance = entry.create_instance(&app, &enabled, &[], &()).map_err(|e| Error::NoRuntime(e.to_string()))?;
        let name = instance.properties().map(|p| format!("{} {}", p.runtime_name, p.runtime_version)).unwrap_or_default();
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY).map_err(|e| Error::NoHeadset(e.to_string()))?;
        let headset = instance.system_properties(system).map(|p| p.system_name).unwrap_or_default();
        Ok(Runtime { instance, system, name, headset })
    }

    /// Open the graphics device and the session on it. `describe` gives the features and limits
    /// the window's device is opened with, for the adapter the headset is on; the window then
    /// draws with the returned [`Gpu`]. Keep the [`Session`] no longer than that device's users.
    pub fn open(self, describe: &dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>) -> Result<(Session, Gpu), Error> {
        let Runtime { instance, system, .. } = self;
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
        let blend = instance
            .enumerate_environment_blend_modes(system, kind)
            .ok()
            .and_then(|modes| modes.first().copied())
            .unwrap_or(xr::EnvironmentBlendMode::OPAQUE);
        let space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .map_err(|e| Error::xr("creating the room's space", e))?;
        let session = Session {
            eyes: None,
            window: None,
            space,
            stream,
            waiter,
            session,
            instance,
            events: xr::EventDataBuffer::new(),
            life: Lifecycle::NEW,
            in_frame: false,
            blend,
            format,
            eye_sizes: [left, right],
            max_side,
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

/// The window as a flat layer in the room.
#[derive(Clone, Copy, Debug)]
pub struct WindowLayer {
    /// Its middle and orientation.
    pub pose: Pose,
    /// Its width and height (metres).
    pub size: [f64; 2],
    /// The part of the window's picture that is drawn, from its top left corner (pixels).
    pub pixels: [u32; 2],
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
    window: Option<Chain>,
    space: xr::Space,
    stream: xr::FrameStream<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    session: xr::Session<xr::Vulkan>,
    instance: xr::Instance,
    events: xr::EventDataBuffer,
    life: Lifecycle,
    /// Between a begun and an ended frame.
    in_frame: bool,
    blend: xr::EnvironmentBlendMode,
    format: ColorFormat,
    eye_sizes: [[u32; 2]; 2],
    max_side: u32,
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

/// A [`Pose`] as the runtime takes it.
fn pose_to(pose: &Pose) -> xr::Posef {
    let [x, y, z] = pose.position.map(|v| v as f32);
    let [qx, qy, qz, qw] = pose.orientation.map(|v| v as f32);
    xr::Posef { orientation: xr::Quaternionf { x: qx, y: qy, z: qz, w: qw }, position: xr::Vector3f { x, y, z } }
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

    /// The picture to draw a window of `window` pixels into this frame: larger than the window
    /// ([`plan::window_picture_size`]), the window in its top left corner. Draw into it and
    /// submit before [`Session::end_frame`].
    pub fn window_picture(&mut self, window: [u32; 2]) -> Result<Picture, Error> {
        let size = plan::window_picture_size(window, self.max_side)
            .ok_or_else(|| Error::Unsupported(format!("a window of {} × {} pixels cannot be shown", window[0], window[1])))?;
        if self.window.as_ref().is_none_or(|chain| chain.size != size) {
            // The old pictures go before the new ones are asked for.
            let old = self.window.take();
            retire(&self.device, old);
            self.window = Some(Chain::new(&self.session, &self.device, self.format, size)?);
        }
        match self.window.as_mut() {
            Some(chain) => chain.acquire(),
            None => Err(Error::Unsupported("no window picture".into())),
        }
    }

    /// Hand the frame in: the eyes' pictures as the view of the room when `eyes` (both were drawn
    /// this frame), and the window's picture as a flat layer when `window`. Everything drawn into
    /// this frame's pictures must have been submitted.
    pub fn end_frame(&mut self, frame: &Frame, eyes: bool, window: Option<WindowLayer>) -> Result<(), Error> {
        if !std::mem::take(&mut self.in_frame) {
            return Ok(());
        }
        let mut released = Ok(());
        for chain in self.eyes.iter_mut().flatten().chain(self.window.iter_mut()) {
            released = released.and(chain.release());
        }
        let projection_views: Option<[xr::CompositionLayerProjectionView<'_, xr::Vulkan>; 2]> = match (&self.eyes, frame.views.as_slice()) {
            (Some([left, right]), [left_view, right_view]) if eyes && released.is_ok() && frame.eyes.is_some() => {
                Some([eye_layer(left, left_view), eye_layer(right, right_view)])
            }
            _ => None,
        };
        let projection = projection_views.as_ref().map(|views| xr::CompositionLayerProjection::new().space(&self.space).views(views));
        let quad = match (&self.window, window) {
            (Some(chain), Some(layer)) if released.is_ok() => {
                let pixels = [layer.pixels[0].min(chain.size[0]), layer.pixels[1].min(chain.size[1])];
                Some(
                    xr::CompositionLayerQuad::new()
                        .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                        .space(&self.space)
                        .eye_visibility(xr::EyeVisibility::BOTH)
                        .sub_image(xr::SwapchainSubImage::new().swapchain(&chain.swapchain).image_array_index(0).image_rect(pixel_rect(pixels)))
                        .pose(pose_to(&layer.pose))
                        .size(xr::Extent2Df { width: layer.size[0] as f32, height: layer.size[1] as f32 }),
                )
            }
            _ => None,
        };
        let mut layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> = Vec::with_capacity(2);
        if let Some(projection) = &projection {
            layers.push(projection);
        }
        if let Some(quad) = &quad {
            layers.push(quad);
        }
        let ended = self.stream.end(frame.time, self.blend, &layers).map_err(|e| Error::xr("handing the frame in", e));
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
        let chains: Vec<Chain> = self.eyes.take().into_iter().flatten().chain(self.window.take()).collect();
        retire(&self.device, chains);
    }
}
