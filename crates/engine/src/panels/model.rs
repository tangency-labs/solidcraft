use std::collections::{BTreeMap, BTreeSet};

use super::geometry::Point;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const LAYOUT_VERSION: u32 = 2;
pub const PANEL_API_VERSION: u32 = 2;
pub const MAX_HEADER_ACTIONS: usize = 16;
pub const MAX_ID_BYTES: usize = 64;
pub const MAX_INSTANCES: usize = 128;
pub const MAX_SECTIONS: usize = 64;
pub const MAX_TABS: usize = 32;
pub const MAX_MUTATIONS: usize = 64;
pub const MAX_LOGICAL: f32 = 32_768.0;
pub const MAX_ENVELOPE_BYTES: usize = 262_144;

/// Classic is a permanent compatibility contract; this foundation does not
/// select a new preset or migrate existing preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityPreset {
    Classic,
}

/// Stable machine-readable rule names; presentation translates these in the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    Availability,
    Bounds,
    Capability,
    CollectionLimit,
    DuplicateId,
    ExactlyOnce,
    FeatureDisabled,
    Id,
    IncompatibleTabs,
    Lifecycle,
    MissingTarget,
    NoSpace,
    PinConflict,
    PinOrder,
    Placement,
    Revision,
    Schema,
    Version,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{rule:?}: {detail}")]
pub struct PanelError {
    pub rule: Rule,
    pub detail: String,
}

pub type PanelResult<T> = std::result::Result<T, PanelError>;

pub fn error(rule: Rule, detail: impl Into<String>) -> PanelError {
    // Error replies are bounded too; never echo a hostile payload in full.
    PanelError { rule, detail: detail.into().chars().take(256).collect() }
}

pub fn valid_id(value: &str) -> PanelResult<()> {
    if value.is_empty() || value.len() > MAX_ID_BYTES || !value.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c)) {
        return Err(error(Rule::Id, "expected a nonempty bounded lowercase identifier"));
    }
    Ok(())
}

macro_rules! identity {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(value: impl Into<String>) -> PanelResult<Self> {
                let value = value.into();
                valid_id(&value)?;
                Ok(Self(value))
            }
        }
    )+};
}
identity!(PanelKind, InstanceId, SectionId, ColumnId, WorkspaceId, DocumentId, ViewportId, HostId, ControlId);

/// Leading/trailing logical sides, mirrored by a host for RTL. There is no Top zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    Left,
    Right,
    Bottom,
    Floating,
    Window,
}

impl Zone {
    pub fn side(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
    pub fn docked(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Bottom)
    }
}

pub fn all_zones() -> BTreeSet<Zone> {
    [Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating, Zone::Window].into_iter().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRule {
    None,
    PinOuter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelSurface {
    FollowZone,
    Opaque,
    Translucent,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub fn validate(self) -> PanelResult<()> {
        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
            || self.width > MAX_LOGICAL
            || self.height > MAX_LOGICAL
        {
            return Err(error(Rule::Bounds, "outer size must be finite and in (0, 32768] logical units"));
        }
        Ok(())
    }
    pub fn within(self, min: Self, max: Self) -> bool {
        self.width >= min.width && self.height >= min.height && self.width <= max.width && self.height <= max.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PanelFeatures {
    pub collapsible: bool,
    pub closable: bool,
    #[serde(default)]
    pub hot_closable: bool,
    pub popout: bool,
    pub tabbable: bool,
    pub multi_instance: bool,
}

impl Default for PanelFeatures {
    fn default() -> Self {
        Self { collapsible: true, closable: true, hot_closable: false, popout: true, tabbable: true, multi_instance: false }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SideHeightPolicy {
    #[default]
    Content,
    Fill,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HeaderActionSpec {
    pub id: ControlId,
    pub label_key: String,
    pub icon: String,
}

/// Immutable registration metadata. No callbacks, content state or model references.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelSpec {
    pub kind: PanelKind,
    pub title_key: String,
    pub icon: String,
    #[serde(default = "all_zones")]
    pub allowed_zones: BTreeSet<Zone>,
    pub default_zone: Zone,
    pub min_size: Size,
    pub default_size: Size,
    pub max_size: Size,
    pub edge_rule: EdgeRule,
    pub surface: PanelSurface,
    #[serde(default)]
    pub features: PanelFeatures,
    #[serde(default)]
    pub side_height_policy: SideHeightPolicy,
    /// A rigid, expanded Bottom row. Other zones retain their ordinary bounds
    /// and collapse capability. The declared height must satisfy those bounds.
    #[serde(default)]
    pub fixed_bottom_height: Option<f32>,
    #[serde(default)]
    pub header_actions: Vec<HeaderActionSpec>,
    #[serde(default)]
    pub text_heavy: bool,
}

impl PanelSpec {
    pub fn size_bounds(&self, zone: Zone) -> (Size, Size) {
        let (mut min, mut max) = (self.min_size, self.max_size);
        if zone == Zone::Bottom
            && let Some(height) = self.fixed_bottom_height
        {
            min.height = height;
            max.height = height;
        }
        (min, max)
    }

    pub fn collapsible_in(&self, zone: Zone) -> bool {
        self.features.collapsible && !(zone == Zone::Bottom && self.fixed_bottom_height.is_some())
    }
}

#[derive(Clone, Debug, Default)]
pub struct PanelRegistry {
    specs: BTreeMap<PanelKind, PanelSpec>,
}

impl PanelRegistry {
    pub fn register(&mut self, spec: PanelSpec) -> PanelResult<()> {
        valid_id(&spec.kind.0)?;
        valid_id(&spec.title_key)?;
        valid_id(&spec.icon)?;
        if spec.header_actions.len() > MAX_HEADER_ACTIONS {
            return Err(error(Rule::CollectionLimit, "header action cap"));
        }
        let mut actions = BTreeSet::new();
        for action in &spec.header_actions {
            valid_id(&action.id.0)?;
            valid_id(&action.label_key)?;
            valid_id(&action.icon)?;
            if !actions.insert(&action.id) {
                return Err(error(Rule::DuplicateId, "duplicate header action"));
            }
        }
        spec.min_size.validate()?;
        spec.max_size.validate()?;
        spec.default_size.validate()?;
        if !spec.default_size.within(spec.min_size, spec.max_size) {
            return Err(error(Rule::Bounds, "min <= default <= max required in both dimensions"));
        }
        if spec.fixed_bottom_height.is_some_and(|height| {
            !height.is_finite() || height < spec.min_size.height || height > spec.max_size.height || !spec.allowed_zones.contains(&Zone::Bottom)
        }) {
            return Err(error(Rule::Bounds, "fixed Bottom height must satisfy registered bounds in an allowed Bottom zone"));
        }
        if !spec.allowed_zones.contains(&Zone::Floating) || !spec.allowed_zones.contains(&spec.default_zone) {
            return Err(error(Rule::Placement, "Floating and default_zone must be allowed"));
        }
        if spec.edge_rule == EdgeRule::PinOuter && spec.allowed_zones.contains(&Zone::Bottom) {
            return Err(error(Rule::Placement, "PinOuter must exclude Bottom"));
        }
        if self.specs.contains_key(&spec.kind) {
            return Err(error(Rule::DuplicateId, "panel kind already registered"));
        }
        if self.specs.len() >= MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "registry full"));
        }
        self.specs.insert(spec.kind.clone(), spec);
        Ok(())
    }
    pub fn get(&self, kind: &PanelKind) -> PanelResult<&PanelSpec> {
        self.specs.get(kind).ok_or_else(|| error(Rule::MissingTarget, "unregistered panel kind"))
    }
    pub fn specs(&self) -> impl Iterator<Item = &PanelSpec> {
        self.specs.values()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SectionLocation {
    Dock { zone: Zone },
    Floating { origin: Point },
    Window { host: HostId, origin: Point, return_to: DockReturn },
}

impl SectionLocation {
    pub fn zone(&self) -> Zone {
        match self {
            Self::Dock { zone } => *zone,
            Self::Floating { .. } => Zone::Floating,
            Self::Window { .. } => Zone::Window,
        }
    }
    pub fn validate(&self) -> PanelResult<()> {
        match self {
            Self::Dock { zone } if !zone.docked() => Err(error(Rule::Placement, "dock must name a docked zone")),
            Self::Dock { .. } => Ok(()),
            Self::Floating { origin } => origin.validate(),
            Self::Window { host, origin, return_to } => {
                valid_id(&host.0)?;
                origin.validate()?;
                return_to.validate()
            }
        }
    }
}

/// Native return destinations never recursively contain another native host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DockReturn {
    pub zone: Zone,
    pub index: usize,
    pub size: Size,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_extent: Option<f32>,
    #[serde(default, skip_serializing_if = "is_zero_extent_priority")]
    pub extent_priority: u16,
    /// Restore a former tab group when it still occupies the recorded dock.
    /// Missing/reparented groups use the nonrecursive dock fallback above.
    #[serde(default)]
    pub join: Option<ReturnTab>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReturnTab {
    pub section: SectionId,
    pub index: usize,
}
impl DockReturn {
    pub fn validate(&self) -> PanelResult<()> {
        if !self.zone.docked() || self.index > MAX_SECTIONS {
            return Err(error(Rule::Placement, "invalid native return destination"));
        }
        self.size.validate()?;
        validate_extent_priority(self.preferred_extent, self.extent_priority)?;
        validate_column_memory(self.zone, &self.column, self.column_index, self.column_width, self.weight)?;
        if let Some(section) = &self.section {
            valid_id(&section.0)?;
        }
        if let Some(join) = &self.join {
            valid_id(&join.section.0)?;
            if join.index >= MAX_TABS {
                return Err(error(Rule::Bounds, "native return tab index cap"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    pub id: SectionId,
    pub location: SectionLocation,
    pub tabs: Vec<InstanceId>,
    pub active: InstanceId,
    /// Desired outer dimensions; overflow resolution never modifies these.
    pub size: Size,
    pub collapsed: bool,
    #[serde(default)]
    pub column: Option<ColumnId>,
    /// Saved expansion proportion. Collapse and transient allocation retain it.
    #[serde(default = "default_weight")]
    pub weight: f32,
    /// Explicit dock stack-axis request, independent of surplus proportions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_extent: Option<f32>,
    #[serde(default, skip_serializing_if = "is_zero_extent_priority")]
    pub extent_priority: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlacementMemory {
    pub section: SectionId,
    pub location: SectionLocation,
    pub section_index: usize,
    pub tab_index: usize,
    pub size: Size,
    pub collapsed: bool,
    #[serde(default)]
    pub column: Option<ColumnId>,
    #[serde(default)]
    pub column_index: usize,
    #[serde(default)]
    pub column_width: Option<f32>,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_extent: Option<f32>,
    #[serde(default, skip_serializing_if = "is_zero_extent_priority")]
    pub extent_priority: u16,
}

pub fn dock_stack_extent(zone: Zone, size: Size) -> Option<f32> {
    match zone {
        Zone::Left | Zone::Right => Some(size.height),
        Zone::Bottom => Some(size.width),
        Zone::Floating | Zone::Window => None,
    }
}

pub fn validate_preferred_extent(value: Option<f32>) -> PanelResult<()> {
    if value.is_some_and(|value| !value.is_finite() || value <= 0.0 || value > MAX_LOGICAL) {
        return Err(error(Rule::Bounds, "preferred extent must be finite and in (0, 32768]"));
    }
    Ok(())
}

pub fn is_zero_extent_priority(value: &u16) -> bool {
    *value == 0
}

pub fn validate_extent_priority(preferred_extent: Option<f32>, extent_priority: u16) -> PanelResult<()> {
    validate_preferred_extent(preferred_extent)?;
    if extent_priority > 0 && preferred_extent.is_none() {
        return Err(error(Rule::Bounds, "positive extent priority requires an explicit extent"));
    }
    Ok(())
}

pub fn default_weight() -> f32 {
    1.0
}

pub fn validate_weight(weight: f32) -> PanelResult<()> {
    if !weight.is_finite() || !(0.0..=MAX_LOGICAL).contains(&weight) {
        return Err(error(Rule::Bounds, "weight must be finite and in [0, 32768]"));
    }
    Ok(())
}

pub fn validate_column_memory(zone: Zone, column: &Option<ColumnId>, index: usize, width: Option<f32>, weight: f32) -> PanelResult<()> {
    validate_weight(weight)?;
    if index > MAX_SECTIONS || width.is_some_and(|width| !width.is_finite() || width <= 0.0 || width > MAX_LOGICAL) {
        return Err(error(Rule::Bounds, "invalid remembered column dimensions/rank"));
    }
    if !zone.side() && (column.is_some() || width.is_some() || index != 0) {
        return Err(error(Rule::Placement, "non-side memory cannot contain column geometry"));
    }
    if let Some(column) = column {
        valid_id(&column.0)?;
        if !zone.side() {
            return Err(error(Rule::Placement, "only side sections have columns"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Column {
    pub id: ColumnId,
    pub zone: Zone,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelInstance {
    pub id: InstanceId,
    pub kind: PanelKind,
    pub requested_open: bool,
    pub memory: PlacementMemory,
}

pub const MAX_EXTENT_PRIORITY_RECORDS: usize = 4 * (MAX_INSTANCES + MAX_SECTIONS);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum ExtentPriorityTarget {
    Section(SectionId),
    Memory(InstanceId),
    SectionReturn(SectionId),
    MemoryReturn(InstanceId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtentPriorityRecord {
    pub target: ExtentPriorityTarget,
    pub zone: Zone,
    pub column: Option<ColumnId>,
    pub stored_priority: u16,
    pub priority: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtentPriorityContext {
    pub records: Vec<ExtentPriorityRecord>,
    pub complete: bool,
}
impl ExtentPriorityContext {
    pub fn validate(&self) -> PanelResult<()> {
        if self.records.len() > MAX_EXTENT_PRIORITY_RECORDS {
            return Err(error(Rule::CollectionLimit, "recovery priority record cap"));
        }
        let mut targets = BTreeSet::new();
        for record in &self.records {
            match &record.target {
                ExtentPriorityTarget::Section(id) | ExtentPriorityTarget::SectionReturn(id) => valid_id(&id.0)?,
                ExtentPriorityTarget::Memory(id) | ExtentPriorityTarget::MemoryReturn(id) => valid_id(&id.0)?,
            }
            if let Some(column) = &record.column {
                valid_id(&column.0)?;
            }
            if !targets.insert(&record.target) || (record.column.is_some() && !record.zone.side()) {
                return Err(error(Rule::Schema, "duplicate recovery priority target or invalid column"));
            }
        }
        Ok(())
    }
    pub fn overrides(&self) -> impl Iterator<Item = &ExtentPriorityRecord> {
        self.records.iter().filter(|record| record.priority != record.stored_priority)
    }
}

/// Ephemeral proof of the typed incoming layout's per-identity owner sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplacementIdentityOwnership {
    pub sections: BTreeMap<SectionId, BTreeSet<InstanceId>>,
    pub columns: BTreeMap<ColumnId, BTreeSet<InstanceId>>,
}

impl ReplacementIdentityOwnership {
    /// Capture only typed ownership, never quarantine or snapshot reservations.
    pub fn from_layout(layout: &Layout) -> PanelResult<Self> {
        if layout.instances.len() > MAX_INSTANCES
            || layout.sections.len() > MAX_SECTIONS
            || layout.columns.len() > MAX_SECTIONS
            || layout.sections.iter().any(|section| section.tabs.len() > MAX_TABS)
        {
            return Err(error(Rule::CollectionLimit, "replacement ownership layout cap"));
        }
        let mut ownership = Self::default();
        for section in &layout.sections {
            ownership.sections.entry(section.id.clone()).or_default().extend(section.tabs.iter().cloned());
            if let Some(column) = &section.column {
                ownership.columns.entry(column.clone()).or_default().extend(section.tabs.iter().cloned());
            }
            for instance in &section.tabs {
                ownership.capture_return(instance, &section.location);
            }
        }
        for panel in &layout.instances {
            ownership.sections.entry(panel.memory.section.clone()).or_default().insert(panel.id.clone());
            if let Some(column) = &panel.memory.column {
                ownership.columns.entry(column.clone()).or_default().insert(panel.id.clone());
            }
            ownership.capture_return(&panel.id, &panel.memory.location);
        }
        for column in &layout.columns {
            ownership.columns.entry(column.id.clone()).or_default();
        }
        ownership.validate()?;
        Ok(ownership)
    }

    fn capture_return(&mut self, instance: &InstanceId, location: &SectionLocation) {
        if let SectionLocation::Window { return_to, .. } = location {
            if let Some(section) = &return_to.section {
                self.sections.entry(section.clone()).or_default().insert(instance.clone());
            }
            if let Some(join) = &return_to.join {
                self.sections.entry(join.section.clone()).or_default().insert(instance.clone());
            }
            if let Some(column) = &return_to.column {
                self.columns.entry(column.clone()).or_default().insert(instance.clone());
            }
        }
    }

    pub fn validate(&self) -> PanelResult<()> {
        if self.sections.len() > MAX_RESERVED_IDENTITIES || self.columns.len() > MAX_RESERVED_IDENTITIES {
            return Err(error(Rule::CollectionLimit, "replacement ownership identity cap"));
        }
        for id in self.sections.keys().map(|id| &id.0).chain(self.columns.keys().map(|id| &id.0)) {
            valid_id(id)?;
        }
        let mut count = 0usize;
        for owners in self.sections.values().chain(self.columns.values()) {
            count = count.saturating_add(owners.len());
            if owners.len() > MAX_INSTANCES || count > MAX_RESERVED_IDENTITIES {
                return Err(error(Rule::CollectionLimit, "replacement ownership owner cap"));
            }
            for instance in owners {
                valid_id(&instance.0)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutPreparationContext {
    pub sections: BTreeSet<SectionId>,
    pub columns: BTreeSet<ColumnId>,
    pub extent_priorities: ExtentPriorityContext,
    pub replacement_ownership: Option<ReplacementIdentityOwnership>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Layout {
    pub version: u32,
    pub workspace: WorkspaceId,
    pub instances: Vec<PanelInstance>,
    /// Stable order, outermost first within each logical dock edge.
    pub sections: Vec<Section>,
    /// Outer-to-inner within each side, including retained dormant identities.
    #[serde(default)]
    pub columns: Vec<Column>,
    #[serde(skip)]
    pub extent_priority_context: ExtentPriorityContext,
}

impl PartialEq for Layout {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.workspace == other.workspace
            && self.instances == other.instances
            && self.sections == other.sections
            && self.columns == other.columns
            && self.latent_priority_overrides().eq(other.latent_priority_overrides())
    }
}

/// Compare complete arrangements while ignoring generated section/column names.
/// Ordered groups and every reference to a live or remembered group keep their
/// identity relationships. Instance IDs, geometry and placement memory stay exact.
pub fn arrangements_equal(left: &Layout, right: &Layout) -> bool {
    match (canonical_arrangement(left), canonical_arrangement(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn arrangement_id(names: &mut BTreeMap<String, String>, id: &str, prefix: &str) -> Option<String> {
    if let Some(name) = names.get(id) {
        return Some(name.clone());
    }
    if names.len() >= MAX_RESERVED_IDENTITIES {
        return None;
    }
    let name = format!("{prefix}.{}", names.len());
    names.insert(id.to_string(), name.clone());
    Some(name)
}

fn arrangement_location(
    location: &mut SectionLocation,
    sections: &mut BTreeMap<String, String>,
    columns: &mut BTreeMap<String, String>,
) -> Option<()> {
    if let SectionLocation::Window { return_to, .. } = location {
        if let Some(id) = &mut return_to.section {
            id.0 = arrangement_id(sections, &id.0, "section")?;
        }
        if let Some(id) = &mut return_to.column {
            id.0 = arrangement_id(columns, &id.0, "column")?;
        }
        if let Some(join) = &mut return_to.join {
            join.section.0 = arrangement_id(sections, &join.section.0, "section")?;
        }
    }
    Some(())
}

fn canonical_arrangement(layout: &Layout) -> Option<Layout> {
    // Bound every collection and string before copying. The comparison has no
    // registry and does not allocate geometry or change the authoritative model.
    if layout.instances.len() > MAX_INSTANCES || layout.sections.len() > MAX_SECTIONS || layout.columns.len() > MAX_SECTIONS {
        return None;
    }
    valid_id(&layout.workspace.0).ok()?;
    layout.extent_priority_context.validate().ok()?;
    for column in &layout.columns {
        valid_id(&column.id.0).ok()?;
    }
    for section in &layout.sections {
        if section.tabs.len() > MAX_TABS {
            return None;
        }
        valid_id(&section.id.0).ok()?;
        valid_id(&section.active.0).ok()?;
        section.location.validate().ok()?;
        if let Some(column) = &section.column {
            valid_id(&column.0).ok()?;
        }
        for instance in &section.tabs {
            valid_id(&instance.0).ok()?;
        }
    }
    for instance in &layout.instances {
        valid_id(&instance.id.0).ok()?;
        valid_id(&instance.kind.0).ok()?;
        valid_id(&instance.memory.section.0).ok()?;
        instance.memory.location.validate().ok()?;
        if let Some(column) = &instance.memory.column {
            valid_id(&column.0).ok()?;
        }
    }
    let mut sections = BTreeMap::new();
    let mut columns = BTreeMap::new();
    for section in &layout.sections {
        arrangement_id(&mut sections, &section.id.0, "section")?;
    }
    for column in &layout.columns {
        arrangement_id(&mut columns, &column.id.0, "column")?;
    }
    let mut canonical = layout.clone();
    for column in &mut canonical.columns {
        column.id.0 = arrangement_id(&mut columns, &column.id.0, "column")?;
    }
    for section in &mut canonical.sections {
        section.id.0 = arrangement_id(&mut sections, &section.id.0, "section")?;
        if let Some(column) = &mut section.column {
            column.0 = arrangement_id(&mut columns, &column.0, "column")?;
        }
        arrangement_location(&mut section.location, &mut sections, &mut columns)?;
    }
    for instance in &mut canonical.instances {
        instance.memory.section.0 = arrangement_id(&mut sections, &instance.memory.section.0, "section")?;
        if let Some(column) = &mut instance.memory.column {
            column.0 = arrangement_id(&mut columns, &column.0, "column")?;
        }
        arrangement_location(&mut instance.memory.location, &mut sections, &mut columns)?;
    }
    for record in &mut canonical.extent_priority_context.records {
        match &mut record.target {
            ExtentPriorityTarget::Section(section) | ExtentPriorityTarget::SectionReturn(section) => {
                section.0 = arrangement_id(&mut sections, &section.0, "section")?;
            }
            ExtentPriorityTarget::Memory(_) | ExtentPriorityTarget::MemoryReturn(_) => {}
        }
        if let Some(column) = &mut record.column {
            column.0 = arrangement_id(&mut columns, &column.0, "column")?;
        }
    }
    canonical.extent_priority_context.records.sort_by(|left, right| left.target.cmp(&right.target));
    Some(canonical)
}

impl Layout {
    fn latent_priority_overrides(&self) -> impl Iterator<Item = &ExtentPriorityRecord> {
        self.extent_priority_context.overrides().filter(move |record| match &record.target {
            ExtentPriorityTarget::Section(id) => self.section(id).is_err(),
            ExtentPriorityTarget::Memory(id) => self.instance(id).is_err(),
            ExtentPriorityTarget::SectionReturn(id) => !self.section(id).is_ok_and(|s| matches!(&s.location, SectionLocation::Window { .. })),
            ExtentPriorityTarget::MemoryReturn(id) => !self.instance(id).is_ok_and(|p| matches!(&p.memory.location, SectionLocation::Window { .. })),
        })
    }
    pub fn empty(workspace: WorkspaceId) -> Self {
        Self {
            version: LAYOUT_VERSION,
            workspace,
            instances: Vec::new(),
            sections: Vec::new(),
            columns: Vec::new(),
            extent_priority_context: ExtentPriorityContext::default(),
        }
    }
    pub fn instance(&self, id: &InstanceId) -> PanelResult<&PanelInstance> {
        self.instances.iter().find(|p| p.id == *id).ok_or_else(|| error(Rule::MissingTarget, "instance not found"))
    }
    pub fn section(&self, id: &SectionId) -> PanelResult<&Section> {
        self.sections.iter().find(|s| s.id == *id).ok_or_else(|| error(Rule::MissingTarget, "section not found"))
    }
    pub fn containing(&self, id: &InstanceId) -> PanelResult<&Section> {
        self.sections.iter().find(|s| s.tabs.contains(id)).ok_or_else(|| error(Rule::MissingTarget, "instance has no open section"))
    }
    pub fn column(&self, id: &ColumnId) -> PanelResult<&Column> {
        self.columns.iter().find(|column| column.id == *id).ok_or_else(|| error(Rule::MissingTarget, "column not found"))
    }
}

/// Allocation scans remembered/native identities too; a proposal consumes no IDs.
pub fn next_section_id(layout: &Layout) -> PanelResult<SectionId> {
    next_section_id_with_reservations(layout, &BTreeSet::new())
}

/// External identities are data only, never fake placed sections.
pub const MAX_RESERVED_IDENTITIES: usize = MAX_INSTANCES * 8 + MAX_SECTIONS * 8;

pub fn next_section_id_with_reservations(layout: &Layout, reserved: &BTreeSet<SectionId>) -> PanelResult<SectionId> {
    if reserved.len() > MAX_RESERVED_IDENTITIES {
        return Err(error(Rule::CollectionLimit, "external section reservation cap"));
    }
    for id in reserved {
        valid_id(&id.0)?;
    }
    let mut used: BTreeSet<&str> = layout.sections.iter().map(|s| s.id.0.as_str()).collect();
    used.extend(reserved.iter().map(|id| id.0.as_str()));
    for instance in &layout.instances {
        used.insert(&instance.memory.section.0);
    }
    for location in layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location {
            if let Some(section) = &return_to.section {
                used.insert(&section.0);
            }
            if let Some(join) = &return_to.join {
                used.insert(&join.section.0);
            }
        }
    }
    for ordinal in 1..=MAX_RESERVED_IDENTITIES * 2 + 1 {
        let id = format!("section-{ordinal}");
        if !used.contains(id.as_str()) {
            return SectionId::new(id);
        }
    }
    Err(error(Rule::CollectionLimit, "section identity allocation exhausted"))
}

pub fn next_column_id(layout: &Layout) -> PanelResult<ColumnId> {
    next_column_id_with_reservations(layout, &BTreeSet::new())
}

pub fn next_column_id_with_reservations(layout: &Layout, reserved: &BTreeSet<ColumnId>) -> PanelResult<ColumnId> {
    if reserved.len() > MAX_RESERVED_IDENTITIES {
        return Err(error(Rule::CollectionLimit, "external column reservation cap"));
    }
    for id in reserved {
        valid_id(&id.0)?;
    }
    let mut used: BTreeSet<&str> = layout.columns.iter().map(|c| c.id.0.as_str()).collect();
    used.extend(reserved.iter().map(|id| id.0.as_str()));
    used.extend(layout.sections.iter().filter_map(|s| s.column.as_ref().map(|c| c.0.as_str())));
    for instance in &layout.instances {
        if let Some(column) = &instance.memory.column {
            used.insert(&column.0);
        }
    }
    for location in layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location
            && let Some(column) = &return_to.column
        {
            used.insert(&column.0);
        }
    }
    for ordinal in 1..=MAX_RESERVED_IDENTITIES * 2 + 1 {
        let id = format!("column-{ordinal}");
        if !used.contains(id.as_str()) {
            return ColumnId::new(id);
        }
    }
    Err(error(Rule::CollectionLimit, "column identity allocation exhausted"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable { reason: String },
}

impl Availability {
    pub fn require(&self) -> PanelResult<()> {
        match self {
            Self::Available => Ok(()),
            Self::Unavailable { reason } => Err(error(Rule::Availability, reason)),
        }
    }
}

/// Missing entries mean available. Availability is live context, never layout persistence.
pub type AvailabilityMap = BTreeMap<InstanceId, Availability>;

/// Read-only selection shared by command admission and controller presentation.
pub fn presented_tab<'a>(section: &'a Section, availability: &AvailabilityMap) -> Option<&'a InstanceId> {
    if section.tabs.is_empty() || section.tabs.len() > MAX_TABS {
        return None;
    }
    let available = |id: &InstanceId| availability.get(id).is_none_or(|state| *state == Availability::Available);
    if section.tabs.contains(&section.active) && available(&section.active) {
        Some(&section.active)
    } else {
        section.tabs.iter().find(|id| available(id))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HostCapabilities {
    pub native_windows: bool,
    pub persistence: bool,
    pub max_native_hosts: usize,
}

pub fn tab_bounds(layout: &Layout, registry: &PanelRegistry, section: &Section) -> PanelResult<(Size, Size)> {
    let mut min = Size { width: 0.0, height: 0.0 };
    let mut max = Size { width: MAX_LOGICAL, height: MAX_LOGICAL };
    for id in &section.tabs {
        let spec = registry.get(&layout.instance(id)?.kind)?;
        let (lower, upper) = spec.size_bounds(section.location.zone());
        min.width = min.width.max(lower.width);
        min.height = min.height.max(lower.height);
        max.width = max.width.min(upper.width);
        max.height = max.height.min(upper.height);
    }
    if min.width > max.width || min.height > max.height {
        return Err(error(Rule::IncompatibleTabs, "tab size constraints have no intersection"));
    }
    Ok((min, max))
}

pub fn section_collapsible(layout: &Layout, registry: &PanelRegistry, section: &Section) -> PanelResult<bool> {
    for id in &section.tabs {
        if !registry.get(&layout.instance(id)?.kind)?.collapsible_in(section.location.zone()) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn column_bounds(layout: &Layout, registry: &PanelRegistry, column: &ColumnId) -> PanelResult<(f32, f32)> {
    layout.column(column)?;
    let mut min: f32 = 0.0;
    let mut max = MAX_LOGICAL;
    for section in layout.sections.iter().filter(|s| s.column.as_ref() == Some(column)) {
        let (lower, upper) = tab_bounds(layout, registry, section)?;
        min = min.max(lower.width);
        max = max.min(upper.width);
    }
    if min > max {
        return Err(error(Rule::IncompatibleTabs, "stacked section width constraints have no intersection"));
    }
    Ok((min, max))
}

/// One row thickness must satisfy all Bottom cards, including inactive tabs.
pub fn bottom_height_bounds(layout: &Layout, registry: &PanelRegistry) -> PanelResult<(f32, f32)> {
    if layout.sections.len() > MAX_SECTIONS || layout.instances.len() > MAX_INSTANCES || layout.sections.iter().any(|s| s.tabs.len() > MAX_TABS) {
        return Err(error(Rule::CollectionLimit, "Bottom bounds collection cap"));
    }
    let mut min: f32 = 0.0;
    let mut max = MAX_LOGICAL;
    let mut found = false;
    for section in layout.sections.iter().filter(|s| s.location.zone() == Zone::Bottom) {
        found = true;
        let (lower, upper) = tab_bounds(layout, registry, section)?;
        min = min.max(lower.height);
        max = max.min(upper.height);
    }
    if !found {
        return Err(error(Rule::MissingTarget, "Bottom row absent"));
    }
    if min > max {
        return Err(error(Rule::IncompatibleTabs, "Bottom row height constraints have no intersection"));
    }
    Ok((min, max))
}

/// Bounded semantic validation, also required after serde and every transaction.
pub fn validate(layout: &Layout, registry: &PanelRegistry) -> PanelResult<()> {
    layout.extent_priority_context.validate()?;
    if layout.version != LAYOUT_VERSION {
        return Err(error(Rule::Version, "unsupported layout version"));
    }
    valid_id(&layout.workspace.0)?;
    if layout.instances.len() > MAX_INSTANCES || layout.sections.len() > MAX_SECTIONS || layout.columns.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "layout collection limit"));
    }
    let mut columns = BTreeSet::new();
    for column in &layout.columns {
        valid_id(&column.id.0)?;
        if !columns.insert(&column.id) {
            return Err(error(Rule::DuplicateId, "duplicate column"));
        }
        if !column.zone.side() || !column.width.is_finite() || column.width <= 0.0 || column.width > MAX_LOGICAL {
            return Err(error(Rule::Bounds, "invalid side column"));
        }
    }
    let mut instances = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for p in &layout.instances {
        valid_id(&p.id.0)?;
        valid_id(&p.kind.0)?;
        valid_id(&p.memory.section.0)?;
        if !instances.insert(&p.id) {
            return Err(error(Rule::DuplicateId, "duplicate instance"));
        }
        let spec = registry.get(&p.kind)?;
        if !spec.features.multi_instance && !kinds.insert(&p.kind) {
            return Err(error(Rule::ExactlyOnce, "single-instance kind repeated, including closed instances"));
        }
        p.memory.location.validate()?;
        p.memory.size.validate()?;
        validate_extent_priority(p.memory.preferred_extent, p.memory.extent_priority)?;
        validate_column_memory(p.memory.location.zone(), &p.memory.column, p.memory.column_index, p.memory.column_width, p.memory.weight)?;
        if p.memory.tab_index >= MAX_TABS || p.memory.section_index > MAX_SECTIONS {
            return Err(error(Rule::Bounds, "invalid placement memory"));
        }
        if !spec.allowed_zones.contains(&p.memory.location.zone()) {
            return Err(error(Rule::Placement, "remembered zone forbidden"));
        }
    }
    let mut sections = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut hosts = BTreeSet::new();
    let mut pins = BTreeSet::new();
    let mut edges = BTreeSet::new();
    let mut last_columns = BTreeMap::<Zone, usize>::new();
    for s in &layout.sections {
        valid_id(&s.id.0)?;
        s.location.validate()?;
        s.size.validate()?;
        validate_extent_priority(s.preferred_extent, s.extent_priority)?;
        validate_weight(s.weight)?;
        if !sections.insert(&s.id) {
            return Err(error(Rule::DuplicateId, "duplicate section"));
        }
        if s.tabs.is_empty() || s.tabs.len() > MAX_TABS {
            return Err(error(Rule::CollectionLimit, "section requires 1..32 tabs"));
        }
        if !s.tabs.contains(&s.active) {
            return Err(error(Rule::MissingTarget, "active tab outside section"));
        }
        if let SectionLocation::Window { host, .. } = &s.location
            && !hosts.insert(host)
        {
            return Err(error(Rule::DuplicateId, "native host reused"));
        }
        let zone = s.location.zone();
        if zone.side() {
            let id = s.column.as_ref().ok_or_else(|| error(Rule::Placement, "side section requires column"))?;
            let column = layout.column(id)?;
            let rank = layout
                .columns
                .iter()
                .filter(|c| c.zone == zone)
                .position(|c| c.id == *id)
                .ok_or_else(|| error(Rule::Placement, "section column has wrong side"))?;
            if column.zone != zone || last_columns.get(&zone).is_some_and(|last| *last > rank) {
                return Err(error(Rule::Placement, "sections must follow contiguous column order"));
            }
            last_columns.insert(zone, rank);
        } else if s.column.is_some() {
            return Err(error(Rule::Placement, "non-side section cannot have column"));
        }
        let first = edges.insert(zone);
        for id in &s.tabs {
            let instance = layout.instance(id)?;
            let spec = registry.get(&instance.kind)?;
            if !instance.requested_open || !seen.insert(id) {
                return Err(error(Rule::ExactlyOnce, "closed or duplicate presented instance"));
            }
            if !spec.allowed_zones.contains(&zone) {
                return Err(error(Rule::Placement, "panel zone forbidden"));
            }
            if s.tabs.len() > 1 && !spec.features.tabbable {
                return Err(error(Rule::FeatureDisabled, "panel cannot join tabs"));
            }
            if s.collapsed && !spec.collapsible_in(zone) {
                return Err(error(Rule::FeatureDisabled, "panel cannot collapse"));
            }
            if zone == Zone::Window && !spec.features.popout {
                return Err(error(Rule::FeatureDisabled, "panel cannot pop out"));
            }
            if spec.edge_rule == EdgeRule::PinOuter && matches!(zone, Zone::Left | Zone::Right) {
                if !pins.insert(zone) {
                    return Err(error(Rule::PinConflict, "competing pins on logical edge, including same tab group"));
                }
                if !first {
                    return Err(error(Rule::PinOrder, "pinned section must be outermost"));
                }
                if layout
                    .columns
                    .iter()
                    .find(|column| column.zone == zone && layout.sections.iter().any(|s| s.column.as_ref() == Some(&column.id)))
                    .is_some_and(|column| s.column.as_ref() != Some(&column.id))
                {
                    return Err(error(Rule::PinOrder, "pinned group must occupy first live column"));
                }
            }
        }
        let (min, max) = tab_bounds(layout, registry, s)?;
        if !s.size.within(min, max) {
            return Err(error(Rule::Bounds, "section outside intersected tab bounds"));
        }
        if let SectionLocation::Window { return_to, .. } = &s.location {
            for id in &s.tabs {
                let spec = registry.get(&layout.instance(id)?.kind)?;
                if !spec.allowed_zones.contains(&return_to.zone)
                    || !return_to.size.within(spec.size_bounds(return_to.zone).0, spec.size_bounds(return_to.zone).1)
                    || return_to.column_width.is_some_and(|width| width < spec.min_size.width || width > spec.max_size.width)
                    || (return_to.collapsed && !spec.collapsible_in(return_to.zone))
                {
                    return Err(error(Rule::Placement, "native return violates member constraints"));
                }
            }
        }
    }
    for column in &layout.columns {
        let (min, max) = column_bounds(layout, registry, &column.id)?;
        if column.width < min || column.width > max {
            return Err(error(Rule::Bounds, "column outside stacked tab bounds"));
        }
    }
    for p in &layout.instances {
        // Diagnose impossible tab groups before their shared size can make an
        // individual member's remembered size appear invalid.
        let spec = registry.get(&p.kind)?;
        if !p.memory.size.within(spec.min_size, spec.max_size)
            || p.memory.column_width.is_some_and(|width| width < spec.min_size.width || width > spec.max_size.width)
        {
            return Err(error(Rule::Bounds, "invalid placement memory"));
        }
        if p.requested_open != seen.contains(&p.id) {
            return Err(error(Rule::ExactlyOnce, "open instance must appear exactly once"));
        }
    }
    Ok(())
}
