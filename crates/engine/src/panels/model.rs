use std::collections::{BTreeMap, BTreeSet};

use super::geometry::Point;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const LAYOUT_VERSION: u32 = 1;
pub const PANEL_API_VERSION: u32 = 1;
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
identity!(PanelKind, InstanceId, SectionId, WorkspaceId, DocumentId, ViewportId, HostId, ControlId);

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
    pub fn docked(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Bottom)
    }
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
    pub popout: bool,
    pub tabbable: bool,
    pub multi_instance: bool,
}

impl Default for PanelFeatures {
    fn default() -> Self {
        Self { collapsible: true, closable: true, popout: true, tabbable: true, multi_instance: false }
    }
}

/// Immutable registration metadata. No callbacks, content state or model references.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelSpec {
    pub kind: PanelKind,
    pub title_key: String,
    pub icon: String,
    pub allowed_zones: BTreeSet<Zone>,
    pub default_zone: Zone,
    pub min_size: Size,
    pub default_size: Size,
    pub max_size: Size,
    pub edge_rule: EdgeRule,
    pub surface: PanelSurface,
    pub features: PanelFeatures,
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
        spec.min_size.validate()?;
        spec.max_size.validate()?;
        spec.default_size.validate()?;
        if !spec.default_size.within(spec.min_size, spec.max_size) {
            return Err(error(Rule::Bounds, "min <= default <= max required in both dimensions"));
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlacementMemory {
    pub section: SectionId,
    pub location: SectionLocation,
    pub section_index: usize,
    pub tab_index: usize,
    pub size: Size,
    pub collapsed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelInstance {
    pub id: InstanceId,
    pub kind: PanelKind,
    pub requested_open: bool,
    pub memory: PlacementMemory,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layout {
    pub version: u32,
    pub workspace: WorkspaceId,
    pub instances: Vec<PanelInstance>,
    /// Stable order, outermost first within each logical dock edge.
    pub sections: Vec<Section>,
}

impl Layout {
    pub fn empty(workspace: WorkspaceId) -> Self {
        Self { version: LAYOUT_VERSION, workspace, instances: Vec::new(), sections: Vec::new() }
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
        min.width = min.width.max(spec.min_size.width);
        min.height = min.height.max(spec.min_size.height);
        max.width = max.width.min(spec.max_size.width);
        max.height = max.height.min(spec.max_size.height);
    }
    if min.width > max.width || min.height > max.height {
        return Err(error(Rule::IncompatibleTabs, "tab size constraints have no intersection"));
    }
    Ok((min, max))
}

/// Bounded semantic validation, also required after serde and every transaction.
pub fn validate(layout: &Layout, registry: &PanelRegistry) -> PanelResult<()> {
    if layout.version != LAYOUT_VERSION {
        return Err(error(Rule::Version, "unsupported layout version"));
    }
    valid_id(&layout.workspace.0)?;
    if layout.instances.len() > MAX_INSTANCES || layout.sections.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "layout collection limit"));
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
    for s in &layout.sections {
        valid_id(&s.id.0)?;
        s.location.validate()?;
        s.size.validate()?;
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
            if s.collapsed && !spec.features.collapsible {
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
                    || !return_to.size.within(spec.min_size, spec.max_size)
                    || (return_to.collapsed && !spec.features.collapsible)
                {
                    return Err(error(Rule::Placement, "native return violates member constraints"));
                }
            }
        }
    }
    for p in &layout.instances {
        // Diagnose impossible tab groups before their shared size can make an
        // individual member's remembered size appear invalid.
        let spec = registry.get(&p.kind)?;
        if !p.memory.size.within(spec.min_size, spec.max_size) {
            return Err(error(Rule::Bounds, "invalid placement memory"));
        }
        if p.requested_open != seen.contains(&p.id) {
            return Err(error(Rule::ExactlyOnce, "open instance must appear exactly once"));
        }
    }
    Ok(())
}
