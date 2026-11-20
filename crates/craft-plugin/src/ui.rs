//! Host-drawn plugin panels and workspaces (RFC 0015). Declarations are inert data.
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Effect, EntryKind, Manifest, Result, RuntimeManifest, invalid, plain, valid_command_name, valid_id};

/// Workspace hooks finish quickly even when the plugin's ordinary entries have larger budgets.
pub const WORKSPACE_HOOK_DEADLINE_MS: u64 = 1_000;
pub const MAX_PLUGIN_PANELS: usize = 8;
pub const MAX_PANEL_NODES: usize = 512;
pub const MAX_PLUGIN_WORKSPACES: usize = 2;
pub const MAX_WORKSPACE_RIBBON_TABS: usize = 6;
pub const MAX_WORKSPACE_RIBBON_ENTRIES: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockZone {
    Left,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DesignKind {
    Part,
    Assembly,
    Sketch,
    Drawing,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RibbonGroup {
    pub name: String,
    pub entries: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RibbonTab {
    pub name: String,
    pub groups: Vec<RibbonGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSpec {
    pub id: String,
    pub name: String,
    /// A name in the host's icon registry, never image data or a path.
    pub icon: String,
    pub applies_to: Vec<DesignKind>,
    #[serde(default)]
    pub ribbon: Vec<RibbonTab>,
    #[serde(default)]
    pub panels: Vec<(DockZone, Vec<String>)>,
    #[serde(default)]
    pub builtin_panels: Vec<String>,
    #[serde(default)]
    pub on_enter: Option<String>,
    #[serde(default)]
    pub on_exit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
    pub zones: Vec<DockZone>,
    pub layout: PanelLayout,
}

/// Flat node indexes avoid recursive input and allow bounded validation and drawing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelLayout {
    pub root: u32,
    pub nodes: Vec<PanelNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelNode {
    pub widget: PanelWidget,
}

/// The initial reviewed widget set. A newer kind remains loadable as a host placeholder.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PanelWidget {
    Group { children: Vec<u32> },
    Text { text: String },
    Button {
        label: String,
        #[serde(default)]
        icon: Option<String>,
        entry: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDeclarations {
    pub ui_version: u32,
    #[serde(default)]
    pub panels: Vec<PanelSpec>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSpec>,
}

impl Default for UiDeclarations {
    fn default() -> Self {
        Self { ui_version: 1, panels: Vec::new(), workspaces: Vec::new() }
    }
}

impl UiDeclarations {
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty() && self.workspaces.is_empty()
    }

    /// Validate the complete declaration before any part becomes visible or a hook can run.
    pub fn validate(&self, runtime: &RuntimeManifest) -> Result<()> {
        if self.ui_version == 0 {
            return Err(invalid("declarations.ui_version", "must be at least 1"));
        }
        if self.panels.len() > MAX_PLUGIN_PANELS || self.workspaces.len() > MAX_PLUGIN_WORKSPACES {
            return Err(invalid("declarations", "at most 8 panels and 2 workspaces per plugin"));
        }
        let mut ids = BTreeSet::new();
        for panel in &self.panels {
            if !valid_id(&panel.id) || !ids.insert(&panel.id) || !plain(&panel.title, 80) || !distinct_nonempty(&panel.zones, 3) {
                return Err(invalid("declarations.panels", "distinct lowercase ids, printable titles and 1..3 distinct dock zones are required"));
            }
            validate_layout(&panel.layout, runtime)?;
        }
        ids.clear();
        for workspace in &self.workspaces {
            if !valid_id(&workspace.id) || !ids.insert(&workspace.id) || !plain(&workspace.name, 80) || !valid_command_name(&workspace.icon) {
                return Err(invalid("declarations.workspaces", "distinct lowercase ids, printable names and icon registry names are required"));
            }
            if !distinct_nonempty(&workspace.applies_to, 4) {
                return Err(invalid("declarations.workspaces.applies_to", "requires 1..4 distinct design kinds"));
            }
            validate_ribbon(workspace, runtime)?;
            validate_placements(workspace, &self.panels)?;
            for hook in [&workspace.on_enter, &workspace.on_exit].into_iter().flatten() {
                let entry = runtime.entries.iter().find(|entry| entry.name == *hook);
                if !entry.is_some_and(|entry| entry.kind == EntryKind::Command && entry.effect == Effect::Read) {
                    return Err(invalid("declarations.workspaces.hooks", "enter and exit hooks must name declared read-only command entries"));
                }
            }
        }
        Ok(())
    }
}

fn distinct_nonempty<T: Ord>(values: &[T], max: usize) -> bool {
    !values.is_empty() && values.len() <= max && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn command_entry(runtime: &RuntimeManifest, name: &str) -> bool {
    runtime.entries.iter().any(|entry| entry.kind == EntryKind::Command && entry.name == name)
}

fn validate_ribbon(workspace: &WorkspaceSpec, runtime: &RuntimeManifest) -> Result<()> {
    let mut count = 0usize;
    if workspace.ribbon.len() > MAX_WORKSPACE_RIBBON_TABS {
        return Err(invalid("declarations.workspaces.ribbon", "at most 6 ribbon tabs per workspace"));
    }
    let mut tabs = BTreeSet::new();
    for tab in &workspace.ribbon {
        if !plain(&tab.name, 80) || !tabs.insert(&tab.name) || tab.groups.len() > MAX_WORKSPACE_RIBBON_ENTRIES {
            return Err(invalid("declarations.workspaces.ribbon", "distinct printable tab names and at most 60 groups per tab"));
        }
        let mut groups = BTreeSet::new();
        for group in &tab.groups {
            if !plain(&group.name, 80) || !groups.insert(&group.name) {
                return Err(invalid("declarations.workspaces.ribbon", "distinct printable group names are required"));
            }
            count = count.saturating_add(group.entries.len());
            if count > MAX_WORKSPACE_RIBBON_ENTRIES || group.entries.iter().any(|entry| !command_entry(runtime, entry)) {
                return Err(invalid("declarations.workspaces.ribbon", "at most 60 entries per workspace, each naming a declared command entry"));
            }
        }
    }
    Ok(())
}

fn validate_placements(workspace: &WorkspaceSpec, panels: &[PanelSpec]) -> Result<()> {
    let mut zones = BTreeSet::new();
    let mut placed = BTreeSet::new();
    if workspace.panels.len() > 3 {
        return Err(invalid("declarations.workspaces.panels", "at most one placement per dock zone"));
    }
    for (zone, ids) in &workspace.panels {
        if !zones.insert(zone) || ids.len() > MAX_PLUGIN_PANELS {
            return Err(invalid("declarations.workspaces.panels", "distinct dock zones and at most 8 panels per zone"));
        }
        for id in ids {
            if !placed.insert(id) || !panels.iter().any(|panel| panel.id == *id && panel.zones.contains(zone)) {
                return Err(invalid("declarations.workspaces.panels", "each panel appears once in one of its declared dock zones"));
            }
        }
    }
    let mut builtin = BTreeSet::new();
    if workspace.builtin_panels.len() > 32 || workspace.builtin_panels.iter().any(|id| !valid_id(id) || !builtin.insert(id)) {
        return Err(invalid("declarations.workspaces.builtin_panels", "at most 32 distinct host panel ids"));
    }
    Ok(())
}

fn validate_layout(layout: &PanelLayout, runtime: &RuntimeManifest) -> Result<()> {
    if layout.nodes.is_empty() || layout.nodes.len() > MAX_PANEL_NODES || layout.nodes.get(layout.root as usize).is_none() {
        return Err(invalid("declarations.panels.layout", "requires 1..512 nodes and a valid root index"));
    }
    let mut incoming = vec![0usize; layout.nodes.len()];
    for node in &layout.nodes {
        match &node.widget {
            PanelWidget::Group { children } => {
                if children.len() > MAX_PANEL_NODES {
                    return Err(invalid("declarations.panels.layout", "at most 512 children per group"));
                }
                for child in children {
                    let count = incoming
                        .get_mut(*child as usize)
                        .ok_or_else(|| invalid("declarations.panels.layout", "child index is outside the layout"))?;
                    *count += 1;
                    if *count > 1 || *child == layout.root {
                        return Err(invalid("declarations.panels.layout", "layout nodes form a tree without shared children or cycles"));
                    }
                }
            }
            PanelWidget::Text { text } if !plain(text, 4096) => {
                return Err(invalid("declarations.panels.layout", "text requires 1..4096 printable characters"));
            }
            PanelWidget::Button { label, icon, entry }
                if !plain(label, 80) || icon.as_ref().is_some_and(|icon| !valid_command_name(icon)) || !command_entry(runtime, entry) =>
            {
                return Err(invalid("declarations.panels.layout", "buttons require printable labels, registry icons and declared command entries"));
            }
            _ => {}
        }
    }
    // Every node must be reachable from the root. Together with the single-parent check this
    // rules out disconnected cycles without recursion or an input-controlled call stack.
    let mut stack = vec![layout.root];
    let mut seen = BTreeSet::new();
    while let Some(index) = stack.pop() {
        if !seen.insert(index) {
            return Err(invalid("declarations.panels.layout", "layout contains a cycle"));
        }
        if let Some(PanelNode { widget: PanelWidget::Group { children } }) = layout.nodes.get(index as usize) {
            stack.extend(children.iter().copied());
        }
    }
    if seen.len() != layout.nodes.len() {
        return Err(invalid("declarations.panels.layout", "every layout node must be reachable from its root"));
    }
    Ok(())
}

pub(crate) fn check_declarations(manifest: &Manifest) -> Result<()> {
    let enabled = manifest.capabilities.iter().any(|capability| capability.starts_with("workspaces@"));
    if !enabled {
        if !manifest.declarations.is_empty() {
            return Err(invalid("declarations", "plugin UI requires the workspaces@0.1 capability"));
        }
        return Ok(());
    }
    let runtime = manifest
        .runtime
        .as_ref()
        .filter(|runtime| !runtime.is_importer())
        .ok_or_else(|| invalid("capabilities", "workspaces requires a command runtime and commands capability"))?;
    manifest.declarations.validate(runtime)
}
