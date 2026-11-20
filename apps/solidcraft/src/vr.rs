//! VR (#2172, docs/vr.md): the design as a thing in the room, held, turned and scaled with the
//! controllers. Asked for with `--vr`, `SOLIDCRAFT_VR=1` or Preferences › XR ([`wanted`]),
//! because the runtime creates the Vulkan device the window then draws with. Off, none of it runs.
//!
//! The window on the desktop goes on as ever, so whoever sits at it sees and drives the same
//! design.
//!
//! How a frame is put together: the model is drawn for each eye from the scenes the window's
//! design view uploaded this frame (`Renderer::draw_second_view`), over a backdrop and a floor
//! fixed in the room, and the controllers' marks over that, and the signs naming what their rays
//! point at. The pictures go to the runtime as the room's layer.
//!
//! Few inputs, so that a controller with one button and a tracked hand work too: Select (the
//! trigger, a face button, a pinch) and Grab (the grip, a closed hand). Each controller's ray stops
//! at the part it points at: the part lights up in the design (on the desktop too) and a sign
//! names it. A short Select selects it (or clears the selection, pointing at nothing); a held one
//! opens the ring menu at the hand (`menu`). The stick, where there is one, undoes and redoes,
//! and says so beside it.

mod belt;
mod controllers;
mod menu;
mod model_depth;
mod panel;
mod sign;

use eframe::egui_wgpu::{self, wgpu};
use menu::{Act, Moved};
use solidcraft_engine::Sel;
use solidcraft_ui_egui::SolveApp;
use solidcraft_ui_egui::stereo::Source;
use solidcraft_ui_egui::vr_pick::{self, Part};
use solidcraft_ui_egui::vr_settings::{self as settings, Input, VrSettings};
use solidcraft_viewport_gpu::view::{Renderer, ViewTarget};
use solidcraft_vr::flick::{Direction, Flick};
use solidcraft_vr::grab::WorldGrab;
use solidcraft_vr::marks::{self, Style, Vertex};
use solidcraft_vr::press::{self, Press};
use solidcraft_vr::room::{self, Transform};
use solidcraft_vr::section::{self, Plane};
use solidcraft_xr::input::{Button, Hand, HandInput, Pulse};
use solidcraft_xr::view::{self, Eye, Pose};
use std::collections::HashMap;

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
    // Whether the real room can be shown: SteamVR offers it to PC apps only in newer betas.
    let passthrough = session.set_passthrough(false);
    solidcraft_engine::note!("SolidCraft VR: the real room (passthrough) is {}offered by the VR software", if passthrough { "" } else { "not " });
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
    /// "Put back" asked for the model to be put back: kept until it can be (a frame with a
    /// design view and a tracked head).
    put_back: bool,
    /// Where the model is in the room: `None` until there is a model and a tracked head to place
    /// it in front of.
    transform: Option<Transform>,
    grab: WorldGrab,
    /// Each hand's actions ([`GRAB`], [`SELECT`], [`MENU`], [`PUT_BACK`], [`SECTION`], [`UNDO`],
    /// [`REDO`]), as the
    /// settings bind them to the controller's inputs, left first.
    buttons: [[Button; ACTIONS]; 2],
    belt: belt::Belt,
    /// The Browser's window on the belt, made when it first opens.
    panel: Option<panel::Panel>,
    /// Where each hand's ray is on the Browser's window (across and down it, 0 to 1) and how far
    /// along the ray, left first.
    window_hover: [Option<([f64; 2], f64)>; 2],
    /// Each hand's Select was pressed on the window and is not let go yet.
    window_pressed: [bool; 2],
    /// The scale was shown last at this time (seconds, [`Vr::clock`]): it fades after.
    scaled_at: Option<f64>,
    /// The frame's time last frame (seconds, [`Vr::clock`]): how far the stick moves a held model.
    last_frame: f64,
    /// The model was at real size last frame (a tick is felt when a grab snaps it there).
    real_size: bool,
    /// The design the model was placed for (`Session::uid`): another one opened or switched to is
    /// put in front again.
    design: Option<u64>,
    pointer: Pointer,
    /// Each hand's Select: short selects (or leaves the section where it is), held opens the menu.
    presses: [Press; 2],
    /// The ring menu, while Select holds it open.
    menu: Option<menu::Open>,
    /// Each hand's stick: left undoes, right redoes.
    flicks: [Flick; 2],
    /// The controllers' own models (by their asset's id), as the VR software handed them out.
    models: controllers::Models,
    /// Reads the models the VR software hands out, beside the frame.
    model_reader: controllers::Reader,
    /// The joints' marks, kept while the design and what is drawn of it stay the same.
    joint_marks: vr_pick::JointMarks,
    /// Which joint each joint's sign slot shows.
    joint_slots: JointSlots,
    /// The theme's generation, for the signs and the Browser's window.
    theme: sign::ThemeWatch,
    section: Section,
    /// When the session began: the presses' clock.
    clock: std::time::Instant,
}

/// The section plane in the headset (the window's own section is not touched).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Section {
    Off,
    /// Held at the tip of this hand's controller (its index, left first).
    Held(usize),
    /// Left in place in the model (model units): it moves with the model.
    Placed(Plane),
}

/// What the controllers' rays point at, and what that lit in the design.
#[derive(Default)]
struct Pointer {
    /// Each hand's, left first.
    pointed: [Option<Pointed>; 2],
    /// The bodies this lit in the design as pointed at (`vr_pick::light`).
    lit: Vec<String>,
    /// The parts [`Pointer::lit`] is for, in which design (`Session::uid`) at which `revision`.
    lit_for: (Vec<Sel>, u64, u64),
    parts: Parts,
    /// What each hand last did, shown at the controller until the time given.
    said: [Option<(String, std::time::Instant)>; 2],
}

/// How far past the controller's tip the sign saying what the hand did stands (metres).
const SAID_AHEAD: f64 = 0.06;

/// How long a sign at the controller says what the hand did.
const SAID_FOR: std::time::Duration = std::time::Duration::from_millis(1500);

/// What one controller's ray points at.
#[derive(Clone, Debug)]
struct Pointed {
    part: std::sync::Arc<Part>,
    /// Where the ray stops, in the room (metres).
    at: [f64; 3],
    /// How far the ray goes before it stops (metres).
    reach: f64,
}

/// At most this many bodies' parts are kept: past it the list starts again.
const MAX_PARTS: usize = 4096;

/// The parts bodies belong to, for one state of one design (`Session::uid` and `revision`, which
/// goes up with every change to the design, renames and moves included): finding a body's part
/// walks the assembly, too slow for every frame.
#[derive(Default)]
struct Parts {
    key: Option<(u64, u64)>,
    of: std::collections::HashMap<String, std::sync::Arc<Part>>,
}

impl Parts {
    /// The part `body` belongs to in `app`'s design as it is now.
    fn of(&mut self, app: &SolveApp, body: &str) -> std::sync::Arc<Part> {
        let key = (app.session.uid, app.session.revision);
        if self.key != Some(key) || self.of.len() >= MAX_PARTS {
            self.of.clear();
            self.key = Some(key);
        }
        self.of.entry(body.to_string()).or_insert_with(|| std::sync::Arc::new(vr_pick::part_of(app, body))).clone()
    }
}

/// The light buzz felt when a ray comes onto another part.
const ONTO_PART: Pulse = Pulse { amplitude: 0.15, seconds: 0.006 };

impl Pointer {
    /// Follow each `free` hand's ray to the part it points at, the model placed by `transform` and
    /// cut by `cut`, and light it. A hand holding the model, the section or the menu is not free
    /// and points at nothing. The buzz each hand gets when its ray comes onto another part.
    fn point(
        &mut self,
        app: &mut SolveApp,
        hands: &[HandInput; 2],
        free: [bool; 2],
        transform: Option<&Transform>,
        cut: Option<&Plane>,
        ray_length: [f64; 2],
    ) -> [Option<Pulse>; 2] {
        let mut pulses = [None; 2];
        // What the rays pass through: the headset's section, else the design view's, worked out
        // once for both (with the Section dialog open, on a copy of the design).
        let mut view_section = None;
        for hand in Hand::BOTH {
            let i = hand.index();
            let now = match (transform, hands[i].aim.as_ref()) {
                (Some(t), Some(aim)) if free[i] => {
                    let section = match cut {
                        Some(p) => Some((p.origin, p.normal)),
                        None => *view_section.get_or_insert_with(|| vr_pick::view_section(app)),
                    };
                    self.pointed_by(app, t, aim, section, ray_length[i])
                }
                _ => None,
            };
            let before = self.pointed[i].as_ref().map(|p| &p.part.sel);
            if let Some(p) = &now
                && before != Some(&p.part.sel)
            {
                pulses[i] = Some(ONTO_PART);
            }
            if self.said[i].as_ref().is_some_and(|(_, until)| std::time::Instant::now() >= *until) {
                self.said[i] = None;
            }
            self.pointed[i] = now;
        }
        // Lit again only when another part is pointed at, or the design changed (a part's bodies
        // may have): a part can hold thousands of bodies.
        let pointed = (self.pointed.iter().flatten().map(|p| p.part.sel.clone()).collect(), app.session.uid, app.session.revision);
        if pointed != self.lit_for {
            vr_pick::light(app, &mut self.lit, vr_pick::bodies_of(self.pointed.iter().flatten().map(|p| &*p.part)));
            self.lit_for = pointed;
        }
        pulses
    }

    /// Show `words` at hand `i`'s controller for a moment (nothing for no words).
    fn say(&mut self, i: usize, words: String) {
        if words.is_empty() {
            return;
        }
        if let Some(slot) = self.said.get_mut(i) {
            *slot = Some((words, std::time::Instant::now() + SAID_FOR));
        }
    }

    /// What the ray from `aim` (its −Z) stops at, the model placed by `transform` and cut by
    /// `section` (a point on its plane and the side it cuts toward, model units); nothing past
    /// the ray's end (`ray_length`, metres).
    fn pointed_by(
        &mut self,
        app: &SolveApp,
        transform: &Transform,
        aim: &Pose,
        section: Option<([f64; 3], [f64; 3])>,
        ray_length: f64,
    ) -> Option<Pointed> {
        let [_, _, back] = aim.axes()?;
        let forward = back.map(|v| -v);
        let hit = vr_pick::hit(app, transform.to_model(aim.position), transform.direction_to_model(forward), section)?;
        let reach = hit.depth * transform.scale;
        if !(reach.is_finite() && reach <= ray_length) {
            return None;
        }
        let part = self.parts.of(app, &hit.body);
        Some(Pointed { part, at: transform.apply(hit.point), reach })
    }

    /// No ray points at anything: what they lit goes dark.
    fn clear(&mut self, app: &mut SolveApp) {
        self.pointed = Default::default();
        self.said = Default::default();
        self.lit_for = Default::default();
        vr_pick::light(app, &mut self.lit, Vec::new());
    }
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
            scaled_at: None,
            belt: belt::Belt::default(),
            panel: None,
            window_hover: [None, None],
            window_pressed: [false, false],
            last_frame: 0.0,
            real_size: false,
            design: None,
            pointer: Pointer::default(),
            presses: Default::default(),
            menu: None,
            flicks: Default::default(),
            models: HashMap::new(),
            model_reader: controllers::Reader::default(),
            joint_marks: vr_pick::JointMarks::default(),
            joint_slots: JointSlots::default(),
            theme: sign::ThemeWatch::default(),
            section: Section::Off,
            clock: std::time::Instant::now(),
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
        // Preferences › XR › Disconnect: the session is asked to end; the next polls close it.
        if std::mem::take(&mut app.services.vr_disconnect)
            && let Some(session) = self.session.as_mut()
        {
            session.request_exit();
        }
        app.services.vr_connected = self.session.is_some();
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
        app.services.vr_passthrough = None;
        self.let_go(app);
        self.panel = None;
        self.gpu = None;
        self.session = None;
        self.source = None;
        if self.status == Status::Running || self.status == Status::Waiting {
            self.status = Status::Ended;
        }
    }

    /// Nothing is held, pressed or pointed at any more (the headset was taken off, or the
    /// session ends): the model and the belt are let go, the menu closes, the Browser's window
    /// lets go of its press and its pointer, and what the rays and the window lit in the design
    /// (on the desktop too) goes out.
    fn let_go(&mut self, app: &mut SolveApp) {
        self.grab.release();
        self.pointer.clear(app);
        if let Some(panel) = self.panel.as_mut() {
            panel.close(app);
        }
        self.window_hover = [None, None];
        self.window_pressed = [false, false];
        self.menu = None;
        self.belt.held = None;
        self.belt.hover = [None, None];
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
                self.let_go(app);
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
        // The controllers' own models: read beside the frame when the VR software hands them out
        // (again when other controllers are picked up), placed every frame. Until they are read
        // the ones there were are drawn (a controller of the same kind is still drawn as one).
        if let Some(assets) = session.new_controller_models() {
            self.model_reader.start(assets);
        }
        if let Some(models) = self.model_reader.poll() {
            self.models = models;
        }
        let draws: Vec<controllers::Draw> = session
            .controller_models(&frame)
            .into_iter()
            .filter_map(|m| {
                let model = self.models.get(&m.cache)?.clone();
                let pose = solidcraft_vr::model::pose_matrix(&m.pose?)?;
                let places = model.places(&m.nodes);
                Some(controllers::Draw { key: (m.cache, m.hand.map(Hand::index)), model, pose, places })
            })
            .collect();
        // How VR is set to work, read every frame: a change in Preferences applies at once (read
        // settings are already at the current layout: `VrSettings::from_saved`).
        let set = app.preferences.vr.sane();
        app.services.vr_passthrough = Some(session.set_passthrough(set.passthrough));
        let buzz = |session: &solidcraft_xr::Session, hand: Hand, pulse: Pulse| {
            session.pulse(hand, Pulse { amplitude: pulse.amplitude * set.haptics, ..pulse });
        };
        let now = self.clock.elapsed().as_secs_f64();
        let dt = (now - std::mem::replace(&mut self.last_frame, now)).clamp(0.0, 0.1);
        for hand in Hand::BOTH {
            let i = hand.index();
            let bindings = [&set.grab, &set.select, &set.menu, &set.put_back, &set.section, &set.undo, &set.redo];
            for (button, binding) in self.buttons[i].iter_mut().zip(bindings) {
                *button = button.next(action_value(&hands[i], binding, i));
            }
            if self.buttons[i][GRAB].pressed {
                buzz(session, hand, Pulse::GRAB);
            }
        }
        let head = frame.eyes.as_ref().map(|[l, r]| [0, 1, 2].map(|k| (l.pose.position[k] + r.pose.position[k]) * 0.5));
        // The first frame with a model and a tracked head puts the model in front; so do "Put
        // back", another design opened or switched to, and the VR software recentring the room.
        // Asked for, it waits for a frame where it can be done.
        self.put_back |= session.recentred();
        let design = app.session.uid;
        // A section or a menu of another design's means nothing in this one.
        if self.design.is_some_and(|d| d != design) {
            self.section = Section::Off;
            self.menu = None;
        }
        let place = [set.first_distance, set.first_size, set.first_drop].map(f64::from);
        if (self.transform.is_none() || self.put_back || self.design != Some(design))
            && let (Some(source), Some(eyes)) = (source, &frame.eyes)
            && let Some(placed) = first_placement(source, eyes, model_box(app), place)
        {
            self.grab.release();
            self.transform = Some(placed);
            self.design = Some(design);
            if self.put_back {
                self.belt.put_back();
            }
            self.put_back = false;
        }
        let mode = solidcraft_vr::grab::Mode {
            two_hands: match set.two_hands {
                settings::TwoHands::ScaleMove => solidcraft_vr::grab::TwoHands::ScaleMove,
                settings::TwoHands::Equal => solidcraft_vr::grab::TwoHands::Equal,
                settings::TwoHands::Lead => solidcraft_vr::grab::TwoHands::Lead,
            },
            snap: set.snap_real_size,
        };
        // Grab on the belt (a bubble or its handle, pointed at or touched) moves the belt, not the
        // model; letting go leaves it there.
        let hand_frame = |i: usize| hands[i].grip.or(hands[i].aim).and_then(|p| Some((p.position, p.axes()?)));
        for i in 0..2 {
            let b = self.buttons[i];
            if b[GRAB].pressed
                && set.belt
                && self.belt.held.is_none()
                && (self.belt.hover[i].is_some() || self.window_hover[i].is_some())
                && let Some(frame) = hand_frame(i)
            {
                self.belt.grab(i, frame);
                buzz(session, Hand::BOTH[i], Pulse::GRAB);
            }
            if !b[GRAB].down && self.belt.held.is_some_and(|h| h.hand == i) {
                self.belt.held = None;
            }
        }
        let belt_hand = self.belt.held.map(|h| h.hand);
        if let Some(current) = &self.transform {
            let grips = Hand::BOTH.map(|hand| {
                let i = hand.index();
                hands[i].grip.filter(|_| self.buttons[i][GRAB].down && belt_hand != Some(i))
            });
            self.transform = Some(self.grab.update_with(current, grips, mode));
        }
        // Snapped to real size by a grab: a tick in both hands.
        let real_size = self.transform.is_some_and(|t| t.scale == room::REAL_SIZE);
        if real_size && !self.real_size && self.grab.holding() && set.snap_real_size {
            for hand in Hand::BOTH {
                buzz(session, hand, Pulse::TICK);
            }
        }
        self.real_size = real_size;
        // Both hands scaling: the scale is shown (and for a while after).
        let both = self.buttons.iter().all(|b| b[GRAB].down) && self.grab.holding();
        if both {
            self.scaled_at = Some(now);
        }

        // How big the Browser's window is, in the window's points (what fits in it), and how its
        // picture is drawn: the pointer, the picture and its board all go by this.
        let window = panel::drawable(egui::vec2(set.browser_width, set.browser_width / belt::WINDOW_RATIO as f32));
        // Select on the Browser's window is its pointer's button, pressed and let go: down when
        // the first hand presses on it, up when the last one lets go.
        let pressed_before = self.window_pressed;
        for i in 0..2 {
            let b = self.buttons[i][SELECT];
            if b.pressed && self.window_hover[i].is_some() {
                self.window_pressed[i] = true;
            }
            if self.window_pressed[i] && !b.down {
                self.window_pressed[i] = false;
            }
        }
        if let Some(down) = panel::button_change(pressed_before, self.window_pressed)
            && let Some(panel) = self.panel.as_mut()
        {
            panel.press(down);
        }
        // The belt this frame: what its items say, and where it stands.
        let belt_state = belt::State {
            designs: solidcraft_ui_egui::documents::tabs_info(app)
                .into_iter()
                .map(|(name, dirty)| if dirty { format!("{name} •") } else { name })
                .collect(),
            active: app.docs.active,
            passthrough: set.passthrough,
            passthrough_offered: session.passthrough() || app.services.vr_passthrough == Some(true),
            can_undo: !app.session.undo.is_empty(),
            can_redo: !app.session.redo.is_empty(),
        };

        // Select acts at once, and the ring menu has a button of its own: pressed, the ring opens
        // at the hand; let go, the item under the hand is taken. Without a menu button, holding
        // Select opens the ring (after `hold_time`) and a quick Select selects when let go.
        let menu_by_select = set.menu.is_empty();
        for hand in Hand::BOTH {
            let i = hand.index();
            let b = self.buttons[i];
            let holds = b[GRAB].down;
            // Letting go of what holds the menu open takes the item under the hand.
            let closes = if menu_by_select { b[SELECT].released } else { b[MENU].released };
            if closes && self.menu.as_ref().is_some_and(|m| m.hand == i) {
                if let Some(open) = self.menu.take()
                    && let Some(act) = open.chosen()
                {
                    let said = act_on(app, act, open.target.as_deref(), i, &mut self.section, &mut self.put_back);
                    self.pointer.say(i, said);
                    buzz(session, hand, Pulse::TICK);
                }
                if menu_by_select {
                    // The press that opened the menu is not a click as well.
                    self.presses[i].next_after(false, now, f64::from(set.hold_time));
                    continue;
                }
            }
            let (click, open) = if menu_by_select {
                match self.presses[i].next_after(b[SELECT].down, now, f64::from(set.hold_time)) {
                    Some(press::Kind::Short) => (true, false),
                    Some(press::Kind::Long) => (false, true),
                    None => (false, false),
                }
            } else {
                (b[SELECT].pressed, b[MENU].pressed)
            };
            if open && self.menu.is_none() && !holds && self.window_hover[i].is_none() && !self.window_pressed[i] {
                let target = self.pointer.pointed[i].as_ref().map(|p| p.part.clone());
                // The part actions act on what is pointed at, else on the selection.
                let parts = vr_pick::acted_on(app, target.as_deref());
                let state = menu::State {
                    parts: !parts.is_empty(),
                    hidden: vr_pick::all_hidden(app, &parts),
                    exploded: !app.session.exploded.is_empty(),
                    section: self.section != Section::Off,
                    can_undo: !app.session.undo.is_empty(),
                    can_redo: !app.session.redo.is_empty(),
                    passthrough_offered: app.services.vr_passthrough == Some(true),
                    passthrough: set.passthrough,
                };
                if let (Some(aim), Some(head)) = (hands[i].aim, head) {
                    self.menu = menu::Open::new(i, aim.position, head, f64::from(set.menu_size), target, state, &set);
                    if self.menu.is_some() {
                        buzz(session, hand, Pulse::GRAB);
                    }
                }
            }
            if click && !holds && self.menu.as_ref().is_none_or(|m| m.hand != i) {
                if let Some((spot, _)) = self.belt.hover[i] {
                    let said = belt_act(app, &mut self.belt, spot, &belt_state, &mut self.put_back);
                    self.pointer.say(i, said);
                } else if self.window_hover[i].is_some() || self.window_pressed[i] {
                    // The window took it as its pointer's click.
                } else if self.section == Section::Held(i) {
                    if let Some(placed) = held_plane(&hands, i).zip(self.transform).and_then(|(p, t)| p.to_model(&t)) {
                        self.section = Section::Placed(placed);
                        self.pointer.say(i, solidcraft_ui_text::tr("vr.section_placed").to_string());
                    }
                } else {
                    vr_pick::toggle(app, self.pointer.pointed[i].as_ref().map(|p| &*p.part));
                }
                buzz(session, hand, Pulse::TICK);
            }
            // Put back, the section, undo and redo, where the settings give them a button.
            for (button, act) in [(PUT_BACK, Act::PutBack), (SECTION, Act::Section), (UNDO, Act::Undo), (REDO, Act::Redo)] {
                if b[button].pressed {
                    let said = act_on(app, act, None, i, &mut self.section, &mut self.put_back);
                    self.pointer.say(i, said);
                    buzz(session, hand, Pulse::TICK);
                }
            }
        }
        // The open menu follows the hand holding it.
        if let Some(open) = self.menu.as_mut()
            && let Some(aim) = hands[open.hand].aim
        {
            match open.moved(aim.position, now, f64::from(set.section_wait)) {
                Moved::Marked => buzz(session, Hand::BOTH[open.hand], ONTO_PART),
                Moved::Ring => buzz(session, Hand::BOTH[open.hand], Pulse::TICK),
                Moved::Nothing => {}
            }
        }
        // The sticks. Holding the model with one hand (when set): its stick pulls the model closer
        // or pushes it away, the other hand's stick turns it. Otherwise left and right undo and
        // redo (when set). Every stick is read every frame, so a push made while it did something
        // else does not count later.
        let flicks: [Option<Direction>; 2] = std::array::from_fn(|i| self.flicks[i].next(hands[i].stick));
        let holder = match self.buttons.map(|b| b[GRAB].down) {
            [true, false] => Some(0),
            [false, true] => Some(1),
            _ => None,
        }
        .filter(|_| set.hold_stick && self.grab.holding());
        let mut stick_words: [Option<StickWords>; 2] = [None, None];
        if let (Some(h), Some(t)) = (holder, self.transform) {
            // Which stick does what while one hand holds: the right one moves and turns, the left
            // one rolls and tips (the default); or the holding hand's moves and rolls and the
            // other's turns and tips.
            let (mover, tipper) = match set.hold_sticks {
                settings::HoldSticks::RightMoves => (1, 0),
                settings::HoldSticks::HoldingMoves => (h, 1 - h),
            };
            let pivot = t.apply(t.pivot);
            let mut moved = t;
            // Along the holding hand's ray: up pushes the model the way the ray points.
            let push = stick_curve(hands[mover].stick[1]);
            if push != 0.0
                && let Some(aim) = &hands[h].aim
                && let Some([_, _, back]) = aim.axes()
            {
                let step = push * f64::from(set.push_speed) * dt;
                // Never pulled back through the hand.
                if let Some(by) = pushed(pivot, aim.position, back.map(|v| -v), step) {
                    moved.translation = [0, 1, 2].map(|k| moved.translation[k] + by[k]);
                }
            }
            let speed = f64::from(set.turn_speed).to_radians() * dt;
            let sight = head.and_then(|head| unit3([0, 1, 2].map(|k| pivot[k] - head[k])));
            let across = sight.and_then(|s| unit3([-s[2], 0.0, s[0]]));
            let yaw = stick_curve(hands[mover].stick[0]);
            if yaw != 0.0 {
                moved = solidcraft_vr::grab::turned_about_vertical(&moved, pivot, -yaw * speed);
            }
            let pitch = stick_curve(hands[tipper].stick[1]);
            if pitch != 0.0
                && let Some(across) = across
            {
                moved = solidcraft_vr::grab::turned_about(&moved, pivot, across, -pitch * speed);
            }
            let roll = stick_curve(hands[tipper].stick[0]);
            if roll != 0.0
                && let Some(sight) = sight
            {
                moved = solidcraft_vr::grab::turned_about(&moved, pivot, sight, roll * speed);
            }
            if moved != t
                && let Some(tidy) = moved.tidy()
            {
                self.transform = Some(tidy);
                // The hand holds on from where the model is now.
                self.grab.release();
            }
            stick_words[mover] = Some(StickWords::both(["vr.stick_turn_left", "vr.stick_turn_right"], ["vr.stick_closer", "vr.stick_away"]));
            stick_words[tipper] = Some(StickWords::both(["vr.stick_roll_left", "vr.stick_roll_right"], ["vr.stick_tip_toward", "vr.stick_tip_away"]));
        } else if let Some(h) = self.belt.held.map(|h| h.hand) {
            // Holding the belt, its stick pulls it closer or pushes it away.
            let push = f64::from(hands[h].stick[1]);
            if push.abs() > 0.15 {
                self.belt.push(push * f64::from(set.push_speed) * dt);
            }
            stick_words[h] = Some(StickWords::vertical("vr.stick_away", "vr.stick_closer"));
        } else {
            for hand in Hand::BOTH {
                let i = hand.index();
                // On the Browser's window, the stick scrolls it.
                if self.window_hover[i].is_some() && !self.buttons[i][GRAB].down {
                    let push = hands[i].stick[1];
                    if push.abs() > 0.15
                        && let Some(panel) = self.panel.as_mut()
                    {
                        panel.scroll(push * WHEEL_SPEED * dt as f32);
                    }
                    stick_words[i] = Some(StickWords::vertical("vr.stick_scroll_up", "vr.stick_scroll_down"));
                    continue;
                }
                if set.stick != settings::StickAction::UndoRedo {
                    continue;
                }
                let act = match flicks[i] {
                    Some(Direction::Left) => Some(Act::Undo),
                    Some(Direction::Right) => Some(Act::Redo),
                    _ => None,
                };
                if let Some(act) = act.filter(|_| !self.buttons[i][GRAB].down) {
                    let said = act_on(app, act, None, i, &mut self.section, &mut self.put_back);
                    self.pointer.say(i, said);
                    buzz(session, hand, Pulse::TICK);
                }
                stick_words[i] = Some(StickWords::across("vr.menu_undo", "vr.menu_redo"));
            }
        }
        // The section this frame, in the model (for the cut and the ray) and in the room (for its
        // outline).
        let (cut, outline) = match (self.section, self.transform) {
            (Section::Held(i), Some(t)) => {
                let room = held_plane(&hands, i);
                (room.and_then(|p| p.to_model(&t)), room)
            }
            (Section::Placed(p), Some(t)) => (Some(p), p.to_room(&t)),
            _ => (None, None),
        };
        let free = Hand::BOTH.map(|hand| {
            let i = hand.index();
            !self.buttons[i][GRAB].down && self.section != Section::Held(i) && self.menu.as_ref().is_none_or(|m| m.hand != i)
        });
        // The belt, and what each free hand's ray reaches on it: nearer than the model, the belt
        // takes the ray (and Select).
        let open_count = self.belt.open.and_then(|g| belt::GROUPS.get(g)).map_or(0, |g| belt::items(*g, &belt_state).len());
        let layout = match (set.belt, head, frame.eyes.as_ref().and_then(|e| e[0].pose.axes())) {
            (true, Some(head), Some([_, _, back])) => Some(self.belt.place(
                head,
                back,
                f64::from(set.belt_drop),
                f64::from(set.belt_distance),
                f64::from(set.belt_curve),
                f64::from(set.menu_bubble),
                open_count,
                [0, 1].map(hand_frame),
                (f64::from(set.browser_height), set.browser_side),
            )),
            _ => {
                self.belt.open = None;
                None
            }
        };
        // What each free hand's ray reaches on the belt and on the Browser's window: the nearer
        // takes it.
        let mut belt_hits: [Option<(belt::Spot, f64)>; 2] = [None, None];
        let mut window_hits: [Option<([f64; 2], f64)>; 2] = [None, None];
        for i in 0..2 {
            let Some(aim) = hands[i].aim.as_ref().filter(|_| free[i]) else { continue };
            let Some([_, _, back]) = aim.axes() else { continue };
            let Some(layout) = layout.as_ref() else { continue };
            let dir = back.map(|v| -v);
            let bubble = layout.hit(aim.position, dir);
            let window = layout.window_hit(aim.position, dir);
            match (bubble, window) {
                (Some(b), Some(w)) if w.1 < b.1 => window_hits[i] = Some(w),
                (Some(b), _) => belt_hits[i] = Some(b),
                (None, w) => window_hits[i] = w,
            }
        }
        let ray_length: [f64; 2] = std::array::from_fn(|i| {
            let on = belt_hits[i].map(|(_, t)| t).or(window_hits[i].map(|(_, t)| t));
            on.map_or(f64::from(set.ray_length), |t| t.min(f64::from(set.ray_length)))
        });
        let pulses = self.pointer.point(app, &hands, free, self.transform.as_ref(), cut.as_ref(), ray_length);
        for (i, hit) in belt_hits.iter().enumerate() {
            if hit.is_some() && self.belt.hover[i].map(|h| h.0) != hit.map(|h| h.0) {
                buzz(session, Hand::BOTH[i], ONTO_PART);
            }
        }
        self.belt.hover = belt_hits;
        for (i, hit) in window_hits.iter().enumerate() {
            if hit.is_some() && self.window_hover[i].is_none() {
                buzz(session, Hand::BOTH[i], ONTO_PART);
            }
        }
        self.window_hover = window_hits;
        // The window's pointer: the hand that pressed on it, else the first on it.
        let window_open = layout.as_ref().is_some_and(|l| l.window.is_some());
        if window_open {
            let device = &rs.device;
            let panel = self.panel.get_or_insert_with(|| panel::Panel::new(device));
            let pressing = (0..2).find(|i| self.window_pressed[*i]);
            let at = pressing.and_then(|i| self.window_hover[i]).or(self.window_hover.iter().flatten().next().copied()).map(|(uv, _)| uv);
            panel.point(at, window.points);
        } else if let Some(panel) = self.panel.as_mut() {
            panel.close(app);
        }
        for (hand, pulse) in Hand::BOTH.into_iter().zip(pulses) {
            if let Some(pulse) = pulse {
                buzz(session, hand, pulse);
            }
        }
        let held = self.buttons.map(|b| [b[GRAB].down, b[SELECT].down]);
        // The scale, while both hands scale (and a moment after), or always.
        let readout = match (set.scale_readout, self.transform) {
            (settings::ScaleReadout::Never, _) | (_, None) => None,
            (shown, Some(t)) => {
                let recent = self.scaled_at.is_some_and(|at| now - at < READOUT_FOR);
                let between = (both || recent)
                    .then(|| hands[0].grip.zip(hands[1].grip))
                    .flatten()
                    .map(|(l, r)| [0, 1, 2].map(|k| (l.position[k] + r.position[k]) * 0.5 + if k == 1 { 0.08 } else { 0.0 }));
                let above = || {
                    let p = t.apply(t.pivot);
                    [p[0], p[1] + 0.12, p[2]]
                };
                match (between, shown) {
                    (Some(at), _) => Some((scale_words(t.scale), at)),
                    (None, settings::ScaleReadout::Always) => Some((scale_words(t.scale), above())),
                    _ => None,
                }
            }
        };
        let floor = session.floor_height(&frame);
        let reach: [Option<f64>; 2] = std::array::from_fn(|i| {
            let on_belt = self.belt.hover[i].map(|(_, t)| t).or(self.window_hover[i].map(|(_, t)| t));
            self.pointer.pointed[i].as_ref().map(|p| p.reach).or(on_belt).filter(|t| *t > 0.0)
        });
        // A hand with its controller's own model drawn has no box drawn for it; its stick's
        // labels stand at the model's stick.
        let mut modelled = [false; 2];
        let mut sticks: [Option<Stick>; 2] = [None; 2];
        for draw in &draws {
            let Some(i) = draw.key.1.filter(|i| *i < 2) else { continue };
            modelled[i] = true;
            if let Some(node) = draw.model.stick()
                && let Some(Some(place)) = draw.places.get(node)
            {
                // Where the stick's own mesh is: a runtime often leaves the node at the model's
                // origin and puts the part's place in its vertices.
                let m = solidcraft_vr::model::mul(&draw.pose, place);
                let [x, y, z] = draw.model.middle_of(node).unwrap_or([0.0; 3]);
                let at = [0, 1, 2].map(|k| m[0][k] * x + m[1][k] * y + m[2][k] * z + m[3][k]);
                let p = &draw.pose;
                sticks[i] =
                    Some(Stick { at, right: [p[0][0], p[0][1], p[0][2]], up: [p[1][0], p[1][1], p[1][2]], forward: [-p[2][0], -p[2][1], -p[2][2]] });
            }
        }
        // Where the model names no stick (or there is no model), where sticks usually sit.
        for (i, stick) in sticks.iter_mut().enumerate() {
            if stick.is_none() {
                *stick = hands[i].grip.as_ref().and_then(stick_at);
            }
        }
        let boxed = Hand::BOTH.map(|hand| {
            let i = hand.index();
            HandInput { grip: hands[i].grip.filter(|_| !modelled[i]), ..hands[i] }
        });
        let words = stick_words.map(|w| w.filter(|_| set.stick_labels));
        let mut boards = boards(&self.pointer, self.menu.as_ref(), &hands, sticks, words, readout, &set);
        if let Some(layout) = &layout {
            belt_boards(&mut boards, layout, &self.belt, &belt_state, set.fade);
        }
        // The joints' glyphs at the joints: the nearest where there are more than there is room
        // for, each as big to the eye wherever it is, each joint keeping its slot (and so its
        // painted glyph) while it is shown.
        let shown_joints = match (set.joints, self.transform, head) {
            (true, Some(t), Some(head)) => {
                let far = |p: &[f64; 3]| (0..3).map(|k| (p[k] - head[k]).powi(2)).sum::<f64>();
                let marks = self.joint_marks.get(app).iter().map(|m| {
                    let at = t.apply(m.at);
                    (far(&at), (m.id, at, m.badge))
                });
                nearest(marks.collect(), MAX_JOINTS)
            }
            _ => Vec::new(),
        };
        let ids: Vec<u64> = shown_joints.iter().map(|(_, (id, _, _))| *id).collect();
        let tokens = solidcraft_theme::Tokens::get();
        for ((far, (_, at, badge)), k) in shown_joints.into_iter().zip(self.joint_slots.assign(&ids)) {
            let Some(k) = k else { continue };
            let height = (far.sqrt() * JOINT_PER_METRE).clamp(JOINT_MIN, JOINT_MAX) * f64::from(set.sign_size);
            boards[JOINTS + k] = Some(Board {
                text: String::new(),
                at,
                height: Some(height),
                factor: 1.0,
                look: sign::Look::Joint { badge },
                fill: tokens.nav_tip,
                ink: tokens.nav_tip_ink,
                bend: None,
            });
        }
        // What the signs and the window are painted in: the window's fonts and the theme.
        let fonts = ctx.fonts(|f| sign::fonts_fingerprint(f.definitions()));
        let theme = self.theme.generation(tokens);
        // The Browser's window drawn again where it needs to be, then its board: a new picture
        // made by this drawing is bound anew (its generation is read after it is made).
        if window_open && let Some(panel) = self.panel.as_mut() {
            panel.paint(rs, ctx, fonts, theme, app, window, now);
        }
        let panel = self.panel.as_ref().filter(|_| window_open);
        let picture = panel.and_then(panel::Panel::picture);
        if let (Some(layout), Some(panel)) = (&layout, panel)
            && let Some(area) = layout.window
        {
            boards[WINDOW] = Some(Board {
                text: panel.generation.to_string(),
                at: area.middle,
                height: Some(area.height),
                factor: 1.0,
                look: sign::Look::Picture { size: window.pixels },
                fill: tokens.dialog_body,
                ink: tokens.form_ink,
                bend: Some(layout.bend),
            });
        }
        let language = app.preferences.language.as_str();
        let ray_length = f64::from(set.ray_length);
        let scene = Scene {
            picture,
            world: [set.grid, set.horizon, set.grid_reach],
            source,
            transform: self.transform.as_ref(),
            hands: &boxed,
            held,
            floor,
            reach,
            ray_length,
            boards,
            head,
            ctx,
            language,
            fonts,
            theme,
            cut,
            outline,
            passthrough: session.passthrough(),
            controllers: draws,
            live_models: self.models.keys().copied().collect(),
        };
        let drawn = if frame.should_render { gpu.draw(session, rs, &frame, &scene) } else { Ok(false) };
        // A begun frame is ended whatever happened in it.
        let ended = session.end_frame(&frame, *drawn.as_ref().unwrap_or(&false));
        ctx.request_repaint();
        drawn.map(|_| ()).and(ended)
    }
}

/// The actions the settings bind to inputs: places in [`Vr::buttons`].
const GRAB: usize = 0;
const SELECT: usize = 1;
const MENU: usize = 2;
const PUT_BACK: usize = 3;
const SECTION: usize = 4;
const UNDO: usize = 5;
const REDO: usize = 6;
const ACTIONS: usize = 7;

/// How long the scale stays shown after both hands let go (seconds).
const READOUT_FOR: f64 = 1.5;

/// How far `hand` (hand `i`) presses the action `binding` binds, 0 to 1: the most of its inputs.
fn action_value(hand: &HandInput, binding: &settings::Binding, i: usize) -> f32 {
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    Input::ALL
        .iter()
        .filter(|input| binding.reads(i, **input))
        .map(|input| match input {
            Input::Trigger => hand.trigger,
            Input::Grip => hand.squeeze,
            Input::Primary => on(hand.primary),
            Input::Secondary => on(hand.secondary),
            Input::StickClick => on(hand.stick_click),
            Input::Menu => on(hand.menu),
        })
        .fold(0.0, f32::max)
}

/// The scale as a ratio to real size: `1:1`, `2:1`, `1:4`, `1:2.5`.
fn scale_words(scale: f64) -> String {
    let ratio = scale / room::REAL_SIZE;
    if !ratio.is_finite() || ratio <= 0.0 {
        return String::new();
    }
    let short = |v: f64| {
        let digits = if v >= 10.0 { 0 } else { 1 };
        let text = format!("{v:.digits$}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if (ratio - 1.0).abs() < 0.005 {
        "1:1".into()
    } else if ratio > 1.0 {
        format!("{}:1", short(ratio))
    } else {
        format!("1:{}", short(1.0 / ratio))
    }
}

/// Where a controller's stick is: its place, and the controller's right and forward.
#[derive(Clone, Copy, Debug)]
struct Stick {
    at: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    forward: [f64; 3],
}

/// What a stick's directions do, written beside it (catalogue entries).
#[derive(Clone, Copy, Debug)]
struct StickWords {
    /// Left, then right.
    across: Option<[&'static str; 2]>,
    /// Down, then up.
    vertical: Option<[&'static str; 2]>,
}

impl StickWords {
    /// Left and right do these.
    fn across(left: &'static str, right: &'static str) -> StickWords {
        StickWords { across: Some([left, right]), vertical: None }
    }

    /// Up and down do these.
    fn vertical(up: &'static str, down: &'static str) -> StickWords {
        StickWords { across: None, vertical: Some([down, up]) }
    }

    /// Left and right do `across` (left first), down and up `vertical` (down first).
    fn both(across: [&'static str; 2], vertical: [&'static str; 2]) -> StickWords {
        StickWords { across: Some(across), vertical: Some(vertical) }
    }
}

/// A stick's push (−1 to 1) as a speed: nothing within the middle's play, then growing
/// smoothly from nothing (squared), so a motion starts gently instead of at once.
/// A reading that is not a number moves nothing.
fn stick_curve(v: f32) -> f64 {
    const PLAY: f64 = 0.15;
    let v = f64::from(v);
    if !v.is_finite() {
        return 0.0;
    }
    let v = v.clamp(-1.0, 1.0);
    let past = ((v.abs() - PLAY) / (1.0 - PLAY)).max(0.0);
    past * past * v.signum()
}

/// How near the holding hand (metres, along its ray) the stick may pull a held model's pivot.
const PUSH_NEAREST: f64 = 0.05;

/// The move (room, metres) that pushes a held model whose pivot is at `pivot` `step` further
/// along the holding hand's ray (from `hand`, along unit `ahead`; less than nothing: back toward
/// the hand). `None` where a pull would bring the pivot nearer the hand than [`PUSH_NEAREST`]
/// (never back through it) or the move is not a number.
fn pushed(pivot: [f64; 3], hand: [f64; 3], ahead: [f64; 3], step: f64) -> Option<[f64; 3]> {
    let along = (0..3).map(|k| (pivot[k] - hand[k]) * ahead[k]).sum::<f64>();
    let by = (step > 0.0 || along + step > PUSH_NEAREST).then(|| ahead.map(|v| v * step))?;
    by.iter().all(|v| v.is_finite()).then_some(by)
}

/// `v` made a unit vector, `None` when it has no length.
fn unit3(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    (l > 1e-9 && l.is_finite()).then(|| v.map(|x| x / l))
}

/// Do `act`, chosen with hand `i` (from the menu or the stick), `target` the part its ray pointed
/// at when the menu opened. Hiding and isolating act on the selection when `target` is in it. What
/// was done, in the user's language, for the sign at the controller (nothing for nothing done).
fn act_on(app: &mut SolveApp, act: Act, target: Option<&Part>, i: usize, section: &mut Section, put_back: &mut bool) -> String {
    use solidcraft_ui_text::tr;
    let done = |result: Result<(), String>, words: &str| match result {
        Ok(()) => tr(words).to_string(),
        Err(why) => why,
    };
    match act {
        Act::Hide | Act::Isolate | Act::ToggleHide => {
            // What is pointed at (with the selection, when it is in it), else the selection.
            let parts = vr_pick::acted_on(app, target);
            if parts.is_empty() {
                return tr("vr.nothing_selected").to_string();
            }
            match act {
                Act::Isolate => done(vr_pick::isolate(app, &parts), "vr.did_isolate"),
                Act::ToggleHide if vr_pick::all_hidden(app, &parts) => done(vr_pick::show(app, &parts), "vr.did_show"),
                _ => done(vr_pick::hide(app, &parts), "vr.did_hide"),
            }
        }
        Act::Passthrough if app.services.vr_passthrough != Some(true) => tr("vr.passthrough_missing").to_string(),
        Act::Passthrough => {
            app.preferences.vr.passthrough = !app.preferences.vr.passthrough;
            tr(if app.preferences.vr.passthrough { "vr.passthrough_on" } else { "vr.passthrough_off" }).to_string()
        }
        Act::ShowAll => done(vr_pick::show_all(app), "vr.did_show_all"),
        Act::Explode => match vr_pick::explode(app) {
            Ok(true) => tr("vr.did_explode").to_string(),
            Ok(false) => tr("vr.did_assemble").to_string(),
            Err(why) => why,
        },
        Act::Section if *section == Section::Off => {
            *section = Section::Held(i);
            tr("vr.section_on").to_string()
        }
        Act::Section => {
            *section = Section::Off;
            tr("vr.section_off").to_string()
        }
        Act::PutBack => {
            *put_back = true;
            tr("vr.did_put_back").to_string()
        }
        Act::Undo if app.session.undo.is_empty() => tr("vr.nothing_to_undo").to_string(),
        Act::Redo if app.session.redo.is_empty() => tr("vr.nothing_to_redo").to_string(),
        Act::Undo => {
            app.start("edit.undo");
            tr("vr.did_undo").to_string()
        }
        Act::Redo => {
            app.start("edit.redo");
            tr("vr.did_redo").to_string()
        }
        Act::Group(_) => String::new(),
    }
}

/// The signs in the room: [`BOARDS`] places, each always the same sign's (so its picture is
/// painted again only when its words or colours change). What each ray points at; what each hand
/// just did, past its tip; what the stick does, beside it (`sticks`, `words`); the open menu's
/// items, the one under the hand bigger; the scale (`readout`). Sizes as `set` says.
fn boards(
    pointer: &Pointer,
    open: Option<&menu::Open>,
    hands: &[HandInput; 2],
    sticks: [Option<Stick>; 2],
    words: [Option<StickWords>; 2],
    readout: Option<(String, [f64; 3])>,
    set: &VrSettings,
) -> [Option<Board>; BOARDS] {
    use solidcraft_ui_text::tr;
    let t = solidcraft_theme::Tokens::get();
    let size = f64::from(set.sign_size);
    let plain = |text: String, at: [f64; 3], height: Option<f64>| Board {
        text,
        at,
        height: height.map(|h| h * size),
        factor: size,
        look: sign::Look::Sign,
        fill: t.nav_tip,
        ink: t.nav_tip_ink,
        bend: None,
    };
    let mut out: [Option<Board>; BOARDS] = std::array::from_fn(|_| None);
    for i in 0..2 {
        out[i] = pointer.pointed[i].as_ref().map(|p| plain(p.part.name.clone(), p.at, None));
        out[2 + i] = pointer.said[i].as_ref().and_then(|(text, _)| {
            let aim = hands[i].aim.as_ref()?;
            let [_, _, back] = aim.axes()?;
            Some(plain(text.clone(), [0, 1, 2].map(|k| aim.position[k] - back[k] * SAID_AHEAD), Some(SAID_HEIGHT)))
        });
        if hands[i].has_stick
            && let (Some(stick), Some(words)) = (sticks[i], words[i])
        {
            let (gap, lift) = (f64::from(set.stick_label_gap), f64::from(set.stick_label_lift));
            let beside = |along: [f64; 3], side: f64| [0, 1, 2].map(|k| stick.at[k] + along[k] * side * gap + stick.up[k] * lift);
            let first = STICK_SLOTS + 4 * i;
            for (k, (pair, along)) in [(words.across, stick.right), (words.vertical, stick.forward)].into_iter().enumerate() {
                if let Some([less, more]) = pair {
                    out[first + 2 * k] = Some(plain(tr(less).to_string(), beside(along, -1.0), Some(STICK_LABEL_HEIGHT)));
                    out[first + 2 * k + 1] = Some(plain(tr(more).to_string(), beside(along, 1.0), Some(STICK_LABEL_HEIGHT)));
                }
            }
        }
    }
    // The menu's tree: every open ring's bubbles. The way down the tree is lit; a ring the hand has
    // gone past is faded, as is a slot that cannot be taken.
    let fade = (set.fade * 100.0).round().clamp(5.0, 100.0) as u8;
    if let Some(open) = open {
        let last = open.levels.len().saturating_sub(1);
        for (l, level) in open.levels.iter().enumerate().take(MENU_LEVELS) {
            for (k, item) in level.items.iter().enumerate().take(solidcraft_vr::menu::MAX_ITEMS) {
                let marked = level.marked == Some(k);
                let (fill, ink) = match (marked, item.on && l == last) {
                    (true, _) => (t.accent, t.on_accent),
                    (false, true) => (t.nav_tip, t.nav_tip_ink),
                    (false, false) => (t.nav_tip, t.text_dim),
                };
                let height = f64::from(set.menu_bubble) * if marked && l == last { f64::from(set.menu_grow) } else { 1.0 };
                out[MENU_SLOTS + l * solidcraft_vr::menu::MAX_ITEMS + k] = Some(Board {
                    text: item.words.clone(),
                    at: level.ring.item(k, level.items.len()),
                    height: Some(height),
                    factor: 1.0,
                    look: sign::Look::Bubble { icon: item.icon, lit: marked, fade: if item.on && l == last { 100 } else { fade } },
                    fill,
                    ink,
                    bend: None,
                });
            }
        }
    }
    out[READOUT] = readout.map(|(text, at)| plain(text, at, Some(READOUT_HEIGHT)));
    out
}

/// Where the stick is on a controller held at `grip`: a guess from where sticks sit on the
/// controllers SolidCraft knows (above and ahead of the hand).
fn stick_at(grip: &Pose) -> Option<Stick> {
    let [right, up, back] = grip.axes()?;
    let at = [0, 1, 2].map(|k| grip.position[k] + up[k] * 0.035 - back[k] * 0.03);
    Some(Stick { at, right, up, forward: back.map(|v| -v) })
}

/// How many rings of the menu's tree are drawn at once.
const MENU_LEVELS: usize = 2;

/// Where the scale's sign is among the boards.
const READOUT: usize = MENU_SLOTS + MENU_LEVELS * solidcraft_vr::menu::MAX_ITEMS;

/// Where the sticks' labels are among the boards (after the two pointed at and the two said):
/// four for each hand, its left, right, down and up.
const STICK_SLOTS: usize = 4;

/// Where the ring menu's bubbles are among the boards.
const MENU_SLOTS: usize = STICK_SLOTS + 8;

/// Where the belt is among the boards: its box, its handle, its groups, then the open group's
/// items (drawn in that order, the box behind).
const BELT: usize = READOUT + 1;

/// Where the Browser's window is among the boards: after the belt's bubbles.
const WINDOW: usize = BELT + 2 + belt::GROUPS.len() + belt::MAX_ITEMS;

/// Where the joints' glyphs are among the boards: after the belt and its window (the boards
/// before [`JOINTS`] from [`BELT`] on are the belt's, hidden behind what is nearer).
const JOINTS: usize = WINDOW + 1;

/// The most joints' glyphs drawn: the nearest.
const MAX_JOINTS: usize = 48;

/// The `max` of `items` with the least first number (a squared distance), in no particular order:
/// chosen by a partial selection, not a sort of them all (a design may have thousands of joints).
fn nearest<T>(mut items: Vec<(f64, T)>, max: usize) -> Vec<(f64, T)> {
    if max == 0 {
        return Vec::new();
    }
    if items.len() > max {
        items.select_nth_unstable_by(max, |a, b| a.0.total_cmp(&b.0));
        items.truncate(max);
    }
    items
}

/// Which joint each of the joints' sign slots ([`JOINTS`] on) shows. A joint keeps its slot
/// while it is shown, so its painted glyph stays with it as the head moves and other joints come
/// nearer or go further, instead of the slots' glyphs being painted again in another order.
struct JointSlots {
    ids: [Option<u64>; MAX_JOINTS],
}

impl Default for JointSlots {
    /// No joint shown.
    fn default() -> JointSlots {
        JointSlots { ids: [None; MAX_JOINTS] }
    }
}

impl JointSlots {
    /// The slot of each joint in `ids` (the joints shown now), in their order: the one it had,
    /// else one a joint no longer shown left. Slots of joints no longer shown are freed. A joint
    /// named again (a design file may repeat an id) or past [`MAX_JOINTS`] gets none.
    fn assign(&mut self, ids: &[u64]) -> Vec<Option<usize>> {
        for slot in &mut self.ids {
            if slot.is_some_and(|id| !ids.contains(&id)) {
                *slot = None;
            }
        }
        ids.iter()
            .enumerate()
            .map(|(n, id)| {
                if ids[..n].contains(id) {
                    return None;
                }
                if let Some(k) = self.ids.iter().position(|s| *s == Some(*id)) {
                    return Some(k);
                }
                let k = self.ids.iter().position(Option::is_none)?;
                self.ids[k] = Some(*id);
                Some(k)
            })
            .collect()
    }
}

/// How tall a joint's glyph is for each metre it is from the head, and at least and at most
/// (metres): about as big to the eye as the window's.
const JOINT_PER_METRE: f64 = 0.03;
const JOINT_MIN: f64 = 0.006;
const JOINT_MAX: f64 = 0.08;

/// How many signs there can be: two pointed at, two said, four stick directions, the menu's
/// bubbles, the scale, the belt and its window, the joints.
const BOARDS: usize = JOINTS + MAX_JOINTS;

/// How far the stick scrolls the Browser's window at full push (points a second).
const WHEEL_SPEED: f32 = 700.0;

/// How tall the scale's sign is (metres).
const READOUT_HEIGHT: f64 = 0.03;

/// How tall the sign saying what a hand did is (metres).
const SAID_HEIGHT: f64 = 0.02;

/// How tall the stick's labels are (metres).
const STICK_LABEL_HEIGHT: f64 = 0.012;

/// A sign: its words, where it stands in the room, its height (`None`: as tall as its distance
/// from the head asks, standing above the point), and its colours.
#[derive(Clone, Debug, PartialEq)]
struct Board {
    text: String,
    at: [f64; 3],
    height: Option<f64>,
    /// Without a height: a factor on the size its distance asks for.
    factor: f64,
    /// A sign or a menu bubble.
    look: sign::Look,
    fill: egui::Color32,
    ink: egui::Color32,
    /// Bent round the belt's curve, fixed in the room (not turned toward the head).
    bend: Option<belt::Bend>,
}

/// The belt's bubbles into `out` ([`BELT`] on): its groups (the open one lit), and the open
/// group's items above it; the one a ray points at bigger.
fn belt_boards(out: &mut [Option<Board>; BOARDS], layout: &belt::Layout, belt: &belt::Belt, state: &belt::State, fade: f32) {
    let t = solidcraft_theme::Tokens::get();
    let fade = (fade * 100.0).round().clamp(5.0, 100.0) as u8;
    let hovered = |spot: belt::Spot| belt.hover.iter().flatten().any(|(s, _)| *s == spot);
    // The box around the groups, and the handle under it (lit while pointed at or held), all
    // bent round the belt's curve.
    let bend = Some(layout.bend);
    let belt::Area { middle, width, height } = layout.panel;
    let ratio_tenths = ((width / height) * 10.0).round().clamp(10.0, 200.0) as u32;
    let panel =
        |look, at, height| Board { text: String::new(), at, height: Some(height), factor: 1.0, look, fill: t.dialog_body, ink: t.form_ink, bend };
    out[BELT] = Some(panel(sign::Look::Panel { ratio_tenths }, middle, height));
    let held = belt.held.is_some() || hovered(belt::Spot::Handle);
    out[BELT + 1] = Some(panel(sign::Look::Handle { lit: held }, layout.handle, layout.bubble * 0.22));
    let bubble = |item: &belt::Item, at: [f64; 3], lit: bool, big: bool| {
        let look = sign::Look::Bubble { icon: item.icon, lit, fade: if item.on { 100 } else { fade } };
        let (fill, ink) = match (lit, item.on) {
            (true, _) => (t.accent, t.on_accent),
            (false, true) => (t.nav_tip, t.nav_tip_ink),
            (false, false) => (t.nav_tip, t.text_dim),
        };
        Board { text: item.words.clone(), at, height: Some(layout.bubble * if big { 1.25 } else { 1.0 }), factor: 1.0, look, fill, ink, bend }
    };
    for (g, (group, at)) in belt::GROUPS.iter().zip(&layout.groups).enumerate() {
        let spot = belt::Spot::Group(g);
        let open = belt.open == Some(g) || (group.is_window() && belt.window);
        out[BELT + 2 + g] = Some(bubble(&belt::group_item(*group), *at, open || hovered(spot), hovered(spot)));
    }
    if let Some(group) = belt.open.and_then(|g| belt::GROUPS.get(g)) {
        for (k, (item, at)) in belt::items(*group, state).iter().zip(&layout.items).enumerate().take(belt::MAX_ITEMS) {
            let spot = belt::Spot::Item(k);
            out[BELT + 2 + belt::GROUPS.len() + k] = Some(bubble(item, *at, item.current || hovered(spot), hovered(spot)));
        }
    }
}

/// Select on the belt's `spot`: a group opens (or closes, when open), an item is done and its
/// group closes. What was done, in the user's language, for the sign at the controller.
fn belt_act(app: &mut SolveApp, belt: &mut belt::Belt, spot: belt::Spot, state: &belt::State, put_back: &mut bool) -> String {
    let act = match spot {
        belt::Spot::Handle => return String::new(),
        belt::Spot::Group(g) if belt::GROUPS.get(g).is_some_and(|group| group.is_window()) => {
            belt.window = !belt.window;
            return String::new();
        }
        belt::Spot::Group(g) => {
            belt.open = if belt.open == Some(g) { None } else { Some(g) };
            return String::new();
        }
        belt::Spot::Item(k) => {
            let Some(item) = belt.open.and_then(|g| belt::GROUPS.get(g)).and_then(|g| belt::items(*g, state).into_iter().nth(k)) else {
                return String::new();
            };
            // The real room is always offered, to say why it cannot be shown where it cannot.
            if !item.on && item.act != belt::Act::Passthrough {
                return String::new();
            }
            item.act
        }
    };
    belt.open = None;
    belt_do(app, act, state, put_back)
}

/// Why the belt's `act` is refused in the headset, where it would only ask something on the
/// desktop's screen and nothing would seem to happen: Save on a design that has no file yet
/// (`has_file`: it would open Save As), Close on one with unsaved changes (`dirty`: it would ask
/// whether to save them), Save while parts are moved and not captured (`moved`: it would ask
/// whether to capture their positions). The catalogue entry the sign at the controller says;
/// `None` where it can be done.
fn belt_refusal(act: belt::Act, has_file: bool, dirty: bool, moved: bool) -> Option<&'static str> {
    match act {
        belt::Act::Save if !has_file => Some("vr.belt_save_first"),
        belt::Act::Save if moved => Some("vr.belt_save_moved"),
        belt::Act::Close if dirty => Some("vr.belt_close_unsaved"),
        _ => None,
    }
}

/// Do the belt's item `act` ([`belt_act`]), or say why it is refused ([`belt_refusal`]). What was
/// done, in the user's language, for the sign at the controller.
fn belt_do(app: &mut SolveApp, act: belt::Act, state: &belt::State, put_back: &mut bool) -> String {
    use solidcraft_ui_text::tr;
    let moved = solidcraft_ui_egui::browser::needs_capture(app, solidcraft_ui_egui::browser::SAVE);
    if let Some(why) = belt_refusal(act, app.session.path.is_some(), app.session.is_dirty(), moved) {
        return tr(why).to_string();
    }
    match act {
        belt::Act::Switch(i) => {
            solidcraft_ui_egui::documents::switch(app, i);
            String::new()
        }
        belt::Act::NewDesign => {
            solidcraft_ui_egui::documents::new_tab(app);
            String::new()
        }
        belt::Act::Save => {
            solidcraft_ui_egui::toolbar::save(app);
            tr("vr.belt_saving").to_string()
        }
        belt::Act::Close => {
            let active = app.docs.active;
            solidcraft_ui_egui::documents::close(app, active, false);
            String::new()
        }
        belt::Act::Passthrough if !state.passthrough_offered => tr("vr.passthrough_missing").to_string(),
        belt::Act::Passthrough => {
            app.preferences.vr.passthrough = !app.preferences.vr.passthrough;
            tr(if app.preferences.vr.passthrough { "vr.passthrough_on" } else { "vr.passthrough_off" }).to_string()
        }
        belt::Act::PutBack => {
            *put_back = true;
            tr("vr.did_put_back").to_string()
        }
        belt::Act::Undo | belt::Act::Redo => {
            let undo = act == belt::Act::Undo;
            let none_left = if undo { app.session.undo.is_empty() } else { app.session.redo.is_empty() };
            if none_left {
                return tr(if undo { "vr.nothing_to_undo" } else { "vr.nothing_to_redo" }).to_string();
            }
            app.start(if undo { "edit.undo" } else { "edit.redo" });
            tr(if undo { "vr.did_undo" } else { "vr.did_redo" }).to_string()
        }
        belt::Act::Group(_) => String::new(),
    }
}

/// The section plane at the tip of hand `i`'s controller, in the room, while it is tracked.
fn held_plane(hands: &[HandInput; 2], i: usize) -> Option<Plane> {
    hands.get(i)?.aim.as_ref().and_then(Plane::at_tip)
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
/// shows toward the user, `bounds` (the model's box, model units) across; `place` how far in
/// front, how big and how far below the eyes (metres, the settings'). `None` while there is
/// no model to go by (an empty design) or the head is not where a pose can be.
fn first_placement(source: &Source, eyes: &[Eye; 2], bounds: Option<[[f64; 3]; 2]>, place: [f64; 3]) -> Option<Transform> {
    let bounds = bounds?;
    let [left, right] = eyes;
    let position = [0, 1, 2].map(|k| (left.pose.position[k] + right.pose.position[k]) * 0.5);
    let head = Pose { position, orientation: left.pose.orientation };
    let up = source.camera.up_dir();
    let back = source.camera.back();
    room::first_transform_at(Some(bounds), [up.x, up.y, up.z], [back.x, back.y, back.z], &head, place)
}

/// What a frame shows in the headset.
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
    /// How far each controller's ray goes before it stops on the model (metres), left first.
    reach: [Option<f64>; 2],
    /// How far a ray reaches that stops on nothing (metres).
    ray_length: f64,
    /// The signs ([`boards`]).
    boards: [Option<Board>; BOARDS],
    /// The middle of the eyes in the room, while the head is tracked.
    head: Option<[f64; 3]>,
    /// The real room shows where nothing is drawn: no backdrop, no floor.
    passthrough: bool,
    /// The section the headset cuts the model with (model units), in place of the window's.
    cut: Option<Plane>,
    /// Where the section is in the room, for its outline.
    outline: Option<Plane>,
    /// The controllers' own models, where the VR software hands them out.
    controllers: Vec<controllers::Draw>,
    /// The assets of the models the VR software handed out (drawn this frame or not): their
    /// uploads are kept.
    live_models: Vec<[u8; 16]>,
    /// The window's context: the signs are painted in its fonts.
    ctx: &'a egui::Context,
    /// The language the fonts are for.
    language: &'a str,
    /// The window's fonts' fingerprint ([`sign::fonts_fingerprint`]) and the theme's generation
    /// ([`sign::ThemeWatch`]): a sign painted in others is painted again.
    fonts: u64,
    theme: u64,
    /// The Browser's window, drawn ([`panel::Panel`]).
    picture: Option<&'a wgpu::TextureView>,
    /// The floor grid's strength, the horizon's glow and the grid's reach (metres).
    world: [f32; 3],
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
    // w: the real room shows where nothing is drawn (1): no backdrop, no floor.
    flags: vec4<f32>,
    // The eye in the room (xyz) and the floor's height (w).
    eye: vec4<f32>,
    // x: how strongly the floor's grid shows (0 to 1), y: how much brighter the sky is at the
    // horizon (0 to 1), z: how far the grid reaches before it has faded to a third (metres).
    look: vec4<f32>,
};
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: U;
// The room's view for the floor's grid (the marks' matrix).
struct FM { view_proj: mat4x4<f32> };
@group(0) @binding(4) var<uniform> fm: FM;

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

// The floor under the user, fixed in the room: its ground, darker toward the feet and fading
// with the distance, over `background` (a colour as the screen shows it) where `ray` looks down
// at it. Its grid is drawn after the model ([`fs_floor`]), so a model below the floor shows it.
fn on_floor(background: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    let height = u.eye.y - u.eye.w;
    let down = min(ray.y, -1e-4);
    let distance = height / -down;
    let seen = select(0.0, 1.0, ray.y < -1e-3 && height > 0.0 && u.flags.z > 0.5);
    let fade = exp(-distance / max(u.look.z, 0.5)) * seen;
    return mix(background, u.bottom.rgb * 0.75, 0.6 * fade);
}

// The ray of the eye through this pixel, in the room.
fn eye_ray(uv: vec2<f32>) -> vec3<f32> {
    let tx = mix(u.tangents.x, u.tangents.y, uv.x);
    let ty = mix(u.tangents.z, u.tangents.w, uv.y);
    return normalize(u.right.xyz * tx + u.up.xyz * ty - u.back.xyz);
}

struct F { @location(0) colour: vec4<f32>, @builtin(frag_depth) depth: f32 };

// The floor's grid of half-metre squares (every metre stronger), fading with the distance, at
// the floor's depth: over whatever is below the floor (a model too), under what is above it.
// In the theme's own colour, lighter than the ground.
@fragment
fn fs_floor(i: O) -> F {
    let ray = eye_ray(i.uv);
    let height = u.eye.y - u.eye.w;
    let distance = height / -min(ray.y, -1e-4);
    let p = u.eye.xyz + ray * distance;
    let at = p.xz / 0.5;
    // Lines about one and a half pixels wide (worked out before anything is discarded: a
    // derivative needs every pixel around it).
    let width = max(fwidth(at), vec2<f32>(1e-4)) * 1.5;
    if (!(ray.y < -1e-3 && height > 0.0 && u.flags.z > 0.5 && u.flags.w < 0.5)) {
        discard;
    }
    let cell = abs(fract(at - 0.5) - 0.5) / width;
    let line = 1.0 - min(min(cell.x, cell.y), 1.0);
    let metre = abs(fract(at * 0.5 - 0.5) - 0.5) / (width * 0.5);
    let major = 1.0 - min(min(metre.x, metre.y), 1.0);
    let fade = exp(-distance / max(u.look.z, 0.5));
    let a = clamp(max(line * 0.75, major) * u.look.x * fade, 0.0, 1.0);
    if (a < 0.01) {
        discard;
    }
    let ink = to_linear(mix(u.top.rgb, vec3<f32>(1.0), 0.3));
    var o: F;
    o.colour = vec4<f32>(ink * a, a);
    let clip = fm.view_proj * vec4<f32>(p, 1.0);
    o.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return o;
}

// The room's backdrop, fixed in the room (the view's background colours from below to above),
// the floor, and the model's picture (colours times alpha) over them.
@fragment
fn fs_eye(i: O) -> @location(0) vec4<f32> {
    let tx = mix(u.tangents.x, u.tangents.y, i.uv.x);
    let ty = mix(u.tangents.z, u.tangents.w, i.uv.y);
    let ray = normalize(u.right.xyz * tx + u.up.xyz * ty - u.back.xyz);
    // The sky lighter toward the horizon (as the open air is), the floor in the theme's colours.
    let sky = mix(u.bottom.rgb, u.top.rgb, smoothstep(-0.6, 0.6, ray.y));
    let glow = exp(-abs(ray.y) * 4.0) * select(1.0, 0.0, ray.y < 0.0) * u.look.y;
    let lifted = mix(sky, mix(u.top.rgb, vec3<f32>(1.0), 0.45), glow);
    let background = on_floor(lifted, ray);
    var model = vec4<f32>(0.0);
    if (u.flags.y > 0.5) {
        model = textureSampleLevel(src, smp, i.uv, 0.0);
    }
    // The real room around the model: the model alone, its alpha letting the room through.
    if (u.flags.w > 0.5) {
        if (u.flags.x > 0.5) {
            let a = max(model.a, 1e-4);
            return vec4<f32>(to_linear(model.rgb / a) * model.a, model.a);
        }
        return model;
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

// The signs: a board in the room with the picture egui painted on it (colours as the screen
// shows them, times alpha), over everything else.
@group(0) @binding(3) var sign_src: texture_2d<f32>;

struct S { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_sign(@location(0) at: vec3<f32>, @location(1) uv: vec2<f32>) -> S {
    var o: S;
    o.pos = m.view_proj * vec4<f32>(at, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn fs_sign(i: S) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(sign_src, smp, i.uv, 0.0);
    // Around a board's rounded corners: nothing, not even depth (what is behind shows).
    if c.a < 0.02 {
        discard;
    }
    let a = max(c.a, 1e-4);
    return vec4<f32>(to_linear(c.rgb / a) * c.a, c.a);
}

// The controllers' own models: each mesh placed in the room (`world`), its picture (sRGB, read as
// linear light) times its colour, lit from above.
struct C { world: mat4x4<f32>, colour: vec4<f32> };
@group(0) @binding(4) var<uniform> mesh: C;

struct CO { @builtin(position) pos: vec4<f32>, @location(0) normal: vec3<f32>, @location(1) uv: vec2<f32> };

@vertex
fn vs_ctrl(@location(0) at: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> CO {
    var o: CO;
    o.pos = m.view_proj * (mesh.world * vec4<f32>(at, 1.0));
    o.normal = (mesh.world * vec4<f32>(normal, 0.0)).xyz;
    o.uv = uv;
    return o;
}

@fragment
fn fs_ctrl(i: CO) -> @location(0) vec4<f32> {
    let base = textureSampleLevel(sign_src, smp, i.uv, 0.0) * mesh.colour;
    let n = normalize(i.normal + vec3<f32>(0.0, 1e-6, 0.0));
    let light = 0.45 + 0.55 * abs(dot(n, normalize(vec3<f32>(0.3, 1.0, 0.4))));
    return vec4<f32>(base.rgb * light, 1.0);
}
"#;

/// The eye uniform's size in bytes: nine `vec4<f32>`.
const UNIFORM_SIZE: u64 = 9 * 16;

/// The marks' uniform: one `mat4x4<f32>`.
const MARK_UNIFORM_SIZE: u64 = 64;

/// A mark vertex: its place (3 × f32), then its colour (4 × f32).
const MARK_VERTEX: u64 = 28;

/// The most mark vertices drawn: the controllers' and the section's outline.
const MARK_VERTICES: usize = marks::MAX_VERTICES + section::OUTLINE_VERTICES;

/// A sign vertex: its place (3 × f32), then where it is on the sign's picture (2 × f32).
const SIGN_VERTEX: u64 = 20;

/// The vertices of one sign: two triangles.
const SIGN_VERTICES: u64 = 6 * MAX_STRIPS as u64;

/// The most strips a board bent round the belt's curve is drawn in.
const MAX_STRIPS: usize = 16;

/// The most signs drawn: [`BOARDS`].
const SIGNS: usize = BOARDS;

/// A sign painted for one hand, and what it was painted for.
struct Sign {
    /// The text, the language, the fonts' fingerprint and the theme's generation it was painted
    /// in, the board's, the text's and the accent's colours, and what it looks like.
    key: (String, String, u64, u64, [egui::Color32; 3], sign::Look),
    /// The marks' uniform, the sampler and the picture.
    bind: wgpu::BindGroup,
}

/// What draws the session's pictures on the window's device.
struct Gpu {
    eye: wgpu::RenderPipeline,
    controllers: controllers::Controllers,
    /// The depth buffer of the controllers, rays and signs, and its size.
    depth: Option<(wgpu::TextureView, [u32; 2])>,
    marks: wgpu::RenderPipeline,
    /// Signs over everything (what a ray points at, the menu, the joints).
    signs: wgpu::RenderPipeline,
    /// Signs in the room (the belt): hidden behind the model, the controllers and what is nearer.
    signs_in_room: wgpu::RenderPipeline,
    /// The model's depth into the depth buffer.
    model_depth: model_depth::ModelDepth,
    /// The floor's grid, depth-tested, and what it reads (the eye's uniform and the room's view).
    floor: wgpu::RenderPipeline,
    floor_bind: wgpu::BindGroup,
    sign_layout: wgpu::BindGroupLayout,
    /// Room for [`SIGNS`] signs.
    sign_vertices: wgpu::Buffer,
    painter: sign::Painter,
    /// Each sign as last painted, in [`boards`]' places: painted again only when what it shows
    /// changes.
    painted: [Option<Sign>; SIGNS],
    layout: wgpu::BindGroupLayout,
    mark_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    mark_uniform: wgpu::Buffer,
    /// Room for [`MARK_VERTICES`].
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
        let sign_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_vr_signs"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX, MARK_UNIFORM_SIZE),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
            ],
        });
        let signs_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_vr_signs"),
            bind_group_layouts: &[Some(&sign_layout)],
            immediate_size: 0,
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
            // Each mark is a box wound counter-clockwise from outside. Depth-tested, so a ray goes
            // out of the controller's tip, not across it.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), front_face: wgpu::FrontFace::Ccw, ..Default::default() },
            depth_stencil: Some(controllers::depth_state(true, wgpu::CompareFunction::LessEqual)),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        const SIGN_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        let sign_pipeline = |depth_stencil| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_vr_signs"),
                layout: Some(&signs_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_sign"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: SIGN_VERTEX,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &SIGN_ATTRIBUTES,
                    })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_sign"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                // Seen from either side.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(depth_stencil),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let floor_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_vr_floor"),
            entries: &[
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT, UNIFORM_SIZE),
                uniform_entry(4, wgpu::ShaderStages::FRAGMENT, MARK_UNIFORM_SIZE),
            ],
        });
        let floor = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_vr_floor"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sc_vr_floor"),
                bind_group_layouts: &[Some(&floor_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_floor"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(controllers::depth_state(false, wgpu::CompareFunction::LessEqual)),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let signs = sign_pipeline(controllers::depth_state(false, wgpu::CompareFunction::Always));
        let signs_in_room = sign_pipeline(controllers::depth_state(true, wgpu::CompareFunction::LessEqual));
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
        let mark_vertices = buffer("sc_vr_marks", MARK_VERTICES as u64 * MARK_VERTEX, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
        let sign_vertices =
            buffer("sc_vr_signs", SIGNS as u64 * SIGN_VERTICES * SIGN_VERTEX, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
        let floor_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_vr_floor"),
            layout: &floor_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: mark_uniform.as_entire_binding() },
            ],
        });
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
            controllers: controllers::Controllers::new(device, &rs.queue, &module, format, MARK_UNIFORM_SIZE),
            depth: None,
            marks,
            signs,
            signs_in_room,
            model_depth: model_depth::ModelDepth::new(device, format, controllers::DEPTH),
            floor,
            floor_bind,
            sign_layout,
            sign_vertices,
            painter: sign::Painter::new(device),
            painted: std::array::from_fn(|_| None),
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
        let mut vertices = marks::marks(scene.hands, scene.held, scene.reach, scene.ray_length, &style);
        if let Some(plane) = &scene.outline {
            vertices.extend(section::outline(plane, style.ray));
        }
        let count = vertices.len().min(MARK_VERTICES);
        if count > 0 {
            rs.queue.write_buffer(&self.mark_vertices, 0, &mark_bytes(&vertices[..count]));
        }
        let signs = self.place_signs(rs, scene);
        self.controllers.prepare(rs, &scene.controllers, &scene.live_models, &self.mark_uniform, &self.sampler);
        let background = scene.source.map_or([t.viewport_top, t.viewport_bottom], |s| s.background);
        let placement = scene.transform.map(Transform::placement);
        // The light and the glass order follow the head (one direction for both eyes, so they
        // agree), as the window's follow its camera.
        let back = scene.transform.zip(eyes[0].pose.axes()).map(|(t, [_, _, back])| model_direction(t, back));
        let mut pictures = 0;
        for (index, eye) in eyes.iter().enumerate() {
            let size = session.eye_size(index);
            let model = match (scene.source, placement.as_ref(), back) {
                (Some(source), Some(placement), Some(back)) => self.draw_placed(rs, source, eye, placement, back, size, scene.cut.as_ref()),
                _ => false,
            };
            let (Some(axes), Some(room)) = (eye.pose.axes(), view::room_view_proj(eye)) else { continue };
            let picture = session.eye_picture(index)?;
            let fov = [eye.fov.left, eye.fov.right, eye.fov.up, eye.fov.down];
            let uniform = eye_uniform(background, axes, fov, self.encoded, model, eye.pose.position, scene.floor, scene.passthrough, scene.world);
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
            }
            // The controllers, the rays and the signs over that, with a depth buffer of their own
            // (the model's picture has none): a controller hides what is behind it.
            if self.depth.as_ref().is_none_or(|(_, size)| *size != picture.size) {
                let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("sc_vr_depth"),
                    size: wgpu::Extent3d { width: picture.size[0], height: picture.size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: controllers::DEPTH,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                self.depth = Some((texture.create_view(&wgpu::TextureViewDescriptor::default()), picture.size));
            }
            // The model's depth, where it was drawn for this eye.
            let model_depth = self
                .model
                .as_ref()
                .filter(|_| model)
                .and_then(ViewTarget::depth)
                .map(|(samples, reversed, view)| self.model_depth.bind(&rs.device, &rs.queue, samples, reversed, view, picture.size));
            if let Some((depth, _)) = &self.depth {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_vr_over"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &picture.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, picture.size[0] as f32, picture.size[1] as f32, 0.0, 1.0);
                if let Some((pipeline, bind)) = &model_depth {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, bind, &[]);
                    pass.draw(0..3, 0..1);
                }
                // The floor's grid over what is below the floor, the model too.
                pass.set_pipeline(&self.floor);
                pass.set_bind_group(0, &self.floor_bind, &[]);
                pass.draw(0..3, 0..1);
                self.controllers.draw(&mut pass);
                if count > 0 {
                    pass.set_pipeline(&self.marks);
                    pass.set_bind_group(0, &self.mark_bind, &[]);
                    pass.set_vertex_buffer(0, self.mark_vertices.slice(..));
                    pass.draw(0..count as u32, 0..1);
                }
                // The belt's first (in the room), then what stands over everything.
                for (slot, first, count) in &signs {
                    let Some(sign) = &self.painted[*slot] else { continue };
                    pass.set_pipeline(if (BELT..JOINTS).contains(slot) { &self.signs_in_room } else { &self.signs });
                    pass.set_bind_group(0, &sign.bind, &[]);
                    pass.set_vertex_buffer(0, self.sign_vertices.slice(..));
                    pass.draw(*first..*first + *count, 0..1);
                }
            }
            rs.queue.submit([encoder.finish()]);
            pictures += 1;
        }
        Ok(pictures == eyes.len())
    }

    /// Put up the signs of `scene` ([`boards`]), turned toward the head, each picture painted when
    /// what it shows changed (the words, the language, the window's fonts, the theme, the
    /// colours). The slots of
    /// [`Gpu::painted`] to draw, in the order their boards are in [`Gpu::sign_vertices`].
    fn place_signs(&mut self, rs: &egui_wgpu::RenderState, scene: &Scene<'_>) -> Vec<(usize, u32, u32)> {
        let Some(head) = scene.head else { return Vec::new() };
        let mut slots = Vec::new();
        let mut bytes = Vec::new();
        // The belt's boards first: they are drawn first, so what stands over everything is
        // drawn over them.
        let order = (BELT..JOINTS).chain(0..BELT).chain(JOINTS..BOARDS);
        for slot in order {
            let Some(board) = &scene.boards[slot] else { continue };
            let [w, h] = board.look.size();
            let aspect = f64::from(w) / f64::from(h);
            // A sign's picture holds a few lines: drawn that much taller, its line keeps the size
            // asked for.
            let lines = if board.look == sign::Look::Sign { sign::SIGN_LINE_SHARE } else { 1.0 };
            let corners = match board.height {
                Some(height) => marks::board_corners(board.at, head, height * lines, aspect),
                None => marks::sign_corners_sized(board.at, head, aspect, board.factor * lines),
            };
            let strips = match (board.bend, board.height) {
                (Some(bend), Some(height)) => bent(board.at, &bend, height * aspect, height),
                _ => corners.map(|c| vec![(c, [0.0, 1.0])]),
            };
            let Some(strips) = strips else { continue };
            let name = board.text.as_str();
            // The tab's accent colours the menu's slots.
            let accent = solidcraft_theme::TabAccent::current(scene.ctx).color;
            let key = (name.to_string(), scene.language.to_string(), scene.fonts, scene.theme, [board.fill, board.ink, accent], board.look);
            if self.painted[slot].as_ref().is_none_or(|sign| sign.key != key) {
                // The Browser's window is drawn by its panel; the other signs here.
                let painted;
                let picture = if let sign::Look::Picture { .. } = board.look {
                    let Some(view) = scene.picture else { continue };
                    view
                } else {
                    painted = self.painter.paint(rs, scene.ctx, scene.fonts, name, board.fill, board.ink, board.look);
                    &painted
                };
                let bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sc_vr_sign"),
                    layout: &self.sign_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.mark_uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(picture) },
                    ],
                });
                self.painted[slot] = Some(Sign { key, bind });
            }
            let first = (bytes.len() / SIGN_VERTEX as usize) as u32;
            for (corners, u) in strips.iter().take(MAX_STRIPS) {
                bytes.extend(sign_bytes(corners, *u));
            }
            slots.push((slot, first, (strips.len().min(MAX_STRIPS) * 6) as u32));
        }
        if !bytes.is_empty() {
            rs.queue.write_buffer(&self.sign_vertices, 0, &bytes);
        }
        slots
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
        cut: Option<&Plane>,
    ) -> bool {
        let Some(seen) = view::eye_view(eye, placement) else { return false };
        let mut frame = source.frame.clone();
        // The headset's section in place of the window's.
        if let Some(plane) = cut {
            frame.clip = plane.clip();
        }
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
    passthrough: bool,
    world: [f32; 3],
) -> Vec<u8> {
    let floor = floor.filter(|h| h.is_finite());
    let colour = |c: egui::Color32| {
        let [r, g, b, _] = c.to_normalized_gamma_f32();
        [r, g, b]
    };
    let [top, bottom] = background.map(colour);
    let direction = |a: [f64; 3]| [a[0] as f32, a[1] as f32, a[2] as f32, 0.0];
    let tangents = fov.map(|angle| angle.tan() as f32);
    let rows: [[f32; 4]; 9] = [
        [top[0], top[1], top[2], 1.0],
        [bottom[0], bottom[1], bottom[2], 1.0],
        direction(axes[0]),
        direction(axes[1]),
        direction(axes[2]),
        // Left, right, up, down; the picture's top row is the up tangent.
        [tangents[0], tangents[1], tangents[2], tangents[3]],
        [
            if encoded { 1.0 } else { 0.0 },
            if model { 1.0 } else { 0.0 },
            if floor.is_some() { 1.0 } else { 0.0 },
            if passthrough { 1.0 } else { 0.0 },
        ],
        [eye[0] as f32, eye[1] as f32, eye[2] as f32, floor.unwrap_or(0.0) as f32],
        [world[0], world[1], world[2], 0.0],
    ];
    rows.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// A matrix as the shader takes it: its columns one after another.
fn matrix_bytes(m: &[[f32; 4]; 4]) -> Vec<u8> {
    m.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// A sign's two triangles as the shader takes them, from its corners (top left, top right, bottom
/// right, bottom left), the picture's top row at the top, from `u[0]` across to `u[1]` (a strip
/// of a bent board).
fn sign_bytes(corners: &[[f64; 3]; 4], u: [f32; 2]) -> Vec<u8> {
    let uv = [[u[0], 0.0], [u[1], 0.0], [u[1], 1.0], [u[0], 1.0]];
    [0, 1, 2, 0, 2, 3].into_iter().flat_map(|k| corners[k].map(|v| v as f32).into_iter().chain(uv[k])).flat_map(f32::to_le_bytes).collect()
}

/// A board `width` × `height` (metres), its middle at `at`, bent round `bend`: upright along the
/// curve's axis, fixed in the room. Its strips' corners (as [`sign_bytes`] takes them) and how far
/// across the picture each runs. `None` where `at` is on the axis or a size is not one.
fn bent(at: [f64; 3], bend: &belt::Bend, width: f64, height: f64) -> Option<Vec<([[f64; 3]; 4], [f32; 2])>> {
    let (axis, up) = bend.of();
    let d = [0, 1, 2].map(|k| at[k] - axis[k]);
    let rise = d[0] * up[0] + d[1] * up[1] + d[2] * up[2];
    let flat = [0, 1, 2].map(|k| d[k] - up[k] * rise);
    let radius = flat.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !(radius > 1e-6 && width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        return None;
    }
    let out = flat.map(|v| v / radius);
    // Seen from the axis, looking out.
    let right = [out[1] * up[2] - out[2] * up[1], out[2] * up[0] - out[0] * up[2], out[0] * up[1] - out[1] * up[0]];
    let strips = ((width / 0.02).ceil() as usize).clamp(1, MAX_STRIPS);
    let half = width * 0.5 / radius;
    let point = |a: f64, y: f64| {
        let (sin, cos) = a.sin_cos();
        [0, 1, 2].map(|k| axis[k] + up[k] * (rise + y) + radius * (out[k] * cos + right[k] * sin))
    };
    Some(
        (0..strips)
            .map(|j| {
                let (u0, u1) = (j as f64 / strips as f64, (j + 1) as f64 / strips as f64);
                let (a0, a1) = (-half + 2.0 * half * u0, -half + 2.0 * half * u1);
                let h = height * 0.5;
                ([point(a0, h), point(a1, h), point(a1, -h), point(a0, -h)], [u0 as f32, u1 as f32])
            })
            .collect(),
    )
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

    /// The buffers' sizes are what the shader reads: nine vectors for the eye, one matrix for
    /// the marks, seven numbers a vertex.
    #[test]
    fn uniforms_and_vertices_have_the_shaders_layout() {
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let u = eye_uniform(
            [egui::Color32::WHITE, egui::Color32::BLACK],
            axes,
            [-1.0, 1.0, 1.0, -1.0],
            true,
            false,
            [0.0, 1.6, 0.0],
            Some(-0.1),
            false,
            [0.8, 0.35, 16.0],
        );
        assert_eq!(u.len() as u64, UNIFORM_SIZE);
        let float = |at: usize| f32::from_le_bytes([u[at], u[at + 1], u[at + 2], u[at + 3]]);
        assert_eq!((float(6 * 16 + 8), float(7 * 16 + 4), float(7 * 16 + 12)), (1.0, 1.6, -0.1), "floor known, eye and floor height");
        let none =
            eye_uniform([egui::Color32::WHITE; 2], axes, [-1.0, 1.0, 1.0, -1.0], true, false, [0.0; 3], Some(f64::NAN), false, [0.8, 0.35, 16.0]);
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
        assert_eq!(names, ["vs", "fs_floor", "fs_eye", "vs_mark", "fs_mark", "vs_sign", "fs_sign", "vs_ctrl", "fs_ctrl"]);
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

    /// The size (bytes) WGSL gives the struct `name` in `source`, as a uniform buffer holds it.
    pub(super) fn wgsl_struct_size(source: &str, name: &str) -> u64 {
        let module = wgpu::naga::front::wgsl::parse_str(source).expect("the shader parses");
        let mut layouter = wgpu::naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).expect("the types lay out");
        let (handle, _) = module.types.iter().find(|(_, ty)| ty.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no struct {name}"));
        u64::from(layouter[handle].size)
    }

    /// The eye's uniform is as big as the shader's `U` (nine vectors since the floor's look was
    /// added), the marks' as its `M`, and the bytes written fill it exactly, also for poses,
    /// fields of view and floors that are not numbers.
    #[test]
    fn the_eye_uniform_is_as_big_as_the_shader_reads_it() {
        assert_eq!(wgsl_struct_size(SHADER, "U"), UNIFORM_SIZE);
        assert_eq!(wgsl_struct_size(SHADER, "M"), MARK_UNIFORM_SIZE);
        assert_eq!(wgsl_struct_size(SHADER, "FM"), MARK_UNIFORM_SIZE, "the floor reads the marks' matrix");
        let nan = [f64::NAN; 3];
        let u = eye_uniform(
            [egui::Color32::TRANSPARENT; 2],
            [nan; 3],
            [f64::NAN, f64::INFINITY, -1e300, 0.0],
            false,
            true,
            nan,
            Some(f64::INFINITY),
            true,
            [f32::NAN; 3],
        );
        assert_eq!(u.len() as u64, UNIFORM_SIZE);
        let float = |at: usize| f32::from_le_bytes([u[at], u[at + 1], u[at + 2], u[at + 3]]);
        assert_eq!((float(6 * 16 + 8), float(7 * 16 + 12)), (0.0, 0.0), "an endless floor is no floor");
        assert_eq!((float(6 * 16 + 4), float(6 * 16 + 12)), (1.0, 1.0), "the model and the real room flags");
    }

    /// Inside the middle's play the stick does nothing; past it the speed grows from nothing as
    /// a square, keeps the push's sign, and is whole at the end.
    #[test]
    fn a_stick_does_nothing_in_its_play_and_grows_as_a_square_past_it() {
        for v in [0.0, 0.1, -0.1, 0.149, -0.149] {
            assert_eq!(stick_curve(v), 0.0, "{v}");
        }
        // At the play's edge (0.15 as an f32 is a hair past it): next to nothing.
        assert!(stick_curve(0.15).abs() < 1e-12 && stick_curve(-0.15).abs() < 1e-12);
        assert_eq!(stick_curve(1.0), 1.0);
        assert_eq!(stick_curve(-1.0), -1.0);
        // Half-way through the travel past the play: a quarter of the speed.
        let half = 0.15 + 0.85 * 0.5;
        assert!((stick_curve(half) - 0.25).abs() < 1e-6, "{}", stick_curve(half));
        assert!((stick_curve(-half) + 0.25).abs() < 1e-6, "the sign is kept");
        let mut last = 0.0;
        for i in 0..=100 {
            let now = stick_curve(i as f32 / 100.0);
            assert!(now >= last, "it only grows: {i}");
            last = now;
        }
    }

    /// A reading that is not a number, or one past the stick's end, never gives more than a whole
    /// push: not a number moves nothing.
    #[test]
    fn a_stick_reading_that_is_not_a_number_moves_nothing() {
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(stick_curve(v), 0.0, "{v}");
        }
        assert_eq!(stick_curve(7.0), 1.0);
        assert_eq!(stick_curve(-f32::MAX), -1.0);
        for v in [f32::MIN_POSITIVE, -f32::MIN_POSITIVE, 1.0 + f32::EPSILON, 1e30] {
            assert!(stick_curve(v).abs() <= 1.0, "{v}");
        }
    }

    /// Pushing a held model goes along the holding hand's ray; pulling it stops before its pivot
    /// comes nearer the hand than a few centimetres, so it is never pulled back through the hand.
    #[test]
    fn a_held_model_is_never_pulled_back_through_the_hand() {
        let hand = [0.0, 1.2, 0.0];
        let ahead = [0.0, 0.0, -1.0];
        let pivot = [0.0, 1.2, -0.3];
        assert_eq!(pushed(pivot, hand, ahead, 0.02), Some([0.0, 0.0, -0.02]), "pushed away along the ray");
        assert_eq!(pushed(pivot, hand, ahead, -0.1), Some([0.0, 0.0, 0.1]), "pulled closer");
        assert_eq!(pushed(pivot, hand, ahead, -0.26), None, "not nearer the hand than 5 cm");
        // Already behind the hand (the hand reached past it): a pull is refused, a push takes it
        // out in front again.
        let behind = [0.0, 1.2, 0.1];
        assert_eq!(pushed(behind, hand, ahead, -0.01), None);
        assert_eq!(pushed(behind, hand, ahead, 0.01), Some([0.0, 0.0, -0.01]));
        // Off the ray to a side, only the part along the ray counts.
        assert!(pushed([0.4, 1.2, -0.06], hand, ahead, -0.005).is_some());
        assert_eq!(pushed([0.4, 1.2, -0.06], hand, ahead, -0.02), None);
    }

    /// A push or a pose that is not a number moves the held model nowhere.
    #[test]
    fn a_push_that_is_not_a_number_moves_nothing() {
        let (hand, ahead, pivot) = ([0.0, 1.2, 0.0], [0.0, 0.0, -1.0], [0.0, 1.2, -0.3]);
        assert_eq!(pushed(pivot, hand, ahead, f64::NAN), None);
        assert_eq!(pushed(pivot, hand, ahead, f64::INFINITY), None);
        assert_eq!(pushed(pivot, hand, [f64::NAN; 3], 0.01), None);
        assert_eq!(pushed([f64::NAN; 3], hand, ahead, -0.01), None, "a pull from nowhere is refused");
    }

    /// The nearest joints are the ones with the least distance, however many there are and in
    /// whatever order they come; none asked for, none given; a distance that is not a number is
    /// never nearer than one that is.
    #[test]
    fn the_nearest_joints_are_chosen_without_sorting_them_all() {
        let items: Vec<(f64, u64)> = (0..1000u64).rev().map(|k| ((k as f64) * 0.5, k)).collect();
        let mut near: Vec<u64> = nearest(items.clone(), MAX_JOINTS).into_iter().map(|(_, k)| k).collect();
        near.sort();
        assert_eq!(near, (0..MAX_JOINTS as u64).collect::<Vec<_>>());
        assert!(nearest(items.clone(), 0).is_empty());
        assert_eq!(nearest(items[..3].to_vec(), MAX_JOINTS).len(), 3, "fewer than there is room for: all");
        let odd = vec![(f64::NAN, 1u64), (2.0, 2), (1.0, 3)];
        let mut kept: Vec<u64> = nearest(odd, 2).into_iter().map(|(_, k)| k).collect();
        kept.sort();
        assert_eq!(kept, [2, 3]);
    }

    /// A joint keeps its glyph's slot while it is shown, whatever the order the joints come in
    /// (nearest first changes as the head moves); a joint no longer shown frees its slot for the
    /// next new one; a repeated id or one past the slots gets none.
    #[test]
    fn a_joint_keeps_its_slot_while_it_is_shown() {
        let mut slots = JointSlots::default();
        assert_eq!(slots.assign(&[10, 20, 30]), [Some(0), Some(1), Some(2)]);
        assert_eq!(slots.assign(&[30, 10, 20]), [Some(2), Some(0), Some(1)], "the head moved: same slots");
        assert_eq!(slots.assign(&[30, 40, 10]), [Some(2), Some(1), Some(0)], "20 went: 40 takes its slot");
        assert_eq!(slots.assign(&[30, 30, 10]), [Some(2), None, Some(0)], "named twice: one slot");
        let many: Vec<u64> = (100..100 + MAX_JOINTS as u64 + 5).collect();
        let given = slots.assign(&many);
        assert_eq!(given.iter().flatten().count(), MAX_JOINTS, "never more than there are slots");
        let mut distinct: Vec<usize> = given.iter().flatten().copied().collect();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), MAX_JOINTS, "each slot once");
        assert!(slots.assign(&[]).is_empty() && slots.ids.iter().all(Option::is_none), "none shown: all free");
    }

    /// Belt Save on a design that has no file yet or with moved parts not captured, and Close on
    /// one with unsaved changes, would only ask on the desktop's screen: refused in the headset
    /// with a reason; Save of a saved design and Close of an unchanged one are not.
    #[test]
    fn belt_save_without_a_file_and_close_with_changes_are_refused() {
        assert_eq!(belt_refusal(belt::Act::Save, false, false, false), Some("vr.belt_save_first"));
        assert_eq!(belt_refusal(belt::Act::Save, true, true, false), None);
        assert_eq!(belt_refusal(belt::Act::Save, true, true, true), Some("vr.belt_save_moved"));
        assert_eq!(belt_refusal(belt::Act::Close, true, true, false), Some("vr.belt_close_unsaved"));
        assert_eq!(belt_refusal(belt::Act::Close, false, false, true), None);
        for act in [belt::Act::Undo, belt::Act::Redo, belt::Act::PutBack, belt::Act::NewDesign, belt::Act::Passthrough] {
            assert_eq!(belt_refusal(act, false, true, true), None, "{act:?}");
        }
        // Every refusal has its words in the catalogue, in every language there.
        let catalogue: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/locales/pending/vr-pick.json")).expect("the VR texts parse");
        let languages = catalogue.as_object().expect("languages");
        for key in ["vr.belt_save_first", "vr.belt_close_unsaved", "vr.belt_save_moved", "vr.explode_moved"] {
            for (language, words) in languages {
                assert!(words[key].as_str().is_some_and(|w| !w.is_empty()), "{language}: {key}");
            }
        }
    }

    /// In a design with no file and unsaved changes, belt Save and Close change nothing and ask
    /// nothing on the desktop: no Save As, no question about closing, the design still open.
    #[test]
    fn belt_save_and_close_refused_leave_the_design_as_it_was() {
        let mut h = solidcraft_ui_egui::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(h.app.session.path.is_none() && h.app.session.is_dirty());
        let state = belt::State {
            designs: vec!["Design".into()],
            active: 0,
            passthrough: false,
            passthrough_offered: false,
            can_undo: true,
            can_redo: false,
        };
        let (uid, revision) = (h.app.session.uid, h.app.session.revision);
        let mut put_back = false;
        let said = belt_do(&mut h.app, belt::Act::Save, &state, &mut put_back);
        assert_eq!(said, solidcraft_ui_text::tr("vr.belt_save_first"));
        let said = belt_do(&mut h.app, belt::Act::Close, &state, &mut put_back);
        assert_eq!(said, solidcraft_ui_text::tr("vr.belt_close_unsaved"));
        assert_eq!(h.app.docs.closing, None, "no question on the desktop");
        assert!(h.app.session.path.is_none() && h.app.session.is_dirty());
        assert_eq!((h.app.session.uid, h.app.session.revision), (uid, revision), "the same design, unchanged");
    }

    /// The headset taken off (the session waiting) lets go of everything as ending the session
    /// does: the belt and the Browser's window are no longer held or pressed, the menu is shut,
    /// and what the rays lit goes out on the desktop too.
    #[test]
    fn taking_the_headset_off_lets_go_of_everything() {
        let mut h = solidcraft_ui_egui::scenario::Harness::new();
        let mut vr = Vr::blank(Status::Running);
        vr_pick::light(&mut h.app, &mut vr.pointer.lit, vec!["Lit".into()]);
        assert_eq!(h.app.viewport.hover_bodies, ["Lit"]);
        vr.window_pressed = [true, true];
        vr.window_hover = [Some(([0.5, 0.5], 1.0)), None];
        vr.belt.held = Some(belt::Held::by(1));
        vr.let_go(&mut h.app);
        assert_eq!(vr.window_pressed, [false, false]);
        assert_eq!(vr.window_hover, [None, None]);
        assert!(vr.belt.held.is_none() && vr.menu.is_none());
        assert!(h.app.viewport.hover_bodies.is_empty(), "the lit part went out");
        assert_eq!(vr.status(), &Status::Running, "only let go: the session goes on");
    }

    /// A curve round an upright axis through the room's origin, with a board 0.6 m out in front.
    fn bend_and_board() -> (belt::Bend, [f64; 3]) {
        (belt::Bend { axis: [0.0, 1.0, 0.0], up: [0.0, 1.0, 0.0] }, [0.0, 1.2, -0.6])
    }

    /// A board bent round the belt's curve: every corner on the curve, the strips running left to
    /// right across the picture without a gap, as wide as the board, never more than
    /// [`MAX_STRIPS`] however wide it is.
    #[test]
    fn a_bent_board_stands_on_the_curve_in_at_most_sixteen_strips() {
        let (bend, at) = bend_and_board();
        for width in [0.001, 0.05, 0.2, 0.32, 1.0, 3.0, 1e6] {
            let strips = bent(at, &bend, width, 0.1).expect("a board");
            assert!((1..=MAX_STRIPS).contains(&strips.len()), "{width}: {}", strips.len());
            assert_eq!(strips[0].1[0], 0.0);
            assert_eq!(strips[strips.len() - 1].1[1], 1.0);
            for pair in strips.windows(2) {
                assert_eq!(pair[0].1[1], pair[1].1[0], "no gap across the picture");
                assert_eq!(pair[0].0[1], pair[1].0[0], "nor in the room");
            }
            for (corners, _) in &strips {
                for c in corners {
                    let r = (c[0] * c[0] + c[2] * c[2]).sqrt();
                    assert!((r - 0.6).abs() < 1e-9, "{width}: on the curve: {r}");
                    assert!((c[1] - 1.15).abs() < 1e-9 || (c[1] - 1.25).abs() < 1e-9, "upright, as tall as asked: {c:?}");
                }
            }
            if width < 1.0 {
                // As wide along the curve as asked (the angle it spans times the radius).
                let [left, right] = [strips[0].0[0], strips[strips.len() - 1].0[1]];
                let span = (left[0].atan2(-left[2]) - right[0].atan2(-right[2])).abs() * 0.6;
                assert!((span - width).abs() < 1e-9, "{width}: {span}");
                assert!(left[0] < right[0], "left to right seen from the axis");
            }
        }
        assert_eq!(bent(at, &bend, 1e6, 0.1).map(|s| s.len()), Some(MAX_STRIPS), "capped");
    }

    /// A board of no size, one whose size is not a number, or one on the curve's axis is not
    /// drawn.
    #[test]
    fn a_board_of_no_size_or_on_the_axis_is_not_bent() {
        let (bend, at) = bend_and_board();
        for (width, height) in [(0.0, 0.1), (0.1, 0.0), (-0.1, 0.1), (f64::NAN, 0.1), (0.1, f64::NAN), (f64::INFINITY, 0.1), (0.1, f64::INFINITY)] {
            assert!(bent(at, &bend, width, height).is_none(), "{width} × {height}");
        }
        assert!(bent([0.0, 3.0, 0.0], &bend, 0.1, 0.1).is_none(), "on the axis");
        assert!(bent([f64::NAN; 3], &bend, 0.1, 0.1).is_none(), "nowhere");
    }

    /// A point of a bent board's picture, put in the room by [`bent`] and found again on the
    /// board by [`belt::Bend::on_area`], is where it was on the picture: the drawn board and its
    /// hit test agree.
    #[test]
    fn a_point_on_a_bent_board_is_found_where_it_was_on_the_picture() {
        let (bend, at) = bend_and_board();
        let area = belt::Area { middle: at, width: 0.3, height: 0.4 };
        let strips = bent(at, &bend, area.width, area.height).expect("a board");
        for (corners, [u0, u1]) in &strips {
            for (corner, expected) in corners.iter().zip([[*u0, 0.0], [*u1, 0.0], [*u1, 1.0], [*u0, 1.0]]) {
                let uv = bend.on_area(&area, *corner);
                assert!((uv[0] - f64::from(expected[0])).abs() < 1e-6 && (uv[1] - f64::from(expected[1])).abs() < 1e-6, "{uv:?} {expected:?}");
            }
        }
    }
}
