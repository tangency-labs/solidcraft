//! Transient plugin panels, shared by the window, commands and headless callers.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct PanelPackage {
    pub name: String,
    pub approval: String,
    pub declarations: ui::Declarations,
    pub data: BTreeMap<String, ui::PanelData>,
    pub visible: BTreeMap<String, bool>,
    /// Explicit guest visibility requests; user docking and closing stays in the workspace layout.
    pub visibility_revision: BTreeMap<String, u64>,
}

#[derive(Clone, Debug)]
pub struct PanelSnapshot {
    pub plugin: String,
    pub units: String,
    pub plugin_name: String,
    pub approval: String,
    pub ui_version: u32,
    pub panel: ui::PanelSpec,
    pub data: ui::PanelData,
    pub visible: bool,
    pub visibility_revision: u64,
}

pub(super) fn register_package(session: &mut Session, package: &Package) -> Result<()> {
    let Some(runtime) = package.manifest.runtime.as_ref() else { return Ok(()) };
    let Some(declarations) = runtime.ui.as_ref() else { return Ok(()) };
    declarations.validate(runtime)?;
    let id = &package.manifest.id;
    if session.plugin_state.panels.get(id).is_some_and(|panels| panels.approval == package.approval) {
        return Ok(());
    }
    session.plugin_state.panels.insert(
        id.clone(),
        PanelPackage {
            name: package.manifest.name.clone(),
            approval: package.approval.clone(),
            declarations: declarations.clone(),
            data: declarations.panels.iter().map(|panel| (panel.id.clone(), ui::PanelData::default())).collect(),
            visible: declarations.panels.iter().map(|panel| (panel.id.clone(), true)).collect(),
            visibility_revision: declarations.panels.iter().map(|panel| (panel.id.clone(), 0)).collect(),
        },
    );
    Ok(())
}

/// Discover inert declarations when the installed package list changes. No guest code runs.
pub fn sync_panels(session: &mut Session) {
    let mut found = BTreeSet::new();
    for (id, folder) in import::package_folders(session) {
        if session.disabled_plugins.contains(&id) {
            continue;
        }
        let Ok(manifest) = with_installed(session, &folder, || import::read_manifest(&folder)) else { continue };
        if manifest.runtime.as_ref().and_then(|runtime| runtime.ui.as_ref()).is_none() {
            continue;
        }
        let Ok(package) = with_installed(session, &folder, || read_package(&folder)) else { continue };
        if register_package(session, &package).is_ok() {
            found.insert(id.clone());
            session.plugin_state.packages.entry(id).or_insert(folder);
        }
    }
    session.plugin_state.panels.retain(|id, _| found.contains(id));
}

impl Session {
    /// Adopt successful command updates once at the start of a UI frame.
    pub fn merge_plugin_panel_data(&mut self) {
        for package in self.plugin_state.panels.values_mut() {
            for data in package.data.values_mut() {
                data.merge_frame();
            }
        }
    }
    pub fn plugin_panels(&self) -> Vec<PanelSnapshot> {
        self.plugin_state
            .panels
            .iter()
            .filter(|(id, _)| !self.disabled_plugins.contains(*id))
            .flat_map(|(id, package)| {
                package.declarations.panels.iter().map(move |panel| PanelSnapshot {
                    plugin: id.clone(),
                    units: self.doc.units.clone(),
                    plugin_name: package.name.clone(),
                    approval: package.approval.clone(),
                    ui_version: package.declarations.ui_version,
                    panel: panel.clone(),
                    data: package.data.get(&panel.id).cloned().unwrap_or_default(),
                    visible: package.visible.get(&panel.id).copied().unwrap_or(true),
                    visibility_revision: package.visibility_revision.get(&panel.id).copied().unwrap_or(0),
                })
            })
            .collect()
    }
}

fn caller<'a>(session: Option<&'a mut Session>, panel: &str) -> craft_plugin::Result<&'a mut PanelPackage> {
    let session = session.ok_or_else(|| HostError::InvalidInput { field: "ui".into(), reason: "no active transaction".into() })?;
    let package = session
        .plugin_state
        .ui_caller
        .as_ref()
        .and_then(|id| session.plugin_state.panels.get_mut(id))
        .ok_or_else(|| HostError::PermissionDenied { permission: "ui: no panel namespace for this component".into() })?;
    if package.declarations.panel(panel).is_none() {
        return Err(HostError::InvalidInput { field: "ui.panel".into(), reason: "unknown panel in this component's namespace".into() });
    }
    Ok(package)
}
pub(super) fn set_data(session: Option<&mut Session>, panel: &str, updates: &[ui::Datum]) -> craft_plugin::Result<()> {
    let package = caller(session, panel)?;
    let layout = &package
        .declarations
        .panel(panel)
        .ok_or_else(|| HostError::InvalidInput { field: "ui.panel".into(), reason: "unknown panel".into() })?
        .layout;
    package
        .data
        .get_mut(panel)
        .ok_or_else(|| HostError::InvalidInput { field: "ui.panel".into(), reason: "unknown panel".into() })?
        .set_data_for(layout, updates)
}
pub(super) fn clear_data(session: Option<&mut Session>, panel: &str) -> craft_plugin::Result<()> {
    if let Some(data) = caller(session, panel)?.data.get_mut(panel) {
        data.clear_data();
    }
    Ok(())
}
pub(super) fn show_panel(session: Option<&mut Session>, panel: &str, visible: bool) -> craft_plugin::Result<()> {
    let package = caller(session, panel)?;
    let revision = package
        .visibility_revision
        .get(panel)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| HostError::LimitExceeded { limit: "ui visibility requests".into() })?;
    package.visible.insert(panel.into(), visible);
    package.visibility_revision.insert(panel.into(), revision);
    Ok(())
}

/// Only the host envelope opts into event delivery; entry input remains plugin-owned.
#[cfg(any(test, all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
pub(super) fn event_for(manifest: &Manifest, entry: &str, event: Option<&ui::UiEvent>) -> Result<Option<ui::UiEvent>> {
    let Some(event) = event else { return Ok(None) };
    let declarations = manifest
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.ui.as_ref())
        .ok_or_else(|| HostError::InvalidInput { field: "ui_event".into(), reason: "the plugin does not declare a panel UI".into() })?;
    event.validate()?;
    let panel = declarations.panel(&event.panel).ok_or_else(|| error("unknown panel event target"))?;
    let command = format!("{}.{}", manifest.id, entry);
    if !panel.layout.nodes.iter().enumerate().any(|(index, _)| {
        u32::try_from(index).ok().and_then(|node| declarations.action_command(&manifest.id, node, event).ok()).as_ref() == Some(&command)
    }) {
        return Err(error("panel event does not name this entry's declared action"));
    }
    Ok(Some(event.clone()))
}
