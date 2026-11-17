//! Native windows of the same design. Only their view strips and presentation state move.
//! The same live session executes commands and keeps the undo history in every window.

use egui::{Context, Ui, ViewportBuilder, ViewportClass, ViewportId};
use solidcraft_views::ViewSet;

use crate::{SolveApp, documents};

#[derive(Default)]
pub(crate) struct Windows {
    available: bool,
    next_window: u64,
    painting: Option<ViewportId>,
    closing: Option<ViewportId>,
    parked: Option<(u64, Vec<u64>)>,
    windows: Vec<Window>,
}

struct Window {
    id: ViewportId,
    tab: u64,
    views: ViewSet,
    viewport: crate::viewport::ViewportState,
    shell: crate::shell::State,
    title: documents::WindowTitle,
    home: bool,
    ribbon_tab: String,
    view_rename: Option<documents::ViewRename>,
    view_placement: Option<(u64, u64)>,
    menu: crate::context_menu::MenuState,
    preview: crate::preview::PreviewState,
}

impl Windows {
    pub(crate) fn is_child(&self) -> bool {
        self.painting.is_some()
    }

    pub(crate) fn close_current(&mut self) -> bool {
        self.closing = self.painting;
        self.closing.is_some()
    }

    pub(crate) fn keeps_view(&self, tab: u64, id: u64) -> bool {
        self.parked.as_ref().is_some_and(|(key, ids)| *key == tab && ids.contains(&id))
            || self.windows.iter().any(|window| window.tab == tab && window.views.has_open_or_closed(id))
    }

    pub(crate) fn has_render(&self) -> bool {
        self.windows.iter().any(|window| {
            let views = &window.views;
            views.active().kind == solidcraft_views::ViewKind::Render
                || (views.is_tiled()
                    && views
                        .views()
                        .iter()
                        .any(|view| view.kind == solidcraft_views::ViewKind::Render && views.tiles().is_some_and(|tiles| tiles.contains(view.id))))
        })
    }

    fn sync_limits(&self, tab: u64, views: &mut ViewSet, other: Option<&ViewSet>) {
        let mut count = other.map_or(0, ViewSet::len);
        let mut next = other.map_or(0, ViewSet::next_id);
        for window in self.windows.iter().filter(|window| window.tab == tab) {
            count += window.views.len();
            next = next.max(window.views.next_id());
        }
        views.share_window_limits(next, count);
    }
}

/// Move a view from its current strip into a native window. The browser keeps it in its strip.
pub(crate) fn detach(app: &mut SolveApp, id: u64) -> Result<(), String> {
    if !app.view_windows.available {
        return Err(app.tr("ui.new_window.unsupported").to_string());
    }
    let tab = app.docs.active_tab();
    // A child already counts the root window while it is being painted.
    if !app.view_windows.is_child() {
        app.view_windows.sync_limits(tab, &mut app.docs.views, None);
    }
    let views = documents::detach_view(app, id).ok_or("there is no such view or the design has too many views")?;
    let ribbon_tab = match views.active().kind {
        solidcraft_views::ViewKind::Render => "RENDER",
        solidcraft_views::ViewKind::Sketch => "SKETCH",
        _ => "MODEL",
    }
    .into();
    let mut viewport = crate::viewport::ViewportState::default();
    viewport.isolate_window_scenes();
    viewport.gpu = app.viewport.gpu;
    viewport.gpu_unavailable = app.viewport.gpu_unavailable;
    app.view_windows.next_window = app.view_windows.next_window.saturating_add(1);
    let id = ViewportId::from_hash_of(("design_view_window", app.view_windows.next_window));
    app.view_windows.windows.push(Window {
        id,
        tab,
        views,
        viewport,
        shell: Default::default(),
        title: Default::default(),
        home: false,
        ribbon_tab,
        view_rename: None,
        view_placement: None,
        menu: Default::default(),
        preview: Default::default(),
    });
    Ok(())
}

/// Only the desktop shell can make real OS windows. Call before drawing the root app bar so
/// the same view action can report that a native window is unavailable in an embedded host.
pub fn prepare(app: &mut SolveApp, ctx: &Context) {
    app.view_windows.available = !ctx.embed_viewports();
    let tab = app.docs.active_tab();
    app.view_windows.sync_limits(tab, &mut app.docs.views, None);
}

/// Run the same app UI for each native window, borrowing the design for the duration of its
/// frame. Views never round-trip through a file, and the document never needs to be cloned.
pub fn show(app: &mut SolveApp, ctx: &Context, draw: fn(&mut SolveApp, &mut Ui), render_state: Option<&egui_wgpu::RenderState>) {
    if !app.view_windows.available || app.view_windows.is_child() || app.quit_requested {
        return;
    }
    let ids: Vec<_> = app.view_windows.windows.iter().map(|window| window.id).collect();
    let mut retired_scenes = Vec::new();
    for id in ids {
        let Some(index) = app.view_windows.windows.iter().position(|window| window.id == id) else {
            continue;
        };
        let mut window = app.view_windows.windows.remove(index);
        if !app.docs.tab_open(window.tab) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Close);
            retired_scenes.extend(window.viewport.window_scene_slots());
            continue;
        }
        let builder = ViewportBuilder::default().with_title("SolidCraft").with_inner_size([1000.0, 700.0]).with_min_inner_size([480.0, 320.0]);
        let mut return_views = true;
        let closed = ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == ViewportClass::EmbeddedWindow || ui.input(|input| input.viewport().close_requested()) {
                return true;
            }
            let root = app.docs.active_tab();
            let root_home = app.home.open;
            if !documents::select_tab(app, window.tab) {
                return false;
            }

            documents::park_active_look(app);
            app.view_windows.parked = Some((window.tab, app.docs.views.ids().collect()));
            std::mem::swap(&mut app.docs.views, &mut window.views);
            app.view_windows.sync_limits(window.tab, &mut app.docs.views, Some(&window.views));
            window.viewport.gpu = app.viewport.gpu;
            window.viewport.gpu_unavailable = app.viewport.gpu_unavailable;
            std::mem::swap(&mut app.viewport, &mut window.viewport);
            std::mem::swap(&mut app.shell, &mut window.shell);
            std::mem::swap(&mut app.window_title, &mut window.title);
            std::mem::swap(&mut app.docs.view_rename, &mut window.view_rename);
            std::mem::swap(&mut app.docs.view_placement, &mut window.view_placement);
            std::mem::swap(&mut app.menu, &mut window.menu);
            std::mem::swap(&mut app.preview, &mut window.preview);
            let decorations = (app.custom_titlebar, app.integrated_titlebar);
            (app.custom_titlebar, app.integrated_titlebar) = (false, false);
            app.home.open = window.home;
            std::mem::swap(&mut app.ui.tab, &mut window.ribbon_tab);
            documents::show_active_look(app);
            app.view_windows.painting = Some(id);

            let entering_revision = app.session.revision;
            app.view_window_logic(ui.ctx());
            draw(app, ui);
            if crate::render_workspace::active(app) || crate::render_workspace::tiled(app) {
                crate::render_workspace::tick_live(app, ui.ctx());
            }
            let changed = app.session.revision != entering_revision;
            let closing = app.view_windows.closing.take() == Some(id);
            if app.docs.current_tab() == window.tab {
                documents::park_active_look(app);
            }
            std::mem::swap(&mut app.ui.tab, &mut window.ribbon_tab);
            app.view_windows.painting = None;
            app.view_windows.parked = None;
            window.home = app.home.open;
            (app.custom_titlebar, app.integrated_titlebar) = decorations;
            std::mem::swap(&mut app.docs.view_rename, &mut window.view_rename);
            std::mem::swap(&mut app.docs.view_placement, &mut window.view_placement);
            std::mem::swap(&mut app.menu, &mut window.menu);
            std::mem::swap(&mut app.preview, &mut window.preview);
            std::mem::swap(&mut app.window_title, &mut window.title);
            std::mem::swap(&mut app.shell, &mut window.shell);
            std::mem::swap(&mut app.viewport, &mut window.viewport);

            documents::exchange_window_views(app, window.tab, &mut window.views);
            if app.docs.current_tab() == window.tab {
                documents::show_active_look(app);
            } else if app.docs.tab_open(window.tab) {
                let primary_ribbon = std::mem::replace(&mut app.ui.tab, window.ribbon_tab.clone());
                documents::restore_primary_ribbon(app, window.tab, primary_ribbon);
                let old_views = std::mem::take(&mut window.views);
                documents::reattach_window_views(app, window.tab, old_views);
                let new_tab = app.docs.active_tab();
                app.view_windows.sync_limits(new_tab, &mut app.docs.views, None);
                let view = app.docs.views.active().id;
                if !closing && let Some(views) = documents::detach_view(app, view) {
                    window.tab = new_tab;
                    window.views = views;
                } else {
                    return_views = false;
                }
            }
            documents::select_tab(app, root);
            app.home.open = root_home;
            if changed {
                ctx.request_repaint_of(ViewportId::ROOT);
            }
            closing || !return_views || !app.docs.tab_open(window.tab)
        });
        if closed {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Close);
            retired_scenes.extend(window.viewport.window_scene_slots());
            if return_views {
                documents::reattach_window_views(app, window.tab, window.views);
            }
            ctx.request_repaint();
        } else {
            app.view_windows.windows.insert(index.min(app.view_windows.windows.len()), window);
        }
    }
    let tab = app.docs.active_tab();
    app.view_windows.sync_limits(tab, &mut app.docs.views, None);
    if !retired_scenes.is_empty()
        && let Some(state) = render_state
        && let Some(renderer) = state.renderer.write().callback_resources.get_mut::<solidcraft_viewport_gpu::view::Renderer>()
    {
        renderer.discard_views(&retired_scenes);
    }
}
