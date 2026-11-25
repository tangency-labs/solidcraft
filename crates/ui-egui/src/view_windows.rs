//! Views-only hosts for the same live design. View ownership stays in the design's ViewSet.

use egui::{Context, Ui, ViewportClass, ViewportCommand, ViewportId};
use serde_json::{Value, json};
use solidcraft_views::{DesignView, PopOut, TopBar, ViewKind, ViewSet, WindowId, WindowLevel, WindowOptions, WindowPlacement, WindowTarget};

use crate::ui_kit::{BackingTooltipExt as _, Container, Edge, Menu, Surface};
use crate::{SolveApp, documents};

#[derive(Default)]
pub(crate) struct Windows {
    native: bool,
    render_open: bool,
    embedded: bool,
    painting: Option<WindowId>,
    scoped_focus: Option<u64>,
    command_window: Option<WindowId>,
    opening_host: Option<WindowId>,
    painting_options: Option<WindowOptions>,
    pub(crate) painting_panels: bool,
    closing: Option<Option<u64>>,
    parked: Option<(u64, Vec<u64>)>,
    focus: Option<(u64, WindowId)>,
    last_focus: Option<(u64, WindowId)>,
    tool_owner: std::collections::BTreeMap<u64, WindowId>,
    root_rect: Option<egui::Rect>,
    monitor_size: Option<egui::Vec2>,
    root_scale: Option<f32>,
    displays: Vec<Display>,
    #[cfg(not(target_arch = "wasm32"))]
    displays_updated: Option<std::time::Instant>,
    hosts: Vec<Host>,
    returned: Option<Returned>,
    choices: Vec<WindowChoice>,
    foreign_views: Vec<(WindowId, DesignView)>,
    strip_order: std::collections::BTreeMap<u64, Vec<u64>>,
    clean: std::collections::BTreeSet<(u64, WindowId)>,
    root_placement: Option<WindowPlacement>,
    pending_root_placement: Option<(u64, WindowPlacement)>,
    pending_child_placements: std::collections::BTreeSet<(u64, WindowId)>,
    setup_snapshot: Option<Value>,
    pending_action: Option<PendingAction>,
}

#[derive(Clone)]
pub(crate) struct WindowChoice {
    pub(crate) id: WindowId,
    pub(crate) label: String,
    pub(crate) name: String,
    pub(crate) number: u32,
    pub(crate) custom_name: Option<String>,
    pub(crate) display_label: Option<String>,
    display: Option<String>,
}

enum PendingAction {
    Move { view: u64, target: WindowTarget },
    Focus { view: u64, window: WindowId },
    Close { view: u64 },
    Bring { source: WindowId, target: WindowId },
    Setup(crate::view_setups::DeferredSetup),
    MoveToDisplay { window: WindowId, display: usize },
}

struct Host {
    tab: u64,
    id: WindowId,
    viewport: crate::viewport::ViewportState,
    shell: crate::shell::State,
    title: documents::WindowTitle,
    layout_fill: documents::LayoutFill,
    view_rename: Option<documents::ViewRename>,
    view_placement: Option<(u64, u64)>,
    menu: crate::context_menu::MenuState,
    preview: crate::preview::PreviewState,
    panels: crate::panel_runtime::PopOutPanels,
    ribbon_reveal: crate::ui_kit::reveal::State,
    ribbon_focus_pending: bool,
    top_bar_reveal: crate::ui_kit::reveal::State,
    top_bar_focus_pending: bool,
    ribbon_tab: String,
    ribbon_view_workspace: &'static str,
    ribbon_context: Option<String>,
    ribbon_popup: Option<egui::Id>,
    sketch_return_tab: Option<String>,
    ready: bool,
    startup: Option<f64>,
}

struct Display {
    name: String,
    bounds: egui::Rect,
    scale: f32,
    primary: bool,
}

struct Returned {
    tab: u64,
    design: String,
    window: PopOut,
    expires: f64,
}

/// Whether the window being drawn shows the open command (always, with one window).
pub(crate) fn shows_tool(app: &SolveApp) -> bool {
    app.view_windows.shows_tool(app.docs.current_tab())
}

fn viewport_id(tab: u64, id: WindowId) -> ViewportId {
    ViewportId::from_hash_of(("solidcraft-view-window", tab, id))
}

impl Windows {
    pub(crate) fn child_tool_owner(&self, tab: u64) -> Option<WindowId> {
        self.tool_owner.get(&tab).copied()
    }

    /// Whether the window being drawn shows the open command: the one it was started in.
    fn shows_tool(&self, tab: u64) -> bool {
        self.child_tool_owner(tab) == self.painting
    }

    pub(crate) fn start_tool_panel(&mut self, tab: u64) {
        if let Some(id) = self.painting {
            self.tool_owner.insert(tab, id);
        } else {
            self.tool_owner.remove(&tab);
        }
    }

    pub(crate) fn setup_snapshot(&self) -> Option<&Value> {
        self.setup_snapshot.as_ref()
    }

    pub(crate) fn current_window(&self) -> WindowId {
        self.painting.unwrap_or_default()
    }

    pub(crate) fn opening_host(&mut self, window: Option<WindowId>) -> Option<WindowId> {
        std::mem::replace(&mut self.opening_host, window)
    }

    pub(crate) fn is_child(&self) -> bool {
        self.painting.is_some()
    }

    pub(crate) fn close_current(&mut self) -> bool {
        if !self.is_child() {
            return false;
        }
        self.closing = Some(None);
        true
    }

    pub(crate) fn keeps_view(&self, tab: u64, id: u64) -> bool {
        self.parked.as_ref().is_some_and(|(key, ids)| *key == tab && ids.contains(&id))
    }

    pub(crate) fn has_render(&self) -> bool {
        self.render_open
    }
}

pub(crate) fn supports_pop_out(app: &SolveApp) -> bool {
    !cfg!(target_arch = "wasm32") && !app.view_windows.embedded
}

pub(crate) fn detached_views(app: &SolveApp) -> impl Iterator<Item = (WindowId, &DesignView)> {
    app.docs.views.windows().iter().flat_map(|window| window.views.views().iter().map(|view| (window.id, view)))
}

fn available_strip_views(app: &SolveApp) -> impl Iterator<Item = (WindowId, &DesignView)> {
    let window = app.view_windows.current_window();
    app.docs
        .views
        .views()
        .iter()
        .map(move |view| (window, view))
        .chain(detached_views(app))
        .chain(app.view_windows.foreign_views.iter().map(|(window, view)| (*window, view)))
}

/// Draw every view without changing its position when it moves to another window.
pub(crate) fn remember_strip_order(app: &mut SolveApp) {
    if !app.preferences.clear_view_tabs {
        return;
    }
    let available: Vec<_> = available_strip_views(app).map(|(window, view)| (window, view.id)).collect();
    let tab = app.docs.active_tab();
    app.view_windows.strip_order.retain(|tab, _| app.docs.tab_open(*tab));
    let order = app.view_windows.strip_order.entry(tab).or_default();
    order.retain(|id| available.iter().any(|(_, available)| available == id));
    for (i, (window, id)) in available.iter().copied().enumerate() {
        if order.contains(&id) {
            continue;
        }
        let at = available[..i]
            .iter()
            .rev()
            .filter(|(owner, _)| *owner == window)
            .find_map(|(_, before)| order.iter().position(|known| known == before).map(|at| at + 1))
            .or_else(|| {
                available[i + 1..].iter().filter(|(owner, _)| *owner == window).find_map(|(_, after)| order.iter().position(|known| known == after))
            })
            .unwrap_or(order.len());
        order.insert(at, id);
    }
}

/// Update the local positions while leaving views in other windows where they were.
pub(crate) fn remember_local_reorder(app: &mut SolveApp, window: WindowId) {
    remember_strip_order(app);
    let local = if app.view_windows.painting == Some(window) { Some(&app.docs.views) } else { app.docs.views.views_of(window) };
    let Some(views) = local else { return };
    let Some(order) = app.view_windows.strip_order.get_mut(&app.docs.current_tab()) else { return };
    let mut local = views.ids();
    for id in order.iter_mut().filter(|id| views.get(**id).is_some()) {
        if let Some(next) = local.next() {
            *id = next;
        }
    }
}

/// The command's window or the last native window that gained focus, never a passive paint.
pub(crate) fn focused_view(app: &SolveApp) -> u64 {
    if let Some(view) = app.view_windows.scoped_focus {
        return view;
    }
    if app.view_windows.is_child() {
        return app.docs.views.active().id;
    }
    app.view_windows
        .last_focus
        .filter(|(tab, _)| *tab == app.docs.current_tab())
        .and_then(|(_, window)| app.docs.views.views_of(window))
        .map_or(app.docs.views.active().id, |views| views.active().id)
}

/// An explicit host/command scope wins over native focus used by idle API calls.
pub(crate) fn command_view(app: &SolveApp) -> u64 {
    if app.view_windows.painting.is_some() || app.view_windows.command_window.is_some() {
        app.docs.views.active().id
    } else {
        app.session.view_context.focused()
    }
}

pub(crate) fn opening_window(app: &SolveApp) -> WindowId {
    app.view_windows.painting.or(app.view_windows.command_window).or(app.view_windows.opening_host).unwrap_or_else(|| {
        app.view_windows
            .last_focus
            .filter(|(tab, window)| *tab == app.docs.current_tab() && app.docs.views.views_of(*window).is_some())
            .map_or(0, |(_, window)| window)
    })
}

pub(crate) fn layout_views(app: &SolveApp) -> Vec<(WindowId, &DesignView)> {
    available_strip_views(app).collect()
}

pub(crate) fn sync_sketch_editing(app: &mut SolveApp, editing: Option<(u64, solidcraft_views::SketchSession)>) {
    for (_, view) in &mut app.view_windows.foreign_views {
        view.editing = editing.filter(|(id, _)| *id == view.id).map(|(_, editing)| editing);
    }
}

fn set_camera_in(views: &mut ViewSet, id: u64, camera: solidcraft_viewport::Camera) -> bool {
    if views.set_spec(id, solidcraft_viewport::ViewSpec::new(camera)) {
        return true;
    }
    let windows: Vec<_> = views.windows().iter().map(|window| window.id).collect();
    windows.into_iter().any(|window| views.window_mut(window).is_some_and(|window| set_camera_in(&mut window.views, id, camera)))
}

pub(crate) fn set_sketch_camera(app: &mut SolveApp, id: u64, camera: solidcraft_viewport::Camera) {
    set_view_camera(app, id, camera);
}

/// Update the real window or its borrowed strip snapshot without changing view focus.
pub(crate) fn set_view_camera(app: &mut SolveApp, id: u64, camera: solidcraft_viewport::Camera) {
    if set_camera_in(&mut app.docs.views, id, camera) {
        return;
    }
    if let Some((_, view)) = app.view_windows.foreign_views.iter_mut().find(|(_, view)| view.id == id) {
        view.spec = Some(solidcraft_viewport::ViewSpec::new(camera));
    }
}

pub(crate) fn strip_views(app: &SolveApp) -> Vec<(WindowId, &DesignView)> {
    let order = app.view_windows.strip_order.get(&app.docs.current_tab());
    let mut views = layout_views(app);
    if app.view_windows.is_child() && !app.preferences.clear_view_tabs {
        views.retain(|(window, _)| *window == app.view_windows.current_window());
    }
    if app.preferences.clear_view_tabs
        && let Some(order) = order
    {
        views.sort_by_key(|(_, view)| order.iter().position(|id| *id == view.id).unwrap_or(usize::MAX));
    }
    views
}

fn window_number(app: &SolveApp, id: WindowId) -> u32 {
    if id == 0 {
        return 1;
    }
    if app.view_windows.is_child() {
        return app.view_windows.choices.iter().find(|window| window.id == id).map_or(2, |window| window.number);
    }
    app.docs.views.windows().iter().position(|window| window.id == id).map_or(2, |index| index as u32 + 2)
}

fn assigned_window_name(app: &SolveApp, id: WindowId) -> Option<&str> {
    app.docs.views.views_of(id).and_then(ViewSet::window_name).or_else(|| {
        if app.view_windows.is_child() {
            app.view_windows.choices.iter().find(|choice| choice.id == id).and_then(|choice| choice.custom_name.as_deref())
        } else {
            None
        }
    })
}

pub(crate) fn window_name(app: &SolveApp, id: WindowId) -> String {
    let label = window_number(app, id).to_string();
    if let Some(name) = assigned_window_name(app, id) {
        format!("{label} {name}")
    } else if id == 0 {
        let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
        format!("{label} · {}", locale.text("view_tab.main_window"))
    } else {
        label
    }
}

pub(crate) fn view_at(app: &SolveApp, window: Option<WindowId>, index: usize) -> Result<u64, String> {
    if let Some(child) = app.view_windows.painting
        && window.is_none_or(|window| window == child)
    {
        return app.docs.views.views().get(index).map(|view| view.id).ok_or_else(|| "no_such_view".into());
    }
    let current = app
        .view_windows
        .last_focus
        .filter(|(tab, window)| *tab == app.docs.current_tab() && app.docs.views.views_of(*window).is_some())
        .map_or(0, |(_, window)| window);
    let views = app.docs.views.views_of(window.unwrap_or(current)).ok_or("no_such_window")?;
    views.views().get(index).map(|view| view.id).ok_or_else(|| "no_such_view".into())
}

/// Only an explicit activation or native focus transition records the MRU receiver.
pub(crate) fn record_local_focus(app: &mut SolveApp, view: u64) {
    app.view_windows.last_focus = Some((app.docs.current_tab(), app.view_windows.current_window()));
    if app.view_windows.scoped_focus.is_some() {
        app.view_windows.scoped_focus = Some(view);
    }
}

pub(crate) fn focus_view(app: &mut SolveApp, view: u64, window: Option<WindowId>) -> Result<(), String> {
    if let Some(child) = app.view_windows.painting {
        if window.is_none_or(|window| window == child) && app.docs.views.get(view).is_some() {
            return documents::activate_local_view(app, view).then_some(()).ok_or_else(|| "no_such_view".into());
        }
        if let Some((owner, _)) =
            app.view_windows.foreign_views.iter().find(|(owner, candidate)| candidate.id == view && window.is_none_or(|window| window == *owner))
        {
            app.view_windows.pending_action = Some(PendingAction::Focus { view, window: *owner });
            return Ok(());
        }
        return Err("no_such_view".into());
    }
    let owner = app.docs.views.window_of(view).ok_or("no_such_view")?;
    if let Some(window) = window.filter(|window| *window != owner) {
        return Err(if app.docs.views.views_of(window).is_some() { "no_such_view" } else { "no_such_window" }.into());
    }
    if owner == 0 {
        if !documents::activate_local_view(app, view) {
            return Err("no_such_view".into());
        }
        app.view_windows.last_focus = Some((app.docs.current_tab(), 0));
        return Ok(());
    }
    let child = app.docs.views.window_mut(owner).ok_or("no_such_window")?;
    if !child.views.activate(view) {
        return Err("no_such_view".into());
    }
    focus(app, owner)
}

fn focus(app: &mut SolveApp, id: WindowId) -> Result<(), String> {
    let exists = if app.view_windows.is_child() {
        app.view_windows.choices.iter().any(|choice| choice.id == id)
    } else {
        app.docs.views.views_of(id).is_some()
    };
    if !exists {
        return Err("no_such_window".into());
    }
    let target = (app.docs.active_tab(), id);
    app.view_windows.focus = Some(target);
    app.view_windows.last_focus = Some(target);
    Ok(())
}

pub(crate) fn defer_close_view(app: &mut SolveApp, view: u64) -> bool {
    if !app.view_windows.foreign_views.iter().any(|(_, candidate)| candidate.id == view) {
        return false;
    }
    app.view_windows.pending_action = Some(PendingAction::Close { view });
    true
}

fn placement(app: &SolveApp, view: u64, request: &Value, pointer: Option<egui::Pos2>) -> Option<WindowPlacement> {
    let number = |key: &str| request.get(key).and_then(Value::as_f64).map(|value| value as f32);
    let child = app.view_windows.painting.or_else(|| app.docs.views.window_of(view).filter(|id| *id != 0));
    let rect = app.view_windows.root_rect;
    if !app.view_windows.native && ["x", "y", "w", "h"].iter().all(|key| number(key).is_none()) {
        return None;
    }
    let source_viewport = child
        .filter(|id| Some(*id) != app.view_windows.painting)
        .and_then(|id| app.view_windows.hosts.iter().find(|host| host.tab == app.docs.current_tab() && host.id == id))
        .map_or(&app.viewport, |host| &host.viewport);
    let tiled =
        child.filter(|id| Some(*id) != app.view_windows.painting).and_then(|id| app.docs.views.views_of(id)).unwrap_or(&app.docs.views).is_tiled();
    let source = source_viewport
        .cards
        .iter()
        .find(|(id, _)| *id == view)
        .map(|(_, rect)| *rect)
        .or(source_viewport.rect)
        .filter(|rect| rect.is_positive() && rect.is_finite())
        .map_or(egui::vec2(800.0, 560.0), |rect| rect.size() + egui::vec2(0.0, if tiled { crate::view_tiles::HEADER } else { 0.0 }));
    let displays = &app.view_windows.displays;
    let selected = request.get("display").and_then(|display| {
        if let Some(name) = display.as_str() {
            displays.iter().find(|entry| entry.name == name)
        } else {
            display.as_u64().and_then(|index| usize::try_from(index).ok()).and_then(|index| displays.get(index))
        }
    });
    let source_display = rect.zip(app.view_windows.root_scale).and_then(|(rect, scale)| {
        let center = (rect.center().to_vec2() * scale).to_pos2();
        displays.iter().find(|display| display.bounds.contains(center))
    });
    let pointer_display = pointer.and_then(|point| displays.iter().find(|display| display.bounds.contains(point)));
    let child_display = child
        .and_then(|id| {
            if Some(id) == app.view_windows.painting {
                app.view_windows.choices.iter().find(|choice| choice.id == id)?.display.as_deref()
            } else {
                app.docs.views.window(id)?.placement.as_ref()?.display.as_deref()
            }
        })
        .and_then(|name| displays.iter().find(|display| display.name == name));
    let display = selected
        .or(pointer_display)
        .or(child_display)
        .or(source_display)
        .or_else(|| displays.iter().find(|display| display.primary))
        .or(displays.first());
    let size = display
        .map(|display| display.bounds.size() / display.scale)
        .or(app.view_windows.monitor_size)
        .filter(|size| size.is_finite() && size.x > 0.0 && size.y > 0.0)
        .unwrap_or(egui::vec2(1920.0, 1080.0));
    let max = size * 0.9;
    let w = number("w").unwrap_or(source.x).clamp(640.0_f32.min(max.x), max.x);
    let h = number("h").unwrap_or(source.y).clamp(420.0_f32.min(max.y), max.y);
    let center = display.zip(pointer).map_or((size * 0.5).to_pos2(), |(display, point)| ((point - display.bounds.min) / display.scale).to_pos2());
    let cascade = if pointer.is_some() || number("x").is_some() || number("y").is_some() {
        0.0
    } else {
        32.0 * window_choices(app).len().saturating_sub(1) as f32
    };
    Some(WindowPlacement {
        display: display.map(|display| display.name.clone()),
        x: number("x").unwrap_or(center.x - w / 2.0 + cascade).clamp(0.0, size.x - w),
        y: number("y").unwrap_or(center.y - h / 2.0 + cascade).clamp(0.0, size.y - h),
        w,
        h,
        maximised: false,
        fullscreen: false,
    })
}

#[derive(Clone, Copy)]
pub(crate) struct DragSource {
    frame: solidcraft_views::Rect,
    cell: Option<solidcraft_views::Rect>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct DragWindow {
    placement: WindowPlacement,
    pub(crate) outline: egui::Rect,
    pub(crate) scale: f32,
    pub(crate) label: String,
}

pub(crate) fn drag_source(app: &SolveApp, view: u64, frame: egui::Rect) -> DragSource {
    let convert = |rect: egui::Rect| solidcraft_views::Rect { x: rect.left(), y: rect.top(), w: rect.width(), h: rect.height() };
    let card = app.viewport.cards.iter().find(|(id, _)| *id == view).map(|(_, rect)| *rect);
    let tile = app.docs.views.tiles().is_some_and(|tiles| app.docs.views.is_tiled() && tiles.contains(view));
    let fitted = if app.docs.views.active().id == view { app.viewport.rect } else { card };
    let cell = fitted.filter(|rect| rect.is_finite() && rect.is_positive()).map(|mut rect| {
        if tile {
            rect.min.y -= crate::view_tiles::HEADER;
        }
        convert(rect)
    });
    DragSource { frame: convert(frame), cell }
}

pub(crate) fn drag_display_count(app: &SolveApp) -> usize {
    app.view_windows.displays.len().max(1)
}

pub(crate) fn drag_window(
    app: &SolveApp,
    source: DragSource,
    pointer: Option<egui::Pos2>,
    display_index: Option<usize>,
    snap: bool,
) -> Option<DragWindow> {
    let displays = &app.view_windows.displays;
    let target = display_index
        .and_then(|index| displays.get(index))
        .or_else(|| pointer.and_then(|point| displays.iter().find(|display| display.bounds.contains(point))))
        .or_else(|| displays.iter().find(|display| display.primary))
        .or(displays.first());
    let fallback = egui::Rect::from_min_size(egui::Pos2::ZERO, app.view_windows.monitor_size.unwrap_or(egui::vec2(1920.0, 1080.0)));
    let (bounds, scale) = target.map_or((fallback, 1.0), |display| (display.bounds, display.scale));
    let size = bounds.size() / scale;
    let point = pointer.map(|point| (point - bounds.min) / scale).map(|point| [point.x, point.y]);
    let work = solidcraft_views::Rect { x: 0.0, y: 0.0, w: size.x, h: size.y };
    let rect = solidcraft_views::window_rect_for(source.frame, source.cell, work, point, snap)?;
    let origin = bounds.min + egui::vec2(rect.x, rect.y) * scale;
    let outline = egui::Rect::from_min_size(origin, egui::vec2(rect.w, rect.h) * scale);
    let number = target.and_then(|target| displays.iter().position(|display| std::ptr::eq(display, target)));
    let label = number.map_or_else(
        || app.tr("multi_window.drag_display").to_owned(),
        |index| solidcraft_ui_text::format("multi_window.display_number", &[("n", &(index + 1).to_string())]),
    );
    let placement = WindowPlacement {
        display: target.map(|display| display.name.clone()),
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        maximised: false,
        fullscreen: false,
    };
    Some(DragWindow { placement, outline, scale, label })
}

pub(crate) fn drop_new_window(app: &mut SolveApp, view: u64, target: DragWindow) -> Result<(), String> {
    let rect = target.placement;
    let mut request = json!({"view":view,"window":"new","x":rect.x,"y":rect.y,"w":rect.w,"h":rect.h});
    if let Some(display) = rect.display {
        request["display"] = json!(display);
    }
    app.run("tab.view.move_to_window", request).map(|_| ())
}

fn placement_json(placement: Option<&WindowPlacement>) -> Value {
    placement.map_or(Value::Null, |p| json!({"x":p.x,"y":p.y,"w":p.w,"h":p.h,"maximised":p.maximised,"fullscreen":p.fullscreen}))
}

fn collect_choices(app: &SolveApp) -> Vec<WindowChoice> {
    let displays = &app.view_windows.displays;
    let display_label = |display: Option<&str>| {
        displays
            .iter()
            .position(|entry| display == Some(entry.name.as_str()))
            .map(|index| solidcraft_ui_text::format("multi_window.display_number", &[("n", &(index + 1).to_string())]))
    };
    let root_display = app.view_windows.root_rect.zip(app.view_windows.root_scale).and_then(|(rect, scale)| {
        let position = (rect.center().to_vec2() * scale).to_pos2();
        displays.iter().find(|display| display.bounds.contains(position))
    });
    let choice = |id: WindowId, fallback: &str, display: Option<String>, label_display: Option<&str>| {
        let name = if app.preferences.clear_view_tabs { window_name(app, id) } else { fallback.to_owned() };
        let display_label = display_label(label_display);
        let label = display_label.as_ref().map_or_else(|| name.clone(), |display| format!("{name} · {display}"));
        WindowChoice {
            id,
            label,
            name,
            number: window_number(app, id),
            custom_name: app.docs.views.views_of(id).and_then(ViewSet::window_name).map(str::to_owned),
            display,
            display_label,
        }
    };
    std::iter::once(choice(0, &app.session.doc.name, None, root_display.map(|display| display.name.as_str())))
        .chain(app.docs.views.windows().iter().map(|window| {
            let display = window.placement.as_ref().and_then(|placement| placement.display.as_deref());
            choice(window.id, &window.views.active().name, display.map(str::to_owned), display)
        }))
        .collect()
}

pub(crate) fn window_choices(app: &SolveApp) -> std::borrow::Cow<'_, [WindowChoice]> {
    if app.view_windows.is_child() { std::borrow::Cow::Borrowed(&app.view_windows.choices) } else { std::borrow::Cow::Owned(collect_choices(app)) }
}

pub(crate) fn can_create(app: &SolveApp) -> bool {
    window_choices(app).len().saturating_sub(1) < solidcraft_views::MAX_POPOUTS
}

pub(crate) fn bring_views(app: &mut SolveApp, source: WindowId) -> Result<(), String> {
    let target = app.view_windows.current_window();
    if source == 0 || source == target {
        return Err("last_view_of_main_window".into());
    }
    if app.view_windows.is_child() {
        app.view_windows.pending_action = Some(PendingAction::Bring { source, target });
        return Ok(());
    }
    apply_bring(app, source, target)
}

fn apply_bring(app: &mut SolveApp, source: WindowId, target: WindowId) -> Result<(), String> {
    let ids: Vec<_> = app.docs.views.views_of(source).ok_or("no_such_window")?.ids().collect();
    let destination = WindowTarget::Existing { window: target, index: None };
    // A layout or ownership error must leave all the views in their original windows.
    let mut check = app.docs.views.clone();
    for &id in &ids {
        check.move_to_window(id, destination.clone()).map_err(|error| error.code().to_owned())?;
    }
    for id in ids {
        app.run("tab.view.move_to_window", json!({"view": id, "window": target}))?;
    }
    Ok(())
}

fn apply_move(app: &mut SolveApp, view: u64, target: WindowTarget) -> Result<Value, String> {
    remember_strip_order(app);
    let tab = app.docs.active_tab();
    let indexed = matches!(target, WindowTarget::Existing { index: Some(_), .. });
    let moved = documents::move_view_to_window(app, view, target).map_err(|error| error.code().to_owned())?;
    if moved.created
        && let Some(window) = app.docs.views.window_mut(moved.window)
    {
        window.options = app.preferences.pop_out_window_defaults;
    }
    if indexed {
        remember_local_reorder(app, moved.window);
    }
    if moved.from != moved.window {
        app.view_windows.focus = Some((tab, moved.window));
        app.view_windows.last_focus = Some((tab, moved.window));
    }
    Ok(json!({"view":moved.view,"window":moved.window,"created":moved.created,"from":moved.from,
        "closed_source":moved.closed_source,"placement":placement_json(moved.placement.as_ref())}))
}

fn move_view(app: &mut SolveApp, view: u64, target: WindowTarget) -> Result<Value, String> {
    if !supports_pop_out(app) {
        return Err("windows_unavailable".into());
    }
    if let Some(source) = app.view_windows.painting {
        let source = if app.docs.views.get(view).is_some() {
            source
        } else {
            app.view_windows.foreign_views.iter().find(|(_, candidate)| candidate.id == view).map(|(owner, _)| *owner).ok_or("no_such_view")?
        };
        match &target {
            WindowTarget::New { .. } if !can_create(app) => return Err("too_many_windows".into()),
            WindowTarget::Existing { window, .. } | WindowTarget::Dock { window, .. }
                if !app.view_windows.choices.iter().any(|choice| choice.id == *window) =>
            {
                return Err("no_such_window".into());
            }
            _ => {}
        }
        // The callback owns the source strip until it finishes drawing. Apply membership changes
        // after the strip has been put back in the design.
        app.view_windows.pending_action = Some(PendingAction::Move { view, target });
        return Ok(json!({"view": view, "from": source}));
    }
    apply_move(app, view, target)
}

pub(crate) fn detach(app: &mut SolveApp, view: u64, request: &Value) -> Result<Value, String> {
    let target = match request.get("window").and_then(Value::as_u64).and_then(|id| WindowId::try_from(id).ok()) {
        Some(window) => {
            if let Some(placement) = request.get("placement") {
                let placement = serde_json::from_value::<solidcraft_engine::schema::tab_views::TabViewPlacement>(placement.clone())
                    .map_err(|error| error.to_string())?;
                WindowTarget::Dock { window, placement: placement.into() }
            } else {
                WindowTarget::Existing { window, index: request.get("index").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()) }
            }
        }
        None => WindowTarget::New { placement: placement(app, view, request, None) },
    };
    move_view(app, view, target)
}

#[cfg(test)]
pub(crate) fn drag_drop(app: &mut SolveApp, view: u64, target: Option<(WindowId, usize)>, screen: Option<egui::Pos2>) -> Result<(), String> {
    let target = match target {
        Some((window, index)) => WindowTarget::Existing { window, index: Some(index) },
        None => WindowTarget::New { placement: placement(app, view, &Value::Null, screen) },
    };
    move_view(app, view, target).map(|_| ())
}

pub(crate) fn cycle_view(app: &mut SolveApp) {
    let source = app.view_windows.current_window();
    let view = app.docs.views.active().id;
    let choices = window_choices(app);
    let target =
        if choices.len() == 1 { json!("new") } else { json!(choices.iter().find(|choice| choice.id > source).map_or(0, |choice| choice.id)) };
    crate::design_view_ui::move_to_window(app, view, target);
}

pub(crate) fn with_strip<T>(
    app: &mut SolveApp,
    requested: Option<WindowId>,
    action: impl FnOnce(&mut SolveApp) -> Result<T, String>,
) -> Result<T, String> {
    let tab = app.docs.current_tab();
    let local = app.view_windows.current_window();
    let target = requested.unwrap_or_else(|| {
        app.view_windows
            .painting
            .or_else(|| app.view_windows.last_focus.filter(|(owner, id)| *owner == tab && app.docs.views.views_of(*id).is_some()).map(|(_, id)| id))
            .unwrap_or_default()
    });
    if local == target {
        let previous = app.view_windows.command_window.replace(target);
        let result = action(app);
        app.view_windows.command_window = previous;
        return result;
    }
    if local != 0 {
        return Err("no_such_window".into());
    }
    let scope_focus = focused_view(app);
    let mut window = app.docs.views.take_window(target).ok_or("no_such_window")?;
    let receiver_scope =
        (!window.views.views().iter().any(|view| view.id == scope_focus)).then(|| crate::drawing_workspace::ReceiverScope::capture(app));
    let host_index = app.view_windows.hosts.iter().position(|host| host.tab == tab && host.id == target);
    let mut host = host_index.map_or_else(|| Host::new(tab, target), |index| app.view_windows.hosts.remove(index));
    let previous_focus = app.view_windows.scoped_focus.replace(scope_focus);
    let foreign: Vec<_> = available_strip_views(app).map(|(owner, view)| (owner, view.clone())).collect();
    let previous_foreign = std::mem::replace(&mut app.view_windows.foreign_views, foreign);
    documents::park_active_look(app);
    app.view_windows.parked = Some((tab, app.docs.views.ids().collect()));
    std::mem::swap(&mut app.docs.views, &mut window.views);
    host.swap(app);
    let workspace = documents::view_workspace(app.docs.views.active().kind);
    if host.ribbon_view_workspace != workspace {
        host.ribbon_view_workspace = workspace;
        host.ribbon_tab = workspace.into();
        host.ribbon_context = None;
    }
    let ribbon = std::mem::replace(&mut app.ui.tab, host.ribbon_tab.clone());
    std::mem::swap(&mut app.ui.ribbon_context, &mut host.ribbon_context);
    std::mem::swap(&mut app.ui.sketch_return_tab, &mut host.sketch_return_tab);
    crate::drawing_workspace::swap_selection(app);
    crate::workspace::sync_history(app);
    documents::show_active_look(app);
    app.view_windows.painting = Some(target);
    app.view_windows.painting_options = Some(window.options);
    let result = if app.preferences.workspace_follows_view && app.docs.views.active().kind == ViewKind::Sheet {
        let view = app.docs.views.active().clone();
        crate::sheet_views::focus(app, &view).and_then(|()| action(app))
    } else {
        action(app)
    };
    if let Some(options) = app.view_windows.painting_options.take() {
        window.options = options;
    }
    documents::park_active_look(app);
    host.ribbon_tab = std::mem::replace(&mut app.ui.tab, ribbon);
    host.ribbon_view_workspace = documents::view_workspace(app.docs.views.active().kind);
    std::mem::swap(&mut app.ui.ribbon_context, &mut host.ribbon_context);
    std::mem::swap(&mut app.ui.sketch_return_tab, &mut host.sketch_return_tab);
    crate::drawing_workspace::swap_selection(app);
    crate::workspace::sync_history(app);
    app.view_windows.painting = None;
    app.view_windows.parked = None;
    for (_, view) in &app.view_windows.foreign_views {
        if let Some(spec) = view.spec {
            set_camera_in(&mut window.views, view.id, spec.camera);
        }
    }
    app.view_windows.foreign_views = previous_foreign;
    app.view_windows.scoped_focus = previous_focus;
    host.swap(app);
    std::mem::swap(&mut app.docs.views, &mut window.views);
    app.docs.views.put_window(window);
    crate::sketch_view::project(app);
    documents::show_active_look(app);
    if let Some(scope) = receiver_scope {
        scope.restore(app);
    }
    if let Some(index) = host_index {
        app.view_windows.hosts.insert(index, host);
    }
    result
}

pub(crate) fn close_view(app: &mut SolveApp, view: u64) -> Option<bool> {
    if app.view_windows.is_child() {
        if app.docs.views.len() == 1 && app.docs.views.active().id == view {
            app.view_windows.closing = Some(Some(view));
            return Some(true);
        }
        return None;
    }
    let owner = app.docs.views.window_of(view)?;
    (owner != 0).then(|| app.docs.views.close_in_window(owner, view))
}

fn close(app: &mut SolveApp, id: WindowId, toast: bool) -> Result<Vec<u64>, String> {
    if id == 0 {
        return Err("cannot_close_main_window".into());
    }
    let window = app.docs.views.window(id).cloned().ok_or("no_such_window")?;
    let closing: Vec<_> = window.views.ids().collect();
    if !crate::sketch_view::before_close(app, &closing) {
        return Err("sketch could not be finished".into());
    }
    let ids = app.docs.views.close_window(id).ok_or("no_such_window")?;
    app.sketch_notices.close_window(app.session.uid, id);
    if toast {
        let tab = app.docs.active_tab();
        app.view_windows.returned = Some(Returned { tab, design: app.session.doc.name.clone(), window, expires: app.now + 8.0 });
    }
    Ok(ids)
}

fn window_row(id: WindowId, views: &ViewSet, placement: Option<&WindowPlacement>, clean: bool) -> Value {
    let layout = views
        .tiles()
        .and_then(solidcraft_views::Tiles::preset_kind)
        .map_or(if views.is_tiled() { "custom" } else { "single" }, solidcraft_views::Preset::key);
    json!({"id":id,"level":if clean {"clean"} else if id==0 {"full"} else {"views_only"},"views":views.views().iter().map(|view|view.id).collect::<Vec<_>>(),
        "active":views.active().id,"layout":layout,"display":placement.and_then(|p|p.display.as_deref()),
        "placement":placement_json(placement),"tool_window":id==0})
}

pub(crate) fn is_clean(app: &SolveApp) -> bool {
    app.view_windows.clean.contains(&(app.docs.current_tab(), app.view_windows.current_window()))
}

pub(crate) fn show_clean(app: &mut SolveApp, ui: &mut Ui) {
    if !app.view_windows.is_child() {
        crate::panel_runtime::hide_app_host(app);
    }
    app.shell.cards.clear();
    app.shell.workspace_panels.clear();
    app.shell.browser = None;
    app.shell.history = None;
    app.shell.status_bar = None;
    let rect = ui.available_rect_before_wrap();
    app.shell.content = Some(rect);
    app.viewport.host_windows = Some(Vec::new());
    app.viewport.host_confirm = None;
    let mut scene = ui.new_child(egui::UiBuilder::new().id_salt("clean_workspace").max_rect(rect));
    app.show_workspace(&mut scene);
    crate::view_tiles::focus_pressed(app);
    app.resolve_fit(ui.ctx());
    documents::sync_linked_active(app);
}

pub(crate) fn child_options(app: &SolveApp) -> WindowOptions {
    app.view_windows.painting_options.unwrap_or_default()
}

fn options_command(app: &mut SolveApp, command: &str, request: Value) -> Result<Value, String> {
    use solidcraft_engine::schema::tab_windows::{TabWindowOptionsReply, TabWindowSetOptionsParams};
    let change: TabWindowSetOptionsParams = serde_json::from_value(request).map_err(|error| error.to_string())?;
    let tab = app.docs.current_tab();
    let window = change
        .window
        .or(app.view_windows.painting)
        .or_else(|| app.view_windows.last_focus.filter(|(owner, _)| *owner == tab).map(|(_, id)| id))
        .filter(|id| *id != 0)
        .ok_or("window_not_found")?;
    let current = if app.view_windows.painting == Some(window) {
        app.view_windows.painting_options.as_mut()
    } else {
        app.docs.views.window_mut(window).map(|window| &mut window.options)
    }
    .ok_or("window_not_found")?;
    if command == "tab.window.set_options" {
        if let Some(card_headers) = change.card_headers {
            current.card_headers = card_headers;
        }
        if let Some(keep_on_top) = change.keep_on_top {
            current.keep_on_top = keep_on_top;
        }
        if let Some(interface_size) = change.interface_size {
            current.interface_size = interface_size;
        }
        if let Some(panels) = change.panels {
            current.panels = panels;
        }
        if let Some(top_bar) = change.top_bar {
            current.top_bar = top_bar;
        }
        if let Some(ribbon) = change.ribbon {
            current.ribbon = ribbon;
        }
        if let Some(ribbon_auto_hide) = change.ribbon_auto_hide {
            current.ribbon_auto_hide = ribbon_auto_hide;
        }
    }
    let options = *current;
    if command == "window.options.use_as_default" {
        app.preferences.pop_out_window_defaults = options;
    }
    serde_json::to_value(TabWindowOptionsReply::Current { window, options }).map_err(|error| error.to_string())
}

pub(crate) fn command(app: &mut SolveApp, command: &str, request: Value) -> Result<Value, String> {
    let id = request.get("window").and_then(Value::as_u64).and_then(|id| WindowId::try_from(id).ok()).unwrap_or_default();
    match command {
        "tab.window.options" | "tab.window.set_options" | "window.options.use_as_default" => options_command(app, command, request),
        "tab.window.list" => {
            let clean = |id| app.view_windows.clean.contains(&(app.docs.current_tab(), id));
            let rows = std::iter::once(window_row(0, &app.docs.views, None, clean(0)))
                .chain(app.docs.views.windows().iter().map(|w| window_row(w.id, &w.views, w.placement.as_ref(), clean(w.id))))
                .collect::<Vec<_>>();
            Ok(json!({"windows":rows}))
        }
        "tab.window.clean" => {
            let params =
                serde_json::from_value::<solidcraft_engine::schema::tab_windows::TabWindowCleanParams>(request).map_err(|error| error.to_string())?;
            with_strip(app, params.window, |app| {
                let key = (app.docs.current_tab(), app.view_windows.current_window());
                let on = params.on.unwrap_or_else(|| !app.view_windows.clean.contains(&key));
                if on {
                    app.view_windows.clean.insert(key);
                    app.docs.view_placement = None;
                } else {
                    app.view_windows.clean.remove(&key);
                }
                Ok(json!({"window": key.1, "on": on}))
            })
        }
        "tab.window.rename" => {
            let name = request.get("name").and_then(Value::as_str).ok_or("invalid_window_name")?;
            with_strip(app, Some(id), |app| {
                if !app.docs.views.rename_window(name) {
                    return Err("invalid_window_name".into());
                }
                Ok(json!({"window": id, "name": name}))
            })
        }
        "tab.window.move_to_display" => {
            let display = request.get("display").and_then(Value::as_u64).and_then(|index| usize::try_from(index).ok()).ok_or("no_such_display")?;
            if app.view_windows.displays.get(display).is_none() {
                return Err("no_such_display".into());
            }
            if app.view_windows.painting == Some(id) {
                app.view_windows.pending_action = Some(PendingAction::MoveToDisplay { window: id, display });
            } else {
                move_to_display(app, id, display)?;
            }
            Ok(request)
        }
        "tab.window.focus" => {
            focus(app, id)?;
            Ok(json!({"window":id}))
        }
        "tab.window.close" => {
            if app.view_windows.painting == Some(id) {
                app.view_windows.closing = Some(None);
                return Ok(json!({"returned":app.docs.views.views().iter().map(|v|v.id).collect::<Vec<_>>()}));
            }
            Ok(json!({"returned":close(app,id,app.view_windows.native)?}))
        }
        _ => Err("no_such_window".into()),
    }
}

fn move_to_display(app: &mut SolveApp, id: WindowId, display: usize) -> Result<(), String> {
    let window = app.docs.views.window(id).ok_or("window_not_found")?;
    let existing = window.placement.as_ref();
    let destination = crate::view_setups::Placement {
        display: Some(display),
        x: existing.map_or(24.0, |p| p.x),
        y: existing.map_or(24.0, |p| p.y),
        w: existing.map_or(850.0, |p| p.w),
        h: existing.map_or(600.0, |p| p.h),
        maximised: existing.is_some_and(|p| p.maximised),
        fullscreen: existing.is_some_and(|p| p.fullscreen),
    };
    let placement = resolve_setup_placement(app, destination);
    let window = app.docs.views.window_mut(id).ok_or("window_not_found")?;
    window.placement = Some(placement);
    app.view_windows.pending_child_placements.insert((app.docs.active_tab(), id));
    Ok(())
}

fn restore_and_focus(ctx: &Context) {
    if ctx.input(|input| input.viewport().minimized == Some(true)) {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    }
    ctx.send_viewport_cmd(ViewportCommand::Focus);
}

pub(crate) fn defer_setup(app: &mut SolveApp, action: crate::view_setups::DeferredSetup) {
    app.view_windows.pending_action = Some(PendingAction::Setup(action));
}

pub(crate) fn setup_level(app: &SolveApp, window: WindowId) -> crate::view_setups::SetupLevel {
    use crate::view_setups::SetupLevel;
    if app.view_windows.clean.contains(&(app.docs.current_tab(), window)) {
        SetupLevel::Clean
    } else if window == 0 || app.docs.views.window(window).is_some_and(|window| window.level == WindowLevel::Full) {
        SetupLevel::Full
    } else {
        SetupLevel::ViewsOnly
    }
}

pub(crate) fn setup_placement(app: &SolveApp, window: WindowId) -> Option<&WindowPlacement> {
    if window == 0 { app.view_windows.root_placement.as_ref() } else { app.docs.views.window(window)?.placement.as_ref() }
}

pub(crate) fn display_index(app: &SolveApp, name: &str) -> Option<usize> {
    app.view_windows.displays.iter().position(|display| display.name == name)
}

pub(crate) fn resolve_setup_placement(app: &SolveApp, p: crate::view_setups::Placement) -> WindowPlacement {
    let windows = &app.view_windows;
    let saved = p.display.and_then(|index| windows.displays.get(index));
    let root = windows.root_placement.as_ref().and_then(|placement| placement.display.as_deref());
    let display = saved
        .or_else(|| windows.displays.iter().find(|display| root == Some(display.name.as_str())))
        .or_else(|| windows.displays.iter().find(|display| display.primary));
    let (x, y) = display.map_or((p.x, p.y), |display| {
        let size = display.bounds.size() / display.scale;
        (p.x.clamp(0.0, (size.x - p.w.min(64.0)).max(0.0)), p.y.clamp(0.0, (size.y - p.h.min(48.0)).max(0.0)))
    });
    WindowPlacement { display: display.map(|display| display.name.clone()), x, y, w: p.w, h: p.h, maximised: p.maximised, fullscreen: p.fullscreen }
}

pub(crate) fn apply_setup_presentation(app: &mut SolveApp, presentation: Vec<(WindowLevel, Option<WindowPlacement>)>) {
    let tab = app.docs.active_tab();
    let ids: Vec<_> = std::iter::once(0).chain(app.docs.views.windows().iter().map(|window| window.id)).collect();
    app.view_windows.clean.retain(|(owner, _)| *owner != tab);
    if app.view_windows.pending_root_placement.as_ref().is_some_and(|(owner, _)| *owner == tab) {
        app.view_windows.pending_root_placement = None;
    }
    app.view_windows.pending_child_placements.retain(|(owner, _)| *owner != tab);
    for (id, (level, placement)) in ids.into_iter().zip(presentation) {
        if level == WindowLevel::Clean {
            app.view_windows.clean.insert((tab, id));
        }
        if let Some(placement) = placement {
            if id == 0 {
                app.view_windows.pending_root_placement = Some((tab, placement));
            } else {
                app.view_windows.pending_child_placements.insert((tab, id));
            }
        }
    }
}

fn measured_placement(windows: &Windows, ctx: &Context) -> Option<WindowPlacement> {
    let zoom = ctx.zoom_factor();
    ctx.input(|i| {
        let viewport = i.viewport();
        let outer = viewport.outer_rect.filter(|rect| rect.is_finite() && rect.is_positive())?;
        let inner = viewport.inner_rect.filter(|rect| rect.is_finite() && rect.is_positive())?;
        let scale = viewport.native_pixels_per_point.unwrap_or(1.0) * zoom;
        let physical = (outer.center().to_vec2() * scale).to_pos2();
        let display = windows.displays.iter().find(|display| display.bounds.contains(physical));
        let origin = display.map_or(egui::Vec2::ZERO, |display| display.bounds.min.to_vec2() / display.scale);
        Some(WindowPlacement {
            display: display.map(|display| display.name.clone()),
            x: outer.left() - origin.x,
            y: outer.top() - origin.y,
            w: inner.width(),
            h: inner.height(),
            maximised: viewport.maximized.unwrap_or_default(),
            fullscreen: viewport.fullscreen.unwrap_or_default(),
        })
    })
}

fn send_placement(windows: &Windows, ctx: &Context, p: &WindowPlacement) {
    let origin = windows
        .displays
        .iter()
        .find(|display| p.display.as_deref() == Some(display.name.as_str()))
        .map_or(egui::Vec2::ZERO, |display| display.bounds.min.to_vec2() / display.scale);
    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(false));
    ctx.send_viewport_cmd(ViewportCommand::OuterPosition((origin + egui::vec2(p.x, p.y)).to_pos2()));
    ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(p.w, p.h)));
    if p.maximised {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
    }
    if p.fullscreen {
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
    }
}

pub fn prepare(app: &mut SolveApp, ctx: &Context) {
    app.view_windows.native = !ctx.embed_viewports();
    app.view_windows.embedded = ctx.embed_viewports();
    let render_open = detached_views(app).any(|(_, view)| view.kind == solidcraft_views::ViewKind::Render);
    app.view_windows.render_open = render_open;
    if ctx.input(|input| input.viewport().focused == Some(true)) {
        app.view_windows.last_focus = Some((app.docs.current_tab(), 0));
    }
    let zoom = ctx.zoom_factor();
    (app.view_windows.root_rect, app.view_windows.monitor_size, app.view_windows.root_scale) =
        ctx.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size, i.viewport().native_pixels_per_point.map(|scale| scale * zoom)));
    if app.view_windows.pending_root_placement.as_ref().is_some_and(|(tab, _)| *tab == app.docs.current_tab())
        && let Some((_, placement)) = app.view_windows.pending_root_placement.take()
    {
        if app.view_windows.native {
            send_placement(&app.view_windows, ctx, &placement);
        }
        app.view_windows.root_placement = Some(placement);
    } else {
        app.view_windows.root_placement = measured_placement(&app.view_windows, ctx);
    }
    if app.view_windows.focus == Some((app.docs.current_tab(), 0)) {
        restore_and_focus(ctx);
        app.view_windows.focus = None;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn refresh_displays(app: &mut SolveApp, monitors: impl FnOnce() -> Vec<crate::panel_runtime::NativeMonitor>) {
    let now = std::time::Instant::now();
    if app.view_windows.displays_updated.is_some_and(|last| now.duration_since(last).as_secs() < 1) {
        return;
    }
    app.view_windows.displays_updated = Some(now);
    let monitors = monitors();
    app.view_windows.displays = monitors
        .iter()
        .enumerate()
        .filter(|(_, monitor)| monitor.bounds.is_positive() && monitor.bounds.is_finite() && (0.1..=16.0).contains(&monitor.scale))
        .map(|(index, monitor)| Display {
            name: monitor.name.clone().unwrap_or_else(|| (index + 1).to_string()),
            bounds: monitor.bounds,
            scale: monitor.scale,
            primary: monitor.primary,
        })
        .collect();
}

fn toast(app: &mut SolveApp, ctx: &Context) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let Some(returned) = app.view_windows.returned.as_ref() else { return };
    if app.now >= returned.expires || !app.docs.tab_open(returned.tab) {
        app.view_windows.returned = None;
        return;
    }
    let count = returned.window.views.len().to_string();
    let label = solidcraft_ui_text::format(
        if count == "1" { "multi_window.returned.one" } else { "multi_window.returned.other" },
        &[("n", &count), ("design", &returned.design)],
    );
    let edge = Container::new(ctx, Surface::ViewportStatus)
        .area(egui::Id::unique("returned_design_views"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -64.0));
    let undo = edge
        .show(ctx, |ui| {
            Menu::show(ui, 280.0, |ui| {
                ui.horizontal(|ui| {
                    ui.label(label);
                    crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Undo"), crate::ui_kit::Button::Normal).clicked()
                })
                .inner
            })
        })
        .inner;
    if undo && let Some(returned) = app.view_windows.returned.take() {
        let root = app.docs.current_tab();
        if documents::select_tab(app, returned.tab) {
            let id = returned.window.id;
            documents::park_active_look(app);
            if app.docs.views.restore_window(returned.window) {
                app.view_windows.focus = Some((returned.tab, id));
            }
            documents::show_active_look(app);
            documents::select_tab(app, root);
        }
    }
    if let Some(returned) = &app.view_windows.returned {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((returned.expires - app.now).max(0.0)));
    }
}

fn window_menu(app: &mut SolveApp, ui: &mut Ui, id: WindowId, anchor: egui::Response, context_open: bool) {
    use crate::ui_kit::window_options_menu::{self as kit, Action, CardHeaders, DisplayChoice, WindowMenu};
    let options = child_options(app);
    let card_headers = match options.card_headers {
        solidcraft_views::CardHeaders::Always => CardHeaders::Always,
        solidcraft_views::CardHeaders::Hover => CardHeaders::Hover,
        solidcraft_views::CardHeaders::Hidden => CardHeaders::Hidden,
    };
    let current = app.view_windows.choices.iter().find(|choice| choice.id == id).and_then(|choice| choice.display.as_deref());
    let labels: Vec<_> = app
        .view_windows
        .displays
        .iter()
        .enumerate()
        .map(|(index, _)| solidcraft_ui_text::format("window_menu.display_number", &[("n", &(index + 1).to_string())]))
        .collect();
    let displays: Vec<_> = app
        .view_windows
        .displays
        .iter()
        .zip(&labels)
        .enumerate()
        .map(|(id, (display, name))| DisplayChoice { id, name, current: current == Some(display.name.as_str()) })
        .collect();
    let title = window_name(app, id);
    let interface_sizes = solidcraft_views::InterfaceSize::ALL.map(u16::from);
    let tr = solidcraft_ui_text::tr;
    let text = kit::WindowText {
        preview: tr("window_menu.preview").to_owned(),
        show_here: tr("window_menu.show_here").to_owned(),
        window: tr("window_menu.window").to_owned(),
        keep_on_top: tr("window_options.keep_on_top").to_owned(),
        ribbon: tr("window_ribbon.ribbon").to_owned(),
        auto_hide: tr("window_ribbon.auto_hide").to_owned(),
        auto_hide_unavailable: tr("window_ribbon.auto_hide_unavailable").to_owned(),
        other_panels: tr("window_panels.other_panels").to_owned(),
        workspace_panels: tr("window_panels.workspace_panels").to_owned(),
        top_bar: tr("window_topbar.top_bar").to_owned(),
        always: tr("window_options.always").to_owned(),
        edge: tr("window_topbar.edge").to_owned(),
        card_headers: tr("window_options.card_headers").to_owned(),
        on_hover: tr("window_options.on_hover").to_owned(),
        hidden: tr("window_options.hidden").to_owned(),
        move_to_display: tr("window_menu.move_to_display").to_owned(),
        interface_size: tr("window_size.interface_size").to_owned(),
        rename: tr("window_menu.rename").to_owned(),
        use_default: tr("window_menu.use_default").to_owned(),
        return_views: tr("multi_window.return_views").to_owned(),
        close_window: tr("multi_window.close_window").to_owned(),
    };
    let menu = WindowMenu {
        text: &text,
        title: &title,
        displays: &displays,
        card_headers,
        keep_on_top: options.keep_on_top,
        interface_sizes: &interface_sizes,
        interface_size: options.interface_size.into(),
        panels: options.panels,
        top_bar: if options.top_bar == TopBar::Always { kit::TopBar::Always } else { kit::TopBar::Edge },
        ribbon: options.ribbon,
        ribbon_auto_hide: options.ribbon_auto_hide,
    };
    let Some(action) = kit::show(ui, anchor, context_open, &menu) else { return };
    let (command, request) = match action {
        Action::CardHeaders(headers) => {
            let value = match headers {
                CardHeaders::Always => "always",
                CardHeaders::Hover => "hover",
                CardHeaders::Hidden => "hidden",
            };
            ("tab.window.set_options", json!({"window": id, "card_headers": value}))
        }
        Action::KeepOnTop(on) => ("tab.window.set_options", json!({"window": id, "keep_on_top": on})),
        Action::InterfaceSize(percent) => ("tab.window.set_options", json!({"window": id, "interface_size": percent})),
        Action::Panels(on) => ("tab.window.set_options", json!({"window": id, "panels": on})),
        Action::TopBar(mode) => {
            let top_bar = if mode == kit::TopBar::Always { TopBar::Always } else { TopBar::Edge };
            ("tab.window.set_options", json!({"window": id, "top_bar": top_bar}))
        }
        Action::Ribbon(on) => ("tab.window.set_options", json!({"window": id, "ribbon": on})),
        Action::RibbonAutoHide(on) => ("tab.window.set_options", json!({"window": id, "ribbon_auto_hide": on})),
        Action::MoveToDisplay(display) => ("tab.window.move_to_display", json!({"window": id, "display": display})),
        Action::UseAsDefault => ("window.options.use_as_default", json!({"window": id})),
        Action::ReturnViews | Action::Close => ("tab.window.close", json!({"window": id})),
        Action::Rename => {
            start_window_rename(app, ui.ctx(), id);
            return;
        }
    };
    if let Err(error) = app.run(command, request) {
        app.set_status(error, true);
    }
}

fn window_rename_id(ctx: &Context) -> egui::Id {
    egui::Id::unique(("view_window_rename", ctx.viewport_id()))
}

fn start_window_rename(app: &SolveApp, ctx: &Context, id: WindowId) {
    let key = window_rename_id(ctx);
    let name = assigned_window_name(app, id).unwrap_or_default().to_owned();
    ctx.data_mut(|data| data.insert_temp(key, (name, true)));
}

fn show_window_rename(app: &mut SolveApp, ctx: &Context, id: WindowId) {
    let key = window_rename_id(ctx);
    let Some((mut name, first)) = ctx.data(|data| data.get_temp::<(String, bool)>(key)) else { return };
    let modal = Container::new(ctx, Surface::Window).modal(key).show(ctx, |ui| {
        ui.set_width(280.0);
        ui.heading(app.tr("window_menu.rename"));
        let (keys, field) = crate::ui_kit::text_field_reserving(ui, &mut name, app.tr("window_menu.name"), 0.0);
        if let Some((cut, _)) = name.char_indices().nth(solidcraft_views::MAX_NAME) {
            name.truncate(cut);
        }
        if first {
            field.request_focus();
        }
        ui.horizontal(|ui| {
            let cancel = crate::ui_kit::button(ui, app.tr("ui.cancel"), crate::ui_kit::Button::Normal).clicked();
            let apply = crate::ui_kit::button(ui, app.tr("ui.apply"), crate::ui_kit::Button::Enabled(!name.trim().is_empty())).clicked();
            (cancel, apply || keys && !name.trim().is_empty())
        })
        .inner
    });
    let (cancel, submit) = modal.inner;
    let saved = if submit {
        match app.run("tab.window.rename", json!({"window": id, "name": name})) {
            Ok(_) => true,
            Err(error) => {
                app.set_status(error, true);
                false
            }
        }
    } else {
        false
    };
    let closed = modal.should_close() || cancel || saved;
    ctx.data_mut(|data| {
        if closed {
            data.remove::<(String, bool)>(key);
        } else {
            data.insert_temp(key, (name, false));
        }
    });
}

struct PanelContext<'a> {
    workspace: Option<&'a solidcraft_engine::panels::WorkspaceId>,
    tab: &'a str,
    layout: &'a Option<Value>,
}

const GRIP_SIZE: egui::Vec2 = egui::vec2(68.0, 6.0);
const REVEAL_EDGE: f32 = 6.0;

#[derive(Default)]
struct Chrome {
    edge: bool,
    bar_fraction: f32,
    stack_height: f32,
    bar_focus: bool,
    grip_menu: bool,
    ribbon: (f32, bool),
}

impl Chrome {
    fn slide(&self) -> f32 {
        self.stack_height * (1.0 - self.bar_fraction)
    }
}

fn bar_layer(ctx: &Context) -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(("pop_out_top_bar", ctx.viewport_id())))
}

fn bar_menu_id(ctx: &Context) -> egui::Id {
    bar_layer(ctx).id.with("window_options_anchor")
}

fn bar_grip_id(ctx: &Context) -> egui::Id {
    bar_layer(ctx).id.with("grip")
}

fn bar_grip(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(rect.center().x - GRIP_SIZE.x * 0.5, rect.top() + 7.0), GRIP_SIZE)
}

fn near_top_bar(canvas: egui::Rect, at: egui::Pos2, visible_height: f32) -> bool {
    (canvas.contains(at) && at.y <= canvas.top() + visible_height.max(REVEAL_EDGE)) || bar_grip(canvas).contains(at)
}

fn surrender_chrome_focus(ctx: &Context) {
    ctx.memory_mut(|memory| {
        if let Some(focus) = memory.focused() {
            memory.surrender_focus(focus);
        }
    });
}

fn schedule_reveal(ctx: &Context, reveal: crate::ui_kit::reveal::State, now: f64) {
    if let Some(delay) = reveal.repaint_after(now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
    }
}

fn bar_action(app: &mut SolveApp, ctx: &Context, action: Option<crate::window_chrome::WindowAction>) {
    if let Some(action) = action {
        if action == crate::window_chrome::WindowAction::Close {
            app.view_windows.close_current();
        } else {
            action.send(ctx);
        }
    }
}

fn window_bar(app: &mut SolveApp, ui: &mut Ui, id: WindowId, rect: egui::Rect, chrome: &Chrome) {
    let tokens = solidcraft_theme::Tokens::get();
    let titlebar = crate::window_chrome::TitleBar { traffic_lights: app.integrated_titlebar, caption_buttons: app.custom_titlebar };
    let layout = titlebar.layout(rect);
    if let Some(drag) = layout.drag {
        crate::window_chrome::drag_area(ui, drag);
    }
    let content = layout.content;
    let brand_end = crate::app_bar::paint_brand(ui.painter(), content, &solidcraft_look::glyphs::brand_mark, Some("SolidCraft"));
    let menu = egui::Rect::from_center_size(egui::pos2(content.right() - 17.0, content.center().y), egui::Vec2::splat(26.0));
    let menu_tip = app.tr("multi_window.window_menu");
    let response = ui.interact(menu, bar_menu_id(ui.ctx()), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, menu_tip));
    if chrome.bar_focus && ui.is_visible() && !ui.is_sizing_pass() {
        response.request_focus();
    }
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(menu, tokens.radius_sm, tokens.app_bar_hover_button);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(menu, tokens.radius_sm, egui::Stroke::new(1.0, tokens.app_bar_text), egui::StrokeKind::Inside);
    }
    for x in [-4.0, 0.0, 4.0] {
        ui.painter().circle_filled(menu.center() + egui::vec2(x, 0.0), 1.25, tokens.app_bar_text);
    }
    let background = ui.ctx().viewport(|viewport| viewport.hits.click.is_none_or(|hit| hit.id == egui::Id::unique("titlebar-drag")));
    let context_open = ui.input(|input| {
        input.pointer.secondary_clicked()
            && input.pointer.interact_pos().is_some_and(|at| content.contains(at) && (background || at.x <= brand_end || menu.contains(at)))
    });
    if response.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
        start_window_rename(app, ui.ctx(), id);
    }
    window_menu(app, ui, id, response.backing_on_hover_text(menu_tip), context_open || chrome.grip_menu);
    let mut left = brand_end + 12.0;
    let right = menu.left() - 6.0;
    let font = crate::ui_kit::type_face::semibold(tokens.font_small);
    let name = ui.painter().layout_no_wrap(window_number(app, id).to_string(), font.clone(), tokens.app_bar_text);
    let icon_width = 16.0 + tokens.space_6;
    let width = icon_width + name.size().x;
    if right - left >= width {
        let icon = egui::Rect::from_center_size(egui::pos2(left + 8.0, content.center().y), egui::Vec2::splat(16.0));
        crate::ui_kit::icons::paint_ink(ui.painter(), icon, crate::ui_kit::icons::window_icon(true), tokens.app_bar_text);
        let at = egui::pos2(icon.right() + tokens.space_6, content.center().y - name.size().y / 2.0);
        ui.painter().galley(at, name, tokens.app_bar_text);
        left += width + tokens.space_12;
        if let Some(name) = assigned_window_name(app, id) {
            let ink = tokens.app_bar_text.gamma_multiply(0.63);
            let name = ui.painter().layout_no_wrap(name.to_owned(), font, ink);
            if right - left >= name.size().x + tokens.space_12 + documents::preferred_tab_width(ui.ctx()) {
                ui.painter().galley(egui::pos2(left, content.center().y - name.size().y / 2.0), name.clone(), ink);
                left += name.size().x + tokens.space_12;
            }
        }
    }
    if right > left {
        documents::tabs(app, ui, rect, left, right);
    }

    if app.custom_titlebar {
        let action = crate::window_chrome::caption_buttons(ui, rect);
        bar_action(app, ui.ctx(), action);
    }
}

fn overlay_bar(app: &mut SolveApp, ui: &mut Ui, id: WindowId, canvas: egui::Rect, chrome: &Chrome) {
    use egui::{Rect, pos2, vec2};
    let t = solidcraft_theme::Tokens::get();
    let layer = bar_layer(ui.ctx());
    let slide = chrome.slide();
    let fixed = Rect::from_min_size(canvas.min, vec2(canvas.width(), crate::app_bar::HEIGHT));
    let rect = fixed.translate(vec2(0.0, -slide));
    let clip = canvas;
    let bar = rect;
    let visible = bar.intersect(canvas);
    let grip = bar_grip(canvas);
    let grip_visible = visible.bottom() < grip.bottom();
    let host = Container::new(ui.ctx(), Surface::ApplicationBar);
    app.shell.top_bar_overlays = [
        visible.is_positive().then_some(visible),
        grip_visible.then_some(grip),
        app.custom_titlebar
            .then(|| Rect::from_min_max(pos2(rect.right() - crate::window_chrome::WIDTH, rect.top()), rect.right_bottom()).intersect(canvas))
            .filter(|rect| rect.is_positive()),
    ];
    if visible.is_positive() {
        host.area(layer.id).order(layer.order).fixed_pos(visible.min).constrain(false).show(ui.ctx(), |ui| {
            ui.set_min_size(visible.size());
            ui.set_clip_rect(ui.clip_rect().intersect(clip));
            let frame = crate::ui_kit::Menu::frame().fill(t.app_bar).inner_margin(0.0).outer_margin(0.0).corner_radius(0);
            Container::paint_frame(ui.painter(), frame, bar);
            window_bar(app, ui, id, rect, chrome);
        });
    }
    if !visible.is_positive() {
        crate::view_window_drag::hover(app, ui.ctx(), None);
    }
    if grip_visible {
        let grip_id = bar_grip_id(ui.ctx());
        host.area(layer.id.with("grip_area")).order(layer.order).fixed_pos(grip.min).constrain(false).show(ui.ctx(), |ui| {
            ui.set_min_size(grip.size());
            let response = ui.interact(grip, grip_id, egui::Sense::click());
            let label = app.tr("window_topbar.show");
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            ui.painter().rect_filled(grip, 5, t.text_dim.gamma_multiply(0.5));
            let response = response.backing_on_hover_text(app.tr("window_topbar.show_help"));
            if response.clicked() {
                response.request_focus();
            }
            crate::scenario::publish_rect("pop_out:top_bar_grip", grip);
        });
    }
    if visible.is_positive() {
        crate::scenario::publish_rect("pop_out:top_bar", visible);
    }
}

fn window_content(app: &mut SolveApp, ui: &mut Ui, id: WindowId, chrome: Chrome, had_dialog: bool, panel_context: PanelContext<'_>) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    if is_clean(app) {
        let tab = app.docs.current_tab();
        if app.view_windows.child_tool_owner(tab) == Some(id) {
            app.view_windows.tool_owner.remove(&tab);
        }
        app.shell.ribbon_overlay = None;
        app.shell.top_bar_overlays = [None; 3];
        show_clean(app, ui);
        return;
    }
    if chrome.edge {
        let canvas = ui.available_rect_before_wrap();
        overlay_bar(app, ui, id, canvas, &chrome);
    } else {
        app.shell.top_bar_overlays = [None; 3];
        let tokens = solidcraft_theme::Tokens::get();
        let container = Container::new(ui.ctx(), Surface::ApplicationBar);
        let edge = container.edge("sc_view_window_bar", Edge::Top);
        let frame = container.edge_frame(tokens.app_bar);
        container.show_edge(ui, edge.exact_size(crate::app_bar::HEIGHT).show_separator_line(false), frame, |ui| {
            window_bar(app, ui, id, ui.max_rect(), &chrome);
        });
    }
    let rect = ui.available_rect_before_wrap();
    app.shell.content = Some(rect);
    if child_options(app).ribbon {
        let reveal_top = chrome.edge.then(|| {
            let bar_bottom = rect.top() + crate::app_bar::HEIGHT * chrome.bar_fraction;
            bar_bottom.max(bar_grip(rect).bottom() + 4.0).min(rect.bottom())
        });
        crate::toolbar::pop_out_ribbon(app, ui, rect, chrome.ribbon.0, chrome.ribbon.1, reveal_top);
    } else {
        app.shell.ribbon_overlay = None;
    }
    crate::ui_kit::Container::begin_host_windows(ui.ctx());
    app.shell.browser = None;
    app.shell.history = None;
    app.shell.workspace_panels.clear();
    app.viewport.palette_rect = None;
    let panels = if child_options(app).panels {
        let tab = std::mem::replace(&mut app.ui.tab, panel_context.tab.into());
        let owner = format!("solidcraft.panels.{}.{}", app.docs.current_tab(), id);
        let prepared = crate::panel_runtime::prepare_pop_out_panels(app, ui, rect, panel_context.workspace, &owner, panel_context.layout);
        app.ui.tab = tab;
        prepared
    } else {
        crate::panel_runtime::hide_app_host(app);
        false
    };
    let mut canvas = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    canvas.set_clip_rect(canvas.clip_rect().intersect(rect));
    app.show_workspace(&mut canvas);
    if panels {
        let tab = std::mem::replace(&mut app.ui.tab, panel_context.tab.into());
        crate::panel_runtime::paint_pop_out_panels(app, &mut canvas);
        app.ui.tab = tab;
    }
    if let Some(rect) = app.viewport.rect.filter(|r| r.is_positive() && r.is_finite()) {
        if crate::view_tiles::shown(app) {
            crate::view_tiles::card_controls(app, &mut canvas);
        }
        let mut controls = crate::view_tiles::controls_ui(&mut canvas, app.session.uid, app.docs.views.active().id, rect);
        crate::render_workspace::host_result_overlays(app, &mut controls);
        crate::viewport::show_host_controls(app, &mut controls, rect);
    }
    crate::view_tiles::focus_pressed(app);
    crate::sketch_view::show_notice(app, ui);
    crate::view_tiles::placement_overlay(app, ui.ctx());
    app.resolve_fit(ui.ctx());
    documents::sync_linked_active(app);
    crate::shortcut_box::show(app, ui.ctx());
    let tab = app.docs.current_tab();
    if !had_dialog && app.dialog.is_some() {
        app.view_windows.start_tool_panel(tab);
    }
    if app.view_windows.child_tool_owner(tab) == Some(id) {
        crate::view_confirm::show(app, ui.ctx());
        crate::dialogs::show(app, ui.ctx());
        if app.dialog.is_none() {
            app.view_windows.tool_owner.remove(&tab);
        }
    }
    show_window_rename(app, ui.ctx(), id);
    crate::context_menu::show(app, ui.ctx());
    if panels {
        let tab = std::mem::replace(&mut app.ui.tab, panel_context.tab.into());
        crate::panel_runtime::finish_app_host(app, &mut canvas);
        app.ui.tab = tab;
    }
    documents::sync_window_title(app, ui.ctx());
    if app.custom_titlebar {
        crate::titlebar::resize_zones(ui);
    }
}

impl Host {
    fn new(tab: u64, id: WindowId) -> Self {
        let mut viewport = crate::viewport::ViewportState::default();
        viewport.isolate_window_scenes();
        Self {
            tab,
            id,
            viewport,
            shell: Default::default(),
            title: Default::default(),
            layout_fill: Default::default(),
            view_rename: None,
            view_placement: None,
            menu: Default::default(),
            preview: Default::default(),
            panels: Default::default(),
            ribbon_reveal: Default::default(),
            ribbon_focus_pending: false,
            top_bar_reveal: Default::default(),
            top_bar_focus_pending: false,
            ribbon_tab: String::new(),
            ribbon_view_workspace: "",
            ribbon_context: None,
            ribbon_popup: None,
            sketch_return_tab: None,
            ready: false,
            startup: None,
        }
    }

    fn chrome(&mut self, app: &SolveApp, ctx: &Context, canvas: egui::Rect) -> Chrome {
        use crate::ui_kit::reveal::{Input, reveal_step};
        let options = child_options(app);
        if options.top_bar == TopBar::Always || is_clean(app) {
            self.top_bar_reveal = Default::default();
            self.top_bar_focus_pending = false;
            return Chrome { ribbon: self.ribbon(app, ctx), ..Chrome::default() };
        }

        let bar_height = crate::app_bar::HEIGHT;
        let stack_height = bar_height;
        let (pointer, window_focused, now) = ctx.input(|i| (i.pointer.hover_pos(), i.viewport().focused.unwrap_or(i.focused), i.time));
        let f10 = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F10));
        let reduced_motion = solidcraft_theme::panel_motion_duration(ctx, false) == 0.0;
        let own_popup = crate::ui_kit::window_options_menu::is_open(ctx, bar_menu_id(ctx))
            || crate::app_bar::tab_popup_open(ctx, documents::tab_strip_id(ctx.viewport_id()));
        let ribbon_popup = ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open")).is_some());
        let popup = own_popup || ribbon_popup || ctx.any_popup_open() || app.viewport.context_menu.is_some();
        let focus = ctx.memory(|memory| memory.focused());
        let grip_focused = focus == Some(bar_grip_id(ctx));
        let bar_focused = window_focused
            && (focus == Some(bar_menu_id(ctx))
                || focus.and_then(|id| ctx.read_response(id)).is_some_and(|response| response.layer_id == bar_layer(ctx)));
        let visible_height = stack_height * self.top_bar_reveal.fraction(now);
        let grip = bar_grip(canvas);
        let grip_menu = visible_height < (stack_height - bar_height) + grip.bottom() - canvas.top()
            && ctx.input(|i| i.pointer.secondary_clicked() && i.pointer.interact_pos().is_some_and(|at| grip.contains(at)));
        let hovered = pointer.is_some_and(|at| near_top_bar(canvas, at, visible_height));
        let keyboard_show = f10 || (window_focused && grip_focused);
        let held = own_popup
            || grip_menu
            || (visible_height > 0.0 && ctx.any_popup_open())
            || (visible_height > 0.0 && crate::view_window_drag::active(app));
        if !popup && !bar_focused {
            self.top_bar_reveal.release_keyboard();
        }
        let escape = !popup
            && (visible_height > 0.0 || self.top_bar_reveal.keyboard())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        self.top_bar_focus_pending = keyboard_show || (self.top_bar_focus_pending && window_focused && !bar_focused && !escape);
        if escape && (bar_focused || grip_focused) {
            surrender_chrome_focus(ctx);
        }
        self.top_bar_reveal = reveal_step(
            self.top_bar_reveal,
            Input {
                hovered,
                focused: bar_focused,
                held,
                keyboard_show,
                escape,
                dismiss_on_escape: true,
                reduced_motion: reduced_motion || keyboard_show || grip_menu,
                ..Input::default()
            },
            now,
        );
        schedule_reveal(ctx, self.top_bar_reveal, now);
        let bar_fraction = self.top_bar_reveal.fraction(now);
        Chrome { edge: true, bar_fraction, stack_height, bar_focus: self.top_bar_focus_pending, grip_menu, ribbon: self.ribbon(app, ctx) }
    }

    fn ribbon(&mut self, app: &SolveApp, ctx: &Context) -> (f32, bool) {
        use crate::ui_kit::reveal::{Input, reveal_step};
        let options = child_options(app);
        if !options.ribbon || is_clean(app) {
            self.ribbon_reveal = Default::default();
            self.ribbon_focus_pending = false;
            ctx.data_mut(|data| data.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
            return (0.0, false);
        }
        let (hovered, focused, now) = ctx.input(|i| (i.pointer.hover_pos().is_some(), i.viewport().focused.unwrap_or(i.focused), i.time));
        let keyboard_show = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F10));
        let ribbon_focused = crate::toolbar::pop_out_ribbon_focus(ctx);
        let panel_open = ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open")).is_some());
        let popup_open = ctx.any_popup_open() || app.viewport.context_menu.is_some() || panel_open;
        let escape = !popup_open
            && (self.ribbon_reveal.keyboard() || ribbon_focused)
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        self.ribbon_focus_pending = keyboard_show || (self.ribbon_focus_pending && focused && !ribbon_focused && !escape);
        if escape || (!focused && ribbon_focused) {
            ctx.memory_mut(|memory| {
                if let Some(focus) = memory.focused() {
                    memory.surrender_focus(focus);
                }
            });
        }
        let held = focused && (popup_open || ctx.dragged_id().is_some());
        let reduced_motion = keyboard_show || solidcraft_theme::panel_motion_duration(ctx, false) == 0.0;
        self.ribbon_reveal = reveal_step(
            self.ribbon_reveal,
            Input { hovered, focused, held, keyboard_show, escape: escape || !focused, reduced_motion, hide_delay: Some(0.12), ..Input::default() },
            now,
        );
        if options.ribbon_auto_hide {
            if let Some(delay) = self.ribbon_reveal.repaint_after(now) {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
            }
            (self.ribbon_reveal.fraction(now), self.ribbon_focus_pending)
        } else {
            (1.0, self.ribbon_focus_pending)
        }
    }

    fn swap_ribbon_popup(&mut self, ctx: &Context) {
        // egui's temporary widget data is shared across viewports; the ribbon drop-down is
        // part of this window's interaction, even though the controls act on the same design.
        ctx.data_mut(|data| {
            let key = egui::Id::unique("sc_panel_open");
            let previous = data.get_temp::<egui::Id>(key);
            data.remove::<egui::Id>(key);
            if let Some(open) = self.ribbon_popup.take() {
                data.insert_temp(key, open);
            }
            self.ribbon_popup = previous;
        });
    }

    fn swap(&mut self, app: &mut SolveApp) {
        self.viewport.gpu = app.viewport.gpu;
        self.viewport.gpu_unavailable = app.viewport.gpu_unavailable;
        std::mem::swap(&mut app.viewport, &mut self.viewport);
        std::mem::swap(&mut app.shell, &mut self.shell);
        std::mem::swap(&mut app.window_title, &mut self.title);
        std::mem::swap(&mut app.docs.layout_fill, &mut self.layout_fill);
        std::mem::swap(&mut app.docs.view_rename, &mut self.view_rename);
        std::mem::swap(&mut app.docs.view_placement, &mut self.view_placement);
        std::mem::swap(&mut app.menu, &mut self.menu);
        std::mem::swap(&mut app.preview, &mut self.preview);
    }
}

/// Called only after every child has restored its temporary root app state.
/// Hidden hosts retain their pictures; removed ownership releases the old mapping.
pub(crate) fn retain_picture_slots(app: &mut SolveApp) -> Vec<u64> {
    let owners = documents::view_picture_window_owners(app);
    let mut slots = Vec::new();
    for host in &mut app.view_windows.hosts {
        let owned = owners.get(&(host.tab, host.id)).map(Vec::as_slice);
        slots.extend(host.viewport.retain_window_scene_slots(owned));
    }
    slots
}

pub fn show(app: &mut SolveApp, ctx: &Context, render_state: Option<&egui_wgpu::RenderState>) {
    if !app.view_windows.native || app.view_windows.is_child() || app.quit_requested {
        return;
    }
    toast(app, ctx);
    let open_windows = documents::view_window_ids(app);
    app.view_windows.tool_owner.retain(|tab, id| open_windows.contains(&(*tab, *id)));
    app.view_windows.clean.retain(|(tab, id)| if *id == 0 { app.docs.tab_open(*tab) } else { open_windows.contains(&(*tab, *id)) });
    let root = app.docs.active_tab();
    let root_home = app.home.open;
    let mut old = std::mem::take(&mut app.view_windows.hosts);
    let mut retired = Vec::new();
    let mut returned_focus_tab = None;
    for (tab, id) in open_windows {
        let mut host = old.iter().position(|host| (host.tab, host.id) == (tab, id)).map_or_else(|| Host::new(tab, id), |index| old.remove(index));
        if !documents::select_tab(app, tab) {
            continue;
        }
        app.view_windows.choices = collect_choices(app);
        app.view_windows.setup_snapshot =
            (!app.preferences.view_window_setups.is_empty()).then(|| crate::view_setups::current_snapshot(app)).flatten();
        let panel_workspace = crate::panel_runtime::panel_workspace(app);
        let Some(mut window) = app.docs.views.take_window(id) else { continue };
        let os_id = viewport_id(tab, id);
        let custom = !cfg!(target_os = "macos") || window.options.top_bar == TopBar::Edge;
        let level = if window.options.keep_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
        let mut builder = crate::window_chrome::viewport("SolidCraft", solidcraft_engine::io::APP_ID, custom)
            .with_min_inner_size([640.0, 420.0])
            .with_window_level(level);
        if cfg!(target_os = "macos") {
            builder = builder.with_titlebar_buttons_shown(!custom);
        }
        // The shared builder carries the main window's size. Only ask for a child size at startup,
        // so subsequent frames leave user resizing alone.
        builder.inner_size = None;
        if !host.ready {
            builder = builder.with_visible(false);
            if let Some(p) = &window.placement {
                let origin = app
                    .view_windows
                    .displays
                    .iter()
                    .find(|display| p.display.as_deref() == Some(display.name.as_str()))
                    .map_or(egui::Vec2::ZERO, |display| display.bounds.min.to_vec2() / display.scale);
                builder = builder
                    .with_inner_size([p.w, p.h])
                    .with_position((origin + egui::vec2(p.x, p.y)).to_pos2())
                    .with_maximized(p.maximised)
                    .with_fullscreen(p.fullscreen);
            }
            // An immediate viewport requires eframe to call its draw callback. The native
            // integration cannot call it when creating the OS window failed. Negotiate a hidden
            // viewport first, as the panel host does, then paint it once the OS has reported DPI.
            let created = ctx.input(|input| {
                input.raw.viewports.get(&os_id).and_then(|info| info.native_pixels_per_point).is_some_and(|scale| scale.is_finite() && scale > 0.0)
            });
            if !created {
                let start = *host.startup.get_or_insert(app.now);
                documents::put_view_window(app, tab, window);
                if app.now - start >= 2.0 {
                    let _ = close(app, id, false);
                    app.set_status(solidcraft_ui_text::format("multi_window.open_failed", &[("design", &app.session.doc.name)]), true);
                    ctx.send_viewport_cmd_to(os_id, ViewportCommand::Close);
                } else {
                    ctx.show_viewport_deferred(os_id, builder, |_, _| {});
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    app.view_windows.hosts.push(host);
                }
                continue;
            }
        }
        let mut refused = false;
        let mut close_request = None;
        let interface_scale = window.options.interface_size.factor();
        let draw = |ui: &mut egui::Ui, class| {
            if class == ViewportClass::EmbeddedWindow {
                refused = true;
                return;
            }
            if app.view_windows.pending_child_placements.remove(&(tab, id))
                && let Some(p) = window.placement.as_ref()
            {
                send_placement(&app.view_windows, ui.ctx(), p);
            } else if host.ready
                && let Some(p) = window.placement.as_mut()
            {
                let zoom = ui.ctx().zoom_factor();
                ui.input(|i| {
                    let viewport = i.viewport();
                    if let Some(rect) = viewport.outer_rect.filter(|r| r.is_positive() && r.is_finite()) {
                        let scale = viewport.native_pixels_per_point.unwrap_or(1.0) * zoom;
                        let physical = (rect.center().to_vec2() * scale).to_pos2();
                        let display = app.view_windows.displays.iter().find(|display| display.bounds.contains(physical));
                        let origin = display.map_or(egui::Vec2::ZERO, |display| display.bounds.min.to_vec2() / display.scale);
                        (p.x, p.y) = (rect.left() * interface_scale - origin.x, rect.top() * interface_scale - origin.y);
                        p.display = display.map(|display| display.name.clone());
                    }
                    if let Some(rect) = viewport.inner_rect.filter(|r| r.is_positive() && r.is_finite()) {
                        (p.w, p.h) = (rect.width() * interface_scale, rect.height() * interface_scale);
                    }
                    p.maximised = viewport.maximized.unwrap_or_default();
                    p.fullscreen = viewport.fullscreen.unwrap_or_default();
                });
            }
            if ui.input(|i| i.viewport().close_requested()) {
                close_request = Some(None);
                return;
            }
            if !host.ready {
                host.ready = true;
                if window.level == WindowLevel::Clean {
                    app.view_windows.clean.insert((tab, id));
                }
                if let Some(p) = &window.placement {
                    ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(p.w, p.h)));
                }
                ui.ctx().send_viewport_cmd(ViewportCommand::Visible(true));
            }
            if app.view_windows.focus == Some((tab, id)) {
                restore_and_focus(ui.ctx());
                app.view_windows.focus = None;
            }
            if ui.input(|input| input.viewport().focused == Some(true)) && app.view_windows.last_focus != Some((tab, id)) {
                app.view_windows.last_focus = Some((tab, id));
                if let Err(error) = crate::sketch_view::sync(app) {
                    app.set_status(error, true);
                }
            }
            let panel_tab = match app.view_windows.last_focus {
                Some((owner, focused)) if owner == tab && focused == id => documents::view_workspace(window.views.active().kind).into(),
                Some((owner, focused)) if owner == tab && focused != 0 => {
                    app.docs.views.views_of(focused).map_or_else(|| app.ui.tab.clone(), |views| documents::view_workspace(views.active().kind).into())
                }
                _ => app.ui.tab.clone(),
            };
            documents::park_active_look(app);
            app.view_windows.parked = Some((tab, app.docs.views.ids().collect()));
            let receiver_scope =
                (!ui.input(|input| input.viewport().focused == Some(true))).then(|| crate::drawing_workspace::ReceiverScope::capture(app));
            app.view_windows.foreign_views = available_strip_views(app).map(|(owner, view)| (owner, view.clone())).collect();
            std::mem::swap(&mut app.docs.views, &mut window.views);
            host.swap(app);
            let decorations = (app.custom_titlebar, app.integrated_titlebar);
            (app.custom_titlebar, app.integrated_titlebar) = (custom, !custom);
            let home = std::mem::replace(&mut app.home.open, false);
            let workspace = documents::view_workspace(app.docs.views.active().kind);
            let options = window.options;
            if host.ribbon_view_workspace != workspace {
                host.ribbon_view_workspace = workspace;
                host.ribbon_tab = workspace.into();
                host.ribbon_context = None;
            }
            let child_tab = if options.ribbon { host.ribbon_tab.clone() } else { workspace.into() };
            let ribbon = std::mem::replace(&mut app.ui.tab, child_tab);
            std::mem::swap(&mut app.ui.ribbon_context, &mut host.ribbon_context);
            std::mem::swap(&mut app.ui.sketch_return_tab, &mut host.sketch_return_tab);
            crate::drawing_workspace::swap_selection(app);
            crate::workspace::sync_history(app);
            if app.preferences.workspace_follows_view && app.docs.views.active().kind == ViewKind::Sheet {
                let view = app.docs.views.active().clone();
                let result = if receiver_scope.is_some() { crate::sheet_views::bind(app, &view) } else { crate::sheet_views::focus(app, &view) };
                if let Err(error) = result {
                    app.set_status(error, true);
                }
            }
            documents::show_active_look(app);
            app.view_windows.painting = Some(id);
            app.view_windows.painting_options = Some(window.options);
            let revision = app.session.revision;
            let had_dialog = app.dialog.is_some();
            host.swap_ribbon_popup(ui.ctx());
            let chrome = host.chrome(app, ui.ctx(), ui.available_rect_before_wrap());
            host.panels.load(&window.panel_layout);
            host.panels.swap(app);
            app.view_windows.painting_panels = true;
            if !options.panels || is_clean(app) {
                crate::panel_runtime::hide_app_host(app);
            }
            crate::workspace::sync_context(app);
            app.view_window_logic(ui.ctx());
            window_content(
                app,
                ui,
                id,
                chrome,
                had_dialog,
                PanelContext { workspace: panel_workspace.as_ref(), tab: &panel_tab, layout: &window.panel_layout },
            );
            if app.docs.current_tab() == tab {
                host.panels.save(app, &mut window.panel_layout);
            }
            app.view_windows.painting_panels = false;
            host.panels.swap(app);
            host.swap_ribbon_popup(ui.ctx());
            if crate::render_workspace::active(app) || crate::render_workspace::tiled(app) {
                crate::render_workspace::tick_live(app, ui.ctx());
            }
            close_request = app.view_windows.closing.take();
            let on_child_design = app.docs.current_tab() == tab;
            if on_child_design {
                documents::park_active_look(app);
                host.ribbon_tab = std::mem::replace(&mut app.ui.tab, ribbon);
                host.ribbon_view_workspace = documents::view_workspace(app.docs.views.active().kind);
            }
            std::mem::swap(&mut app.ui.ribbon_context, &mut host.ribbon_context);
            std::mem::swap(&mut app.ui.sketch_return_tab, &mut host.sketch_return_tab);
            crate::drawing_workspace::swap_selection(app);
            crate::workspace::sync_history(app);
            app.home.open = home;
            (app.custom_titlebar, app.integrated_titlebar) = decorations;
            app.view_windows.painting = None;
            if let Some(options) = app.view_windows.painting_options.take() {
                if options != window.options {
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
                window.options = options;
            }
            app.view_windows.parked = None;
            for (_, view) in &app.view_windows.foreign_views {
                if let Some(spec) = view.spec {
                    set_camera_in(&mut window.views, view.id, spec.camera);
                }
            }
            app.view_windows.foreign_views.clear();
            app.view_windows.setup_snapshot = None;
            window.level = if app.view_windows.clean.contains(&(tab, id)) {
                WindowLevel::Clean
            } else if window.level == WindowLevel::Clean {
                WindowLevel::ViewsOnly
            } else {
                window.level
            };
            host.swap(app);
            documents::exchange_window_views(app, tab, &mut window.views);
            crate::sketch_view::project(app);
            if let Some(scope) = receiver_scope {
                scope.restore(app);
            }
            if on_child_design {
                documents::show_active_look(app);
            }
            if revision != app.session.revision {
                ctx.request_repaint_of(ViewportId::ROOT);
            }
        };
        crate::view_window_scale::in_window(ctx, os_id, interface_scale, || ctx.show_viewport_immediate(os_id, builder, draw));
        if !app.docs.tab_open(tab) {
            ctx.send_viewport_cmd_to(os_id, ViewportCommand::Close);
            retired.extend(host.viewport.window_scene_slots());
            continue;
        }
        documents::put_view_window(app, tab, window);
        if let Some(pending) = app.view_windows.pending_action.take() {
            documents::select_tab(app, tab);
            let result = match pending {
                PendingAction::Move { view, target } => apply_move(app, view, target).map(|_| ()),
                PendingAction::Focus { view, window } => focus_view(app, view, Some(window)).and_then(|()| focus(app, window)),
                PendingAction::Close { view } => app.run("tab.view.close", json!({"id": view})).map(|_| ()),
                PendingAction::MoveToDisplay { window, display } => move_to_display(app, window, display),
                PendingAction::Bring { source, target } => apply_bring(app, source, target),
                PendingAction::Setup(action) => {
                    if let Err(reason) = action.run(app) {
                        let message = crate::view_setups_ui::message(app, &reason);
                        app.set_status(message, true);
                    }
                    Ok(())
                }
            };
            if let Err(reason) = result {
                crate::design_view_ui::show_move_error(app, reason);
            }
            ctx.request_repaint();
        }
        if app.view_windows.focus == Some((tab, 0)) {
            returned_focus_tab = Some(tab);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if refused || close_request.is_some() {
            documents::select_tab(app, tab);
            if let Some(Some(view)) = close_request {
                app.docs.views.close_in_window(id, view);
            } else {
                let _ = close(app, id, !refused);
            }
            if refused {
                app.set_status(solidcraft_ui_text::format("multi_window.open_failed", &[("design", &app.session.doc.name)]), true);
            }
            ctx.send_viewport_cmd_to(os_id, ViewportCommand::Close);
            retired.extend(host.viewport.window_scene_slots());
        } else if app.docs.views.window(id).is_some() || app.docs.current_tab() != tab {
            app.view_windows.hosts.push(host);
        } else {
            ctx.send_viewport_cmd_to(os_id, ViewportCommand::Close);
            retired.extend(host.viewport.window_scene_slots());
        }
    }
    let root_focus = returned_focus_tab;
    documents::select_tab(app, root_focus.unwrap_or(root));
    app.home.open = root_focus.is_none() && root_home;
    crate::view_window_drag::show_native_preview(app, ctx);
    for host in old {
        ctx.send_viewport_cmd_to(viewport_id(host.tab, host.id), ViewportCommand::Close);
        retired.extend(host.viewport.window_scene_slots());
    }
    if !retired.is_empty()
        && let Some(state) = render_state
        && let Some(renderer) = state.renderer.write().callback_resources.get_mut::<solidcraft_viewport_gpu::view::Renderer>()
    {
        renderer.discard_views(&retired);
    }
}
