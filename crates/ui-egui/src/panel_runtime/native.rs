//! Desktop registrations, startup and placement for shared panel viewports.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use solidcraft_engine::panels::{
    HostCapabilities, HostEffect, HostId, InstanceId, MAX_INSTANCES, MAX_LOGICAL, MAX_SECTIONS, PanelResult, Rule, SectionId, Size, error, valid_id,
};

use super::NativeHostAdapter;

const MAX_MONITORS: usize = 64;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(2);
/// A window's frame and title bar before the OS has reported its own.
const DEFAULT_DECORATION: egui::Vec2 = egui::vec2(16.0, 48.0);

/// Physical monitor bounds and DPI from the desktop window service.
pub struct NativeMonitor {
    pub name: Option<String>,
    pub bounds: egui::Rect,
    pub scale: f32,
    pub primary: bool,
}

#[derive(Clone)]
struct Registration {
    section: SectionId,
    viewport: egui::ViewportId,
    startup: Option<Instant>,
    ready: bool,
}

struct Placement {
    size: egui::Vec2,
    decoration: egui::Vec2,
    monitor: Option<MonitorPlacement>,
}

struct MonitorPlacement {
    name: Option<String>,
    origin: egui::Pos2,
    offset: egui::Vec2,
}

#[derive(Default)]
struct DesktopState {
    hosts: BTreeMap<HostId, Registration>,
    placement: BTreeMap<InstanceId, Placement>,
    monitors: Vec<NativeMonitor>,
    monitors_updated: Option<Instant>,
}

/// Both the layout runtime and the UI use the same desktop registration table.
#[derive(Clone, Default)]
pub struct DesktopNativeAdapter(Rc<RefCell<DesktopState>>);

impl DesktopNativeAdapter {
    pub fn update_monitors(&self, monitors: impl FnOnce() -> Vec<NativeMonitor>) {
        let now = Instant::now();
        {
            let state = self.0.borrow();
            if state.hosts.is_empty() || state.monitors_updated.is_some_and(|last| now.duration_since(last) < Duration::from_secs(1)) {
                return;
            }
        }
        let monitors = monitors()
            .into_iter()
            .take(MAX_MONITORS)
            .filter(|m| m.bounds.is_finite() && m.bounds.is_positive() && (0.1..=16.0).contains(&m.scale))
            .collect();
        let mut state = self.0.borrow_mut();
        state.monitors = monitors;
        state.monitors_updated = Some(now);
    }

    pub fn owns(&self, host: &HostId, section: &SectionId) -> bool {
        self.0.borrow().hosts.get(host).is_some_and(|registration| &registration.section == section)
    }

    pub fn viewport_id(&self, host: &HostId) -> Option<egui::ViewportId> {
        self.0.borrow().hosts.get(host).map(|registration| registration.viewport)
    }

    /// The next visible presentation must negotiate a fresh OS window after
    /// the old viewport was omitted from egui's output.
    pub fn suspend(&self, host: &HostId) {
        let mut state = self.0.borrow_mut();
        if let Some(registration) = state.hosts.get_mut(host) {
            let was_started = registration.startup.take().is_some();
            if was_started || registration.ready {
                registration.viewport = next_viewport_id(host);
                registration.ready = false;
            }
        }
    }

    pub fn builder(
        &self,
        ctx: &egui::Context,
        host: &HostId,
        instance: &InstanceId,
        title: String,
        preferred: Size,
        bounds: (Size, Size),
    ) -> Option<egui::ViewportBuilder> {
        let (min, max) = bounds;
        preferred.validate().ok()?;
        min.validate().ok()?;
        max.validate().ok()?;
        if !min.within(min, max) {
            return None;
        }
        let state = self.0.borrow();
        let registration = state.hosts.get(host)?;
        let min = egui::vec2(min.width, min.height);
        let max = egui::vec2(max.width, max.height);
        let builder = egui::ViewportBuilder::default().with_title(title).with_min_inner_size(min).with_max_inner_size(max).with_visible(true);
        if registration.ready {
            return Some(builder);
        }
        let saved = state.placement.get(instance);
        let size = saved.map_or_else(|| egui::vec2(preferred.width, preferred.height), |saved| saved.size).clamp(min, max);
        let remembered = saved.and_then(|saved| saved.monitor.as_ref());
        let monitor = remembered.and_then(|saved| {
            state
                .monitors
                .iter()
                .filter(|m| match &saved.name {
                    Some(name) => m.name.as_ref() == Some(name),
                    None => m.bounds.min == saved.origin,
                })
                .min_by(|a, b| a.bounds.min.distance_sq(saved.origin).total_cmp(&b.bounds.min.distance_sq(saved.origin)))
        });
        let offset = monitor.and(remembered).map(|saved| saved.offset);
        let parent = (remembered.is_none())
            .then(|| {
                ctx.input(|input| {
                    let viewport = input.viewport();
                    viewport.outer_rect.zip(viewport.native_pixels_per_point)
                })
            })
            .flatten();
        let monitor = monitor
            .or_else(|| {
                let (parent, scale) = parent?;
                let center = (parent.center().to_vec2() * (ctx.zoom_factor() * scale)).to_pos2();
                state.monitors.iter().find(|m| m.bounds.contains(center))
            })
            .or_else(|| state.monitors.iter().find(|m| m.primary))
            .or_else(|| state.monitors.first());
        let decoration = saved.map_or(DEFAULT_DECORATION, |saved| saved.decoration);
        let Some((size, position)) = monitor.and_then(|monitor| fit(monitor, ctx.zoom_factor() * monitor.scale, size, min, decoration, offset))
        else {
            return Some(builder.with_inner_size(size));
        };
        Some(builder.with_inner_size(size).with_position(position))
    }

    /// Ask the integration to create the OS window first. Immediate viewports
    /// require a callback, which eframe cannot call if OS creation failed.
    pub fn ready(&self, ctx: &egui::Context, host: &HostId, builder: &egui::ViewportBuilder) -> PanelResult<bool> {
        if ctx.embed_viewports() {
            return Err(error(Rule::Capability, "native viewports unavailable"));
        }
        let mut state = self.0.borrow_mut();
        let registration = state.hosts.get_mut(host).ok_or_else(|| error(Rule::MissingTarget, "native host absent"))?;
        let viewport = registration.viewport;
        let ready = ctx.input(|input| {
            input.raw.viewports.get(&viewport).and_then(|info| info.native_pixels_per_point).is_some_and(|scale| scale.is_finite() && scale > 0.0)
        });
        if ready {
            registration.ready = true;
            registration.startup = None;
            return Ok(true);
        }
        let start = registration.startup.get_or_insert_with(Instant::now);
        if start.elapsed() >= STARTUP_TIMEOUT {
            return Err(error(Rule::Lifecycle, "desktop panel window did not open"));
        }
        drop(state);
        ctx.show_viewport_deferred(viewport, builder.clone().with_visible(false), |_, _| {});
        ctx.request_repaint_after(Duration::from_millis(30));
        Ok(false)
    }

    /// Record normal window geometry from the child viewport. Egui reports no
    /// screen position on Wayland, where only the inner size can be restored.
    pub fn observe(&self, ctx: &egui::Context, host: &HostId, instance: &InstanceId) {
        let mut state = self.0.borrow_mut();
        if !state.hosts.get(host).is_some_and(|registration| registration.viewport == ctx.viewport_id()) {
            return;
        }
        let sample = ctx.input(|input| {
            let info = input.viewport();
            if info.visible() == Some(false) || info.maximized == Some(true) || info.fullscreen == Some(true) {
                return None;
            }
            let size = info.inner_rect.map_or_else(|| input.content_rect().size(), |rect| rect.size());
            let valid_size = Size { width: size.x, height: size.y }.validate().is_ok();
            valid_size.then_some((size, info.outer_rect, info.native_pixels_per_point))
        });
        let Some((size, outer, scale)) = sample else {
            return;
        };
        let outer = outer.filter(|rect| rect.is_finite() && rect.is_positive() && rect.size().max_elem() <= MAX_LOGICAL);
        let decoration = outer.map_or(egui::Vec2::ZERO, |rect| (rect.size() - size).max(egui::Vec2::ZERO));
        let monitor = outer.zip(scale).and_then(|(outer, scale)| monitor_placement(&state.monitors, outer, |_| ctx.zoom_factor() * scale));
        state.remember(instance, Placement { size, decoration, monitor });
    }

    /// The outer screen rectangle a panel's window opens at when a drag puts its
    /// corner at `corner`: its remembered size, else `preferred`, within `bounds`,
    /// kept on the monitor under it exactly as `builder` keeps it.
    pub fn landing(
        &self,
        ctx: &egui::Context,
        instance: &InstanceId,
        corner: egui::Pos2,
        preferred: Size,
        bounds: (Size, Size),
    ) -> Option<egui::Rect> {
        let state = self.0.borrow();
        let saved = state.placement.get(instance);
        let (min, max) = (egui::vec2(bounds.0.width, bounds.0.height), egui::vec2(bounds.1.width, bounds.1.height));
        let size = saved.map_or_else(|| egui::vec2(preferred.width, preferred.height), |saved| saved.size).clamp(min, max);
        let decoration = saved.map_or(DEFAULT_DECORATION, |saved| saved.decoration);
        let wanted = egui::Rect::from_min_size(corner, size + decoration);
        let scale = |monitor: &NativeMonitor| ctx.zoom_factor() * monitor.scale;
        // The monitor under the window's centre, else the nearest one.
        let area = |monitor: &NativeMonitor| egui::Rect::from_min_size(monitor.bounds.min / scale(monitor), monitor.bounds.size() / scale(monitor));
        let nearest =
            state.monitors.iter().min_by(|a, b| area(a).distance_sq_to_pos(wanted.center()).total_cmp(&area(b).distance_sq_to_pos(wanted.center())));
        let placed = nearest.and_then(|monitor| fit(monitor, scale(monitor), size, min, decoration, Some(corner - area(monitor).min)));
        let rect = placed.map_or(wanted, |(size, corner)| egui::Rect::from_min_size(corner, size + decoration));
        (rect.is_finite() && rect.is_positive() && rect.size().max_elem() <= MAX_LOGICAL).then_some(rect)
    }

    /// Open the panel's next window at `outer`, the rectangle a drag showed.
    pub fn place(&self, ctx: &egui::Context, instance: &InstanceId, outer: egui::Rect) {
        let mut state = self.0.borrow_mut();
        let decoration = state.placement.get(instance).map_or(DEFAULT_DECORATION, |saved| saved.decoration);
        let size = outer.size() - decoration;
        if !outer.is_finite() || size.min_elem() <= 0.0 || size.max_elem() > MAX_LOGICAL {
            return;
        }
        let monitor = monitor_placement(&state.monitors, outer, |monitor| ctx.zoom_factor() * monitor.scale);
        state.remember(instance, Placement { size, decoration, monitor });
    }
}

impl DesktopState {
    fn remember(&mut self, instance: &InstanceId, placement: Placement) {
        if self.placement.len() >= MAX_INSTANCES && !self.placement.contains_key(instance) {
            self.placement.pop_first();
        }
        self.placement.insert(instance.clone(), placement);
    }
}

/// A window's inner size and outer corner on `monitor`: as large as fits, at
/// `offset` from the monitor's corner where it fits, else centred.
fn fit(
    monitor: &NativeMonitor,
    scale: f32,
    size: egui::Vec2,
    min: egui::Vec2,
    decoration: egui::Vec2,
    offset: Option<egui::Vec2>,
) -> Option<(egui::Vec2, egui::Pos2)> {
    let area = monitor.bounds.size() / scale;
    if !scale.is_finite() || scale <= 0.0 || !area.is_finite() {
        return None;
    }
    let size = size.min((area - decoration).max(min));
    let travel = (area - size - decoration).max(egui::Vec2::ZERO);
    let offset = offset.unwrap_or(travel / 2.0).clamp(egui::Vec2::ZERO, travel);
    let corner = (monitor.bounds.min.to_vec2() / scale + offset).to_pos2();
    corner.is_finite().then_some((size, corner))
}

/// The monitor under the centre of `outer` (logical points), and the window's
/// offset on it. `scale` gives a monitor's points-to-pixels factor.
fn monitor_placement(monitors: &[NativeMonitor], outer: egui::Rect, scale: impl Fn(&NativeMonitor) -> f32) -> Option<MonitorPlacement> {
    monitors.iter().find_map(|monitor| {
        let scale = scale(monitor);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let center = (outer.center().to_vec2() * scale).to_pos2();
        monitor.bounds.contains(center).then(|| MonitorPlacement {
            name: monitor.name.clone(),
            origin: monitor.bounds.min,
            offset: outer.min.to_vec2() - monitor.bounds.min.to_vec2() / scale,
        })
    })
}

impl NativeHostAdapter for DesktopNativeAdapter {
    fn capabilities(&self) -> HostCapabilities {
        HostCapabilities { native_windows: true, persistence: false, max_native_hosts: MAX_SECTIONS }
    }

    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()> {
        if !effects
            .iter()
            .any(|effect| matches!(effect, HostEffect::CreateNative { .. } | HostEffect::RebindNative { .. } | HostEffect::ReleaseNative { .. }))
        {
            return Ok(());
        }
        let mut hosts: std::collections::BTreeSet<_> = self.0.borrow().hosts.keys().cloned().collect();
        for effect in effects {
            match effect {
                HostEffect::CreateNative { host, section } => {
                    valid_id(&host.0)?;
                    valid_id(&section.0)?;
                    if !hosts.insert(host.clone()) {
                        return Err(error(Rule::DuplicateId, "desktop panel host already registered"));
                    }
                }
                HostEffect::RebindNative { host, section } => {
                    valid_id(&section.0)?;
                    if !hosts.contains(host) {
                        return Err(error(Rule::MissingTarget, "desktop panel host cannot rebind"));
                    }
                }
                HostEffect::ReleaseNative { host } => {
                    hosts.remove(host);
                }
                _ => {}
            }
        }
        if hosts.len() > MAX_SECTIONS {
            return Err(error(Rule::Capability, "desktop panel window capacity"));
        }
        Ok(())
    }

    fn commit(&mut self, effects: &[HostEffect]) {
        let mut state = self.0.borrow_mut();
        for effect in effects {
            match effect {
                HostEffect::CreateNative { host, section } => {
                    let viewport = next_viewport_id(host);
                    state.hosts.insert(host.clone(), Registration { section: section.clone(), viewport, startup: None, ready: false });
                }
                HostEffect::RebindNative { host, section } => {
                    if let Some(registration) = state.hosts.get_mut(host) {
                        registration.section.clone_from(section);
                    }
                }
                HostEffect::ReleaseNative { host } => {
                    state.hosts.remove(host);
                }
                _ => {}
            }
        }
        if state.hosts.is_empty() {
            state.monitors_updated = None;
        }
    }
}

fn next_viewport_id(host: &HostId) -> egui::ViewportId {
    static NEXT_VIEWPORT: AtomicU64 = AtomicU64::new(0);
    let generation = NEXT_VIEWPORT.fetch_add(1, Ordering::Relaxed);
    egui::ViewportId::from_hash_of(("solidcraft-panel-window", host, generation))
}
