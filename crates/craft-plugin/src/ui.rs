//! Declarative, app-independent plugin panels. Values are millimetres and degrees.
use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Effect, EntryKind, HostError, Result, RuntimeManifest, invalid, plain, valid_command_name, valid_id};

pub const UI_VERSION: u32 = 1;
pub const MAX_PANELS: usize = 8;
pub const MAX_WIDGETS: usize = 512;
pub const MAX_PANEL_BYTES: usize = 1024 * 1024;
pub const MAX_ROWS: usize = 100_000;
pub const MAX_CHART_SERIES: usize = 8;
pub const MAX_CHART_POINTS: usize = 20_000;
/// The longest a workspace's enter or exit hook may run.
pub const WORKSPACE_HOOK_DEADLINE_MS: u64 = 1_000;
pub const MAX_WORKSPACES: usize = 2;
pub const MAX_WORKSPACE_RIBBON_TABS: usize = 6;
pub const MAX_WORKSPACE_RIBBON_ENTRIES: usize = 60;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    pub ui_version: u32,
    pub panels: Vec<PanelSpec>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSpec>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
    pub zones: Vec<DockZone>,
    pub layout: Layout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockZone {
    Left,
    Right,
    Bottom,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub root: u32,
    pub nodes: Vec<Node>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub widget: Widget,
}
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Widget {
    Group(Vec<u32>),
    Tabs(Vec<Tab>),
    Split(SplitSpec),
    Toolbar(Vec<ButtonSpec>),
    Button(ButtonSpec),
    Form(FormSpec),
    ItemList(ListSpec),
    ItemTree(TreeSpec),
    Table(TableSpec),
    Chart(ChartSpec),
    Text(String),
    Progress(String),
    EmptyState(String),
    Unknown,
}
impl<'de> Deserialize<'de> for Widget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let (tag, payload) = match Value::deserialize(deserializer)? {
            Value::Object(object) if object.len() == 1 => {
                object.into_iter().next().ok_or_else(|| <D::Error as serde::de::Error>::custom("widget must have one kind"))?
            }
            Value::String(tag) => (tag, Value::Null),
            _ => return Err(serde::de::Error::custom("widget must have one kind")),
        };
        let result = match tag.as_str() {
            "group" => serde_json::from_value(payload).map(Self::Group),
            "tabs" => serde_json::from_value(payload).map(Self::Tabs),
            "split" => serde_json::from_value(payload).map(Self::Split),
            "toolbar" => serde_json::from_value(payload).map(Self::Toolbar),
            "button" => serde_json::from_value(payload).map(Self::Button),
            "form" => serde_json::from_value(payload).map(Self::Form),
            "item-list" => serde_json::from_value(payload).map(Self::ItemList),
            "item-tree" => serde_json::from_value(payload).map(Self::ItemTree),
            "table" => serde_json::from_value(payload).map(Self::Table),
            "chart" => serde_json::from_value(payload).map(Self::Chart),
            "text" => serde_json::from_value(payload).map(Self::Text),
            "progress" => serde_json::from_value(payload).map(Self::Progress),
            "empty-state" => serde_json::from_value(payload).map(Self::EmptyState),
            _ => Ok(Self::Unknown),
        };
        result.map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Number,
    Length,
    Angle,
    Text,
    Toggle,
    Choice,
    Selection,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub choices: Vec<String>,
    pub on_change: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub key: String,
    pub label: String,
    pub unit: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SelectionMode {
    None,
    Single,
    Multi,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    pub bind: String,
    pub row_id: String,
    pub columns: Vec<Column>,
    pub select: SelectionMode,
    pub on_select: Option<String>,
    pub on_edit: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ButtonSpec {
    pub label: String,
    pub icon: Option<String>,
    pub entry: String,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub label: String,
    pub child: u32,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitSpec {
    pub vertical: bool,
    pub first: u32,
    pub second: u32,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormSpec {
    pub bind: String,
    pub fields: Vec<Field>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSpec {
    pub bind: String,
    pub on_select: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TreeSpec {
    pub bind: String,
    pub on_select: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub series: Vec<ChartSeries>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Line,
    Bar,
    Scatter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ColorRole {
    Accent,
    Ok,
    Warn,
    Danger,
    Dim,
    #[serde(rename = "series-1")]
    Series1,
    #[serde(rename = "series-2")]
    Series2,
    #[serde(rename = "series-3")]
    Series3,
    #[serde(rename = "series-4")]
    Series4,
    #[serde(rename = "series-5")]
    Series5,
    #[serde(rename = "series-6")]
    Series6,
    #[serde(rename = "series-7")]
    Series7,
    #[serde(rename = "series-8")]
    Series8,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartSeries {
    pub label: String,
    pub role: ColorRole,
    pub x: String,
    pub y: String,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RibbonGroup {
    pub name: String,
    pub entries: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RibbonTab {
    pub name: String,
    pub groups: Vec<RibbonGroup>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DesignKind {
    Part,
    Assembly,
    Sketch,
    Drawing,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSpec {
    pub id: String,
    pub name: String,
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
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum UiValue {
    None,
    Flag(bool),
    Integer(i64),
    Number(f64),
    Text(String),
    GeometryRef(String),
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Datum {
    pub path: String,
    pub value: UiValue,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Press,
    Select,
    Edit,
    Change,
    Enter,
    Exit,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiEvent {
    pub panel: String,
    pub event: EventKind,
    pub path: String,
    /// An opaque row id, separate from the slash path; it may itself contain slashes.
    pub row: Option<String>,
    pub value: Option<UiValue>,
}

impl UiEvent {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.panel)
            || self.path.len() > 1024
            || self.path.chars().any(char::is_control)
            || self.row.as_ref().is_some_and(|row| !plain(row, 1024))
        {
            return Err(HostError::InvalidInput {
                field: "ui.event".into(),
                reason: "requires a panel id, printable path and optional row id".into(),
            });
        }
        match &self.value {
            Some(UiValue::Number(value)) if !value.is_finite() => {
                Err(HostError::InvalidInput { field: "ui.event.value".into(), reason: "number must be finite".into() })
            }
            Some(UiValue::Text(text) | UiValue::GeometryRef(text)) if text.len() > MAX_PANEL_BYTES => Err(limit("ui.event.value: at most 1 MiB")),
            _ => Ok(()),
        }
    }
}

fn limit(name: &str) -> HostError {
    HostError::LimitExceeded { limit: name.into() }
}
fn check_bytes(value: &impl Serialize, name: &str) -> Result<()> {
    struct Counter {
        bytes: usize,
        exceeded: bool,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes = self.bytes.saturating_add(bytes.len());
            if self.bytes > MAX_PANEL_BYTES {
                self.exceeded = true;
                return Err(std::io::Error::other("UI byte limit"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, exceeded: false };
    let result = serde_json::to_writer(&mut counter, value);
    if counter.exceeded {
        return Err(limit(name));
    }
    result.map_err(|error| HostError::InvalidInput { field: "ui".into(), reason: error.to_string() })
}

fn check_path(path: &str) -> Result<()> {
    if path.len() > 1024 || !path.starts_with('/') || path.split('/').skip(1).any(|part| part.is_empty() || part.chars().any(char::is_control)) {
        return Err(HostError::InvalidInput {
            field: "ui.path".into(),
            reason: "requires a slash path with nonempty printable segments, at most 1024 bytes".into(),
        });
    }
    Ok(())
}
fn command_entry(runtime: &RuntimeManifest, name: &str) -> bool {
    runtime.entries.iter().any(|entry| entry.kind == EntryKind::Command && entry.name == name)
}

fn validate_ribbon(workspace: &WorkspaceSpec, runtime: &RuntimeManifest) -> Result<()> {
    let mut count = 0usize;
    if workspace.ribbon.len() > MAX_WORKSPACE_RIBBON_TABS {
        return Err(limit("ui.workspace.ribbon: at most 6 tabs and 60 entries"));
    }
    let mut tabs = BTreeSet::new();
    for tab in &workspace.ribbon {
        if !plain(&tab.name, 80) || !tabs.insert(&tab.name) || tab.groups.len() > MAX_WORKSPACE_RIBBON_ENTRIES {
            return Err(invalid("runtime.ui.workspaces.ribbon", "distinct printable tab names and at most 60 groups per tab"));
        }
        let mut groups = BTreeSet::new();
        for group in &tab.groups {
            if !plain(&group.name, 80) || !groups.insert(&group.name) {
                return Err(invalid("runtime.ui.workspaces.ribbon", "distinct printable group names are required"));
            }
            count = count.saturating_add(group.entries.len());
            if count > MAX_WORKSPACE_RIBBON_ENTRIES {
                return Err(limit("ui.workspace.ribbon: at most 6 tabs and 60 entries"));
            }
            if group.entries.iter().any(|entry| !command_entry(runtime, entry)) {
                return Err(invalid("runtime.ui.workspaces.ribbon", "each ribbon entry must name a declared command entry"));
            }
        }
    }
    Ok(())
}

fn validate_placements(workspace: &WorkspaceSpec, panels: &[PanelSpec]) -> Result<()> {
    let mut zones = BTreeSet::new();
    let mut placed = BTreeSet::new();
    if workspace.panels.len() > 3 {
        return Err(invalid("runtime.ui.workspaces.panels", "at most one placement per dock zone"));
    }
    for (zone, ids) in &workspace.panels {
        if !zones.insert(zone) || ids.len() > MAX_PANELS {
            return Err(invalid("runtime.ui.workspaces.panels", "distinct dock zones and at most 8 panels per zone"));
        }
        for id in ids {
            if !placed.insert(id) || !panels.iter().any(|panel| panel.id == *id && panel.zones.contains(zone)) {
                return Err(invalid("runtime.ui.workspaces.panels", "each panel appears once in one of its declared dock zones"));
            }
        }
    }
    let mut builtin = BTreeSet::new();
    if workspace.builtin_panels.len() > 32 || workspace.builtin_panels.iter().any(|id| !valid_id(id) || !builtin.insert(id)) {
        return Err(invalid("runtime.ui.workspaces.builtin_panels", "at most 32 distinct host panel ids"));
    }
    Ok(())
}

impl Declarations {
    pub fn validate(&self, runtime: &RuntimeManifest) -> Result<()> {
        check_bytes(self, "ui.declarations: at most 1 MiB")?;
        if self.panels.len() > MAX_PANELS {
            return Err(limit("ui.panels: at most 8 panels"));
        }
        if self.workspaces.len() > MAX_WORKSPACES {
            return Err(limit("ui.workspaces: at most 2 workspaces"));
        }
        let mut ids = BTreeSet::new();
        for panel in &self.panels {
            if !valid_id(&panel.id) || !ids.insert(&panel.id) || !plain(&panel.title, 240) || panel.zones.is_empty() || panel.zones.len() > 3 {
                return Err(invalid("runtime.ui.panels", "distinct panel ids, printable titles and 1..3 dock zones required"));
            }
            if panel.zones.iter().collect::<BTreeSet<_>>().len() != panel.zones.len() {
                return Err(invalid("runtime.ui.panels.zones", "distinct dock zones required"));
            }
            panel.layout.validate(runtime)?;
        }
        ids.clear();
        for workspace in &self.workspaces {
            if !valid_id(&workspace.id) || !ids.insert(&workspace.id) || !plain(&workspace.name, 80) || !valid_command_name(&workspace.icon) {
                return Err(invalid("runtime.ui.workspaces", "distinct lowercase ids, printable names and icon registry names are required"));
            }
            let kinds: BTreeSet<_> = workspace.applies_to.iter().collect();
            if kinds.is_empty() || kinds.len() != workspace.applies_to.len() {
                return Err(invalid("runtime.ui.workspaces.applies_to", "requires 1..4 distinct design kinds"));
            }
            validate_ribbon(workspace, runtime)?;
            validate_placements(workspace, &self.panels)?;
            for hook in [&workspace.on_enter, &workspace.on_exit].into_iter().flatten() {
                let entry = runtime.entries.iter().find(|entry| entry.name == *hook);
                if !entry.is_some_and(|entry| entry.kind == EntryKind::Command && entry.effect == Effect::Read) {
                    return Err(invalid("runtime.ui.workspaces.hooks", "enter and exit hooks must name declared read-only command entries"));
                }
            }
        }
        Ok(())
    }
    pub fn panel(&self, id: &str) -> Option<&PanelSpec> {
        self.panels.iter().find(|panel| panel.id == id)
    }
    /// Resolve an action only through the declaration, never a plugin-supplied command id.
    pub fn action_command(&self, plugin: &str, node: u32, event: &UiEvent) -> Result<String> {
        event.validate()?;
        if !valid_id(plugin) {
            return Err(invalid("ui.event.plugin", "unknown plugin"));
        }
        let panel = self.panel(&event.panel).ok_or_else(|| invalid("ui.event.panel", "unknown panel"))?;
        let widget = panel.layout.nodes.get(node as usize).ok_or_else(|| invalid("ui.event.node", "unknown node"))?;
        let entry = match (&widget.widget, event.event) {
            (Widget::Button(button), EventKind::Press) if event.path == button.entry => Some(button.entry.as_str()),
            (Widget::Toolbar(buttons), EventKind::Press) => {
                buttons.iter().find(|button| button.entry == event.path).map(|button| button.entry.as_str())
            }
            (Widget::Form(form), EventKind::Change) => {
                form.fields.iter().find(|field| event.path == format!("{}/{}", form.bind, field.key)).and_then(|field| field.on_change.as_deref())
            }
            (Widget::ItemList(list), EventKind::Select) if event.path == list.bind => list.on_select.as_deref(),
            (Widget::Table(table), EventKind::Select) if event.path == table.bind && table.select != SelectionMode::None => {
                table.on_select.as_deref()
            }
            (Widget::Table(table), EventKind::Edit) if event.path == table.bind => table.on_edit.as_deref(),
            _ => None,
        }
        .ok_or_else(|| invalid("ui.event", "the widget has no declared action for this event"))?;
        Ok(format!("{plugin}.{entry}"))
    }
}
impl Layout {
    fn children(&self, index: usize) -> Vec<u32> {
        match self.nodes.get(index).map(|node| &node.widget) {
            Some(Widget::Group(children)) => children.clone(),
            Some(Widget::Tabs(tabs)) => tabs.iter().map(|tab| tab.child).collect(),
            Some(Widget::Split(split)) => vec![split.first, split.second],
            _ => Vec::new(),
        }
    }
    pub fn validate(&self, runtime: &RuntimeManifest) -> Result<()> {
        let widgets = self.nodes.iter().fold(0usize, |total, node| {
            total.saturating_add(1).saturating_add(match &node.widget {
                Widget::Toolbar(buttons) => buttons.len(),
                Widget::Form(form) => form.fields.len(),
                Widget::Table(table) => table.columns.len(),
                _ => 0,
            })
        });
        if widgets > MAX_WIDGETS {
            return Err(limit("ui.layout: at most 512 widgets"));
        }
        if self.nodes.get(self.root as usize).is_none() {
            return Err(invalid("runtime.ui.layout.root", "root must name a node"));
        }
        // Every edge is checked. A rooted tree bounds rendering to at most 512 visits.
        let mut indegree = vec![0usize; self.nodes.len()];
        for index in 0..self.nodes.len() {
            self.validate_widget(index, runtime)?;
            for child in self.children(index) {
                let degree = indegree.get_mut(child as usize).ok_or_else(|| invalid("runtime.ui.layout", "child must name a node"))?;
                *degree += 1;
                if *degree > 1 {
                    return Err(invalid("runtime.ui.layout", "a node may have only one parent"));
                }
            }
        }
        // An unknown future container may own children this build cannot interpret. It is
        // rendered as one placeholder, so disconnected nodes under it remain inert.
        let has_unknown = self.nodes.iter().any(|node| matches!(node.widget, Widget::Unknown));
        if indegree.get(self.root as usize) != Some(&0)
            || (!has_unknown && indegree.iter().enumerate().any(|(index, degree)| index != self.root as usize && *degree != 1))
        {
            return Err(invalid("runtime.ui.layout", "all nodes must belong to the root tree"));
        }
        let mut ready: Vec<usize> = indegree.iter().enumerate().filter(|(_, degree)| **degree == 0).map(|(index, _)| index).collect();
        let mut visited = 0;
        while let Some(index) = ready.pop() {
            visited += 1;
            for child in self.children(index) {
                if let Some(degree) = indegree.get_mut(child as usize) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(child as usize);
                    }
                }
            }
        }
        if visited != self.nodes.len() {
            return Err(invalid("runtime.ui.layout", "layout must not contain cycles"));
        }
        Ok(())
    }
    fn validate_widget(&self, index: usize, runtime: &RuntimeManifest) -> Result<()> {
        let entry = |name: &str| {
            if runtime.entries.iter().any(|entry| entry.name == name && entry.kind == crate::EntryKind::Command) {
                Ok(())
            } else {
                Err(invalid("runtime.ui.entry", format!("unknown command entry `{name}`")))
            }
        };
        let optional = |name: &Option<String>| name.as_deref().map_or(Ok(()), entry);
        let button = |button: &ButtonSpec| {
            if !plain(&button.label, 240) || button.icon.as_ref().is_some_and(|icon| !plain(icon, 80)) {
                return Err(invalid("runtime.ui.label", "printable label required"));
            }
            entry(&button.entry)
        };
        match self.nodes.get(index).map(|node| &node.widget) {
            Some(Widget::Button(spec)) => button(spec)?,
            Some(Widget::Toolbar(buttons)) => {
                if buttons.len() > MAX_WIDGETS {
                    return Err(limit("ui.toolbar"));
                }
                for spec in buttons {
                    button(spec)?;
                }
            }
            Some(Widget::Form(form)) => {
                check_path(&form.bind)?;
                if form.fields.len() > MAX_WIDGETS {
                    return Err(limit("ui.form.fields"));
                }
                let mut keys = BTreeSet::new();
                for field in &form.fields {
                    if !plain(&field.key, 128)
                        || field.key.contains('/')
                        || !keys.insert(&field.key)
                        || !plain(&field.label, 240)
                        || field.choices.len() > MAX_WIDGETS
                        || field.choices.iter().any(|choice| !plain(choice, 240))
                    {
                        return Err(invalid("runtime.ui.form", "distinct field keys and printable labels required"));
                    }
                    optional(&field.on_change)?;
                }
            }
            Some(Widget::ItemList(list)) => {
                check_path(&list.bind)?;
                optional(&list.on_select)?;
            }
            Some(Widget::ItemTree(tree)) => {
                check_path(&tree.bind)?;
                optional(&tree.on_select)?;
            }
            Some(Widget::Table(table)) => {
                check_path(&table.bind)?;
                if !plain(&table.row_id, 128) || table.row_id.contains('/') {
                    return Err(invalid("runtime.ui.table.row_id", "row id must name a field"));
                }
                optional(&table.on_select)?;
                optional(&table.on_edit)?;
                if table.columns.len() > MAX_WIDGETS {
                    return Err(limit("ui.table.columns"));
                }
                let mut keys = BTreeSet::new();
                for column in &table.columns {
                    if !plain(&column.key, 128)
                        || column.key.contains('/')
                        || !keys.insert(&column.key)
                        || !plain(&column.label, 240)
                        || column.unit.as_ref().is_some_and(|unit| !plain(unit, 80))
                    {
                        return Err(invalid("runtime.ui.table.columns", "distinct column keys and printable labels required"));
                    }
                }
            }
            Some(Widget::Text(text) | Widget::EmptyState(text)) if text.len() > 4096 || text.chars().any(char::is_control) => {
                return Err(invalid("runtime.ui.text", "at most 4096 printable bytes"));
            }
            Some(Widget::Progress(path)) => check_path(path)?,
            Some(Widget::Tabs(tabs)) if tabs.iter().any(|tab| !plain(&tab.label, 240)) => {
                return Err(invalid("runtime.ui.tabs", "printable labels required"));
            }
            Some(Widget::Chart(chart)) => {
                if chart.series.len() > MAX_CHART_SERIES {
                    return Err(limit("ui.chart: at most 8 series"));
                }
                for series in &chart.series {
                    check_path(&series.x)?;
                    check_path(&series.y)?;
                }
            }
            Some(Widget::Group(children)) if children.len() > MAX_WIDGETS => return Err(limit("ui.group")),
            Some(Widget::Tabs(tabs)) if tabs.len() > MAX_WIDGETS => return Err(limit("ui.tabs")),
            _ => (),
        }
        Ok(())
    }
}

/// Successful calls update a private pending snapshot. The renderer adopts it once per frame.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PanelData {
    values: BTreeMap<String, UiValue>,
    #[serde(skip)]
    pending: Option<BTreeMap<String, UiValue>>,
}
// Frame adoption changes presentation, not the logical state used to reject stale jobs.
impl PartialEq for PanelData {
    fn eq(&self, other: &Self) -> bool {
        self.pending.as_ref().unwrap_or(&self.values) == other.pending.as_ref().unwrap_or(&other.values)
    }
}

impl PanelData {
    pub fn values(&self) -> &BTreeMap<String, UiValue> {
        &self.values
    }
    pub fn get(&self, path: &str) -> Option<&UiValue> {
        self.values.get(path)
    }
    pub fn set_data(&mut self, updates: &[Datum]) -> Result<()> {
        if updates.len() > MAX_PANEL_BYTES {
            return Err(limit("ui.data: at most 1 MiB"));
        }
        // Bound hostile input before cloning payloads or serializing an unbounded message.
        let mut incoming = 0usize;
        for datum in updates {
            let bytes = match &datum.value {
                UiValue::Text(text) | UiValue::GeometryRef(text) => text.len(),
                _ => 24,
            };
            incoming = incoming.saturating_add(datum.path.len()).saturating_add(bytes);
            if incoming > MAX_PANEL_BYTES {
                return Err(limit("ui.data: at most 1 MiB"));
            }
        }
        let mut next = self.pending.as_ref().unwrap_or(&self.values).clone();
        for datum in updates {
            check_path(&datum.path)?;
            match &datum.value {
                UiValue::Number(value) if !value.is_finite() => {
                    return Err(HostError::InvalidInput { field: datum.path.clone(), reason: "number must be finite".into() });
                }
                UiValue::Text(text) | UiValue::GeometryRef(text) if text.len() > MAX_PANEL_BYTES => return Err(limit("ui.data: at most 1 MiB")),
                _ => (),
            }
            next.insert(datum.path.clone(), datum.value.clone());
        }
        check_bytes(&next, "ui.data: at most 1 MiB")?;
        // A slash row is bounded independently of its number of cells. No integer indexing or sparse allocation.
        let mut rows: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for path in next.keys() {
            if let Some((bind, rest)) = path.strip_prefix('/').and_then(|path| path.split_once('/')) {
                let row = rest.split('/').next().unwrap_or_default();
                let set = rows.entry(bind).or_default();
                set.insert(row);
                if set.len() > MAX_ROWS {
                    return Err(limit("ui.rows: at most 100000 rows"));
                }
            }
        }
        self.pending = Some(next);
        Ok(())
    }
    /// Apply data with the layout's row and chart point limits, all or nothing.
    pub fn set_data_for(&mut self, layout: &Layout, updates: &[Datum]) -> Result<()> {
        let mut candidate = self.clone();
        candidate.set_data(updates)?;
        let values = candidate.pending.as_ref().unwrap_or(&candidate.values);
        for node in &layout.nodes {
            let check_rows = |bind: &str, cap: usize| {
                let prefix = format!("{}/", bind.trim_end_matches('/'));
                let rows: BTreeSet<_> =
                    values.keys().filter_map(|path| path.strip_prefix(&prefix)).filter_map(|rest| rest.split('/').next()).collect();
                if rows.len() > cap {
                    Err(limit(if cap == MAX_CHART_POINTS { "ui.chart: at most 20000 points" } else { "ui.rows: at most 100000 rows" }))
                } else {
                    Ok(())
                }
            };
            match &node.widget {
                Widget::Table(table) => check_rows(&table.bind, MAX_ROWS)?,
                Widget::ItemList(list) => check_rows(&list.bind, MAX_ROWS)?,
                Widget::ItemTree(tree) => check_rows(&tree.bind, MAX_ROWS)?,
                Widget::Chart(chart) => {
                    for series in &chart.series {
                        check_rows(&series.x, MAX_CHART_POINTS)?;
                        check_rows(&series.y, MAX_CHART_POINTS)?;
                    }
                }
                _ => (),
            }
        }
        *self = candidate;
        Ok(())
    }
    pub fn clear_data(&mut self) {
        self.pending = Some(BTreeMap::new());
    }
    pub fn merge_frame(&mut self) -> bool {
        if let Some(next) = self.pending.take() {
            let changed = self.values != next;
            self.values = next;
            changed
        } else {
            false
        }
    }
    pub fn rows(&self, bind: &str) -> Vec<(String, BTreeMap<String, UiValue>)> {
        let mut rows: BTreeMap<String, BTreeMap<String, UiValue>> = BTreeMap::new();
        let prefix = format!("{}/", bind.trim_end_matches('/'));
        for (path, value) in &self.values {
            if let Some(rest) = path.strip_prefix(&prefix) {
                let (row, key) = rest.split_once('/').unwrap_or((rest, ""));
                rows.entry(row.into()).or_default().insert(key.into(), value.clone());
            }
        }
        let mut rows: Vec<_> = rows.into_iter().collect();
        rows.sort_by(|(left, _), (right, _)| match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left_number), Ok(right_number)) => left_number.cmp(&right_number).then_with(|| left.cmp(right)),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => left.cmp(right),
        });
        rows
    }
    pub fn json(&self) -> Result<Value> {
        serde_json::to_value(&self.values).map_err(|error| HostError::InvalidInput { field: "ui.data".into(), reason: error.to_string() })
    }
}
