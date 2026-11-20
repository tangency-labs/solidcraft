//! Opt-in plugin workspaces. Declarations are data; hooks never own the live session.
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::json;
use sha2::{Digest, Sha256};
use solidcraft_engine::panels::*;
use solidcraft_engine::plugin::{DesignKind, DockZone, PanelWidget, UiPanelSpec, WorkspaceSpec};

use crate::panel_runtime::*;
use crate::{SolveApp, plugins};

pub(crate) type CommandQueue = Rc<RefCell<VecDeque<(String, String)>>>;

#[derive(Clone)]
pub(crate) struct Workspace {
    pub id: String,
    pub plugin: plugins::Plugin,
    pub spec: WorkspaceSpec,
}
#[derive(Default)]
pub(crate) struct State {
    items: Vec<Workspace>,
    active: Option<Workspace>,
    previous: Option<(String, WorkspaceId)>,
    base_layout: Option<Layout>,
    registered: BTreeMap<String, String>,
    design_kind: Option<DesignKind>,
    tabs: BTreeMap<String, usize>,
    errors: BTreeMap<String, String>,
    hooks: VecDeque<(Workspace, bool)>,
    running: Option<Receiver<(String, Result<(), String>)>>,
    commands: CommandQueue,
}

impl State {
    pub(crate) fn active_label(&self) -> Option<(String, String)> {
        self.active.as_ref().map(|workspace| (workspace.spec.name.clone(), workspace.spec.icon.clone()))
    }
}

pub(crate) fn active_id(app: &SolveApp) -> Option<&str> {
    app.plugin_workspaces.active.as_ref().map(|workspace| workspace.id.as_str())
}

pub(crate) fn previous_layout_id(app: &SolveApp) -> Option<&WorkspaceId> {
    app.plugin_workspaces.previous.as_ref().map(|(_, layout)| layout)
}

pub(crate) fn builtin_available(app: &SolveApp, panel: &str) -> bool {
    app.plugin_workspaces.active.as_ref().is_none_or(|workspace| workspace.spec.builtin_panels.iter().any(|id| id == panel))
}

pub(crate) fn discovered(app: &mut SolveApp, plugins: &[plugins::Plugin]) {
    app.plugin_workspaces.items = plugins
        .iter()
        .flat_map(|plugin| {
            plugin.declarations.workspaces.iter().map(move |spec| Workspace {
                id: format!("plugin:{}/{}", plugin.id, spec.id),
                plugin: plugin.clone(),
                spec: spec.clone(),
            })
        })
        .take(64)
        .collect();
}

fn applicable(app: &SolveApp, workspace: &Workspace) -> bool {
    let kind = app.plugin_workspaces.design_kind.unwrap_or_else(|| design_kind(app));
    workspace.spec.applies_to.contains(&kind)
}

fn design_kind(app: &SolveApp) -> DesignKind {
    if crate::drawing_workspace::active(app) {
        DesignKind::Drawing
    } else if app.session.active_sketch.is_some() {
        DesignKind::Sketch
    } else if !app.session.doc.occurrences.is_empty() {
        DesignKind::Assembly
    } else {
        DesignKind::Part
    }
}

pub(crate) fn switcher_items(app: &SolveApp) -> Vec<crate::ui_kit::Item> {
    app.plugin_workspaces
        .items
        .iter()
        .filter(|workspace| plugins::enabled(app, &workspace.plugin.id))
        .map(|workspace| {
            let enabled = applicable(app, workspace) && app.tool.is_none() && app.dialog.is_none() && !app.file_load.busy();
            let action = format!("ui.workspace.{}", workspace.id);
            let mut item = crate::context_menu::act(&action, &workspace.spec.name, &workspace.spec.icon)
                .with(json!({"workspace":workspace.id}))
                .key(if active_id(app) == Some(&workspace.id) { "current" } else { "" })
                .on(enabled);
            if !enabled {
                item.tip = Some(app.tr("ui.plugin_workspace.unavailable").to_owned());
            }
            item
        })
        .collect()
}

fn switch_layout(app: &mut SolveApp, workspace: &WorkspaceId) -> Result<(), String> {
    crate::panel_runtime::refresh_production(app).map_err(|error| error.to_string())?;
    app.panel_runtime
        .as_mut()
        .ok_or_else(|| app_error("ui.plugin_workspace.unavailable"))?
        .command_batch(&app.session, &[json!({"command":"layout.workspace.select","params":{"workspace":workspace}})])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn enter(app: &mut SolveApp, id: &str) -> Result<(), String> {
    if active_id(app) == Some(id) {
        return Ok(());
    }
    let workspace = app
        .plugin_workspaces
        .items
        .iter()
        .find(|workspace| workspace.id == id && plugins::enabled(app, &workspace.plugin.id))
        .cloned()
        .ok_or_else(|| app_error("ui.plugin_workspace.unknown"))?;
    if !applicable(app, &workspace) || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
        return Err(app_error("ui.plugin_workspace.unavailable"));
    }
    // Build and validate registrations before changing the current workspace.
    if app.panel_runtime.is_none() {
        crate::panel_runtime::ensure_production(app).map_err(|error| error.to_string())?;
    }
    if app.plugin_workspaces.registered.get(id).is_some_and(|approval| *approval != workspace.plugin.approval) {
        return Err(app_error("ui.plugin_workspace.updated"));
    }
    let queue = app.plugin_workspaces.commands.clone();
    let base = app
        .plugin_workspaces
        .base_layout
        .clone()
        .or_else(|| app.panel_runtime.as_ref().map(|integration| integration.runtime().layout().layout().clone()))
        .ok_or_else(|| app_error("ui.plugin_workspace.unavailable"))?;
    let integration = app.panel_runtime.as_mut().ok_or_else(|| app_error("ui.plugin_workspace.unavailable"))?;
    integration.register_plugin_workspace(&workspace, &base, queue).map_err(|error| error.to_string())?;
    let previous = integration.runtime().layout().layout().workspace.clone();
    let old_tab = app.ui.tab.clone();
    let kind = design_kind(app);
    let old = app.plugin_workspaces.active.replace(workspace.clone());
    app.ui.tab = "PLUGIN".into();
    if let Err(error) = switch_layout(app, &WorkspaceId::new(&workspace.id).map_err(|error| error.to_string())?) {
        app.plugin_workspaces.active = old;
        app.ui.tab = old_tab;
        return Err(error);
    }
    if let Some(old) = old {
        queue_hook(app, old, false);
    } else {
        app.plugin_workspaces.previous = Some((old_tab, previous));
        app.plugin_workspaces.base_layout = Some(base);
        app.plugin_workspaces.design_kind = Some(kind);
    }
    app.plugin_workspaces.registered.insert(id.to_owned(), workspace.plugin.approval.clone());
    // Built-in workspace state is retained. The plugin uses its own ribbon selection.
    app.ui.tab = "PLUGIN".into();
    app.ui.ribbon_context = None;
    queue_hook(app, workspace, true);
    Ok(())
}

pub(crate) fn leave(app: &mut SolveApp) -> Result<(), String> {
    let Some(workspace) = app.plugin_workspaces.active.take() else { return Ok(()) };
    let Some((tab, layout)) = app.plugin_workspaces.previous.clone() else {
        app.plugin_workspaces.active = Some(workspace);
        return Err(app_error("ui.plugin_workspace.unavailable"));
    };
    let old_tab = std::mem::replace(&mut app.ui.tab, tab);
    if let Err(error) = switch_layout(app, &layout) {
        app.plugin_workspaces.active = Some(workspace);
        app.ui.tab = old_tab;
        return Err(error);
    }
    app.plugin_workspaces.previous = None;
    app.plugin_workspaces.base_layout = None;
    app.plugin_workspaces.design_kind = None;
    queue_hook(app, workspace, false);
    Ok(())
}

fn app_error(key: &str) -> String {
    solidcraft_ui_text::tr(key).to_owned()
}

fn queue_hook(app: &mut SolveApp, workspace: Workspace, entering: bool) {
    let hook = if entering { &workspace.spec.on_enter } else { &workspace.spec.on_exit };
    if hook.is_none() {
        return;
    }
    if entering {
        app.plugin_workspaces.errors.remove(&workspace.id);
    }
    // Bound rapid switching. Hooks are observational; the latest transition stays visible.
    if app.plugin_workspaces.hooks.len() >= 16 {
        app.plugin_workspaces.hooks.pop_front();
    }
    app.plugin_workspaces.hooks.push_back((workspace, entering));
}

#[cfg(test)]
pub(crate) fn pending_hooks(app: &SolveApp) -> Vec<(String, bool)> {
    app.plugin_workspaces.hooks.iter().map(|(workspace, entering)| (workspace.id.clone(), *entering)).collect()
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(receiver) = &app.plugin_workspaces.running {
        match receiver.try_recv() {
            Ok((id, result)) => {
                if let Err(error) = result {
                    if active_id(app) == Some(id.as_str()) {
                        app.set_status(error.clone(), true);
                    }
                    app.plugin_workspaces.errors.insert(id, error);
                }
                app.plugin_workspaces.running = None;
            }
            Err(TryRecvError::Disconnected) => app.plugin_workspaces.running = None,
            Err(TryRecvError::Empty) => {}
        }
    }
    if app.plugin_workspaces.active.as_ref().is_some_and(|workspace| {
        !plugins::enabled(app, &workspace.plugin.id)
            || !app.plugin_workspaces.items.iter().any(|item| item.id == workspace.id && item.plugin.approval == workspace.plugin.approval)
    }) && let Err(error) = leave(app) {
        app.set_status(error, true);
    }
    if app.plugin_workspaces.running.is_none()
        && let Some((workspace, entering)) = app.plugin_workspaces.hooks.pop_front()
    {
        crate::plugins::sync_policy(app);
        let mut session = app.session.scratch();
        session.disabled_plugins = app.session.disabled_plugins.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let id = workspace.id.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let package_changed = app_error("ui.plugin_workspace.updated");
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::thread::Builder::new()
            .name("plugin-workspace-hook".into())
            .spawn(move || {
                let result = solidcraft_engine::plugin::read_package(&workspace.plugin.path)
                    .and_then(|package| {
                        if package.approval != workspace.plugin.approval {
                            return Err(solidcraft_engine::EngineError::Other(package_changed));
                        }
                        solidcraft_engine::plugin::run_workspace_hook(&mut session, package, &workspace.spec.id, entering)
                    })
                    .map(|_| ())
                    .map_err(|error| error.to_string());
                let _ = sender.send((id, result));
            })
            .map(|_| ());
        #[cfg(target_arch = "wasm32")]
        let started: Result<(), String> = {
            let _ = (session, sender, id, workspace, entering);
            Err(app_error("ui.plugins_desktop"))
        };
        match started {
            Ok(()) => app.plugin_workspaces.running = Some(receiver),
            Err(error) => {
                let key = active_id(app).unwrap_or_default().to_owned();
                app.plugin_workspaces.errors.insert(key, error.to_string());
            }
        }
    }
    if app.plugin_workspaces.running.is_some() || !app.plugin_workspaces.hooks.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(25));
    }
    let commands = app.plugin_workspaces.commands.try_borrow_mut().map(|mut commands| std::mem::take(&mut *commands)).unwrap_or_default();
    for (plugin, entry) in commands {
        crate::plugin_menu::ensure_listed(app);
        crate::plugin_menu::choose_ribbon(app, &format!("plugin_entry:{plugin}:{entry}"));
    }
}

pub(crate) fn ribbon(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect, folded: bool) -> Option<String> {
    let workspace = app.plugin_workspaces.active.clone()?;
    let t = solidcraft_theme::Tokens::get();
    let tab = app.plugin_workspaces.tabs.entry(workspace.id.clone()).or_default();
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt("plugin-workspace-ribbon").max_rect(rect));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    child.visuals_mut().selection.stroke.color = t.accent;
    let mut clicked = None;
    egui::ScrollArea::horizontal().id_salt("plugin-workspace-tabs").auto_shrink([false, true]).show(&mut child, |ui| {
        ui.horizontal(|ui| {
            for (index, ribbon) in workspace.spec.ribbon.iter().enumerate() {
                let button = egui::Button::selectable(*tab == index, &ribbon.name);
                if ui.add_sized([0.0, t.control_height], button).clicked() {
                    *tab = index;
                    clicked = Some(ribbon.name.clone());
                }
            }
        });
    });
    let selected = *tab;
    if folded {
        return clicked;
    }
    if let Some(tab) = workspace.spec.ribbon.get(selected) {
        egui::ScrollArea::horizontal().id_salt("plugin-workspace-groups").auto_shrink([false, true]).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                for group in &tab.groups {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            for entry in &group.entries {
                                if ui.add_sized([0.0, t.control_height], egui::Button::new(entry)).clicked()
                                    && let Ok(mut queue) = app.plugin_workspaces.commands.try_borrow_mut()
                                {
                                    queue.push_back((workspace.plugin.id.clone(), entry.clone()));
                                }
                            }
                        });
                        ui.label(&group.name);
                    });
                    ui.separator();
                }
            });
        });
    }
    if let Some(error) = app.plugin_workspaces.errors.get(&workspace.id) {
        child.label(error);
    }
    clicked
}

pub(crate) fn identity(plugin: &str, panel: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(plugin.as_bytes());
    digest.update([0]);
    digest.update(panel.as_bytes());
    format!("plugin.{:x}", digest.finalize()).chars().take(63).collect()
}

pub(crate) fn zone(zone: DockZone) -> Zone {
    match zone {
        DockZone::Left => Zone::Left,
        DockZone::Right => Zone::Right,
        DockZone::Bottom => Zone::Bottom,
    }
}

pub(crate) fn panels(workspace: &Workspace, queue: CommandQueue) -> Vec<(PanelSpec, ControllerBinding)> {
    workspace
        .plugin
        .declarations
        .panels
        .iter()
        .map(|panel| {
            let panel = panel.clone();
            let plugin = workspace.plugin.clone();
            let queue = queue.clone();
            let spec = PanelSpec {
                kind: PanelKind(identity(&plugin.id, &panel.id)),
                title_key: "ui.plugin_workspace.panel".into(),
                icon: "prefs_plugins".into(),
                allowed_zones: panel.zones.iter().copied().map(zone).chain([Zone::Floating]).collect(),
                default_zone: panel.zones.first().copied().map(zone).unwrap_or(Zone::Left),
                min_size: Size { width: 240.0, height: 80.0 },
                default_size: Size { width: 280.0, height: 240.0 },
                max_size: Size { width: MAX_LOGICAL, height: MAX_LOGICAL },
                edge_rule: EdgeRule::None,
                surface: PanelSurface::Translucent,
                features: PanelFeatures { popout: false, ..Default::default() },
                side_height_policy: SideHeightPolicy::Content,
                fixed_bottom_height: None,
                header_actions: Vec::new(),
                text_heavy: false,
            };
            let binding = ControllerBinding {
                ownership: Ownership::Document,
                create: Box::new(move |_, _| {
                    Ok(Box::new(PluginPanel {
                        plugin: plugin.id.clone(),
                        title: format!("{} · {}", plugin.name, panel.title),
                        panel: panel.clone(),
                        queue: queue.clone(),
                    }))
                }),
            };
            (spec, binding)
        })
        .collect()
}

struct PluginPanel {
    plugin: String,
    title: String,
    panel: UiPanelSpec,
    queue: CommandQueue,
}
impl PanelController for PluginPanel {
    fn availability(&self, _owner: &ContentOwner, _models: &dyn PanelModels) -> Availability {
        Availability::Available
    }
    fn update(&mut self, _owner: &ContentOwner, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Ok(())
    }
    fn presentation(&self) -> PanelPresentation {
        PanelPresentation {
            title: self.title.clone(),
            icon: "prefs_plugins".into(),
            status: Some(app_error("ui.plugin_workspace.badge")),
            attention: false,
            actions: Vec::new(),
        }
    }
    fn prepare_lifecycle(&self, _event: &LifecycleEvent) -> PanelResult<()> {
        Ok(())
    }
    fn lifecycle(&mut self, _event: &LifecycleEvent) {}
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, _context: &PanelContext, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        if !matches!(slot, ContentSlot::Body | ContentSlot::Inline) {
            return Ok(());
        }
        let t = solidcraft_theme::Tokens::get();
        // Flatten the validated tree iteratively; the host owns every widget and its size.
        let mut pending = vec![self.panel.layout.root];
        let mut remaining = 512;
        while let Some(index) = pending.pop() {
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            let Some(node) = self.panel.layout.nodes.get(index as usize) else { break };
            match &node.widget {
                PanelWidget::Group { children } => pending.extend(children.iter().rev().copied()),
                PanelWidget::Text { text } => {
                    ui.label(text);
                }
                PanelWidget::Button { label, entry, .. } => {
                    if ui.add_sized([0.0, t.control_height], egui::Button::new(label)).clicked()
                        && let Ok(mut queue) = self.queue.try_borrow_mut()
                    {
                        queue.push_back((self.plugin.clone(), entry.clone()));
                    }
                }
                PanelWidget::Unknown => {
                    ui.label(app_error("ui.plugin_workspace.newer"));
                }
            }
        }
        Ok(())
    }
    fn action(&mut self, _owner: &ContentOwner, _action: &ControlId, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Err(error(Rule::Capability, "plugin panel actions use manifest entries"))
    }
}
