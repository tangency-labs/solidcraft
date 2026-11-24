//! Opt-in plugin workspaces. Declarations are data; hooks never own the live session.
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::json;
use sha2::{Digest, Sha256};
use solidcraft_engine::panels::*;
use solidcraft_engine::plugin::ui::{DesignKind, DockZone, WorkspaceSpec};

use crate::panel_runtime::*;
use crate::{SolveApp, plugins};

type HookReply = Result<solidcraft_engine::plugin::PanelUpdates, String>;
type CommandQueue = Rc<RefCell<VecDeque<(String, String)>>>;

#[derive(Clone)]
pub(crate) struct Workspace {
    pub id: String,
    pub plugin: plugins::Plugin,
    pub spec: WorkspaceSpec,
}

#[derive(Clone, PartialEq, Eq)]
struct HookDocument {
    session: u64,
    id: String,
}
impl HookDocument {
    fn current(app: &SolveApp) -> Self {
        Self { session: app.session.uid, id: app.session.document_id.clone() }
    }
    fn is_current(&self, app: &SolveApp) -> bool {
        self.session == app.session.uid && self.id == app.session.document_id
    }
}

struct QueuedHook {
    workspace: Workspace,
    entering: bool,
    document: HookDocument,
}

struct RunningHook {
    workspace: String,
    document: HookDocument,
    receiver: Receiver<HookReply>,
}

#[derive(Default)]
pub(crate) struct State {
    items: Vec<Workspace>,
    active: Option<Workspace>,
    previous: Option<(String, WorkspaceId)>,
    base_layout: Option<Layout>,
    registered: BTreeMap<String, String>,
    drawing_document: Option<HookDocument>,
    tabs: BTreeMap<String, usize>,
    errors: BTreeMap<String, String>,
    hooks: VecDeque<QueuedHook>,
    running: Option<RunningHook>,
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

/// Whether the plugin panel may open in the current layout. A plugin workspace shows exactly the
/// panels it places; built-in workspaces open every declared panel as usual.
pub(crate) fn panel_allowed(app: &SolveApp, plugin: &str, panel: &str) -> bool {
    app.plugin_workspaces
        .active
        .as_ref()
        .is_none_or(|workspace| workspace.plugin.id == plugin && workspace.spec.panels.iter().any(|(_, ids)| ids.iter().any(|id| id == panel)))
}

/// The identity the shared panel runtime gives one of a plugin's declared panels.
#[cfg(test)]
pub(crate) fn panel_instance(plugin: &plugins::Plugin, panel: &str) -> Option<InstanceId> {
    let declared = plugin.declarations.panels.iter().find(|declared| declared.id == panel)?;
    crate::panel_runtime::plugin_instance(&plugin.id, declared).ok()
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
    workspace.spec.applies_to.contains(&workspace_kind(app))
}

fn workspace_kind(app: &SolveApp) -> DesignKind {
    if app.plugin_workspaces.drawing_document.as_ref().is_some_and(|document| document.is_current(app)) {
        DesignKind::Drawing
    } else {
        design_kind(app)
    }
}

fn design_kind(app: &SolveApp) -> DesignKind {
    if crate::drawing_workspace::active(app) || crate::workspace::sheet_scope(app) {
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
            let enabled = applicable(app, workspace) && switchable(app);
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

/// Whether the window may change workspace now, by the same rule the built-in switcher uses.
fn switchable(app: &SolveApp) -> bool {
    if app.preferences.workspace_follows_view {
        crate::workspace::switcher_available(app)
    } else {
        app.tool.is_none() && app.dialog.is_none() && !app.file_load.busy()
    }
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
    if !applicable(app, &workspace) || !switchable(app) {
        return Err(app_error("ui.plugin_workspace.unavailable"));
    }
    // Build and validate registrations before changing the current workspace.
    if app.panel_runtime.is_none() {
        crate::panel_runtime::ensure_production(app).map_err(|error| error.to_string())?;
    }
    if app.plugin_workspaces.registered.get(id).is_some_and(|approval| *approval != workspace.plugin.approval) {
        return Err(app_error("ui.plugin_workspace.updated"));
    }
    let base = app
        .plugin_workspaces
        .base_layout
        .clone()
        .or_else(|| app.panel_runtime.as_ref().map(|integration| integration.runtime().layout().layout().clone()))
        .ok_or_else(|| app_error("ui.plugin_workspace.unavailable"))?;
    let integration = app.panel_runtime.as_mut().ok_or_else(|| app_error("ui.plugin_workspace.unavailable"))?;
    integration.register_plugin_workspace(&workspace, &base).map_err(|error| error.to_string())?;
    let previous = integration.runtime().layout().layout().workspace.clone();
    let old_tab = app.ui.tab.clone();
    let kind = workspace_kind(app);
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
    }
    app.plugin_workspaces.drawing_document = (kind == DesignKind::Drawing).then(|| HookDocument::current(app));
    app.plugin_workspaces.registered.insert(id.to_owned(), workspace.plugin.approval.clone());
    // Built-in workspace state is retained. The plugin uses its own ribbon selection.
    app.ui.tab = "PLUGIN".into();
    app.ui.ribbon_context = None;
    queue_hook(app, workspace, true);
    Ok(())
}

/// Leave the plugin workspace whatever happens, so the user can always reach a built-in one.
/// When the layout cannot be switched back, the plugin state is reset anyway and the reason is
/// returned for the caller to show.
pub(crate) fn leave_best_effort(app: &mut SolveApp) -> Option<String> {
    let reason = leave(app).err()?;
    if let Some(workspace) = app.plugin_workspaces.active.take() {
        match app.plugin_workspaces.previous.take() {
            Some((tab, _)) => app.ui.tab = tab,
            None if app.ui.tab == "PLUGIN" => app.ui.tab = "MODEL".into(),
            None => {}
        }
        app.plugin_workspaces.base_layout = None;
        app.plugin_workspaces.drawing_document = None;
        queue_hook(app, workspace, false);
    }
    Some(reason)
}

#[cfg(test)]
pub(crate) fn lose_return_layout(app: &mut SolveApp) {
    app.plugin_workspaces.previous = None;
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
    app.plugin_workspaces.drawing_document = None;
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
    let document = HookDocument::current(app);
    app.plugin_workspaces.hooks.push_back(QueuedHook { workspace, entering, document });
}

#[cfg(test)]
pub(crate) fn pending_hooks(app: &SolveApp) -> Vec<(String, bool)> {
    app.plugin_workspaces.hooks.iter().map(|hook| (hook.workspace.id.clone(), hook.entering)).collect()
}

fn report_hook_error(app: &mut SolveApp, document: &HookDocument, id: String, error: String) {
    let message = solidcraft_ui_text::format("ui.plugin_workspace.hook_failed", &[("workspace", &id), ("error", &error)]);
    if document.is_current(app) && active_id(app) == Some(id.as_str()) {
        app.set_status(message, true);
    } else {
        // A finished exit hook has no ribbon; keep its failure in the shared Messages window.
        app.messages.push(&message, true, crate::now_ms());
    }
    if document.is_current(app) {
        app.plugin_workspaces.errors.insert(id, error);
    }
}

pub(crate) fn poll_hook(app: &mut SolveApp) {
    let Some(running) = &app.plugin_workspaces.running else { return };
    let completion = running.receiver.try_recv().map(|result| (running.document.clone(), running.workspace.clone(), result));
    match completion {
        Ok((document, id, result)) => {
            app.plugin_workspaces.running = None;
            match result {
                // The hook ran against a snapshot; only its panel updates reach this design, and
                // only if it is still the design the hook started in.
                Ok(updates) if document.is_current(app) => solidcraft_engine::plugin::adopt_panel_updates(&mut app.session, updates),
                Ok(_) => {}
                Err(error) => report_hook_error(app, &document, id, error),
            }
        }
        Err(TryRecvError::Disconnected) => app.plugin_workspaces.running = None,
        Err(TryRecvError::Empty) => {}
    }
}

#[cfg(test)]
pub(crate) fn hook_reply_channel(app: &mut SolveApp, workspace: &str) -> std::sync::mpsc::Sender<HookReply> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.plugin_workspaces.running = Some(RunningHook { workspace: workspace.to_owned(), document: HookDocument::current(app), receiver });
    sender
}

#[cfg(test)]
pub(crate) fn hook_running(app: &SolveApp) -> bool {
    app.plugin_workspaces.running.is_some()
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    poll_hook(app);
    if app.plugin_workspaces.active.as_ref().is_some_and(|workspace| {
        !plugins::enabled(app, &workspace.plugin.id)
            || !app.plugin_workspaces.items.iter().any(|item| item.id == workspace.id && item.plugin.approval == workspace.plugin.approval)
    }) && let Err(error) = leave(app)
    {
        app.set_status(error, true);
    }
    let current_document = HookDocument::current(app);
    app.plugin_workspaces.hooks.retain(|hook| hook.document == current_document);
    if app.plugin_workspaces.running.is_none()
        && let Some(QueuedHook { workspace, entering, document }) = app.plugin_workspaces.hooks.pop_front()
    {
        crate::plugins::sync_policy(app);
        let mut session = app.session.scratch();
        session.disabled_plugins = app.session.disabled_plugins.clone();
        let (sender, receiver) = std::sync::mpsc::channel::<HookReply>();
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
                    .map(|(_, updates)| updates)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map(|_| ());
        #[cfg(target_arch = "wasm32")]
        let started: Result<(), String> = {
            let _ = (session, sender, workspace, entering);
            Err(app_error("ui.plugins_desktop"))
        };
        match started {
            Ok(()) => app.plugin_workspaces.running = Some(RunningHook { workspace: id, document, receiver }),
            Err(error) => report_hook_error(app, &document, id, error.to_string()),
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

/// Returns `Some(already shown)` when a tab was clicked, like the built-in ribbon.
pub(crate) fn ribbon(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect, folded: bool) -> Option<bool> {
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
                    clicked = Some(*tab == index);
                    *tab = index;
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
