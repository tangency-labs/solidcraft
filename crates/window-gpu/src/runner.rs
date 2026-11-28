//! A window runner of our own: winit's event loop, egui-winit's input and [`Painter`], with no
//! eframe. SolidCraft runs on it only when asked to.
//!
//! The loop does what eframe's wgpu integration does: input goes to each window's egui-winit
//! state, `RedrawRequested` runs that window's pass and paints it, egui's repaint requests (also
//! from other threads) wake the loop through its proxy, and the viewport commands of each pass
//! are applied to its window. Every egui viewport gets a native window:
//! - the main window (`ViewportId::ROOT`) runs [`App::ui`];
//! - a deferred viewport (`Context::show_viewport_deferred`) runs its own callback in its own pass;
//! - an immediate viewport (`Context::show_viewport_immediate`) runs inside its parent's pass and
//!   is painted there.
//!
//! Windows are created only where winit hands out its `ActiveEventLoop` (never from inside a
//! pass, which would need eframe's raw-pointer thread-local): a viewport first seen during a pass
//! gets its window when the loop next waits, and an immediate viewport without a window yet runs
//! its callback in a pass of its own that paints nothing, as egui requires the callback to run.

use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use egui::{
    DeferredViewportUiCallback, ImmediateViewport, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, ViewportIdMap, ViewportIdPair,
    ViewportIdSet, ViewportInfo, ViewportOutput,
};
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::painter::Painter;
use egui_wgpu::{RenderState, WgpuConfiguration, WgpuError};

/// What runs in the main window.
pub trait App {
    /// The app's logic, before each pass's UI, and alone when the loop is asked for it
    /// ([`Waker::run_logic`]) without painting.
    fn logic(&mut self, _ctx: &egui::Context, _frame: &Frame<'_>) {}

    /// One pass of the main window's UI. `frame` has the window's graphics state once the device
    /// is up. Pop-out windows are egui viewports shown from here.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &Frame<'_>);

    /// Change the main window's input before its pass sees it.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw: &mut egui::RawInput) {}

    /// Whether a main-window event asks for a pass; `None` keeps egui-winit's answer.
    fn repaint_on_window_event(&mut self, _event: &WindowEvent) -> Option<bool> {
        None
    }

    /// Whether a device event (raw mouse motion) asks for a pass; `None` keeps egui-winit's answer.
    fn repaint_on_device_event(&mut self, _event: &winit::event::DeviceEvent) -> Option<bool> {
        None
    }

    /// Save what the app keeps across runs: every [`App::auto_save_interval`] and before exit.
    fn save(&mut self) {}

    /// The main window's size and place, given just before each [`App::save`] for the app to keep
    /// (and hand back as [`RunOptions::window_settings`] next run).
    fn window_settings(&mut self, _settings: egui_winit::WindowSettings) {}

    /// How often [`App::save`] runs while the app runs.
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    /// The colour the main window is cleared to before egui draws.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    /// Called once when the main window closes, before the loop ends.
    fn on_exit(&mut self) {}
}

/// The window's state an [`App`] pass can see.
pub struct Frame<'a> {
    render_state: Option<&'a RenderState>,
}

impl Frame<'_> {
    /// The device, queue and egui renderer the windows draw with.
    pub fn render_state(&self) -> Option<&RenderState> {
        self.render_state
    }
}

/// What the app is made with, once the main window and its device are up.
pub struct Created {
    pub ctx: egui::Context,
    pub render_state: Option<RenderState>,
    /// Wakes the loop from other threads.
    pub waker: Waker,
}

/// Wakes the loop from any thread.
#[derive(Clone)]
pub struct Waker(EventLoopProxy<UserEvent>);

impl Waker {
    /// Run the app's logic once now, without painting (a control request on a window the
    /// compositor gives no frames, #743).
    pub fn run_logic(&self) {
        // The loop is gone once the main window has closed: nothing to wake.
        let _ = self.0.send_event(UserEvent::RunLogicNow);
    }
}

/// How the main window is opened and drawn.
pub struct RunOptions {
    pub viewport: ViewportBuilder,
    /// Changes winit's event loop before it is built (X11 instead of Wayland, for one).
    pub event_loop_builder: Option<Box<dyn FnOnce(&mut winit::event_loop::EventLoopBuilder<UserEvent>)>>,
    /// The device and surfaces, as eframe takes them (`NativeOptions::wgpu_options`).
    pub wgpu: WgpuConfiguration,
    pub renderer: egui_wgpu::RendererOptions,
    pub transparent: bool,
    /// The main window's size and place from the last run, restored when it opens (clamped to
    /// the monitors there are now), as eframe's `persist_window` does.
    pub window_settings: Option<egui_winit::WindowSettings>,
}

/// An error running the window.
#[derive(thiserror::Error, Debug)]
pub enum RunError {
    #[error(transparent)]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error(transparent)]
    Os(#[from] winit::error::OsError),
    #[error(transparent)]
    Gpu(#[from] WgpuError),
}

/// What the loop is woken with from other threads.
#[derive(Debug)]
pub enum UserEvent {
    /// egui asked `viewport` to repaint at `when`, during the pass numbered `cumulative_pass_nr`.
    Repaint { viewport: ViewportId, when: Instant, cumulative_pass_nr: u64 },
    /// [`Waker::run_logic`].
    RunLogicNow,
}

/// Opens the main window, makes the app with `create` once the window and its device are up, and
/// runs it until the main window closes.
///
/// # Errors
/// The event loop, the main window or the graphics device could not be created.
pub fn run<'a>(mut options: RunOptions, create: impl FnOnce(Created) -> Box<dyn App + 'a> + 'a) -> Result<(), RunError> {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    if let Some(change) = options.event_loop_builder.take() {
        change(&mut builder);
    }
    let event_loop = builder.build()?;
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let proxy = event_loop.create_proxy();
    let waker = Waker(proxy.clone());
    ctx.set_request_repaint_callback(move |info| {
        let when = Instant::now() + info.delay;
        let cumulative_pass_nr = info.current_cumulative_pass_nr;
        // The loop is gone once the main window has closed: nothing to wake.
        let _ = proxy.send_event(UserEvent::Repaint { viewport: info.viewport_id, when, cumulative_pass_nr });
    });
    let painter = pollster::block_on(Painter::new(ctx.clone(), options.wgpu, options.transparent, options.renderer));
    let mut viewports = ViewportIdMap::default();
    viewports.insert(ViewportId::ROOT, Viewport::new(ViewportIdPair::ROOT, ViewportClass::Root, options.viewport, None));
    let shared = Rc::new(RefCell::new(Shared {
        ctx: ctx.clone(),
        painter,
        viewports,
        by_window: HashMap::new(),
        restore: options.window_settings,
        normal_geometry: None,
    }));
    egui::Context::set_immediate_viewport_renderer({
        let shared = Rc::downgrade(&shared);
        move |ctx, immediate| match shared.upgrade() {
            Some(shared) => render_immediate(&shared, immediate),
            // The loop has ended; egui still needs the callback run.
            None => run_without_window(ctx, immediate),
        }
    });
    let mut runner = Runner { shared, app: None, create: Some(Box::new(create)), waker, saved_at: Instant::now(), error: None };
    event_loop.run_app(&mut runner)?;
    match runner.error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// One native window and its egui viewport.
struct Viewport {
    ids: ViewportIdPair,
    class: ViewportClass,
    builder: ViewportBuilder,
    /// Commands from a pass before the window existed, applied once it does.
    deferred_commands: Vec<ViewportCommand>,
    info: ViewportInfo,
    actions: Vec<egui_winit::ActionRequested>,
    /// A deferred viewport's UI (`None` for the main window and immediate viewports).
    ui_cb: Option<Arc<DeferredViewportUiCallback>>,
    window: Option<Arc<Window>>,
    state: Option<egui_winit::State>,
    pending_delta: egui::epaint::textures::TexturesDelta,
    /// When the window asked to be painted next, if it did.
    repaint_at: Option<Instant>,
}

impl Viewport {
    fn new(ids: ViewportIdPair, class: ViewportClass, builder: ViewportBuilder, ui_cb: Option<Arc<DeferredViewportUiCallback>>) -> Self {
        Self {
            ids,
            class,
            builder,
            deferred_commands: Vec::new(),
            info: ViewportInfo::default(),
            actions: Vec::new(),
            ui_cb,
            window: None,
            state: None,
            pending_delta: Default::default(),
            repaint_at: Some(Instant::now()),
        }
    }

    fn ask_repaint(&mut self, when: Instant) {
        self.repaint_at = Some(self.repaint_at.map_or(when, |at| at.min(when)));
    }
}

/// What the loop and the immediate viewport renderer share.
struct Shared {
    ctx: egui::Context,
    painter: Painter,
    viewports: ViewportIdMap<Viewport>,
    by_window: HashMap<WindowId, ViewportId>,
    /// The main window's settings to restore when it opens.
    restore: Option<egui_winit::WindowSettings>,
    /// The main window's last size and place while neither maximized nor fullscreen.
    normal_geometry: Option<egui_winit::WindowGeometry>,
}

impl Shared {
    /// Opens the windows of viewports that have none yet (the main one first).
    fn open_pending(&mut self, event_loop: &ActiveEventLoop) -> Result<(), RunError> {
        let mut pending: Vec<ViewportId> = self.viewports.iter().filter(|(_, v)| v.window.is_none()).map(|(id, _)| *id).collect();
        pending.sort_by_key(|id| *id != ViewportId::ROOT);
        for id in pending {
            let Some(viewport) = self.viewports.get_mut(&id) else { continue };
            let restore = if id == ViewportId::ROOT { self.restore.take() } else { None };
            if let Some(mut settings) = restore {
                let zoom = self.ctx.zoom_factor();
                settings.clamp_size_to_sane_values(largest_monitor_points(zoom, event_loop));
                settings.clamp_position_to_monitors(zoom, event_loop);
                viewport.builder = settings.initialize_viewport_builder(zoom, viewport.builder.clone());
                self.restore = Some(settings);
            }
            let window = match egui_winit::create_window(&self.ctx, event_loop, &viewport.builder) {
                Ok(window) => Arc::new(window),
                Err(error) if id == ViewportId::ROOT => return Err(error.into()),
                Err(error) => {
                    log::warn!("The window of viewport {id:?} could not be opened: {error}");
                    continue;
                }
            };
            match pollster::block_on(self.painter.set_window(id, Some(Arc::clone(&window)))) {
                Ok(()) => {}
                Err(error) if id == ViewportId::ROOT => return Err(error.into()),
                Err(error) => {
                    log::warn!("The window of viewport {id:?} has no surface: {error}");
                    continue;
                }
            }
            let state = egui_winit::State::new(
                self.ctx.clone(),
                id,
                event_loop,
                Some(window.scale_factor() as f32),
                event_loop.system_theme(),
                self.painter.max_texture_side(),
            );
            egui_winit::update_viewport_info(&mut viewport.info, &self.ctx, &window, true);
            let commands = std::mem::take(&mut viewport.deferred_commands);
            egui_winit::process_viewport_commands(&self.ctx, &mut viewport.info, commands, &window, &mut viewport.actions);
            if id == ViewportId::ROOT
                && let Some(settings) = self.restore.take()
            {
                settings.initialize_window(&window);
            }
            self.by_window.insert(window.id(), id);
            viewport.window = Some(window);
            viewport.state = Some(state);
            viewport.ask_repaint(Instant::now());
        }
        Ok(())
    }

    /// The input of `id`'s next pass, with every viewport's info, if it has a window.
    fn take_input(&mut self, id: ViewportId) -> Option<(egui::RawInput, Arc<Window>)> {
        let infos: ViewportIdMap<ViewportInfo> = self.viewports.iter().map(|(id, v)| (*id, v.info.clone())).collect();
        let viewport = self.viewports.get_mut(&id)?;
        let window = viewport.window.clone()?;
        egui_winit::update_viewport_info(&mut viewport.info, &self.ctx, &window, false);
        let state = viewport.state.as_mut()?;
        let mut input = state.take_egui_input(&window);
        input.viewports = infos;
        if let Some(info) = input.viewports.get_mut(&id) {
            *info = viewport.info.clone();
        }
        Some((input, window))
    }

    /// Paints `id`'s pass output and applies what it asked of the windows.
    fn finish(&mut self, id: ViewportId, output: egui::FullOutput, clear: [f32; 4], event_loop: Option<&ActiveEventLoop>) {
        let egui::FullOutput { platform_output, textures_delta, shapes, pixels_per_point, viewport_output } = output;
        if let Some(viewport) = self.viewports.get_mut(&id) {
            viewport.info.events.clear();
            viewport.pending_delta.append(textures_delta);
            if let (Some(window), Some(state)) = (viewport.window.clone(), viewport.state.as_mut()) {
                match event_loop {
                    Some(event_loop) => state.handle_platform_output_with_event_loop(&window, event_loop, platform_output),
                    None => state.handle_platform_output(&window, platform_output),
                }
                let clipped = self.ctx.tessellate(shapes, pixels_per_point);
                let mut screenshots = Vec::new();
                viewport.actions.retain(|action| match action {
                    egui_winit::ActionRequested::Screenshot(callback) => {
                        screenshots.push(callback.clone());
                        false
                    }
                    _ => true,
                });
                self.painter.paint_and_update_textures(id, pixels_per_point, clear, &clipped, &mut viewport.pending_delta, screenshots, &window);
                for action in viewport.actions.drain(..) {
                    let event = match action {
                        egui_winit::ActionRequested::Cut => Some(egui::Event::Cut),
                        egui_winit::ActionRequested::Copy => Some(egui::Event::Copy),
                        egui_winit::ActionRequested::Paste => state.clipboard_text().map(|text| egui::Event::Paste(text.replace("\r\n", "\n"))),
                        _ => None,
                    };
                    state.egui_input_mut().events.extend(event);
                }
            }
        }
        self.apply_viewport_output(viewport_output);
    }

    /// Adds viewports a pass showed, applies their commands and drops the ones it no longer shows.
    fn apply_viewport_output(&mut self, outputs: egui::OrderedViewportIdMap<ViewportOutput>) {
        let shown: ViewportIdSet = outputs.keys().copied().collect();
        let ctx = self.ctx.clone();
        for (id, output) in outputs {
            let ViewportOutput { parent, class, builder, viewport_ui_cb, commands, repaint_delay } = output;
            let ids = ViewportIdPair::from_self_and_parent(id, parent);
            let viewport = self.viewport_for(ids, class, builder, viewport_ui_cb);
            match &viewport.window {
                Some(window) => egui_winit::process_viewport_commands(&ctx, &mut viewport.info, commands, window, &mut viewport.actions),
                None => viewport.deferred_commands.extend(commands),
            }
            if repaint_delay < std::time::Duration::MAX {
                viewport.ask_repaint(Instant::now() + repaint_delay);
            }
        }
        // A pass only lists the viewports it showed; the main window always stays.
        let gone: Vec<ViewportId> = self.viewports.keys().copied().filter(|id| *id != ViewportId::ROOT && !shown.contains(id)).collect();
        for id in gone {
            if let Some(window) = self.viewports.remove(&id).and_then(|v| v.window) {
                self.by_window.remove(&window.id());
            }
        }
        let alive: ViewportIdSet = self.viewports.keys().copied().collect();
        self.painter.gc_viewports(&alive);
    }

    /// The viewport `ids.this`, added or updated with what its pass gave; a builder change that
    /// needs a new window drops the old one, so the loop opens it again.
    fn viewport_for(
        &mut self,
        ids: ViewportIdPair,
        class: ViewportClass,
        mut builder: ViewportBuilder,
        ui_cb: Option<Arc<DeferredViewportUiCallback>>,
    ) -> &mut Viewport {
        if builder.icon.is_none() {
            builder.icon = self.viewports.get(&ids.parent).and_then(|parent| parent.builder.icon.clone());
        }
        match self.viewports.entry(ids.this) {
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(Viewport::new(ids, class, builder, ui_cb)),
            std::collections::hash_map::Entry::Occupied(entry) => {
                let viewport = entry.into_mut();
                viewport.class = class;
                viewport.ids.parent = ids.parent;
                viewport.ui_cb = ui_cb;
                let (mut commands, recreate) = viewport.builder.patch(builder);
                if recreate {
                    if let Some(window) = viewport.window.take() {
                        self.by_window.remove(&window.id());
                    }
                    viewport.state = None;
                    if let Err(error) = pollster::block_on(self.painter.set_window(ids.this, None)) {
                        log::warn!("The window of viewport {:?} could not be let go: {error}", ids.this);
                    }
                }
                viewport.deferred_commands.append(&mut commands);
                viewport
            }
        }
    }
}

/// The largest monitor's size in points (16000 a side if there is none), as eframe clamps a
/// restored window to.
fn largest_monitor_points(zoom: f32, event_loop: &ActiveEventLoop) -> egui::Vec2 {
    let largest = event_loop.available_monitors().fold(egui::Vec2::ZERO, |largest, monitor| {
        let size = monitor.size().to_logical::<f32>(f64::from(zoom) * monitor.scale_factor());
        largest.max(egui::vec2(size.width, size.height))
    });
    if largest == egui::Vec2::ZERO { egui::Vec2::splat(16000.0) } else { largest }
}

/// An immediate viewport's pass, run inside its parent's: with its window, painted there;
/// without one yet, run once with no input and painted nothing, and its window opened when the
/// loop next waits.
fn render_immediate(shared: &Rc<RefCell<Shared>>, immediate: ImmediateViewport<'_>) {
    let ImmediateViewport { ids, builder, mut viewport_ui_cb } = immediate;
    let (ctx, input) = {
        let Ok(mut shared) = shared.try_borrow_mut() else {
            log::warn!("An immediate viewport was shown while the windows were busy; it runs without a window this pass");
            return run_without_window(&egui::Context::default(), ImmediateViewport { ids, builder, viewport_ui_cb });
        };
        shared.viewport_for(ids, ViewportClass::Immediate, builder, None);
        (shared.ctx.clone(), shared.take_input(ids.this))
    };
    let Some((input, _window)) = input else {
        let raw = egui::RawInput { viewport_id: ids.this, ..Default::default() };
        // Nothing to paint into yet: the output is dropped.
        drop(ctx.run_ui(raw, |ui| viewport_ui_cb(ui)));
        return;
    };
    let output = ctx.run_ui(input, |ui| viewport_ui_cb(ui));
    match shared.try_borrow_mut() {
        Ok(mut shared) => shared.finish(ids.this, output, [0.0; 4], None),
        Err(_) => log::warn!("An immediate viewport's frame was dropped: the windows were busy"),
    }
}

/// Runs an immediate viewport's callback in a pass that paints nothing.
fn run_without_window(ctx: &egui::Context, immediate: ImmediateViewport<'_>) {
    let ImmediateViewport { ids, mut viewport_ui_cb, .. } = immediate;
    let raw = egui::RawInput { viewport_id: ids.this, ..Default::default() };
    drop(ctx.run_ui(raw, |ui| viewport_ui_cb(ui)));
}

struct Runner<'a> {
    shared: Rc<RefCell<Shared>>,
    /// Made once the main window is up.
    app: Option<Box<dyn App + 'a>>,
    create: Option<Box<dyn FnOnce(Created) -> Box<dyn App + 'a> + 'a>>,
    waker: Waker,
    /// When the app last saved.
    saved_at: Instant,
    /// The error that ended the loop.
    error: Option<RunError>,
}

impl Runner<'_> {
    fn open_pending(&mut self, event_loop: &ActiveEventLoop) {
        let opened = self.shared.borrow_mut().open_pending(event_loop);
        if let Err(error) = opened {
            log::error!("The window could not be opened: {error}");
            self.error = Some(error);
            event_loop.exit();
            return;
        }
        if self.app.is_none()
            && let Some(create) = self.create.take()
        {
            let (ctx, render_state) = {
                let shared = self.shared.borrow();
                (shared.ctx.clone(), shared.painter.render_state())
            };
            self.app = Some(create(Created { ctx, render_state, waker: self.waker.clone() }));
        }
    }

    /// The app's logic alone, without painting: no input is taken, so the next pass still has it.
    fn run_logic(&mut self, event_loop: &ActiveEventLoop) {
        let Some(app) = self.app.as_mut() else { return };
        let (ctx, render_state, mut raw) = {
            let shared = self.shared.borrow();
            let mut raw = egui::RawInput { viewport_id: ViewportId::ROOT, ..Default::default() };
            raw.viewports = shared.viewports.iter().map(|(id, v)| (*id, v.info.clone())).collect();
            (shared.ctx.clone(), shared.painter.render_state(), raw)
        };
        app.raw_input_hook(&ctx, &mut raw);
        let output = ctx.run_logic(&raw, |ctx| app.logic(ctx, &Frame { render_state: render_state.as_ref() }));
        let mut shared = self.shared.borrow_mut();
        if let Some(viewport) = shared.viewports.get_mut(&ViewportId::ROOT)
            && let (Some(window), Some(state)) = (viewport.window.clone(), viewport.state.as_mut())
        {
            state.handle_platform_output_with_event_loop(&window, event_loop, output.platform_output);
        }
        for (id, commands) in output.viewport_commands {
            let ctx = shared.ctx.clone();
            if let Some(viewport) = shared.viewports.get_mut(&id) {
                match &viewport.window {
                    Some(window) => egui_winit::process_viewport_commands(&ctx, &mut viewport.info, commands, window, &mut viewport.actions),
                    None => viewport.deferred_commands.extend(commands),
                }
            }
        }
    }

    /// The app saves every [`App::auto_save_interval`].
    fn maybe_save(&mut self) {
        let due = self.app.as_ref().is_some_and(|app| self.saved_at.elapsed() >= app.auto_save_interval());
        if due {
            self.save();
            self.saved_at = Instant::now();
        }
    }

    /// The main window's settings, then the app's save.
    fn save(&mut self) {
        let settings = {
            let mut shared = self.shared.borrow_mut();
            let zoom = shared.ctx.zoom_factor();
            let window = shared.viewports.get(&ViewportId::ROOT).and_then(|root| root.window.clone());
            window.map(|window| {
                let settings = egui_winit::WindowSettings::from_window(zoom, &window, shared.normal_geometry);
                shared.normal_geometry = Some(settings.normal_geometry());
                settings
            })
        };
        if let Some(app) = self.app.as_mut() {
            if let Some(settings) = settings {
                app.window_settings(settings);
            }
            app.save();
        }
    }

    /// Runs viewport `id`'s pass and paints it. An immediate viewport is painted in its parent's.
    fn paint(&mut self, event_loop: &ActiveEventLoop, id: ViewportId) {
        let Some(app) = self.app.as_mut() else { return };
        let (mut input, ui_cb, render_state, ctx) = {
            let mut shared = self.shared.borrow_mut();
            let Some(viewport) = shared.viewports.get_mut(&id) else { return };
            viewport.repaint_at = None;
            if viewport.class == ViewportClass::Immediate {
                let parent = viewport.ids.parent;
                if let Some(parent) = shared.viewports.get_mut(&parent) {
                    parent.ask_repaint(Instant::now());
                }
                return;
            }
            let ui_cb = viewport.ui_cb.clone();
            let Some((input, _)) = shared.take_input(id) else { return };
            (input, ui_cb, shared.painter.render_state(), shared.ctx.clone())
        };
        // No borrow is held here: immediate viewports shown by this pass take it.
        let root = ui_cb.is_none();
        if root {
            app.raw_input_hook(&ctx, &mut input);
        }
        let close_requested = root && input.viewport().close_requested();
        let output = match &ui_cb {
            Some(ui_cb) => ctx.run_ui(input, |ui| ui_cb(ui)),
            None => ctx.run_ui(input, |ui| {
                let frame = Frame { render_state: render_state.as_ref() };
                app.logic(ui.ctx(), &frame);
                app.ui(ui, &frame);
            }),
        };
        // The app answers a close with `ViewportCommand::CancelClose` in the same pass.
        let canceled = output.viewport_output.get(&ViewportId::ROOT).is_some_and(|root| root.commands.contains(&egui::ViewportCommand::CancelClose));
        let clear = if root { app.clear_color(&ctx.global_style().visuals) } else { [0.0; 4] };
        self.shared.borrow_mut().finish(id, output, clear, Some(event_loop));
        if close_requested && !canceled {
            event_loop.exit();
        } else if root {
            self.maybe_save();
        }
    }
}

impl ApplicationHandler<UserEvent> for Runner<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.open_pending(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RunLogicNow => self.run_logic(event_loop),
            UserEvent::Repaint { viewport, when, cumulative_pass_nr } => {
                let mut shared = self.shared.borrow_mut();
                // A request from a pass that has since been followed by another is already met.
                if shared.ctx.cumulative_pass_nr_for(viewport) != cumulative_pass_nr {
                    return;
                }
                if let Some(v) = shared.viewports.get_mut(&viewport) {
                    v.ask_repaint(when);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(id) = self.shared.borrow().by_window.get(&window_id).copied() else { return };
        if matches!(event, WindowEvent::RedrawRequested) {
            self.paint(event_loop, id);
            return;
        }
        let mut shared = self.shared.borrow_mut();
        let Shared { painter, viewports, .. } = &mut *shared;
        let Some(viewport) = viewports.get_mut(&id) else { return };
        match &event {
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                    painter.on_window_resized(id, width, height);
                }
            }
            WindowEvent::CloseRequested => {
                // The app may cancel the close (`ViewportCommand::CancelClose`) in its next pass.
                viewport.info.events.push(egui::ViewportEvent::Close);
                viewport.ask_repaint(Instant::now());
            }
            _ => {}
        }
        if let (Some(window), Some(state)) = (viewport.window.clone(), viewport.state.as_mut()) {
            let repaint = state.on_window_event(&window, &event).repaint;
            // The app may answer for the main window's events (pointer motion that changes nothing).
            let asked = match self.app.as_mut() {
                Some(app) if id == ViewportId::ROOT => app.repaint_on_window_event(&event).unwrap_or(repaint),
                _ => repaint,
            };
            if asked {
                viewport.ask_repaint(Instant::now());
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        let winit::event::DeviceEvent::MouseMotion { delta } = event else { return };
        let mut shared = self.shared.borrow_mut();
        // Raw motion goes to the focused window, as in eframe.
        for (id, viewport) in shared.viewports.iter_mut() {
            if viewport.info.focused == Some(true)
                && let Some(state) = viewport.state.as_mut()
            {
                let repaint = state.on_mouse_motion(delta);
                let asked = match self.app.as_mut() {
                    Some(app) if *id == ViewportId::ROOT => app.repaint_on_device_event(&event).unwrap_or(repaint),
                    _ => repaint,
                };
                if asked {
                    viewport.ask_repaint(Instant::now());
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.open_pending(event_loop);
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for viewport in self.shared.borrow().viewports.values() {
            match (viewport.repaint_at, &viewport.window) {
                (Some(at), Some(window)) if at <= now => window.request_redraw(),
                (Some(at), Some(_)) => next = Some(next.map_or(at, |n| n.min(at))),
                _ => {}
            }
        }
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.save();
        if let Some(app) = self.app.as_mut() {
            app.on_exit();
        }
        self.shared.borrow_mut().painter.destroy();
    }
}
