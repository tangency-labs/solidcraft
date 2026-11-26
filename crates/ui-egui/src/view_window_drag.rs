//! Native routing for view-tab drags. The strips report client coordinates and the
//! source translates its captured pointer to desktop pixels before hit testing.

use egui::{Align2, Context, Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_views::{DesignView, WindowId};

use crate::{SolveApp, app_bar::ViewDropZone};

use crate::view_tiles::DockTarget;
use crate::view_windows::{DragSource, DragWindow};

#[derive(Default)]
pub(crate) struct State {
    strips: Vec<Strip>,
    canvases: Vec<DockCanvas>,
    drag: Option<Drag>,
    flight: Option<Flight>,
    hovered: Option<(egui::ViewportId, u64, u64)>,
}

struct Drag {
    viewport: egui::ViewportId,
    tab: u64,
    window: WindowId,
    view: DesignView,
    origin: Rect,
    source: DragSource,
    keyboard: Option<usize>,
    header: bool,
    target: Option<Target>,
    pointer: Pos2,
    screen: Option<Screen>,
    source_is_last: bool,
}

struct Flight {
    view: DesignView,
    viewport: egui::ViewportId,
    from: Rect,
    origin: Rect,
    start: f64,
}

struct DockCanvas {
    viewport: egui::ViewportId,
    tab: u64,
    window: WindowId,
    screen: Screen,
    bounds: Rect,
    covers: Vec<Rect>,
    targets: Option<(u64, Vec<DockTarget>)>,
}

struct Strip {
    viewport: egui::ViewportId,
    tab: u64,
    window: WindowId,
    screen: Screen,
    zone: ViewDropZone,
    count: usize,
    name: String,
}

#[derive(Clone, Copy)]
struct Screen {
    origin: Pos2,
    pixels_per_point: f32,
}

impl Screen {
    fn current(ctx: &Context) -> Option<Self> {
        let pixels_per_point = ctx.pixels_per_point();
        let inner = ctx.input(|i| i.viewport().inner_rect)?;
        (inner.is_finite() && inner.is_positive() && pixels_per_point.is_finite() && pixels_per_point > 0.0)
            .then_some(Self { origin: (inner.min.to_vec2() * pixels_per_point).to_pos2(), pixels_per_point })
    }

    fn desktop(self, local: Pos2) -> Pos2 {
        self.origin + local.to_vec2() * self.pixels_per_point
    }

    fn local(self, desktop: Pos2) -> Pos2 {
        ((desktop - self.origin) / self.pixels_per_point).to_pos2()
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Slot {
    viewport: egui::ViewportId,
    window: WindowId,
    index: usize,
    hit: Rect,
    center: Pos2,
}

enum Target {
    Strip { slot: Slot, label: String, blocked: bool },
    Elsewhere { window: WindowId, rect: Rect, label: String, blocked: bool },
    Dock { dock: DockTarget, attraction: f32, viewport: egui::ViewportId, window: WindowId },
    Window { window: DragWindow, refusal: Option<&'static str> },
}

impl Target {
    fn label(&self) -> &str {
        match self {
            Self::Strip { label, .. } | Self::Elsewhere { label, .. } => label,
            Self::Dock { dock, .. } => &dock.label,
            Self::Window { window, refusal } => refusal.unwrap_or(&window.label),
        }
    }

    fn viewport(&self, source: egui::ViewportId) -> egui::ViewportId {
        match self {
            Self::Strip { slot, blocked: false, .. } => slot.viewport,
            Self::Dock { viewport, .. } => *viewport,
            _ => source,
        }
    }
}

impl DockCanvas {
    fn target_at(&self, drag: &Drag, point: Pos2) -> Option<Target> {
        let (view, targets) = self.targets.as_ref()?;
        if *view != drag.view.id || self.covers.iter().any(|rect| rect.contains(point)) {
            return None;
        }
        let previous = match &drag.target {
            Some(Target::Dock { dock, attraction, viewport, .. }) if *viewport == self.viewport && *attraction == 1.0 => Some(dock.placement),
            _ => None,
        };
        let (dock, attraction) = crate::view_tiles::dock_at(targets, point, previous)?;
        Some(Target::Dock { dock: dock.clone(), attraction, viewport: self.viewport, window: self.window })
    }
}

impl Strip {
    fn slots(&self) -> Vec<Slot> {
        let mut left = self.zone.rect.left();
        let band = self.zone.rect.expand2(vec2(0.0, 16.0));
        let mut slots = Vec::with_capacity(self.zone.view_centres.len() + 1);
        for (index, right) in self.zone.view_centres.iter().copied().chain([(self.count, self.zone.rect.right())]) {
            if right >= left {
                slots.push(Slot {
                    viewport: self.viewport,
                    window: self.window,
                    index,
                    hit: Rect::from_min_max(pos2(left, band.top()), pos2(right, band.bottom())),
                    center: pos2((left + right) * 0.5, self.zone.rect.center().y),
                });
            }
            left = right;
        }
        slots
    }

    fn drop_at(&self, pointer: Pos2, previous: Option<Slot>) -> Option<Slot> {
        let slots = self.slots();
        slots
            .iter()
            .copied()
            .find(|slot| {
                previous.is_some_and(|old| old.viewport == slot.viewport && old.index == slot.index) && slot.hit.expand(6.0).contains(pointer)
            })
            .or_else(|| slots.into_iter().find(|slot| slot.hit.contains(pointer)))
    }

    fn keyboard_slots(&self, skip: Option<usize>) -> Vec<Slot> {
        let visible = self.slots();
        (0..=self.count)
            .filter(|index| Some(*index) != skip)
            .filter_map(|index| visible.iter().min_by_key(|slot| index.abs_diff(slot.index)).copied().map(|slot| Slot { index, ..slot }))
            .collect()
    }

    fn destination(&self, app: &SolveApp, drag: &Drag, slot: Slot) -> Target {
        let from = app.docs.views.views().iter().position(|view| view.id == drag.view.id);
        let index = slot.index - usize::from(slot.window == drag.window && from.is_some_and(|from| from < slot.index));
        let position = (index + 1).to_string();
        let blocked = slot.window != drag.window && drag.window == 0 && app.docs.views.len() == 1;
        let label = if blocked {
            app.tr("multi_window.last_view").to_owned()
        } else if slot.window == drag.window {
            solidcraft_ui_text::format("multi_window.drag_reorder", &[("n", &position)])
        } else {
            solidcraft_ui_text::format("multi_window.drag_move", &[("window", &self.name), ("n", &position)])
        };
        Target::Strip { slot, label, blocked }
    }
}

/// Register the current window's strips before resolving any input in the frame.
pub(crate) fn active(app: &SolveApp) -> bool {
    app.view_window_drag.drag.is_some()
}

pub(crate) fn record(app: &mut SolveApp, ctx: &Context, zones: Vec<(u64, usize, ViewDropZone)>, drag_continues: bool) {
    let viewport = ctx.viewport_id();
    let native = Screen::current(ctx);
    let window = app.view_windows.current_window();
    let name = crate::view_windows::window_choices(app)
        .iter()
        .find(|choice| choice.id == window)
        .map_or_else(|| app.session.doc.name.clone(), |choice| choice.label.clone());
    let state = &mut app.view_window_drag;
    ctx.input(|input| {
        state
            .strips
            .retain(|strip| strip.viewport != viewport && input.raw.viewports.get(&strip.viewport).is_some_and(|info| info.minimized != Some(true)));
    });
    // A minimised window paints no tab strip, so nothing else would ever clear the highlight it set.
    if let Some((source, _, _)) = state.hovered
        && ctx.input(|input| input.raw.viewports.get(&source).is_some_and(|info| info.minimized == Some(true)))
    {
        state.hovered = None;
        repaint_viewports(ctx);
    }
    let screen = native.unwrap_or(Screen { origin: Pos2::ZERO, pixels_per_point: 1.0 });
    state.strips.extend(zones.into_iter().map(|(tab, count, zone)| Strip { viewport, tab, window, screen, zone, count, name: name.clone() }));
    if state.drag.as_ref().is_some_and(|drag| drag.viewport == viewport && drag.keyboard.is_none() && !drag.header && !drag_continues)
        && let Some(drag) = state.drag.take()
    {
        cancel(state, ctx, drag);
    }
    if let Some(drag) = &state.drag
        && let Some(Target::Strip { slot, label, blocked: false }) = &drag.target
        && drag.viewport != viewport
        && slot.viewport == viewport
    {
        let pointer = drag.screen.map_or(slot.center, |source| screen.local(source.desktop(drag.pointer)));
        paint_tab(ctx, &drag.view, drag.origin, *slot, pointer, label);
    }
    if let Some(flight) = &state.flight
        && flight.viewport == viewport
        && !paint_flight(ctx, flight)
    {
        state.flight = None;
    }
}

pub(crate) fn hover(app: &mut SolveApp, ctx: &Context, view: Option<u64>) {
    let viewport = ctx.viewport_id();
    let state = &mut app.view_window_drag;
    let next = view.map(|view| (viewport, app.docs.current_tab(), view));
    if (next.is_some() || state.hovered.is_some_and(|(source, _, _)| source == viewport)) && state.hovered != next {
        state.hovered = next;
        repaint_viewports(ctx);
    }
}

fn repaint_viewports(ctx: &Context) {
    for viewport in ctx.input(|input| input.raw.viewports.keys().copied().collect::<Vec<_>>()) {
        ctx.request_repaint_of(viewport);
    }
}

pub(crate) fn hovered_view(app: &SolveApp) -> Option<u64> {
    app.view_window_drag.hovered.filter(|(_, tab, _)| *tab == app.docs.current_tab()).map(|(_, _, view)| view)
}

pub(crate) fn unanchored(app: &SolveApp, ctx: &Context, view: u64) -> bool {
    app.view_window_drag
        .drag
        .as_ref()
        .is_some_and(|drag| drag.tab == app.docs.current_tab() && drag.viewport == ctx.viewport_id() && drag.view.id == view)
}

pub(crate) fn start_header(app: &mut SolveApp, ctx: &Context, view: u64, origin: Rect) {
    if app.view_window_drag.drag.is_none()
        && let Some(mut drag) = begin(app, ctx, view, origin, false)
    {
        drag.header = true;
        app.view_window_drag.drag = Some(drag);
        repaint_viewports(ctx);
    }
}

/// Record the canvas after the receiving window has laid out its viewport and panels.
pub(crate) fn record_canvas(app: &mut SolveApp, ctx: &Context) {
    let viewport = ctx.viewport_id();
    if let Some((view, origin, last)) = app
        .view_window_drag
        .drag
        .as_ref()
        .filter(|drag| drag.viewport == viewport && drag.header && drag.tab == app.docs.current_tab())
        .map(|drag| (drag.view.id, drag.origin, drag.pointer))
    {
        let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            if let Some(drag) = app.view_window_drag.drag.take() {
                cancel(&mut app.view_window_drag, ctx, drag);
            }
        } else {
            let (pointer, down) = ctx.input(|i| (i.pointer.interact_pos().unwrap_or(last), i.pointer.primary_down()));
            route(app, ctx, view, origin, pointer, !down);
        }
    }
    let native = Screen::current(ctx);
    let window = app.view_windows.current_window();
    let covers = crate::shell::covered_rects(app);
    let targets = app.view_window_drag.drag.as_ref().filter(|drag| drag.tab == app.docs.current_tab()).map(|drag| {
        let mut targets = crate::view_tiles::incoming_dock_targets(app, &drag.view, &covers);
        if drag.source_is_last && drag.viewport != viewport {
            for target in &mut targets {
                target.blocked = true;
                target.label = app.tr("multi_window.last_view").to_owned();
            }
        }
        (drag.view.id, targets)
    });
    if let Some((_, targets)) = &targets {
        let t = Tokens::get();
        let p = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique(("parked_view_targets", viewport))));
        for target in targets.iter().filter(|target| !target.blocked) {
            if let Some(zone) = target.parked_zone() {
                let marker = Rect::from_center_size(zone.center(), vec2(24.0, 18.0)).intersect(zone);
                p.rect(marker, 4, t.accent_soft, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            }
        }
    }
    let canvas =
        native.map(|screen| DockCanvas { viewport, tab: app.docs.current_tab(), window, screen, bounds: ctx.viewport_rect(), covers, targets });
    let state = &mut app.view_window_drag;
    ctx.input(|input| {
        state.canvases.retain(|canvas| input.raw.viewports.get(&canvas.viewport).is_some_and(|info| info.minimized != Some(true)));
    });
    if let Some(canvas) = canvas {
        let first_targets = canvas.targets.as_ref().is_some_and(|(view, _)| {
            !state.canvases.iter().any(|old| old.viewport == viewport && old.targets.as_ref().is_some_and(|(old_view, _)| old_view == view))
        });
        if first_targets && let Some(drag) = &state.drag {
            ctx.request_repaint_of(drag.viewport);
        }
        state.canvases.retain(|old| old.viewport != viewport);
        state.canvases.push(canvas);
    } else {
        state.canvases.retain(|old| old.viewport != viewport);
    }
    if let Some(drag) = &state.drag
        && let Some(Target::Dock { dock, attraction, viewport: destination, .. }) = &drag.target
        && drag.viewport != viewport
        && *destination == viewport
        && let (Some(source), Some(screen)) = (drag.screen, native)
    {
        let convert = |point| screen.local(source.desktop(point));
        let origin = Rect::from_min_max(convert(drag.origin.min), convert(drag.origin.max));
        let pointer = convert(drag.pointer);
        paint_preview(ctx, &drag.view, origin, pointer, Preview::Dock { dock, attraction: *attraction, displaced: None });
    }
}

fn begin(app: &SolveApp, ctx: &Context, view: u64, origin: Rect, keyboard: bool) -> Option<Drag> {
    let design_view = app.docs.views.get(view)?.clone();
    Some(Drag {
        viewport: ctx.viewport_id(),
        tab: app.docs.current_tab(),
        window: app.view_windows.current_window(),
        view: design_view,
        origin,
        source: crate::view_windows::drag_source(app, view, ctx.viewport_rect()),
        keyboard: keyboard.then_some(0),
        header: false,
        target: None,
        pointer: origin.center(),
        screen: Screen::current(ctx),
        source_is_last: app.view_windows.current_window() == 0 && app.docs.views.len() == 1,
    })
}

fn take_target(app: &SolveApp, ctx: &Context, drag: &Drag, pointer: Pos2) -> Option<Target> {
    let client = ctx.viewport_rect();
    let desktop = Screen::current(ctx).or(drag.screen).map(|screen| screen.desktop(pointer));
    let previous = match &drag.target {
        Some(Target::Strip { slot, .. }) => Some(*slot),
        _ => None,
    };
    let outside = !client.contains(pointer);
    let strips = &app.view_window_drag.strips;
    if let Some((window, rect)) = strips
        .iter()
        .filter(|strip| strip.tab == drag.tab && strip.viewport == drag.viewport)
        .flat_map(|strip| strip.zone.elsewhere.iter().copied())
        .find(|(window, rect)| *window != drag.window && rect.contains(pointer))
        && let Some(choice) = crate::view_windows::window_choices(app).iter().find(|choice| choice.id == window)
    {
        let label = if drag.source_is_last { app.tr("multi_window.last_view").to_owned() } else { choice.label.clone() };
        return Some(Target::Elsewhere { window, rect, label, blocked: drag.source_is_last });
    }
    let strip_at = |local: bool| {
        strips.iter().rev().filter(|strip| strip.tab == drag.tab && (strip.viewport == drag.viewport) == local).find_map(|strip| {
            let point = if local { pointer } else { strip.screen.local(desktop?) };
            strip.drop_at(point, previous).map(|slot| strip.destination(app, drag, slot))
        })
    };
    if outside || drag.viewport == egui::ViewportId::ROOT {
        if let Some(target) = strip_at(false) {
            return Some(target);
        }
        if let Some(desktop) = desktop
            && let Some(canvas) = app
                .view_window_drag
                .canvases
                .iter()
                .rev()
                .find(|canvas| canvas.tab == drag.tab && canvas.viewport != drag.viewport && canvas.bounds.contains(canvas.screen.local(desktop)))
        {
            return canvas.target_at(drag, canvas.screen.local(desktop));
        }
    }
    if let Some(target) = strip_at(true) {
        return Some(target);
    }
    if client.contains(pointer) && !crate::shell::covered(app, pointer) {
        let previous = match &drag.target {
            Some(Target::Dock { dock, attraction, viewport, .. }) if *viewport == drag.viewport && *attraction == 1.0 => Some(dock.placement),
            _ => None,
        };
        let targets = crate::view_tiles::incoming_dock_targets(app, &drag.view, &crate::shell::covered_rects(app));
        if let Some((dock, attraction)) = crate::view_tiles::dock_at(&targets, pointer, previous) {
            return Some(Target::Dock { dock: dock.clone(), attraction, viewport: drag.viewport, window: drag.window });
        }
    }
    let edge = if matches!(drag.target, Some(Target::Window { .. })) { 6.0 } else { 12.0 };
    if !client.expand(edge).contains(pointer) && crate::view_windows::supports_pop_out(app) {
        let snap = !ctx.input(|i| i.modifiers.alt);
        return window_target(app, drag, desktop, None, snap);
    }
    None
}

fn window_target(app: &SolveApp, drag: &Drag, point: Option<Pos2>, display: Option<usize>, snap: bool) -> Option<Target> {
    let mut window = crate::view_windows::drag_window(app, drag.source, point, display, snap)?;
    window.label = solidcraft_ui_text::format("multi_window.drag_new", &[("display", &window.label)]);
    let refusal = if drag.window == 0 && app.docs.views.len() == 1 {
        Some(app.tr("multi_window.last_view"))
    } else if !crate::view_windows::can_create(app) {
        Some(app.tr("multi_window.too_many"))
    } else {
        None
    };
    Some(Target::Window { window, refusal })
}

pub(crate) fn route(app: &mut SolveApp, ctx: &Context, view: u64, origin: Rect, pointer: Pos2, released: bool) {
    if !pointer.is_finite() || !ctx.viewport_rect().is_positive() {
        return;
    }
    if app.view_window_drag.drag.is_none() {
        repaint_viewports(ctx);
    }
    let drag = app
        .view_window_drag
        .drag
        .take()
        .filter(|drag| drag.viewport == ctx.viewport_id() && drag.view.id == view && drag.tab == app.docs.current_tab())
        .or_else(|| begin(app, ctx, view, origin, false));
    let Some(mut drag) = drag else { return };
    app.docs.view_placement = None;
    app.view_window_drag.flight = None;
    let next = take_target(app, ctx, &drag, pointer);
    clear_old_slot(ctx, drag.target.as_ref(), next.as_ref());
    drag.target = next;
    drag.pointer = pointer;
    drag.screen = Screen::current(ctx).or(drag.screen);
    if released {
        finish(app, ctx, drag);
    } else {
        draw(app, ctx, &drag);
        app.view_window_drag.drag = Some(drag);
    }
}

pub(crate) fn lift(app: &mut SolveApp, ctx: &Context, view: u64, origin: Rect) {
    if let Some(drag) = begin(app, ctx, view, origin, true) {
        app.docs.view_placement = None;
        app.view_window_drag.drag = Some(drag);
        app.view_window_drag.flight = None;
    }
}

pub(crate) fn keyboard_active(app: &SolveApp, ctx: &Context) -> bool {
    app.view_window_drag.drag.as_ref().is_some_and(|drag| drag.viewport == ctx.viewport_id() && drag.keyboard.is_some())
}

pub(crate) fn keyboard(app: &mut SolveApp, ctx: &Context) {
    if !keyboard_active(app, ctx) {
        return;
    }
    let Some(mut drag) = app.view_window_drag.drag.take() else { return };
    if drag.tab != app.docs.current_tab() || app.docs.views.get(drag.view.id).is_none() {
        return;
    }
    let pressed = |key| ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
    if pressed(egui::Key::Escape) {
        cancel(&mut app.view_window_drag, ctx, drag);
        return;
    }
    let strips = &app.view_window_drag.strips;
    let from = app.docs.views.views().iter().position(|view| view.id == drag.view.id);
    let local = strips
        .iter()
        .filter(|strip| strip.tab == drag.tab && strip.viewport == drag.viewport)
        .flat_map(|strip| strip.keyboard_slots(from.map(|index| index + 1)).into_iter().map(|slot| strip.destination(app, &drag, slot)))
        .collect::<Vec<_>>();
    let dock = crate::view_tiles::incoming_dock_targets(app, &drag.view, &crate::shell::covered_rects(app))
        .into_iter()
        .filter(|dock| !dock.blocked)
        .map(|dock| Target::Dock { dock, attraction: 1.0, viewport: drag.viewport, window: drag.window });
    let other = strips
        .iter()
        .filter(|strip| strip.tab == drag.tab && strip.viewport != drag.viewport)
        .flat_map(|strip| strip.keyboard_slots(None).into_iter().map(|slot| strip.destination(app, &drag, slot)));
    let windows = (0..crate::view_windows::drag_display_count(app))
        .filter(|_| crate::view_windows::supports_pop_out(app))
        .filter_map(|index| window_target(app, &drag, None, Some(index), true));
    let local_count = local.len();
    let mut targets = local.into_iter().chain(dock).chain(other).chain(windows).collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    let mut index = drag.keyboard.unwrap_or_default().min(targets.len() - 1);
    if drag.target.is_none() {
        index = targets
            .iter()
            .take(local_count)
            .position(|target| match target {
                Target::Strip { slot, .. } => slot.hit.contains(drag.origin.center()),
                _ => false,
            })
            .unwrap_or_default();
    }
    if pressed(egui::Key::ArrowLeft) && local_count > 0 {
        index = index.min(local_count - 1).saturating_sub(1);
    }
    if pressed(egui::Key::ArrowRight) && local_count > 0 {
        index = (index + 1).min(local_count - 1);
    }
    if pressed(egui::Key::ArrowUp) {
        index = (index + targets.len() - 1) % targets.len();
    }
    if pressed(egui::Key::ArrowDown) {
        index = (index + 1) % targets.len();
    }
    let target = targets.swap_remove(index);
    if drag.keyboard != Some(index) || drag.target.is_none() {
        ctx.output_mut(|output| {
            output.events.push(egui::output::OutputEvent::ValueChanged(egui::WidgetInfo::labeled(egui::Role::Label, true, target.label())))
        });
    }
    drag.keyboard = Some(index);
    drag.pointer = match &target {
        Target::Strip { slot, .. } if slot.viewport == drag.viewport => slot.center,
        Target::Dock { dock, .. } => dock.zone.center(),
        _ => drag.origin.center(),
    };
    clear_old_slot(ctx, drag.target.as_ref(), Some(&target));
    drag.target = Some(target);
    if pressed(egui::Key::Enter) {
        finish(app, ctx, drag);
    } else {
        draw(app, ctx, &drag);
        app.view_window_drag.drag = Some(drag);
    }
}

fn clear_old_slot(ctx: &Context, old: Option<&Target>, next: Option<&Target>) {
    if let Some(Target::Strip { slot: old_slot, blocked: false, .. }) = old
        && !matches!(next, Some(Target::Strip {slot, blocked: false, ..}) if slot.viewport == old_slot.viewport)
    {
        update_slot(ctx, old, 0.0);
    }
}

fn update_slot(ctx: &Context, target: Option<&Target>, width: f32) {
    if let Some(Target::Strip { slot, blocked: false, .. }) = target {
        crate::app_bar::set_view_drag_slot(ctx, crate::documents::tab_strip_id(slot.viewport), (width > 0.0).then_some(slot.index), width);
        if slot.viewport != ctx.viewport_id() {
            ctx.request_repaint_of(slot.viewport);
        }
    }
}

fn finish(app: &mut SolveApp, ctx: &Context, mut drag: Drag) {
    repaint_viewports(ctx);
    update_slot(ctx, drag.target.as_ref(), 0.0);
    let result = match drag.target.take() {
        Some(Target::Strip { slot, blocked: false, .. }) => {
            if slot.window == drag.window {
                app.docs.views.views().iter().position(|view| view.id == drag.view.id).map_or(Ok(()), |from| {
                    let to = (slot.index - usize::from(from < slot.index)).min(app.docs.views.len() - 1);
                    if from == to {
                        return Ok(());
                    }
                    app.run("tab.view.move", serde_json::json!({"from":from,"to":to})).map(|_| ())
                })
            } else {
                ctx.request_repaint_of(slot.viewport);
                app.run("tab.view.move_to_window", serde_json::json!({"view":drag.view.id,"window":slot.window,"index":slot.index})).map(|_| ())
            }
        }
        Some(Target::Elsewhere { window, blocked: false, .. }) => {
            app.run("tab.view.move_to_window", serde_json::json!({"view":drag.view.id,"window":window})).map(|_| ())
        }
        Some(Target::Dock { dock, attraction, window, .. }) if !dock.blocked && attraction == 1.0 => {
            if window == drag.window {
                crate::view_tiles::commit_lifted_dock(app, drag.view.id, dock.placement)
            } else {
                let placement = solidcraft_engine::schema::tab_views::TabViewPlacement::from(dock.placement);
                app.run("tab.view.move_to_window", serde_json::json!({"view":drag.view.id,"window":window,"placement":placement})).map(|_| ())
            }
        }
        Some(Target::Window { window, refusal: None }) => crate::view_windows::drop_new_window(app, drag.view.id, window),
        _ => {
            cancel(&mut app.view_window_drag, ctx, drag);
            return;
        }
    };
    if let Err(reason) = result {
        crate::design_view_ui::show_move_error(app, reason);
        cancel(&mut app.view_window_drag, ctx, drag);
    }
}

fn cancel(state: &mut State, ctx: &Context, drag: Drag) {
    repaint_viewports(ctx);
    update_slot(ctx, drag.target.as_ref(), 0.0);
    if let Some(target) = &drag.target {
        ctx.request_repaint_of(target.viewport(drag.viewport));
    }
    // The id outside `data`: a context lock taken inside another can freeze the window (#21091).
    let morph = morph_id(ctx, drag.view.id);
    if ctx.global_style().animation_time > 0.0
        && let Some(morph) = ctx.data(|data| data.get_temp::<Morph>(morph))
    {
        state.flight = Some(Flight { view: drag.view, viewport: drag.viewport, from: morph.rect, origin: drag.origin, start: ctx.input(|i| i.time) });
        ctx.request_repaint();
    }
}

fn native_preview_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("view_drag_native_preview")
}

fn native_preview_window<'a>(ctx: &Context, drag: &'a Drag) -> Option<(&'a DragWindow, Option<&'static str>)> {
    if !cfg!(target_os = "windows") || ctx.embed_viewports() || drag.keyboard.is_some() || drag.screen.is_none() {
        return None;
    }
    match &drag.target {
        Some(Target::Window { window, refusal }) => Some((window, *refusal)),
        _ => None,
    }
}

fn native_preview_scale(ctx: &Context) -> Option<f32> {
    ctx.input(|input| {
        input.raw.viewports.get(&native_preview_id()).and_then(|info| info.native_pixels_per_point).filter(|scale| scale.is_finite() && *scale > 0.0)
    })
}

fn native_preview_builder(position: Pos2, size: egui::Vec2, visible: bool, owner: egui::ViewportId) -> egui::ViewportBuilder {
    crate::window_chrome::popout_viewport(egui::ViewportBuilder::default(), owner)
        .with_title("SolidCraft")
        .with_decorations(false)
        .with_resizable(false)
        .with_active(false)
        .with_taskbar(false)
        .with_position(position)
        .with_inner_size(size)
        .with_visible(visible)
}

/// The source retains pointer capture. The preview is a top-level surface with no controls
/// or focus, sized and positioned from the same desktop rectangle as the drop command.
pub(crate) fn show_native_preview(app: &SolveApp, ctx: &Context) {
    let Some(drag) = app.view_window_drag.drag.as_ref() else { return };
    let Some((window, refusal)) = native_preview_window(ctx, drag) else { return };
    let id = native_preview_id();
    let native_scale = native_preview_scale(ctx);
    let pixels_per_point = native_scale.unwrap_or(window.scale) * ctx.zoom_factor();

    let position = (window.outline.min.to_vec2() / pixels_per_point).to_pos2();
    let size = window.outline.size() / pixels_per_point;
    let ready = native_scale.is_some();
    let builder = native_preview_builder(position, size, ready, drag.viewport);
    if !ready {
        // Negotiate the native surface in a hidden deferred viewport first. Keep
        // the source-window preview if the integration cannot make this window.
        ctx.show_viewport_deferred(id, builder, |_, _| {});
        ctx.request_repaint_after(std::time::Duration::from_millis(32));
        return;
    }
    let blocked = refusal.is_some();
    let label = refusal.unwrap_or(&window.label);
    let pointer = drag.screen.map(|screen| screen.desktop(drag.pointer));
    ctx.show_viewport_immediate(id, builder, |ui, class| {
        if class == egui::ViewportClass::EmbeddedWindow {
            return;
        }
        let ctx = ui.ctx();
        let client = ctx.viewport_rect();
        if !client.is_positive() {
            return;
        }
        let p = painter(ctx).with_clip_rect(client);
        let t = Tokens::get();
        p.rect_filled(client, 0, t.app_bar);
        let ink = if blocked { t.error } else { t.accent };
        draw_shape(ctx, &p, &drag.view, client.shrink(2.0), GhostStyle { form: Form::Window, tilt: 0.0, detail: 1.0, ink, shadow: false });
        let point = pointer.and_then(|point| Screen::current(ctx).map(|screen| screen.local(point))).unwrap_or(client.left_top() + vec2(24.0, 14.0));
        paint_hint(&p, client, &drag.view, point, Some(label), blocked);
    });
}

fn draw(app: &SolveApp, ctx: &Context, drag: &Drag) {
    update_slot(ctx, drag.target.as_ref(), drag.origin.width());
    if let Some(target) = drag.target.as_ref().filter(|target| target.viewport(drag.viewport) != drag.viewport) {
        ctx.request_repaint_of(target.viewport(drag.viewport));
        return;
    }
    match &drag.target {
        Some(Target::Strip { slot, label, blocked: false }) => paint_tab(ctx, &drag.view, drag.origin, *slot, drag.pointer, label),
        Some(Target::Dock { dock, attraction, .. }) => {
            let displaced = dock.displaced.and_then(|(id, rect)| app.docs.views.get(id).map(|view| (view, rect)));
            paint_preview(ctx, &drag.view, drag.origin, drag.pointer, Preview::Dock { dock, attraction: *attraction, displaced });
        }
        Some(Target::Window { window, refusal }) => {
            if native_preview_window(ctx, drag).is_some() && native_preview_scale(ctx).is_some() {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            let rect = Screen::current(ctx).map(|screen| Rect::from_min_max(screen.local(window.outline.min), screen.local(window.outline.max)));
            let label = refusal.unwrap_or(&window.label);
            paint_preview(ctx, &drag.view, drag.origin, drag.pointer, Preview::Window { rect, label, blocked: refusal.is_some() });
        }
        Some(Target::Elsewhere { rect, label, blocked, .. }) => {
            let p = painter(ctx);
            let t = Tokens::get();
            p.rect_stroke(*rect, 6, Stroke::new(2.0, if *blocked { t.error } else { t.accent }), StrokeKind::Inside);
            paint_hint_label(&p, ctx.viewport_rect(), rect.center_bottom() + vec2(0.0, 6.0), label, *blocked);
        }
        Some(Target::Strip { label, .. }) => paint_preview(ctx, &drag.view, drag.origin, drag.pointer, Preview::Held(Some(label))),
        None => paint_preview(ctx, &drag.view, drag.origin, drag.pointer, Preview::Held(None)),
    }
}

fn painter(ctx: &Context) -> egui::Painter {
    ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("view_window_drag")))
}

enum Preview<'a> {
    Held(Option<&'a str>),
    Dock { dock: &'a DockTarget, attraction: f32, displaced: Option<(&'a DesignView, Rect)> },
    Window { rect: Option<Rect>, label: &'a str, blocked: bool },
}

#[derive(Clone, Copy, PartialEq)]
enum Form {
    Tab,
    Card,
    Window,
}

fn paint_preview(ctx: &Context, view: &DesignView, origin: Rect, pointer: Pos2, preview: Preview<'_>) {
    let client = ctx.viewport_rect();
    let t = Tokens::get();
    let p = painter(ctx).with_clip_rect(client);
    let held = held_rect(client, pointer, origin.size() * 1.04, vec2(24.0, 14.0));
    let focused = matches!(&preview, Preview::Dock { dock, .. } if dock.focused && !dock.blocked);
    let (goal, form, label, blocked) = match preview {
        Preview::Held(label) => (held, Form::Tab, label, label.is_some()),
        Preview::Dock { dock, attraction, displaced } => {
            if dock.blocked {
                let hatch = p.with_clip_rect(dock.zone);
                let step = 13.0;
                let count = ((dock.zone.width() + dock.zone.height()) / step).ceil().min(512.0) as usize;
                for index in 0..count {
                    let start = dock.zone.left() + index as f32 * step;
                    hatch.line_segment(
                        [pos2(start, dock.zone.top()), pos2(start - dock.zone.height(), dock.zone.bottom())],
                        Stroke::new(1.0, t.warning),
                    );
                }
                (held, Form::Tab, Some(dock.label.as_str()), true)
            } else {
                for neighbor in &dock.neighbors {
                    p.rect_stroke(*neighbor, 8, Stroke::new(1.0, t.accent.gamma_multiply(0.45)), StrokeKind::Inside);
                }
                if let Some((other, rect)) = displaced {
                    draw_shape(ctx, &p, other, rect, GhostStyle { form: Form::Card, tilt: 0.0, detail: 1.0, ink: t.border, shadow: false });
                }
                let outline = dock.rect.expand(3.0);
                if attraction >= 1.0 {
                    p.rect_filled(outline, 11, t.accent_soft);
                    dashed(&p, outline, t.accent, 2.0);
                }
                (lerp_rect(held, dock.rect, attraction), Form::Card, Some(dock.label.as_str()), false)
            }
        }
        Preview::Window { rect, label, blocked } => {
            if let Some(rect) = rect.filter(|rect| rect.intersect(client).is_positive()) {
                dashed(&p, rect.expand(3.0), if blocked { t.error } else { t.accent }, 2.0);
            }
            let size = rect.map_or(vec2(320.0, 210.0), |rect| rect.size() * 0.5);
            (held_rect(client, pointer, size, vec2(24.0, 14.0)), Form::Window, Some(label), blocked)
        }
    };
    let params = morph(ctx, view.id, origin, goal, form, pointer);
    let ink = if blocked { t.error } else { t.accent };
    draw_shape(ctx, &p, view, params.rect, GhostStyle { form, tilt: params.tilt, detail: params.detail, ink, shadow: true });
    if focused && params.rect.width() > 50.0 && params.detail > 0.65 {
        p.circle_filled(params.rect.right_top() + vec2(-14.0, 14.0), 3.5, t.accent);
    }
    paint_hint(&p, client, view, pointer, label, blocked);
    ctx.set_cursor_icon(if blocked { egui::CursorIcon::NotAllowed } else { egui::CursorIcon::Grabbing });
}

fn paint_tab(ctx: &Context, view: &DesignView, origin: Rect, slot: Slot, pointer: Pos2, label: &str) {
    let client = ctx.viewport_rect();
    let p = painter(ctx).with_clip_rect(client);
    let t = Tokens::get();
    if let Some(tab) = crate::app_bar::view_drag_gap(ctx, crate::documents::tab_strip_id(slot.viewport), slot.index) {
        morph(ctx, view.id, origin, tab, Form::Tab, pointer);
        let tab_painter = p.with_clip_rect(tab);
        draw_shape(ctx, &tab_painter, view, tab, GhostStyle { form: Form::Tab, tilt: 0.0, detail: 0.0, ink: t.accent, shadow: false });
    }
    paint_hint_label(&p, client, pos2(pointer.x, slot.hit.top() + 48.0), label, false);
}

fn paint_flight(ctx: &Context, flight: &Flight) -> bool {
    let elapsed = ctx.input(|input| input.time) - flight.start;
    let linear = (elapsed / 0.2).clamp(0.0, 1.0) as f32;
    if linear >= 1.0 {
        return false;
    }
    let phase = 1.0 - (1.0 - linear).powi(3);
    let rect = lerp_rect(flight.from, flight.origin, phase).translate(vec2(0.0, -42.0 * (phase * std::f32::consts::PI).sin()));
    let painter = painter(ctx).with_clip_rect(ctx.viewport_rect());
    let style = GhostStyle { form: Form::Card, tilt: 0.0, detail: 1.0 - phase, ink: Tokens::get().border, shadow: true };
    draw_shape(ctx, &painter, &flight.view, rect, style);
    ctx.request_repaint();
    true
}

fn dashed(painter: &egui::Painter, rect: Rect, ink: egui::Color32, width: f32) {
    painter.extend(egui::Shape::dashed_line(
        &[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()],
        Stroke::new(width, ink),
        6.0,
        4.0,
    ));
}

struct GhostStyle {
    form: Form,
    tilt: f32,
    detail: f32,
    ink: egui::Color32,
    shadow: bool,
}

fn draw_shape(ctx: &Context, painter: &egui::Painter, view: &DesignView, rect: Rect, style: GhostStyle) {
    let GhostStyle { form, tilt, detail, ink, shadow } = style;
    if !rect.is_positive() {
        return;
    }
    let t = Tokens::get();
    let layer = painter.layer_id();
    let first = ctx.graphics(|graphics| graphics.get(layer).map_or(0, |list| list.all_entries().count()));
    let radius = (5.0 + detail * if form == Form::Window { 5.0 } else { 3.0 }).round() as u8;
    if shadow {
        let shadow = crate::view_tiles::card_shadow(rect, egui::CornerRadius::same(radius));
        painter.add(shadow);
    }
    let fill = if detail > 0.03 { t.viewport_top } else { t.app_bar_hover_strong };
    painter.rect_filled(rect, radius, fill);
    let header_height = (24.0 + 4.0 * detail).min(rect.height());
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), header_height));
    if form == Form::Card && detail > 0.65 {
        crate::view_tiles::paint_card_header(painter, header, radius, &view.name, header.left() + 30.0);
        paint_kind(painter, header.left_center() + vec2(16.0, 0.0), view, t.accent, 15.0);
    } else {
        let bar = if form == Form::Window { t.app_bar } else { t.app_bar_hover_strong };
        painter.rect_filled(header, egui::CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 }, bar);
        paint_header(painter, header, &view.name, view.kind.key(), t.app_bar_text);
    }
    let body = Rect::from_min_max(header.left_bottom(), rect.max).shrink(3.0);
    if body.is_positive() && detail > 0.03 {
        solidcraft_viewport_egui::paint_background(painter, body, t.viewport_top, t.viewport_bottom);
        paint_kind(painter, body.center(), view, t.text_dim.gamma_multiply(0.4 * detail), 32.0_f32.min(body.height()));
        if form == Form::Window && body.height() > 65.0 {
            let name = painter.layout_no_wrap(view.name.clone(), crate::ui_kit::type_face::semibold(t.font_button), t.text);
            let pill = Rect::from_min_size(body.left_bottom() + vec2(5.0, -31.0), vec2((name.size().x + 30.0).min(body.width() - 8.0), 26.0));
            if pill.is_positive() {
                painter.rect_filled(pill, 13, t.dialog_body);
                painter.circle_filled(pill.left_center() + vec2(11.0, 0.0), 3.5, t.accent);
                painter.with_clip_rect(pill.shrink(4.0)).galley(pill.left_center() + vec2(20.0, -name.size().y * 0.5), name, t.text);
            }
        }
    }
    painter.rect_stroke(rect, radius, Stroke::new(if form == Form::Tab { 1.0 } else { 2.0 }, ink), StrokeKind::Inside);
    if tilt != 0.0 {
        let grab = rect.min + vec2(24.0, 14.0);
        let rot = egui::emath::Rot2::from_angle(tilt);
        ctx.graphics_mut(|graphics| {
            let list = graphics.entry(layer);
            let shapes: Vec<_> = list.all_entries().skip(first).cloned().collect();
            for (index, mut entry) in shapes.into_iter().enumerate() {
                rotate_shape(&mut entry.shape, rot, grab, tilt);
                list.set(egui::layers::ShapeIdx(first + index), entry.clip_rect, entry.shape);
            }
        });
    }
}

fn rotate_shape(shape: &mut egui::Shape, rot: egui::emath::Rot2, pivot: Pos2, angle: f32) {
    let rotate = |point: Pos2| pivot + rot * (point - pivot);
    match shape {
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                rotate_shape(shape, rot, pivot, angle);
            }
        }
        egui::Shape::Rect(rect) => *rect = (*rect).clone().with_angle_and_pivot(angle, pivot),
        egui::Shape::Mesh(mesh) => std::sync::Arc::make_mut(mesh).rotate(rot, pivot),
        egui::Shape::Text(text) => {
            text.pos = rotate(text.pos);
            text.angle += angle;
        }
        egui::Shape::Circle(circle) => circle.center = rotate(circle.center),
        egui::Shape::Ellipse(ellipse) => ellipse.center = rotate(ellipse.center),
        egui::Shape::LineSegment { points, .. } => *points = points.map(rotate),
        egui::Shape::Path(path) => {
            for point in &mut path.points {
                *point = rotate(*point);
            }
        }
        _ => {}
    }
}

fn paint_kind(painter: &egui::Painter, center: Pos2, view: &DesignView, ink: egui::Color32, size: f32) {
    crate::app_bar::paint_view_glyph(painter, Rect::from_center_size(center, vec2(size, size)), view.kind.key(), ink);
}

fn paint_header(painter: &egui::Painter, rect: Rect, name: &str, icon: &str, ink: egui::Color32) {
    let t = Tokens::get();
    let painter = painter.with_clip_rect(rect.shrink(4.0));
    let start = rect.left_center() + vec2(13.0, 0.0);
    crate::app_bar::paint_view_glyph(&painter, Rect::from_center_size(start, vec2(15.0, 15.0)), icon, ink);
    painter.text(start + vec2(13.0, 0.0), Align2::LEFT_CENTER, name, crate::ui_kit::type_face::semibold(t.font_button), ink);
}

fn paint_hint(painter: &egui::Painter, client: Rect, view: &DesignView, pointer: Pos2, label: Option<&str>, blocked: bool) {
    let t = Tokens::get();
    let token = held_rect(client, pointer, vec2(20.0, 20.0), vec2(10.0, 10.0));
    painter.circle_filled(token.center(), 10.0, t.app_bar_hover_strong);
    painter.circle_stroke(token.center(), 9.5, Stroke::new(1.0, t.app_bar_text.gamma_multiply(0.5)));
    paint_kind(painter, token.center(), view, t.app_bar_text, 12.0);
    if let Some(label) = label {
        let y = if pointer.y >= client.bottom() - 40.0 { -39.0 } else { 16.0 };
        paint_hint_label(painter, client, token.center() + vec2(0.0, y), label, blocked);
    }
}

fn paint_hint_label(painter: &egui::Painter, client: Rect, at: Pos2, label: &str, blocked: bool) {
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(label.to_owned(), crate::ui_kit::type_face::semibold(t.font_button), t.app_bar_text);
    let size = vec2(galley.size().x + 24.0, 24.0);
    let hint = held_rect(client, at, size, vec2(size.x * 0.5, 0.0));
    painter.rect_filled(hint, 12, if blocked { t.error } else { t.app_bar });
    painter.with_clip_rect(hint.shrink(3.0)).galley(hint.center() - galley.size() * 0.5, galley, t.app_bar_text);
}

fn held_rect(client: Rect, pointer: Pos2, size: egui::Vec2, grab: egui::Vec2) -> Rect {
    let size = size.min(client.size());
    let max = client.max - size;
    let position = pointer - grab;
    Rect::from_min_size(pos2(position.x.clamp(client.left(), max.x), position.y.clamp(client.top(), max.y)), size)
}

fn lerp_rect(a: Rect, b: Rect, amount: f32) -> Rect {
    Rect::from_min_max(a.min.lerp(b.min, amount), a.max.lerp(b.max, amount))
}

#[derive(Clone, Copy)]
struct Morph {
    frame: u64,
    rect: Rect,
    from: Rect,
    goal: Rect,
    form: Form,
    began: f64,
    detail: f32,
    from_detail: f32,
    pointer: Pos2,
    moved: f64,
    tilt: f32,
}

fn morph_id(ctx: &Context, view: u64) -> Id {
    Id::unique(("view_drag_morph", ctx.viewport_id(), view))
}

fn morph(ctx: &Context, view: u64, origin: Rect, goal: Rect, form: Form, pointer: Pos2) -> Morph {
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|input| input.time);
    let id = morph_id(ctx, view);
    let old = ctx.data(|data| data.get_temp::<Morph>(id)).filter(|old| frame.saturating_sub(old.frame) <= 1);
    let mut state = old.unwrap_or(Morph {
        frame,
        rect: origin,
        from: origin,
        goal,
        form,
        began: now,
        detail: 0.0,
        from_detail: 0.0,
        pointer,
        moved: now - 0.32,
        tilt: 0.0,
    });
    if form != state.form || (form == Form::Card && state.goal.center().distance(goal.center()) > 24.0) {
        state.from = state.rect;
        state.from_detail = state.detail;
        state.began = now;
    }
    if pointer.distance(state.pointer) > 0.1 {
        state.moved = now;
    }
    state.pointer = pointer;
    state.form = form;
    state.goal = goal;
    state.frame = frame;
    let move_enabled = ctx.global_style().animation_time > 0.0;
    let duration = match form {
        Form::Tab => {
            if old.is_none() {
                0.1
            } else {
                0.18
            }
        }
        Form::Card => 0.18,
        Form::Window => 0.2,
    };
    let progress = if move_enabled { ((now - state.began) / duration).clamp(0.0, 1.0) as f32 } else { 1.0 };
    let spring = if progress >= 1.0 { 1.0 } else { (1.0 - (1.0 + 7.0 * progress) * (-7.0 * progress).exp()) / (1.0 - 8.0 * (-7.0_f32).exp()) };
    state.rect = lerp_rect(state.from, goal, spring);
    state.detail = egui::lerp(state.from_detail..=if form == Form::Tab { 0.0 } else { 1.0 }, spring);
    state.tilt = if move_enabled && form == Form::Window {
        let rest = ((now - state.moved - 0.16) / 0.16).clamp(0.0, 1.0) as f32;
        -2.0_f32.to_radians() * (1.0 - rest) * spring
    } else {
        0.0
    };
    if progress < 1.0 || state.tilt != 0.0 {
        ctx.request_repaint();
    }
    ctx.data_mut(|data| data.insert_temp(id, state));
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_view_drag_preview_uses_its_owner_for_stacking() {
        let owner = egui::ViewportId::from_hash_of("drag owner");
        let builder = native_preview_builder(Pos2::ZERO, vec2(320.0, 240.0), true, owner);
        assert_eq!(builder.native_owner, Some(owner));
        assert_eq!(builder.window_level, None);
        assert_eq!(builder.active, Some(false));
    }

    #[test]
    fn starting_a_view_drag_draws_immediately_without_requesting_a_screenshot() {
        for keyboard in [false, true] {
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            let view = app.docs.views.active().id;
            app.viewport.rect = Some(Rect::from_min_size(pos2(0.0, 44.0), vec2(800.0, 500.0)));
            let ctx = Context::default();
            ctx.set_embed_viewports(false);
            let origin = Rect::from_min_size(pos2(100.0, 4.0), vec2(80.0, 24.0));
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
            let output = ctx.run_ui(raw, |ui| {
                if keyboard {
                    lift(&mut app, ui.ctx(), view, origin);
                    assert!(keyboard_active(&app, ui.ctx()));
                } else {
                    route(&mut app, ui.ctx(), view, origin, pos2(230.0, 80.0), false);
                    let morph = morph_id(ui.ctx(), view);
                    assert!(ui.ctx().data(|data| data.get_temp::<Morph>(morph)).is_some(), "the preview paints in the first drag frame");
                }
                assert!(app.view_window_drag.drag.is_some());
            });
            assert!(
                output
                    .viewport_output
                    .values()
                    .flat_map(|viewport| &viewport.commands)
                    .all(|command| !matches!(command, egui::ViewportCommand::Screenshot(_))),
                "mouse and keyboard dragging never read back a view image"
            );
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn a_native_header_drag_lifts_immediately_without_requesting_a_screenshot() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let view = app.docs.views.active().id;
        app.viewport.rect = Some(Rect::from_min_size(pos2(0.0, 44.0), vec2(800.0, 500.0)));
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        let title = Rect::from_min_size(pos2(8.0, 44.0), vec2(80.0, 24.0));
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            start_header(&mut app, ui.ctx(), view, title);
            assert!(unanchored(&app, ui.ctx(), view), "the view lifts in the same frame");
        });
        assert!(
            output
                .viewport_output
                .values()
                .flat_map(|viewport| &viewport.commands)
                .all(|command| !matches!(command, egui::ViewportCommand::Screenshot(_))),
            "dragging must not read back the viewport image"
        );
        output.drop_without_applying_deltas();
    }

    #[test]
    fn a_minimised_window_leaves_no_view_highlight() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let view = app.docs.views.active().id;
        let ctx = Context::default();
        ctx.set_embed_viewports(true);
        let child = egui::ViewportId::from_hash_of("minimised window");
        let frame = |app: &mut SolveApp, minimized: bool| {
            let mut raw = egui::RawInput::default();
            raw.viewports.insert(child, egui::ViewportInfo { minimized: Some(minimized), ..Default::default() });
            let output = ctx.run_ui(raw, |ui| {
                if !minimized {
                    let tab = app.docs.current_tab();
                    app.view_window_drag.hovered = Some((child, tab, view));
                }
                record(app, ui.ctx(), vec![], false);
            });
            output.drop_without_applying_deltas();
        };
        frame(&mut app, false);
        assert_eq!(hovered_view(&app), Some(view), "a hovered view is highlighted");
        frame(&mut app, true);
        assert_eq!(hovered_view(&app), None, "minimising its window clears the highlight");
        frame(&mut app, true);
        assert_eq!(hovered_view(&app), None, "it stays cleared until hovered again");
    }

    #[test]
    fn a_header_drag_survives_the_tab_strip_frame() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let view = app.docs.views.active().id;
        let ctx = Context::default();
        ctx.set_embed_viewports(true);
        let title = Rect::from_min_size(pos2(8.0, 44.0), vec2(80.0, 24.0));
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            start_header(&mut app, ui.ctx(), view, title);
            record(&mut app, ui.ctx(), vec![], false);
            assert!(unanchored(&app, ui.ctx(), view));
            assert_eq!(app.docs.views.active().id, view);
        });
    }

    #[test]
    fn a_lifted_view_routes_to_the_expanded_neighbors_current_edge() {
        use solidcraft_views::{Edge, Placement, Preset, ViewKind};
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.home.open = false;
        app.ui.show_browser = false;
        app.ui.show_timeline = false;
        let neighbor = app.docs.views.active().id;
        let view = app.docs.views.open(ViewKind::Model, None, false);
        app.docs.views.set_layout_padded(Preset::SideBySide);
        let ctx = Context::default();
        ctx.set_embed_viewports(true);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 740.0));
        let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let _ = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx();
            app.shell.content = Some(Rect::from_min_max(pos2(0.0, 40.0), screen.max));
            let origin = Rect::from_min_size(pos2(160.0, 4.0), vec2(90.0, 24.0));
            let mut drag = begin(&app, ctx, view, origin, false).unwrap();
            let placement = Placement::Split { target: neighbor, edge: Edge::Right };
            let current = crate::view_tiles::incoming_dock_targets(&app, &drag.view, &[]);
            let preview = current.iter().find(|target| target.placement == placement && !target.blocked).unwrap();
            let pointer = preview.zone.center();
            let stale = crate::view_tiles::dock_targets(&app, view);
            assert_ne!(crate::view_tiles::dock_at(&stale, pointer, None).map(|(target, _)| target.placement), Some(placement));
            let target = take_target(&app, ctx, &drag, pointer).unwrap();
            let Target::Dock { dock, attraction, .. } = &target else { panic!("expanded neighbor must receive the drop") };
            assert_eq!(dock.placement, placement);
            assert_eq!(*attraction, 1.0);
            assert_eq!(dock.rect, preview.rect);
            let expected = preview.rect;
            let bounds = preview.neighbors.iter().fold(preview.rect, |bounds, rect| bounds.union(*rect));
            drag.target = Some(target);
            drag.pointer = pointer;
            finish(&mut app, ctx, drag);
            let bounds = solidcraft_views::Rect { x: bounds.left(), y: bounds.top(), w: bounds.width(), h: bounds.height() };
            let committed = app.docs.views.tiles().unwrap().rects(bounds, 12.0);
            let actual = committed.iter().find(|(id, _)| *id == view).unwrap().1;
            assert_eq!(Rect::from_min_size(pos2(actual.x, actual.y), vec2(actual.w, actual.h)), expected);
        });
    }

    #[test]
    fn a_preview_stays_in_the_client_and_snaps_to_the_target_without_animation() {
        let client = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 550.0));
        let held = held_rect(client, pos2(875.0, -40.0), vec2(230.0, 140.0), vec2(24.0, 14.0));
        assert_eq!(held, Rect::from_min_max(pos2(570.0, 0.0), pos2(800.0, 140.0)));
        let tile = Rect::from_min_max(pos2(400.0, 44.0), pos2(795.0, 540.0));
        let ctx = Context::default();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(morph(ui.ctx(), 2, held, tile, Form::Card, tile.center()).rect, tile);
            assert_eq!(morph(ui.ctx(), 2, held, held, Form::Tab, held.center()).rect, held);
        });
    }

    #[test]
    fn keyboard_reorder_uses_a_distinct_slot_and_escape_preserves_the_order() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let first = app.docs.views.active().id;
        let second = app.docs.views.open(solidcraft_views::ViewKind::Model, None, false);
        let ctx = Context::default();
        let origin = Rect::from_min_max(pos2(105.0, 0.0), pos2(175.0, 26.0));
        let mut frame = |key: Option<egui::Key>, lift_view: bool| {
            let events = key
                .into_iter()
                .map(|key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE })
                .collect();
            let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                record(
                    &mut app,
                    ui.ctx(),
                    vec![(
                        1,
                        2,
                        ViewDropZone {
                            document: 0,
                            elsewhere: Vec::new(),
                            rect: Rect::from_min_max(pos2(85.0, 0.0), pos2(280.0, 32.0)),
                            view_centres: vec![(0, 140.0), (1, 210.0)],
                        },
                    )],
                    false,
                );
                // Use the running design's id rather than an application-global tab number.
                if let Some(strip) = app.view_window_drag.strips.last_mut() {
                    strip.tab = app.docs.current_tab();
                }
                if lift_view {
                    lift(&mut app, ui.ctx(), first, origin);
                }
                keyboard(&mut app, ui.ctx());
            });
        };
        frame(None, true);
        frame(Some(egui::Key::ArrowRight), false);
        frame(Some(egui::Key::Escape), false);
        frame(None, true);
        frame(Some(egui::Key::ArrowRight), false);
        frame(Some(egui::Key::Enter), false);
        let order: Vec<_> = app.docs.views.views().iter().map(|view| view.id).collect();
        assert_eq!(order, [second, first]);
        assert!(!keyboard_active(&app, &ctx));
    }

    #[test]
    fn an_external_pointer_targets_the_secondary_strip_before_a_new_window() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let view = app.docs.views.open(solidcraft_views::ViewKind::Model, None, false);
        let viewport = egui::ViewportId::from_hash_of("secondary");
        let screen = Screen { origin: pos2(1100.0, 80.0), pixels_per_point: 1.5 };
        app.view_window_drag.strips.push(Strip {
            viewport,
            tab: app.docs.current_tab(),
            window: 1,
            screen,
            zone: ViewDropZone {
                document: 0,
                elsewhere: Vec::new(),
                rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(300.0, 30.0)),
                view_centres: vec![(0, 145.0)],
            },
            count: 1,
            name: "Secondary".into(),
        });
        let ctx = Context::default();
        let source = Screen { origin: pos2(40.0, 60.0), pixels_per_point: 1.0 };
        let origin = Rect::from_min_size(pos2(200.0, 4.0), vec2(70.0, 24.0));
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let _ = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx();
            let mut drag = begin(&app, ctx, view, origin, false).unwrap();
            drag.screen = Some(source);
            let pointer = source.local(screen.desktop(pos2(200.0, 20.0)));
            assert!(!ctx.viewport_rect().contains(pointer));
            let target = take_target(&app, ctx, &drag, pointer).unwrap();
            assert_eq!(target.viewport(drag.viewport), viewport);
            assert!(matches!(target, Target::Strip { slot: Slot { window: 1, index: 1, .. }, blocked: false, .. }));
        });
    }

    #[test]
    fn a_detached_tab_routes_the_drop_to_its_window() {
        use solidcraft_views::{ViewKind, WindowTarget};
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let dragged = app.docs.views.open(ViewKind::Model, None, false);
        let detached = app.docs.views.open(ViewKind::Model, None, false);
        let window = app.docs.views.move_to_window(detached, WindowTarget::New { placement: None }).unwrap().window;
        let badge = Rect::from_min_size(pos2(240.0, 4.0), vec2(100.0, 26.0));
        app.view_window_drag.strips.push(Strip {
            viewport: egui::ViewportId::ROOT,
            tab: app.docs.current_tab(),
            window: 0,
            screen: Screen { origin: Pos2::ZERO, pixels_per_point: 1.0 },
            name: "Design".into(),
            zone: ViewDropZone {
                document: 0,
                rect: Rect::from_min_size(pos2(80.0, 0.0), vec2(340.0, 34.0)),
                view_centres: vec![],
                elsewhere: vec![(window, badge)],
            },
            count: 2,
        });
        let ctx = Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let drag = begin(&app, ui.ctx(), dragged, Rect::from_min_size(pos2(170.0, 4.0), vec2(70.0, 26.0)), false).unwrap();
            assert!(matches!(take_target(&app, ui.ctx(), &drag, badge.center()), Some(Target::Elsewhere { window: target, blocked: false, .. }) if target == window));
        });
    }

    #[test]
    fn strip_band_and_slot_hysteresis_use_the_destination_scale() {
        let screen = Screen { origin: pos2(-1000.0, 200.0), pixels_per_point: 1.5 };
        let strip = Strip {
            viewport: egui::ViewportId::ROOT,
            tab: 1,
            window: 0,
            screen,
            name: "Design".into(),
            zone: ViewDropZone {
                document: 0,
                elsewhere: Vec::new(),
                rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(300.0, 30.0)),
                view_centres: vec![(0, 140.0), (1, 180.0), (2, 250.0)],
            },
            count: 3,
        };
        assert_eq!(strip.drop_at(screen.local(screen.desktop(pos2(210.0, 20.0))), None).unwrap().index, 2);
        let first = strip.drop_at(pos2(138.0, 45.0), None).unwrap();
        assert_eq!(first.index, 0);
        assert_eq!(strip.drop_at(pos2(145.0, 45.0), Some(first)).unwrap().index, 0);
        assert_eq!(strip.drop_at(pos2(147.0, 45.0), Some(first)).unwrap().index, 1);
        assert!(strip.drop_at(pos2(147.0, 47.0), None).is_none());
    }
}
