//! Strict external application-layout contracts. These are command parameters,
//! not another persistent layout model.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::panels::*;

/// Discovery and ParamCheck use the exact parser the injected executor uses.
pub struct CommandContract<P> {
    id: &'static str,
    typed: super::TypedContract<P>,
}
impl<P> CommandContract<P> {
    pub const fn new(id: &'static str) -> Self {
        Self { id, typed: super::TypedContract::new() }
    }
}
impl<P: serde::de::DeserializeOwned + JsonSchema + 'static> super::Contract for CommandContract<P> {
    fn input_schema(&self) -> schemars::Schema {
        super::Contract::input_schema(&self.typed)
    }
    fn allowed_keys(&self) -> std::sync::Arc<std::collections::BTreeSet<String>> {
        super::Contract::allowed_keys(&self.typed)
    }
    fn check(&self, params: &serde_json::Value) -> Vec<super::Issue> {
        match crate::panels::commands::parse_command(self.id, params) {
            Ok(_) => Vec::new(),
            Err(reason) => {
                // Keep the shared parser's bounds ahead of deserialization, but
                // retain ordinary contract paths and unknown-key suggestions.
                if reason.rule == Rule::Schema {
                    let issues = super::Contract::check(&self.typed, params);
                    if !issues.is_empty() {
                        return issues;
                    }
                }
                vec![super::Issue { path: String::new(), message: reason.to_string(), suggestion: None }]
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Workspace(WorkspaceId),
    Instance(InstanceId),
    Section(SectionId),
    Column(ColumnId),
    NativeHost(HostId),
}
impl From<Target> for LayoutTarget {
    fn from(value: Target) -> Self {
        match value {
            Target::Workspace(id) => Self::Workspace(id),
            Target::Instance(id) => Self::Instance(id),
            Target::Section(id) => Self::Section(id),
            Target::Column(id) => Self::Column(id),
            Target::NativeHost(id) => Self::NativeHost(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OuterSize {
    pub width: f32,
    pub height: f32,
}
impl From<OuterSize> for Size {
    fn from(value: OuterSize) -> Self {
        Self { width: value.width, height: value.height }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub x: f32,
    pub y: f32,
}
impl From<Origin> for Point {
    fn from(value: Origin) -> Self {
        Self { x: value.x, y: value.y }
    }
}
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReturnJoin {
    pub section: SectionId,
    pub index: usize,
}
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReturnTo {
    pub zone: Zone,
    pub index: usize,
    pub size: OuterSize,
    pub collapsed: bool,
    #[serde(default)]
    pub section: Option<SectionId>,
    #[serde(default)]
    pub column: Option<ColumnId>,
    #[serde(default)]
    pub column_index: usize,
    #[serde(default)]
    pub column_width: Option<f32>,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default)]
    pub preferred_extent: Option<f32>,
    #[serde(default)]
    pub extent_priority: u16,
    #[serde(default)]
    pub join: Option<ReturnJoin>,
}
impl From<ReturnTo> for DockReturn {
    fn from(value: ReturnTo) -> Self {
        Self {
            zone: value.zone,
            index: value.index,
            size: value.size.into(),
            collapsed: value.collapsed,
            section: value.section,
            column: value.column,
            column_index: value.column_index,
            column_width: value.column_width,
            weight: value.weight,
            preferred_extent: value.preferred_extent,
            extent_priority: value.extent_priority,
            join: value.join.map(|join| ReturnTab { section: join.section, index: join.index }),
        }
    }
}
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Location {
    Dock { zone: Zone },
    Floating { origin: Origin },
    Window { host: HostId, origin: Origin, return_to: ReturnTo },
}
impl From<Location> for SectionLocation {
    fn from(value: Location) -> Self {
        match value {
            Location::Dock { zone } => Self::Dock { zone },
            Location::Floating { origin } => Self::Floating { origin: origin.into() },
            Location::Window { host, origin, return_to } => Self::Window { host, origin: origin.into(), return_to: return_to.into() },
        }
    }
}
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    NewSection { section: SectionId, index: usize, location: Location, size: OuterSize },
    JoinTab { section: SectionId, index: usize },
    Relative { section: SectionId, placement: RelativePlacement, new_section: SectionId, new_column: Option<ColumnId>, size: OuterSize },
}
impl From<Placement> for Destination {
    fn from(value: Placement) -> Self {
        match value {
            Placement::NewSection { section, index, location, size } => {
                Self::NewSection { section, index, location: location.into(), size: size.into() }
            }
            Placement::JoinTab { section, index } => Self::JoinTab { section, index },
            Placement::Relative { section, placement, new_section, new_column, size } => {
                Self::Relative { section, placement, new_section, new_column, size: size.into() }
            }
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Open {
    pub kind: PanelKind,
    pub instance: InstanceId,
    pub destination: Option<Placement>,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Select {
    pub target: Option<Target>,
    /// Compatibility selector: exact instance, or a single-instance canonical kind.
    pub id: Option<String>,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub target: Option<Target>,
    pub id: Option<String>,
    pub destination: Placement,
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub activate: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeaderAction {
    pub target: Option<Target>,
    pub id: Option<String>,
    pub action: ControlId,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resize {
    pub target: Option<Target>,
    pub id: Option<String>,
    pub size: OuterSize,
    /// Dock thickness only: Bottom height or side column width. Both size
    /// components must be valid; the other component is ignored.
    #[serde(default)]
    pub thickness_only: bool,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Collapse {
    pub target: Option<Target>,
    pub id: Option<String>,
    pub collapsed: bool,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResizePair {
    pub first: SectionId,
    pub second: SectionId,
    pub first_size: OuterSize,
    pub second_size: OuterSize,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetPair {
    pub first: SectionId,
    pub second: SectionId,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub name: String,
    #[serde(default)]
    pub replace: bool,
    /// Compatibility spelling accepted by earlier panel command clients.
    #[serde(default)]
    pub overwrite: bool,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    pub name: String,
    pub expected_revision: Option<u64>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSelect {
    pub workspace: WorkspaceId,
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub title_key: String,
    pub default_preset: String,
    pub has_remembered_layout: bool,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetReply {
    pub state: LayoutReply,
    pub workspaces: Vec<WorkspaceSummary>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListReply {
    pub state: LayoutReply,
    pub kinds: Vec<PanelSpec>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveReply {
    pub name: String,
    pub overwritten: bool,
    pub state: LayoutReply,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceSource {
    Remembered,
    DefaultPreset,
    Unchanged,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReply {
    pub previous_workspace: WorkspaceId,
    pub source: WorkspaceSource,
    pub state: LayoutReply,
}

/// Successful wire replies have no extensible object fields. Maps (availability)
/// remain maps. Persistence schemas intentionally retain their own opaque fields.
pub fn strict_output<T: JsonSchema>() -> schemars::Schema {
    let mut schema = super::output_schema_for::<T>();
    fn seal(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("properties") {
                    object.insert("additionalProperties".into(), false.into());
                }
                for value in object.values_mut() {
                    seal(value);
                }
            }
            serde_json::Value::Array(values) => values.iter_mut().for_each(seal),
            _ => {}
        }
    }
    if let Some(object) = schema.as_object_mut() {
        if object.contains_key("properties") {
            object.insert("additionalProperties".into(), false.into());
        }
        for value in object.values_mut() {
            seal(value);
        }
    }
    schema
}
