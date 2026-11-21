//! Views-only hosts for the same live design. View ownership stays in the design's ViewSet.

use egui::{Context, Ui, ViewportClass, ViewportCommand, ViewportId};
use serde_json::{Value, json};
use solidcraft_views::{DesignView, PopOut, ViewSet, WindowId, WindowPlacement, WindowTarget};

use crate::ui_kit::{BackingTooltipExt as _, Container, Edge, Menu, Surface};
use crate::{SolveApp, documents};

#[derive(Default)]
pub(crate) struct Windows {
    native: bool,
    render_open: bool,
    embedded: bool,
    painting: Option<WindowId>,
    closing: Option<Option<u64>>,
    parked: Option<(u64, Vec<u64>)>,
    focus: Option<(u64, WindowId)>,
    last_focus: Option<(u64, WindowId)>,
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
    pending_action: Option<PendingAction>,
}

#[derive(Clone)]
pub(crate) struct WindowChoice {
    pub(crate) id: WindowId,
    pub(crate) label: String,
    pub(crate) name: String,
    pub(crate) number: u32,
    display: Option<String>,
}

enum PendingAction {
    Move { view: u64, target: WindowTarget },
    Focus { view: u64, window: WindowId },
    Close { view: u64 },
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

fn viewport_id(tab: u64, id: WindowId) -> ViewportId {
    ViewportId::from_hash_of(("solidcraft-view-window", tab, id))
}

impl Windows {
    pub(crate) fn current_window(&self) -> WindowId {
        self.painting.unwrap_or_default()
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

pub(crate) fn strip_views(app: &SolveApp) -> Vec<(WindowId, &DesignView)> {
    let order = app.view_windows.strip_order.get(&app.docs.current_tab());
    let mut views: Vec<_> = available_strip_views(app).collect();
    if app.preferences.clear_view_tabs
        && let Some(order) = order
    {
        views.sort_by_key(|(_, view)| order.iter().position(|id| *id == view.id).unwrap_or(usize::MAX));
    }
    views
}

pub(crate) fn window_name(app: &SolveApp, id: WindowId) -> String {
    let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
    if id == 0 {
        locale.text("view_tab.main_window").to_owned()
    } else {
        locale.format("view_tab.window_number", &[("n", &id.saturating_add(1).to_string())])
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
    if app.view_windows.painting != Some(id) && app.docs.views.views_of(id).is_none() {
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
    let label = |name: &str, display: Option<&str>| {
        let number = displays.iter().position(|entry| display == Some(entry.name.as_str()));
        if let Some(index) = number {
            let display = solidcraft_ui_text::format("multi_window.display_number", &[("n", &(index + 1).to_string())]);
            format!("{name} · {display}")
        } else {
            name.to_owned()
        }
    };
    let root_display = app.view_windows.root_rect.zip(app.view_windows.root_scale).and_then(|(rect, scale)| {
        let position = (rect.center().to_vec2() * scale).to_pos2();
        displays.iter().find(|display| display.bounds.contains(position))
    });
    let choice = |id: WindowId, fallback: &str, display: Option<String>, label_display: Option<&str>| {
        let name = if app.preferences.clear_view_tabs { window_name(app, id) } else { fallback.to_owned() };
        WindowChoice { id, label: label(&name, label_display), name, number: id.saturating_add(1), display }
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

fn apply_move(app: &mut SolveApp, view: u64, target: WindowTarget) -> Result<Value, String> {
    remember_strip_order(app);
    let tab = app.docs.active_tab();
    let indexed = matches!(target, WindowTarget::Existing { index: Some(_), .. });
    let moved = documents::move_view_to_window(app, view, target).map_err(|error| error.code().to_owned())?;
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
        return action(app);
    }
    if local != 0 {
        return Err("no_such_window".into());
    }
    let mut window = app.docs.views.take_window(target).ok_or("no_such_window")?;
    let host_index = app.view_windows.hosts.iter().position(|host| host.tab == tab && host.id == target);
    let mut host = host_index.map_or_else(|| Host::new(tab, target), |index| app.view_windows.hosts.remove(index));
    documents::park_active_look(app);
    app.view_windows.parked = Some((tab, app.docs.views.ids().collect()));
    std::mem::swap(&mut app.docs.views, &mut window.views);
    host.swap(app);
    let ribbon = std::mem::replace(&mut app.ui.tab, documents::view_workspace(app.docs.views.active().kind).into());
    documents::show_active_look(app);
    app.view_windows.painting = Some(target);
    let result = action(app);
    documents::park_active_look(app);
    app.ui.tab = ribbon;
    app.view_windows.painting = None;
    app.view_windows.parked = None;
    host.swap(app);
    std::mem::swap(&mut app.docs.views, &mut window.views);
    app.docs.views.put_window(window);
    documents::show_active_look(app);
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
    let ids = app.docs.views.close_window(id).ok_or("no_such_window")?;
    if toast {
        let tab = app.docs.active_tab();
        app.view_windows.returned = Some(Returned { tab, design: app.session.doc.name.clone(), window, expires: app.now + 8.0 });
    }
    Ok(ids)
}

fn window_row(id: WindowId, views: &ViewSet, placement: Option<&WindowPlacement>) -> Value {
    let layout = views
        .tiles()
        .and_then(solidcraft_views::Tiles::preset_kind)
        .map_or(if views.is_tiled() { "custom" } else { "single" }, solidcraft_views::Preset::key);
    json!({"id":id,"level":if id==0 {"full"} else {"views_only"},"views":views.views().iter().map(|view|view.id).collect::<Vec<_>>(),
        "active":views.active().id,"layout":layout,"display":placement.and_then(|p|p.display.as_deref()),
        "placement":placement_json(placement),"tool_window":id==0})
}

pub(crate) fn command(app: &mut SolveApp, command: &str, request: Value) -> Result<Value, String> {
    let id = request.get("window").and_then(Value::as_u64).and_then(|id| WindowId::try_from(id).ok()).unwrap_or_default();
    match command {
        "tab.window.list" => {
            let rows = std::iter::once(window_row(0, &app.docs.views, None))
                .chain(app.docs.views.windows().iter().map(|w| window_row(w.id, &w.views, w.placement.as_ref())))
                .collect::<Vec<_>>();
            Ok(json!({"windows":rows}))
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

fn restore_and_focus(ctx: &Context) {
    if ctx.input(|input| input.viewport().minimized == Some(true)) {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    }
    ctx.send_viewport_cmd(ViewportCommand::Focus);
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

fn window_content(app: &mut SolveApp, ui: &mut Ui, id: WindowId) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let tokens = solidcraft_theme::Tokens::get();
    let edge = Container::new(ui.ctx(), Surface::ApplicationBar).edge("sc_view_window_bar", Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(tokens.app_bar);
    Container::new(ui.ctx(), Surface::ApplicationBar).show_edge(
        ui,
        edge.exact_size(crate::app_bar::HEIGHT).show_separator_line(false),
        frame,
        |ui| {
            let rect = ui.max_rect();
            let titlebar = crate::window_chrome::TitleBar { traffic_lights: app.integrated_titlebar, caption_buttons: app.custom_titlebar };
            let action = titlebar.show(ui, rect, |ui, content| {
                let brand_end = crate::app_bar::paint_brand(ui.painter(), content, &solidcraft_look::glyphs::brand_mark, Some("SolidCraft"));
                let menu = egui::Rect::from_center_size(egui::pos2(content.right() - 17.0, content.center().y), egui::Vec2::splat(26.0));
                let menu_tip = app.tr("multi_window.window_menu");
                Menu::button_with(
                    ui,
                    |ui, _| {
                        let response = ui.interact(menu, ui.scope_id().with("view_window_menu"), egui::Sense::click());
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, menu_tip));
                        if response.hovered() {
                            ui.painter().rect_filled(menu, tokens.radius_sm, tokens.app_bar_hover_button);
                        }
                        if response.has_focus() {
                            ui.painter().rect_stroke(menu, tokens.radius_sm, egui::Stroke::new(1.0, tokens.app_bar_text), egui::StrokeKind::Inside);
                        }
                        for x in [-4.0, 0.0, 4.0] {
                            ui.painter().circle_filled(menu.center() + egui::vec2(x, 0.0), 1.25, tokens.app_bar_text);
                        }
                        response.backing_on_hover_text(menu_tip)
                    },
                    |ui| {
                        ui.label(app.tr("multi_window.views_only"));
                        if crate::ui_kit::menu::button(ui, app.tr("multi_window.return_views"), "").clicked() {
                            let _ = app.run("tab.window.close", json!({"window":id}));
                            ui.close();
                        }
                    },
                );
                let left = brand_end + 12.0;
                let right = menu.left() - 6.0;
                if right > left {
                    documents::tabs(app, ui, rect, left, right);
                }
            });
            if let Some(action) = action {
                if action == crate::window_chrome::WindowAction::Close {
                    app.view_windows.close_current();
                } else {
                    action.send(ui.ctx());
                }
            }
        },
    );
    let rect = ui.available_rect_before_wrap();
    app.shell.content = Some(rect);
    let mut canvas = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    app.show_workspace(&mut canvas);
    if let Some(rect) = app.viewport.rect.filter(|r| r.is_positive() && r.is_finite()) {
        if crate::view_tiles::shown(app) {
            crate::view_tiles::card_controls(app, &mut canvas);
        }
        let mut controls = crate::view_tiles::controls_ui(&mut canvas, app.session.uid, app.docs.views.active().id, rect);
        crate::render_workspace::host_result_overlays(app, &mut controls);
        crate::viewport::show_host_controls(app, &mut controls, rect);
    }
    crate::view_tiles::focus_pressed(app);
    crate::view_tiles::placement_overlay(app, ui.ctx());
    app.resolve_fit(ui.ctx());
    crate::shortcut_box::show(app, ui.ctx());
    crate::context_menu::show(app, ui.ctx());
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
            ready: false,
            startup: None,
        }
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
    let root = app.docs.active_tab();
    let root_home = app.home.open;
    let mut old = std::mem::take(&mut app.view_windows.hosts);
    let mut retired = Vec::new();
    let mut tool_tab = None;
    let mut returned_focus_tab = None;
    for (tab, id) in documents::view_window_ids(app) {
        let mut host = old.iter().position(|host| (host.tab, host.id) == (tab, id)).map_or_else(|| Host::new(tab, id), |index| old.remove(index));
        if !documents::select_tab(app, tab) {
            continue;
        }
        app.view_windows.choices = collect_choices(app);
        let Some(mut window) = app.docs.views.take_window(id) else { continue };
        let os_id = viewport_id(tab, id);
        let custom = !cfg!(target_os = "macos");
        let mut builder = crate::window_chrome::viewport("SolidCraft", solidcraft_engine::io::APP_ID, custom).with_min_inner_size([640.0, 420.0]);
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
        ctx.show_viewport_immediate(os_id, builder, |ui, class| {
            if class == ViewportClass::EmbeddedWindow {
                refused = true;
                return;
            }
            if host.ready
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
                        (p.x, p.y) = (rect.left() - origin.x, rect.top() - origin.y);
                        p.display = display.map(|display| display.name.clone());
                    }
                    if let Some(rect) = viewport.inner_rect.filter(|r| r.is_positive() && r.is_finite()) {
                        (p.w, p.h) = (rect.width(), rect.height());
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
                if let Some(p) = &window.placement {
                    ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(p.w, p.h)));
                }
                ui.ctx().send_viewport_cmd(ViewportCommand::Visible(true));
            }
            if app.view_windows.focus == Some((tab, id)) {
                restore_and_focus(ui.ctx());
                app.view_windows.focus = None;
            }
            if ui.input(|input| input.viewport().focused == Some(true)) {
                app.view_windows.last_focus = Some((tab, id));
            }
            documents::park_active_look(app);
            app.view_windows.parked = Some((tab, app.docs.views.ids().collect()));
            if app.preferences.clear_view_tabs {
                app.view_windows.foreign_views = available_strip_views(app).map(|(owner, view)| (owner, view.clone())).collect();
            }
            std::mem::swap(&mut app.docs.views, &mut window.views);
            host.swap(app);
            let decorations = (app.custom_titlebar, app.integrated_titlebar);
            (app.custom_titlebar, app.integrated_titlebar) = (custom, !custom);
            let home = std::mem::replace(&mut app.home.open, false);
            let ribbon = std::mem::replace(&mut app.ui.tab, documents::view_workspace(app.docs.views.active().kind).into());
            documents::show_active_look(app);
            app.view_windows.painting = Some(id);
            let revision = app.session.revision;
            let had_dialog = app.dialog.is_some();
            app.view_window_logic(ui.ctx());
            window_content(app, ui, id);
            if !had_dialog && app.dialog.is_some() {
                tool_tab = Some(tab);
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
            }
            if crate::render_workspace::active(app) || crate::render_workspace::tiled(app) {
                crate::render_workspace::tick_live(app, ui.ctx());
            }
            close_request = app.view_windows.closing.take();
            let on_child_design = app.docs.current_tab() == tab;
            if on_child_design {
                documents::park_active_look(app);
                app.ui.tab = ribbon;
            }
            app.home.open = home;
            (app.custom_titlebar, app.integrated_titlebar) = decorations;
            app.view_windows.painting = None;
            app.view_windows.parked = None;
            app.view_windows.foreign_views.clear();
            host.swap(app);
            documents::exchange_window_views(app, tab, &mut window.views);
            if on_child_design {
                documents::show_active_look(app);
            }
            if revision != app.session.revision {
                ctx.request_repaint_of(ViewportId::ROOT);
            }
        });
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
            };
            if let Err(reason) = result {
                crate::design_view_ui::show_move_error(app, reason);
            }
            if app.view_windows.focus == Some((tab, 0)) {
                returned_focus_tab = Some(tab);
            }
            ctx.request_repaint();
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
    let root_focus = tool_tab.or(returned_focus_tab);
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
