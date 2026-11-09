//! Application-owned command execution shared by UI, control and headless MCP.
//! No Session, document, geometry, dialogs or camera are reachable here.
mod catalogue;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;
use crate::schema::panels as wire;

pub const MAX_USER_PRESETS: usize = 64;
pub const MAX_WORKSPACES: usize = 64;
pub const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CATALOGUE_METADATA_BYTES: usize = 16 * 1024;
/// Framing is bounded separately from the aggregate payload/metadata budgets.
pub const MAX_CATALOGUE_SNAPSHOT_BYTES: usize = MAX_CATALOGUE_BYTES + MAX_CATALOGUE_METADATA_BYTES + 8192;
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;

/// Full envelope plus bounded rejected recovery intent. Arc makes catalogue
/// preparation bounded without copying each maximum-sized envelope 64 times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEnvelope {
    bytes: Arc<[u8]>,
    rejected: Option<Arc<[u8]>>,
}
impl StoredEnvelope {
    pub fn new(envelope: &LayoutEnvelope, rejected: Option<&[u8]>, registry: &PanelRegistry) -> PanelResult<Self> {
        if rejected.is_some_and(|bytes| bytes.len() > MAX_ENVELOPE_BYTES) {
            return Err(error(Rule::CollectionLimit, "rejected recovery byte cap"));
        }
        Ok(Self { bytes: encode(envelope, registry)?.into(), rejected: rejected.map(Arc::from) })
    }
    /// Exact retained inspection, including saved instances whose kinds are now
    /// absent. This is not a live-layout admission check; stage with
    /// `project_envelope` before submitting through the authoritative sink.
    pub fn envelope(&self, registry: &PanelRegistry, workspace: &WorkspaceId) -> PanelResult<LayoutEnvelope> {
        decode_retained(&self.bytes, registry, workspace)
    }
    /// Explicitly stage current-registry quarantine before a guarded live use.
    /// Exact inspection above deliberately keeps absent-kind saved instances.
    pub fn project_envelope(&self, registry: &PanelRegistry, workspace: &WorkspaceId) -> PanelResult<LayoutEnvelope> {
        project_retained_record(&self.bytes, registry, workspace)
    }
    /// Hydration retains the original validated bytes. In particular, optional
    /// order/projection metadata is not filled and recounted against the cap.
    fn from_record(bytes: &[u8], rejected: Option<&[u8]>, registry: &PanelRegistry) -> PanelResult<Self> {
        if rejected.is_some_and(|bytes| bytes.len() > MAX_ENVELOPE_BYTES) {
            return Err(error(Rule::CollectionLimit, "rejected recovery byte cap"));
        }
        decode_retained_record(bytes, registry)?;
        Ok(Self { bytes: Arc::from(bytes), rejected: rejected.map(Arc::from) })
    }
    pub fn rejected(&self) -> Option<&[u8]> {
        self.rejected.as_deref()
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDescriptor {
    pub id: WorkspaceId,
    pub title_key: String,
    pub default_preset: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetKind {
    BuiltIn,
    User,
}

/// Fields are private: registrations and prepared updates must obey aggregate
/// bounds before cloning or publishing. There is no independently stored selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogueState {
    workspaces: BTreeMap<WorkspaceId, WorkspaceDescriptor>,
    builtins: BTreeMap<String, StoredEnvelope>,
    users: BTreeMap<String, StoredEnvelope>,
    remembered: BTreeMap<WorkspaceId, StoredEnvelope>,
}
impl CatalogueState {
    pub fn summaries(&self) -> Vec<wire::WorkspaceSummary> {
        self.workspaces
            .values()
            .map(|item| wire::WorkspaceSummary {
                id: item.id.clone(),
                title_key: item.title_key.clone(),
                default_preset: item.default_preset.clone(),
                has_remembered_layout: self.remembered.contains_key(&item.id),
            })
            .collect()
    }
    pub fn user_count(&self) -> usize {
        self.users.len()
    }
    pub fn user_presets(&self) -> impl Iterator<Item = (&str, &StoredEnvelope)> {
        self.users.iter().map(|(name, record)| (name.as_str(), record))
    }
    pub fn builtin_presets(&self) -> impl Iterator<Item = (&str, &StoredEnvelope)> {
        self.builtins.iter().map(|(name, record)| (name.as_str(), record))
    }
    pub fn remembered(&self, workspace: &WorkspaceId) -> Option<&StoredEnvelope> {
        self.remembered.get(workspace)
    }
    pub fn preset(&self, name: &str) -> PanelResult<&StoredEnvelope> {
        let name = preset_name(name)?;
        let (kind, stored_name) = self.find_preset_name(&name).ok_or_else(|| error(Rule::MissingTarget, "layout preset not registered"))?;
        match kind {
            PresetKind::BuiltIn => self.builtins.get(stored_name),
            PresetKind::User => self.users.get(stored_name),
        }
        .ok_or_else(|| error(Rule::MissingTarget, "layout preset not registered"))
    }
    pub fn preset_kind(&self, name: &str) -> PanelResult<Option<PresetKind>> {
        Ok(self.find_preset_name(&preset_name(name)?).map(|(kind, _)| kind))
    }
    fn find_preset_name(&self, name: &str) -> Option<(PresetKind, &str)> {
        self.builtins
            .keys()
            .find(|stored| preset_names_equal(stored, name))
            .map(|stored| (PresetKind::BuiltIn, stored.as_str()))
            .or_else(|| self.users.keys().find(|stored| preset_names_equal(stored, name)).map(|stored| (PresetKind::User, stored.as_str())))
    }
    fn validate(&self) -> PanelResult<()> {
        if self.users.len() > MAX_USER_PRESETS
            || self.builtins.len() > MAX_USER_PRESETS
            || self.workspaces.len() > MAX_WORKSPACES
            || self.remembered.len() > MAX_WORKSPACES
        {
            return Err(error(Rule::CollectionLimit, "layout catalogue count cap"));
        }
        let mut metadata = 0_usize;
        for (id, item) in &self.workspaces {
            valid_id(&id.0)?;
            valid_id(&item.title_key)?;
            if item.id != *id || !self.builtins.contains_key(&item.default_preset) {
                return Err(error(Rule::MissingTarget, "workspace requires a registered immutable default"));
            }
            metadata = metadata.saturating_add(id.0.len() + item.title_key.len() + item.default_preset.len());
        }
        let mut bytes = 0_usize;
        let mut names = std::collections::BTreeSet::new();
        for (name, stored) in self.builtins.iter().chain(&self.users) {
            if preset_name(name)? != *name {
                return Err(error(Rule::Schema, "preset name must be normalised"));
            }
            if !names.insert(folded_preset_name(name).collect::<String>()) {
                return Err(error(Rule::DuplicateId, "preset names must be distinct regardless of case"));
            }
            metadata = metadata.saturating_add(name.len());
            bytes = bytes.saturating_add(stored.bytes.len() + stored.rejected.as_ref().map_or(0, |r| r.len()));
        }
        for (id, stored) in &self.remembered {
            valid_id(&id.0)?;
            metadata = metadata.saturating_add(id.0.len());
            bytes = bytes.saturating_add(stored.bytes.len() + stored.rejected.as_ref().map_or(0, |r| r.len()));
        }
        if metadata > MAX_CATALOGUE_METADATA_BYTES || bytes > MAX_CATALOGUE_BYTES {
            return Err(error(Rule::CollectionLimit, "aggregate layout catalogue byte cap"));
        }
        Ok(())
    }
}

pub struct PreparedCatalogue(CatalogueState);
impl PreparedCatalogue {
    pub fn new(state: CatalogueState) -> PanelResult<Self> {
        state.validate()?;
        Ok(Self(state))
    }
    pub fn state(&self) -> &CatalogueState {
        &self.0
    }
    pub fn into_state(self) -> CatalogueState {
        self.0
    }
}

/// Prepare MUST stage every fallible storage operation. Publish MUST be infallible.
/// The executor holds the exclusive catalogue borrow across guarded dispatch.
pub trait LayoutCatalogue {
    fn state(&self) -> &CatalogueState;
    fn prepare(&mut self, next: CatalogueState) -> PanelResult<PreparedCatalogue>;
    fn publish(&mut self, prepared: PreparedCatalogue);
}

#[derive(Default)]
pub struct InMemoryCatalogue {
    state: CatalogueState,
    /// Failure-injectable storage staging; no state has been published on failure.
    pub fail_stage: bool,
}
impl LayoutCatalogue for InMemoryCatalogue {
    fn state(&self) -> &CatalogueState {
        &self.state
    }
    fn prepare(&mut self, next: CatalogueState) -> PanelResult<PreparedCatalogue> {
        if self.fail_stage {
            return Err(error(Rule::Capability, "layout storage staging unavailable"));
        }
        PreparedCatalogue::new(next)
    }
    fn publish(&mut self, prepared: PreparedCatalogue) {
        self.state = prepared.into_state();
    }
}
impl InMemoryCatalogue {
    /// Hydration builds a validated candidate, without live dispatch or changing
    /// the mutability of saved user presets. Production storage remains external.
    pub fn from_snapshot(bytes: &[u8], registry: &PanelRegistry) -> PanelResult<Self> {
        Ok(Self { state: CatalogueState::hydrate(bytes, registry)?, fail_stage: false })
    }
    /// Explicit fixture/peer registration, never populated with invented products.
    pub fn register_builtin(&mut self, name: &str, envelope: &LayoutEnvelope, registry: &PanelRegistry) -> PanelResult<()> {
        let name = preset_name(name)?;
        if self.state.find_preset_name(&name).is_some() {
            return Err(error(Rule::DuplicateId, "preset already registered"));
        }
        let stored = StoredEnvelope::new(envelope, None, registry)?;
        let mut next = self.state.clone();
        next.builtins.insert(name, stored);
        let prepared = self.prepare(next)?;
        self.publish(prepared);
        Ok(())
    }
    pub fn register_workspace(&mut self, mut descriptor: WorkspaceDescriptor) -> PanelResult<()> {
        valid_id(&descriptor.id.0)?;
        valid_id(&descriptor.title_key)?;
        if self.state.workspaces.contains_key(&descriptor.id) {
            return Err(error(Rule::DuplicateId, "workspace already registered"));
        }
        let requested = preset_name(&descriptor.default_preset)?;
        match self.state.find_preset_name(&requested) {
            Some((PresetKind::BuiltIn, name)) => descriptor.default_preset = name.to_string(),
            _ => return Err(error(Rule::MissingTarget, "workspace requires a registered immutable default")),
        }
        let mut next = self.state.clone();
        next.workspaces.insert(descriptor.id.clone(), descriptor);
        let prepared = self.prepare(next)?;
        self.publish(prepared);
        Ok(())
    }
    pub fn remember(&mut self, envelope: &LayoutEnvelope, rejected: Option<&[u8]>, registry: &PanelRegistry) -> PanelResult<()> {
        let stored = StoredEnvelope::new(envelope, rejected, registry)?;
        let mut next = self.state.clone();
        next.remembered.insert(envelope.layout.workspace.clone(), stored);
        let prepared = self.prepare(next)?;
        self.publish(prepared);
        Ok(())
    }
}

pub fn preset_name(raw: &str) -> PanelResult<String> {
    // Bound BEFORE trimming, allocating or iterating over scalars.
    if raw.len() > 1024 {
        return Err(error(Rule::CollectionLimit, "raw preset name byte cap"));
    }
    let name = raw.trim();
    if name.is_empty() || name.len() > 256 || name.chars().count() > 64 || raw.chars().any(char::is_control) {
        return Err(error(Rule::Schema, "preset name requires 1..64 Unicode scalars without controls"));
    }
    Ok(name.into())
}

fn folded_preset_name(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars().flat_map(char::to_uppercase)
}

/// Compare bounded normalized names using Unicode uppercase without allocating.
/// Display spelling is retained in the catalogue, including after replacement.
pub fn preset_names_equal(left: &str, right: &str) -> bool {
    left.len() <= 256
        && right.len() <= 256
        && left.chars().count() <= 64
        && right.chars().count() <= 64
        && folded_preset_name(left).eq(folded_preset_name(right))
}

pub fn is_command(id: &str) -> bool {
    crate::find_command(id).is_some_and(|spec| IDS.contains(&spec.id))
}
pub const IDS: &[&str] = &[
    "layout.apply",
    "layout.get",
    "layout.reset",
    "layout.save",
    "layout.workspace.select",
    "panel.action",
    "panel.activate",
    "panel.close",
    "panel.collapse",
    "panel.dismiss",
    "panel.float",
    "panel.focus",
    "panel.focus.next",
    "panel.focus.previous",
    "panel.list",
    "panel.move",
    "panel.move.next_zone",
    "panel.native_close",
    "panel.open",
    "panel.peek",
    "panel.popout",
    "panel.reset_pair",
    "panel.resize",
    "panel.resize_pair",
    "panel.sash.down",
    "panel.sash.left",
    "panel.sash.reset",
    "panel.sash.right",
    "panel.sash.up",
    "panel.toggle",
];

#[derive(Debug)]
pub enum PanelCommand {
    List,
    Get,
    Open(wire::Open),
    HeaderAction(wire::HeaderAction),
    Contextual { action: ContextualPanelAction, params: wire::Revision },
    Select { operation: &'static str, params: wire::Select },
    Move { operation: &'static str, params: wire::Move },
    Resize(wire::Resize),
    Collapse(wire::Collapse),
    Dismiss(wire::Revision),
    ResizePair(wire::ResizePair),
    ResetPair(wire::ResetPair),
    Save(wire::Save),
    Apply(wire::Apply),
    Reset(wire::Revision),
    Workspace(wire::WorkspaceSelect),
}
fn read<T: DeserializeOwned>(value: &Value) -> PanelResult<T> {
    serde_json::from_value(value.clone()).map_err(|_| error(Rule::Schema, "invalid panel command fields or types"))
}
pub fn parse_command(id: &str, params: &Value) -> PanelResult<PanelCommand> {
    // Keep the engine/control convention: absent/null params mean an empty form.
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    if !params.is_object() {
        return Err(error(Rule::Schema, "panel command parameters must be an object"));
    }
    bounded_json(params)?;
    // Capped writer protects direct callers as well as transports, before clone.
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > MAX_COMMAND_BYTES {
                return Err(std::io::Error::other("command byte cap"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(0), params).map_err(|_| error(Rule::CollectionLimit, "panel command byte cap"))?;
    let id = crate::find_command(id).map(|s| s.id).ok_or_else(|| error(Rule::MissingTarget, "unknown panel command"))?;
    if let Some(action) = ContextualPanelAction::from_command_id(id) {
        let command = PanelCommand::Contextual { action, params: read(params)? };
        validate_command(&command)?;
        return Ok(command);
    }
    let command = match id {
        "panel.list" => {
            let _: wire::Empty = read(params)?;
            PanelCommand::List
        }
        "layout.get" => {
            let _: wire::Empty = read(params)?;
            PanelCommand::Get
        }
        "panel.open" => PanelCommand::Open(read(params)?),
        "panel.action" => PanelCommand::HeaderAction(read(params)?),
        "panel.close" | "panel.activate" | "panel.focus" | "panel.peek" | "panel.native_close" => {
            PanelCommand::Select { operation: id, params: read(params)? }
        }
        "panel.move" | "panel.float" | "panel.popout" => PanelCommand::Move { operation: id, params: read(params)? },
        "panel.resize" => PanelCommand::Resize(read(params)?),
        "panel.collapse" => PanelCommand::Collapse(read(params)?),
        "panel.dismiss" => PanelCommand::Dismiss(read(params)?),
        "panel.resize_pair" => PanelCommand::ResizePair(read(params)?),
        "panel.reset_pair" => PanelCommand::ResetPair(read(params)?),
        "layout.save" => {
            if params.get("replace").zip(params.get("overwrite")).is_some_and(|(replace, overwrite)| replace != overwrite) {
                return Err(error(Rule::Schema, "replace and overwrite flags disagree"));
            }
            PanelCommand::Save(read(params)?)
        }
        "layout.apply" => PanelCommand::Apply(read(params)?),
        "layout.reset" => PanelCommand::Reset(read(params)?),
        "layout.workspace.select" => PanelCommand::Workspace(read(params)?),
        _ => return Err(error(Rule::MissingTarget, "not an application layout command")),
    };
    // Validate hostile values even when no application capability is injected.
    validate_command(&command)?;
    Ok(command)
}

fn validate_selector(target: &Option<wire::Target>, id: &Option<String>) -> PanelResult<()> {
    match (target, id) {
        (Some(target), None) => match target {
            wire::Target::Instance(id) => valid_id(&id.0),
            wire::Target::Section(id) => valid_id(&id.0),
            wire::Target::Column(id) => valid_id(&id.0),
            wire::Target::Workspace(id) => valid_id(&id.0),
            wire::Target::NativeHost(id) => valid_id(&id.0),
        },
        (None, Some(id)) => valid_id(id),
        _ => Err(error(Rule::Schema, "supply exactly one typed target or legacy id")),
    }
}
fn validate_destination(destination: Destination) -> PanelResult<()> {
    match destination {
        Destination::NewSection { section, index, location, size } => {
            valid_id(&section.0)?;
            location.validate()?;
            size.validate()?;
            if index > MAX_SECTIONS {
                return Err(error(Rule::Bounds, "section index cap"));
            }
        }
        Destination::JoinTab { section, index } => {
            valid_id(&section.0)?;
            if index >= MAX_TABS {
                return Err(error(Rule::Bounds, "tab index cap"));
            }
        }
        Destination::Relative { section, placement, new_section, new_column, size } => {
            valid_id(&section.0)?;
            valid_id(&new_section.0)?;
            size.validate()?;
            if let Some(column) = &new_column {
                valid_id(&column.0)?;
            }
            let split_column = matches!(placement, RelativePlacement::BeforeColumn | RelativePlacement::AfterColumn);
            if split_column != new_column.is_some() {
                return Err(error(Rule::Placement, "new_column required only for relative column placement"));
            }
        }
    }
    Ok(())
}
fn validate_command(command: &PanelCommand) -> PanelResult<()> {
    match command {
        PanelCommand::Open(p) => {
            valid_id(&p.kind.0)?;
            valid_id(&p.instance.0)?;
            if let Some(destination) = &p.destination {
                validate_destination(destination.clone().into())?;
            }
        }
        PanelCommand::Select { params, .. } => validate_selector(&params.target, &params.id)?,
        PanelCommand::HeaderAction(p) => {
            validate_selector(&p.target, &p.id)?;
            valid_id(&p.action.0)?;
        }
        PanelCommand::Move { params, .. } => {
            validate_selector(&params.target, &params.id)?;
            validate_destination(params.destination.clone().into())?;
        }
        PanelCommand::Resize(p) => {
            validate_selector(&p.target, &p.id)?;
            Size::from(p.size).validate()?;
        }
        PanelCommand::Collapse(p) => validate_selector(&p.target, &p.id)?,
        PanelCommand::ResizePair(p) => {
            valid_id(&p.first.0)?;
            valid_id(&p.second.0)?;
            Size::from(p.first_size).validate()?;
            Size::from(p.second_size).validate()?;
        }
        PanelCommand::ResetPair(p) => {
            valid_id(&p.first.0)?;
            valid_id(&p.second.0)?;
        }
        PanelCommand::Save(p) => {
            preset_name(&p.name)?;
        }
        PanelCommand::Apply(p) => {
            preset_name(&p.name)?;
        }
        PanelCommand::Workspace(p) => valid_id(&p.workspace.0)?,
        _ => {}
    }
    Ok(())
}

pub struct PanelCommandContext<'a> {
    pub sink: &'a mut dyn ActionSink,
    pub registry: &'a PanelRegistry,
    pub catalogue: &'a mut dyn LayoutCatalogue,
    /// Web popout is an explicit host policy, never inferred from no native support.
    pub web: bool,
}

fn target(target: Option<wire::Target>, legacy: Option<String>, layout: &Layout, registry: &PanelRegistry) -> PanelResult<LayoutTarget> {
    if let Some(target) = target {
        return Ok(target.into());
    }
    let id = legacy.ok_or_else(|| error(Rule::Schema, "target required"))?;
    let instance = InstanceId::new(id.clone())?;
    if layout.instance(&instance).is_ok() {
        return Ok(LayoutTarget::Instance(instance));
    }
    let kind = PanelKind::new(id)?;
    let spec = registry.get(&kind)?;
    if spec.features.multi_instance {
        return Err(error(Rule::ExactlyOnce, "kind selector is ambiguous for multi-instance panels; supply typed instance"));
    }
    let matches: Vec<_> = layout.instances.iter().filter(|p| p.kind == kind).collect();
    if matches.len() != 1 {
        return Err(error(Rule::MissingTarget, "kind requires one canonical retained instance"));
    }
    Ok(LayoutTarget::Instance(matches.first().ok_or_else(|| error(Rule::MissingTarget, "canonical instance absent"))?.id.clone()))
}
fn instance(target: LayoutTarget, layout: &Layout) -> PanelResult<InstanceId> {
    match target {
        LayoutTarget::Instance(id) => {
            layout.instance(&id)?;
            Ok(id)
        }
        _ => Err(error(Rule::Schema, "operation requires instance target")),
    }
}
fn section(target: LayoutTarget, layout: &Layout) -> PanelResult<SectionId> {
    match target {
        LayoutTarget::Section(id) => {
            layout.section(&id)?;
            Ok(id)
        }
        LayoutTarget::Instance(id) => Ok(layout.containing(&id)?.id.clone()),
        _ => Err(error(Rule::Schema, "operation requires section or open instance target")),
    }
}
fn revision(expected: Option<u64>, state: &LayoutReply) -> PanelResult<()> {
    if expected.is_some_and(|r| r != state.revision) {
        return Err(error(Rule::Revision, "stale layout revision"));
    }
    Ok(())
}

fn lower(command: PanelCommand, state: &LayoutReply, registry: &PanelRegistry, web: bool) -> PanelResult<Mutation> {
    let layout = &state.layout;
    Ok(match command {
        PanelCommand::Open(p) => {
            revision(p.expected_revision, state)?;
            Mutation::Open { kind: p.kind, instance: p.instance, destination: p.destination.map(Into::into) }
        }
        PanelCommand::Select { operation, params: p } => {
            revision(p.expected_revision, state)?;
            let selected = target(p.target, p.id, layout, registry)?;
            if operation == "panel.native_close" {
                let LayoutTarget::NativeHost(host) = selected else {
                    return Err(error(Rule::Schema, "native close requires typed native_host target"));
                };
                Mutation::NativeClose { host }
            } else {
                let instance = instance(selected, layout)?;
                match operation {
                    "panel.close" => Mutation::Close { instance },
                    "panel.focus" => Mutation::Focus { instance },
                    "panel.activate" => Mutation::Activate { instance },
                    "panel.peek" => Mutation::Peek { instance },
                    _ => return Err(error(Rule::Schema, "unknown target operation")),
                }
            }
        }
        PanelCommand::Move { operation, params: p } => {
            revision(p.expected_revision, state)?;
            let instance = instance(target(p.target, p.id, layout, registry)?, layout)?;
            let mut destination: Destination = p.destination.into();
            if operation == "panel.float" && !matches!(&destination, Destination::NewSection { location: SectionLocation::Floating { .. }, .. }) {
                return Err(error(Rule::Placement, "panel.float requires new_section with floating location"));
            }
            if operation == "panel.popout" {
                if !registry.get(&layout.instance(&instance)?.kind)?.features.popout {
                    return Err(error(Rule::FeatureDisabled, "panel cannot pop out"));
                }
                match &mut destination {
                    Destination::NewSection { location, .. } => match location {
                        SectionLocation::Window { origin, .. } => {
                            if web {
                                *location = SectionLocation::Floating { origin: *origin };
                            }
                        }
                        _ => return Err(error(Rule::Placement, "panel.popout requires new_section with window location")),
                    },
                    _ => return Err(error(Rule::Placement, "panel.popout requires a new section")),
                }
            }
            Mutation::Move { instance, destination }
        }
        PanelCommand::Resize(p) => {
            revision(p.expected_revision, state)?;
            match target(p.target, p.id, layout, registry)? {
                LayoutTarget::Column(column) => Mutation::SetColumnWidth { column, width: p.size.width },
                selected => {
                    let section = section(selected, layout)?;
                    if p.thickness_only {
                        let group = layout.section(&section)?;
                        match &group.location {
                            SectionLocation::Dock { zone: Zone::Bottom } => Mutation::SetBottomHeight { section, height: p.size.height },
                            SectionLocation::Dock { zone: Zone::Left | Zone::Right } => Mutation::SetColumnWidth {
                                column: group.column.clone().ok_or_else(|| error(Rule::Placement, "side section requires a column"))?,
                                width: p.size.width,
                            },
                            _ => return Err(error(Rule::Placement, "thickness-only resize requires a dock target")),
                        }
                    } else {
                        Mutation::SetSize { section, size: p.size.into() }
                    }
                }
            }
        }
        PanelCommand::HeaderAction(p) => {
            revision(p.expected_revision, state)?;
            Mutation::HeaderAction { instance: instance(target(p.target, p.id, layout, registry)?, layout)?, action: p.action }
        }
        PanelCommand::Collapse(p) => {
            revision(p.expected_revision, state)?;
            Mutation::Collapse { section: section(target(p.target, p.id, layout, registry)?, layout)?, collapsed: p.collapsed }
        }
        PanelCommand::Dismiss(p) => {
            revision(p.expected_revision, state)?;
            Mutation::DismissPeek
        }
        PanelCommand::ResizePair(p) => {
            revision(p.expected_revision, state)?;
            Mutation::ResizePair { first: p.first, second: p.second, first_size: p.first_size.into(), second_size: p.second_size.into() }
        }
        PanelCommand::ResetPair(p) => {
            revision(p.expected_revision, state)?;
            Mutation::ResetPair { first: p.first, second: p.second }
        }
        _ => return Err(error(Rule::Capability, "catalogue/query commands require individual transactions")),
    })
}

fn current_envelope(catalogue: &CatalogueState, state: &LayoutReply, registry: &PanelRegistry) -> PanelResult<(LayoutEnvelope, Option<Arc<[u8]>>)> {
    let workspace = &state.layout.workspace;
    match catalogue.remembered.get(workspace) {
        Some(stored) => {
            Ok((with_live_layout(&stored.project_envelope(registry, workspace)?, state.layout.clone(), registry)?, stored.rejected.clone()))
        }
        None => Ok((LayoutEnvelope::new(state.layout.clone()), None)),
    }
}

/// Read-only retained projection for reservation capture. Do not force expanded
/// v1 writes merely to inspect identities; callers union current live IDs too.
pub fn current_envelope_snapshot(catalogue: &dyn LayoutCatalogue, state: &LayoutReply, registry: &PanelRegistry) -> PanelResult<LayoutEnvelope> {
    match catalogue.state().remembered(&state.layout.workspace) {
        Some(stored) => stored.project_envelope(registry, &state.layout.workspace),
        None => Ok(LayoutEnvelope::new(state.layout.clone())),
    }
}

/// Preserve all opaque sidecars. Reusing an identity with different opaque intent
/// is an error, never silently resolved by choosing one preset's value.
fn merge_envelopes(current: &LayoutEnvelope, mut next: LayoutEnvelope, registry: &PanelRegistry) -> PanelResult<LayoutEnvelope> {
    let current_priorities = preparation_context(current)?.extent_priorities;
    let incoming_priorities = preparation_context(&next)?.extent_priorities;
    if let Some(raw) = &current.recovery_wire {
        if next.recovery_wire.as_ref().is_some_and(|other| other != raw) {
            return Err(error(Rule::DuplicateId, "opaque envelope recovery intent collision"));
        }
        next.recovery_wire = Some(raw.clone());
    }
    next.layout.workspace = current.layout.workspace.clone();
    for item in &current.quarantine {
        if next.layout.instances.iter().any(|known| known.id == item.instance) {
            return Err(error(Rule::DuplicateId, "preset collides with quarantined identity"));
        }
        match next.quarantine.iter().find(|q| q.instance == item.instance) {
            Some(existing) if existing != item => return Err(error(Rule::DuplicateId, "quarantine sidecar collision")),
            Some(_) => {}
            None => next.quarantine.push(item.clone()),
        }
    }
    for (id, data) in &current.retained_data {
        if next.retained_data.get(id).is_some_and(|existing| existing != data) {
            return Err(error(Rule::DuplicateId, "retained data collision"));
        }
        if let (Ok(old), Ok(new)) = (current.layout.instance(id), next.layout.instance(id))
            && old.kind != new.kind
        {
            return Err(error(Rule::DuplicateId, "retained instance kind collision"));
        }
        next.retained_data.insert(id.clone(), data.clone());
    }
    for (id, snapshot) in &current.section_snapshots {
        if next.section_snapshots.get(id).is_some_and(|existing| existing != snapshot) {
            return Err(error(Rule::DuplicateId, "section sidecar collision"));
        }
        next.section_snapshots.insert(id.clone(), snapshot.clone());
    }
    // Preserve dormant gaps from current; subsequent refresh overlays live order.
    let dormant: std::collections::BTreeSet<_> = next.quarantine.iter().filter_map(|q| q.section.clone()).collect();
    let live: std::collections::BTreeSet<_> = next.layout.sections.iter().map(|s| s.id.clone()).collect();
    let mut order: Vec<_> = current.section_order.iter().filter(|id| dormant.contains(*id) || live.contains(*id)).cloned().collect();
    for id in &next.section_order {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    next.section_order = order;
    super::persistence::merge_retained_priority_context(&mut next, &current_priorities, &incoming_priorities)?;
    let layout = next.layout.clone();
    with_live_layout(&next, layout, registry)
}

fn stage_current(
    context: &mut PanelCommandContext<'_>,
    state: &LayoutReply,
    action: &LayoutAction,
    envelope: &LayoutEnvelope,
    recovery: Option<Arc<[u8]>>,
    next: CatalogueState,
) -> PanelResult<LayoutReply> {
    let preparation = preparation_context(envelope)?;
    stage_current_prepared(context, state, action, envelope, recovery, next, &preparation)
}

fn stage_current_prepared(
    context: &mut PanelCommandContext<'_>,
    state: &LayoutReply,
    action: &LayoutAction,
    envelope: &LayoutEnvelope,
    recovery: Option<Arc<[u8]>>,
    mut next: CatalogueState,
    preparation: &LayoutPreparationContext,
) -> PanelResult<LayoutReply> {
    let source_catalogue = context.catalogue.state().clone();
    if *action == LayoutAction::Inspect {
        // Saving catalogue metadata does not change placement. Existing overflow
        // must not prevent saving the arrangement that the user can already see.
        let source_context = preparation_context(&current_envelope_snapshot(&*context.catalogue, state, context.registry)?)?;
        let envelope = with_live_layout(envelope, state.layout.clone(), context.registry)?;
        let stored = StoredEnvelope::new(&envelope, recovery.as_deref(), context.registry)?;
        next.remembered.insert(envelope.layout.workspace.clone(), stored);
        let prepared = context.catalogue.prepare(next)?;
        let current = context.sink.inspect();
        if context.catalogue.state() != &source_catalogue || current != *state {
            return Err(error(Rule::Revision, "layout or catalogue changed during storage staging"));
        }
        let current_context = preparation_context(&current_envelope_snapshot(&*context.catalogue, &current, context.registry)?)?;
        if current_context != source_context {
            return Err(error(Rule::Revision, "layout context changed during storage staging"));
        }
        context.catalogue.publish(prepared);
        return Ok(current);
    }
    let projected = context.sink.project_in_context(action.clone(), preparation)?;
    let workspace = match action {
        LayoutAction::Replace { layout, .. } => &layout.workspace,
        _ => &state.layout.workspace,
    };
    if &projected.workspace != workspace {
        return Err(error(Rule::Revision, "projected layout changed requested workspace"));
    }
    let envelope = with_live_layout(envelope, projected, context.registry)?;
    let stored = StoredEnvelope::new(&envelope, recovery.as_deref(), context.registry)?;
    next.remembered.insert(envelope.layout.workspace.clone(), stored);
    if context.catalogue.state() != &source_catalogue {
        return Err(error(Rule::Revision, "catalogue changed during layout projection"));
    }
    let prepared = context.catalogue.prepare(next)?;
    if context.catalogue.state() != &source_catalogue {
        return Err(error(Rule::Revision, "catalogue changed during storage staging"));
    }
    // Inspect can change through external adapters between planning and dispatch.
    // Every action below includes the inspected revision; no stale catalogue publishes.
    if context.sink.inspect().revision != state.revision {
        return Err(error(Rule::Revision, "layout changed during storage staging"));
    }
    let committed = context.sink.dispatch_in_context(action.clone(), preparation)?;
    context.catalogue.publish(prepared);
    Ok(committed)
}

/// Current-workspace GUI actions share the command catalogue preflight. The sink
/// owns the exact private proposal returned by project and consumed by dispatch.
pub fn execute_layout_action(action: LayoutAction, context: &mut PanelCommandContext<'_>) -> PanelResult<LayoutReply> {
    if let LayoutAction::Apply { mutations, .. } = &action {
        if mutations.is_empty() || mutations.len() > MAX_MUTATIONS {
            return Err(error(Rule::CollectionLimit, "transaction requires 1..64 mutations"));
        }
        validate_lifecycle_intents(mutations)?;
        validate_header_action_transaction(mutations)?;
        if matches!(mutations.first(), Some(Mutation::HeaderAction { .. })) {
            // Registration, revision, presentation, availability and controller
            // permission remain authoritative sink responsibilities.
            return context.sink.dispatch(action);
        }
    }
    let state = context.sink.inspect();
    let action = match action {
        LayoutAction::Apply { expected_revision, mutations } => {
            revision(expected_revision, &state)?;
            LayoutAction::Apply { expected_revision: Some(state.revision), mutations }
        }
        LayoutAction::Replace { expected_revision, layout } => {
            revision(expected_revision, &state)?;
            if layout.workspace != state.layout.workspace {
                return Err(error(Rule::Capability, "workspace replacement requires workspace command"));
            }
            LayoutAction::Replace { expected_revision: Some(state.revision), layout }
        }
        LayoutAction::Inspect => LayoutAction::Inspect,
    };
    let next = context.catalogue.state().clone();
    let (current, recovery) = current_envelope(&next, &state, context.registry)?;
    stage_current(context, &state, &action, &current, recovery, next)
}

pub fn execute_command(command: PanelCommand, context: &mut PanelCommandContext<'_>) -> PanelResult<Value> {
    validate_command(&command)?;
    let state = context.sink.inspect();
    match command {
        PanelCommand::Contextual { action, params } => {
            revision(params.expected_revision, &state)?;
            let action = context.sink.resolve_contextual(action)?;
            return value(execute_layout_action(action, context)?);
        }
        PanelCommand::List => return value(wire::ListReply { state, kinds: context.registry.specs().cloned().collect() }),
        PanelCommand::Get => return value(wire::GetReply { state, workspaces: context.catalogue.state().summaries() }),
        command @ PanelCommand::HeaderAction(_) => {
            let mutation = lower(command, &state, context.registry, context.web)?;
            return value(execute_layout_action(
                LayoutAction::Apply { expected_revision: Some(state.revision), mutations: vec![mutation] },
                context,
            )?);
        }
        PanelCommand::Workspace(ref p) if p.workspace == state.layout.workspace => {
            revision(p.expected_revision, &state)?;
            return value(wire::WorkspaceReply {
                previous_workspace: state.layout.workspace.clone(),
                source: wire::WorkspaceSource::Unchanged,
                state,
            });
        }
        _ => {}
    }
    let mut next = context.catalogue.state().clone();
    let (current, recovery) = current_envelope(&next, &state, context.registry)?;
    match command {
        PanelCommand::Save(p) => {
            revision(p.expected_revision, &state)?;
            let requested = preset_name(&p.name)?;
            let existing = next.find_preset_name(&requested);
            if matches!(existing, Some((PresetKind::BuiltIn, _))) {
                return Err(error(Rule::Capability, "built-in presets are immutable"));
            }
            let name = existing.map(|(_, name)| name.to_string()).unwrap_or(requested);
            let overwritten = next.users.contains_key(&name);
            if overwritten && !p.replace && !p.overwrite {
                return Err(error(Rule::DuplicateId, "preset exists; replace must be explicit"));
            }
            if !overwritten && next.users.len() >= MAX_USER_PRESETS {
                return Err(error(Rule::CollectionLimit, "user preset count cap"));
            }
            next.users.insert(name.clone(), StoredEnvelope::new(&current, recovery.as_deref(), context.registry)?);
            let action = LayoutAction::Inspect;
            let state = stage_current(context, &state, &action, &current, recovery, next)?;
            value(wire::SaveReply { name, overwritten, state })
        }
        PanelCommand::Apply(p) => {
            revision(p.expected_revision, &state)?;
            let stored = next.preset(&preset_name(&p.name)?)?;
            let mut envelope: LayoutEnvelope =
                serde_json::from_slice(stored.bytes()).map_err(|_| error(Rule::Schema, "invalid registered envelope"))?;
            // Decode using its original workspace before rebinding to the current one.
            envelope = stored.project_envelope(context.registry, &envelope.layout.workspace)?;
            let mut envelope = merge_envelopes(&current, envelope, context.registry)?;
            restore_quarantine(&mut envelope, context.registry)?;
            let recovery = merge_recovery(recovery, stored.rejected.clone())?;
            let action = LayoutAction::Replace { expected_revision: Some(state.revision), layout: envelope.layout.clone() };
            value(stage_current(context, &state, &action, &envelope, recovery, next)?)
        }
        PanelCommand::Reset(p) => {
            revision(p.expected_revision, &state)?;
            let descriptor = next
                .workspaces
                .get(&state.layout.workspace)
                .ok_or_else(|| error(Rule::Capability, "selected workspace has no registered default"))?;
            let stored = next.preset(&descriptor.default_preset)?;
            let raw: LayoutEnvelope = serde_json::from_slice(stored.bytes()).map_err(|_| error(Rule::Schema, "invalid default envelope"))?;
            let mut envelope = merge_envelopes(&current, stored.project_envelope(context.registry, &raw.layout.workspace)?, context.registry)?;
            restore_quarantine(&mut envelope, context.registry)?;
            let recovery = merge_recovery(recovery, stored.rejected.clone())?;
            let action = LayoutAction::Replace { expected_revision: Some(state.revision), layout: envelope.layout.clone() };
            value(stage_current(context, &state, &action, &envelope, recovery, next)?)
        }
        PanelCommand::Workspace(p) => {
            revision(p.expected_revision, &state)?;
            let previous_workspace = state.layout.workspace.clone();
            if p.workspace == previous_workspace {
                return value(wire::WorkspaceReply { previous_workspace, source: wire::WorkspaceSource::Unchanged, state });
            }
            let descriptor = next.workspaces.get(&p.workspace).ok_or_else(|| error(Rule::MissingTarget, "workspace not registered"))?;
            let (stored, source) = match next.remembered.get(&p.workspace) {
                Some(stored) => (stored, wire::WorkspaceSource::Remembered),
                None => (next.preset(&descriptor.default_preset)?, wire::WorkspaceSource::DefaultPreset),
            };
            let raw: LayoutEnvelope = serde_json::from_slice(stored.bytes()).map_err(|_| error(Rule::Schema, "invalid workspace envelope"))?;
            let mut envelope = stored.project_envelope(context.registry, &raw.layout.workspace)?;
            envelope.layout.workspace = p.workspace;
            restore_quarantine(&mut envelope, context.registry)?;
            let incoming_recovery = stored.rejected.clone();
            next.remembered.insert(previous_workspace.clone(), StoredEnvelope::new(&current, recovery.as_deref(), context.registry)?);
            let action = LayoutAction::Replace { expected_revision: Some(state.revision), layout: envelope.layout.clone() };
            let state = stage_current(context, &state, &action, &envelope, incoming_recovery, next)?;
            value(wire::WorkspaceReply { previous_workspace, source, state })
        }
        command => {
            let activate = matches!(&command, PanelCommand::Move { params, .. } if params.activate);
            let mutation = lower(command, &state, context.registry, context.web)?;
            let activation = if activate {
                match &mutation {
                    Mutation::Move { instance, .. } => Some(Mutation::Activate { instance: instance.clone() }),
                    _ => None,
                }
            } else {
                None
            };
            let mut mutations = vec![mutation];
            mutations.extend(activation);
            let action = LayoutAction::Apply { expected_revision: Some(state.revision), mutations };
            value(stage_current(context, &state, &action, &current, recovery, next)?)
        }
    }
}
fn merge_recovery(first: Option<Arc<[u8]>>, second: Option<Arc<[u8]>>) -> PanelResult<Option<Arc<[u8]>>> {
    if first.is_some() && second.is_some() && first != second {
        return Err(error(Rule::DuplicateId, "rejected recovery intent collision"));
    }
    Ok(first.or(second))
}
fn value(value: impl serde::Serialize) -> PanelResult<Value> {
    serde_json::to_value(value).map_err(|_| error(Rule::Schema, "cannot encode committed layout reply"))
}
pub fn execute(id: &str, params: &Value, context: &mut PanelCommandContext<'_>) -> PanelResult<Value> {
    execute_command(parse_command(id, params)?, context)
}

/// A private transaction planner reuses the common executor and the LIVE sink's
/// pure projection. It owns no reducer or LayoutService and cannot publish live
/// effects. Only the final guarded action and prepared catalogue are published.
struct PlanningSink<'a> {
    authoritative: &'a dyn ActionSink,
    state: LayoutReply,
    mutations: Vec<Mutation>,
    replaced: bool,
    preparation: LayoutPreparationContext,
}
impl ActionSink for PlanningSink<'_> {
    fn inspect(&self) -> LayoutReply {
        self.state.clone()
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        match action {
            LayoutAction::Inspect => Ok(self.state.layout.clone()),
            LayoutAction::Replace { expected_revision, layout } => {
                revision(expected_revision, &self.state)?;
                if !self.mutations.is_empty() {
                    return Err(error(
                        Rule::Capability,
                        "atomic batches cannot combine snapshot replacement and explicit panel mutations; use rollback:false",
                    ));
                }
                self.authoritative.project(LayoutAction::Replace { expected_revision: Some(self.state.revision), layout })
            }
            LayoutAction::Apply { expected_revision, mutations } => {
                revision(expected_revision, &self.state)?;
                if self.replaced {
                    return Err(error(
                        Rule::Capability,
                        "atomic batches cannot combine snapshot replacement and explicit panel mutations; use rollback:false",
                    ));
                }
                if self.mutations.len().saturating_add(mutations.len()) > MAX_MUTATIONS {
                    return Err(error(Rule::CollectionLimit, "layout batch mutation cap"));
                }
                let mut combined = self.mutations.clone();
                combined.extend(mutations);
                self.authoritative.project(LayoutAction::Apply { expected_revision: Some(self.state.revision), mutations: combined })
            }
        }
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        let layout = self.project(action.clone())?;
        match action {
            LayoutAction::Apply { mutations, .. } => self.mutations.extend(mutations),
            LayoutAction::Replace { .. } => {
                self.replaced = true;
                self.state.interaction = InteractionState::default();
            }
            LayoutAction::Inspect => {}
        }
        self.state.layout = layout;
        Ok(self.state.clone())
    }
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        match action {
            LayoutAction::Apply { expected_revision, mutations } => {
                revision(expected_revision, &self.state)?;
                if self.replaced {
                    return Err(error(Rule::Capability, "cannot combine replacement and mutations"));
                }
                if self.mutations.len().saturating_add(mutations.len()) > MAX_MUTATIONS {
                    return Err(error(Rule::CollectionLimit, "layout batch mutation cap"));
                }
                let mut combined = self.mutations.clone();
                combined.extend(mutations);
                self.authoritative
                    .project_in_context(LayoutAction::Apply { expected_revision: Some(self.state.revision), mutations: combined }, &self.preparation)
            }
            LayoutAction::Replace { expected_revision, layout } => {
                revision(expected_revision, &self.state)?;
                if !self.mutations.is_empty() {
                    return Err(error(Rule::Capability, "cannot combine replacement and mutations"));
                }
                self.authoritative.project_in_context(LayoutAction::Replace { expected_revision: Some(self.state.revision), layout }, context)
            }
            LayoutAction::Inspect => Ok(self.state.layout.clone()),
        }
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        let layout = self.project_in_context(action.clone(), context)?;
        match action {
            LayoutAction::Apply { mutations, .. } => self.mutations.extend(mutations),
            LayoutAction::Replace { .. } => {
                self.replaced = true;
                self.state.interaction = InteractionState::default();
            }
            LayoutAction::Inspect => {}
        }
        self.state.layout = layout;
        Ok(self.state.clone())
    }
}

/// Pure structural preflight, usable before application context publication.
/// Same-workspace selection remains a no-op and may accompany mutations.
pub fn preflight_batch(steps: &[Value], workspace: &WorkspaceId) -> PanelResult<()> {
    if steps.is_empty() || steps.len() > MAX_MUTATIONS {
        return Err(error(Rule::CollectionLimit, "layout batch requires 1..64 steps"));
    }
    let mut selected = workspace.clone();
    let mut replacement = false;
    let mut mutation = false;
    let mut header_action = false;
    let mut contextual = false;
    for step in steps {
        let id =
            step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).ok_or_else(|| error(Rule::Schema, "batch command required"))?;
        let empty = json!({});
        match parse_command(id, step.get("params").unwrap_or(&empty))? {
            PanelCommand::List | PanelCommand::Get | PanelCommand::Save(_) => {}
            PanelCommand::HeaderAction(_) => header_action = true,
            PanelCommand::Contextual { .. } => contextual = true,
            PanelCommand::Apply(_) | PanelCommand::Reset(_) => replacement = true,
            PanelCommand::Workspace(params) => {
                replacement |= params.workspace != selected;
                selected = params.workspace;
            }
            _ => mutation = true,
        }
    }
    if contextual && steps.len() != 1 {
        return Err(error(Rule::Capability, "contextual panel actions require a standalone transaction"));
    }
    if header_action && steps.len() != 1 {
        return Err(error(Rule::Capability, "header actions require a standalone transaction"));
    }
    if replacement && mutation {
        return Err(error(Rule::Capability, "atomic batches cannot combine snapshot replacement and explicit panel mutations; use rollback:false"));
    }
    Ok(())
}

/// Atomic batches use one Apply or Replace and one catalogue/native/controller
/// preflight. Explicit lifecycle intent is never flattened into a replacement.
pub fn execute_batch(steps: &[Value], context: &mut PanelCommandContext<'_>) -> PanelResult<Value> {
    if steps.is_empty() || steps.len() > MAX_MUTATIONS {
        return Err(error(Rule::CollectionLimit, "layout batch requires 1..64 steps"));
    }
    let state = context.sink.inspect();
    preflight_batch(steps, &state.layout.workspace)?;
    if let Some(step) = steps.first() {
        let id =
            step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).ok_or_else(|| error(Rule::Schema, "batch command required"))?;
        let empty = json!({});
        let command = parse_command(id, step.get("params").unwrap_or(&empty))?;
        if matches!(&command, PanelCommand::HeaderAction(_) | PanelCommand::Contextual { .. }) {
            let result = execute_command(command, context)?;
            return Ok(json!({"completed":1,"results":[result],"undone":0}));
        }
    }
    let mut planned = InMemoryCatalogue { state: context.catalogue.state().clone(), fail_stage: false };
    let preparation = preparation_context(&current_envelope_snapshot(&*context.catalogue, &state, context.registry)?)?;
    let mut sink = PlanningSink {
        authoritative: &*context.sink,
        state: state.clone(),
        mutations: Vec::new(),
        replaced: false,
        preparation: preparation.clone(),
    };
    let mut rows = Vec::new();
    for step in steps {
        let id =
            step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).ok_or_else(|| error(Rule::Schema, "batch command required"))?;
        let id = crate::find_command(id).map(|spec| spec.id).ok_or_else(|| error(Rule::MissingTarget, "batch command not registered"))?;
        let empty = json!({});
        let result = execute(
            id,
            step.get("params").filter(|p| !p.is_null()).unwrap_or(&empty),
            &mut PanelCommandContext { sink: &mut sink, registry: context.registry, catalogue: &mut planned, web: context.web },
        )?;
        rows.push((id, result));
    }
    let action = if sink.replaced {
        LayoutAction::Replace { expected_revision: Some(state.revision), layout: sink.state.layout.clone() }
    } else if !sink.mutations.is_empty() {
        LayoutAction::Apply { expected_revision: Some(state.revision), mutations: sink.mutations.clone() }
    } else {
        LayoutAction::Inspect
    };
    let planned_state = sink.state.clone();
    let changed = planned.state != *context.catalogue.state() || action != LayoutAction::Inspect;
    // Release the immutable authoritative borrow before committing through it.
    drop(sink);
    let committed = if changed {
        let (envelope, recovery) = current_envelope(&planned.state, &planned_state, context.registry)?;
        let final_context = if matches!(&action, LayoutAction::Apply { .. }) { preparation } else { preparation_context(&envelope)? };
        stage_current_prepared(context, &state, &action, &envelope, recovery, planned.state, &final_context)?
    } else {
        context.sink.inspect()
    };
    let mut results = Vec::new();
    for (id, mut result) in rows {
        match id {
            "panel.list" => result = value(wire::ListReply { state: committed.clone(), kinds: context.registry.specs().cloned().collect() })?,
            "layout.get" => result = value(wire::GetReply { state: committed.clone(), workspaces: context.catalogue.state().summaries() })?,
            "layout.save" | "layout.workspace.select" => result["state"] = value(committed.clone())?,
            _ => result = value(committed.clone())?,
        }
        results.push(result);
    }
    Ok(json!({"completed":steps.len(),"results":results,"undone":0}))
}

/// Transport-compatible error text retains the structured foundation identity.
/// Control/MCP also expose this as panel_error beside their usual error string.
pub fn error_text(reason: PanelError) -> String {
    serde_json::to_string(&reason).unwrap_or_else(|_| "{\"rule\":\"schema\",\"detail\":\"layout error encoding failed\"}".into())
}
pub fn structured_error(text: &str) -> Option<Value> {
    if text.len() > 4096 {
        return None;
    }
    serde_json::from_str::<PanelError>(text).ok().and_then(|reason| value(error(reason.rule, reason.detail)).ok())
}

pub fn sequential_failure(index: usize, command: Option<&str>, text: String, results: Vec<Value>) -> Value {
    let mut failed = json!({"index":index,"command":command,"error":text});
    if let Some(reason) = structured_error(&text) {
        failed["panel_error"] = reason;
    }
    json!({"completed":index,"results":results,"failed":failed,"undone":0,"rolled_back":false})
}

/// One owned, persistent in-memory headless backend, outside Session. No kinds,
/// presets or workspace descriptors are invented. Peers/fixtures may inject them.
pub struct InMemoryPanelBackend {
    pub service: LayoutService,
    pub catalogue: InMemoryCatalogue,
}

/// In-process layout has no native realization. Even a supplied service with
/// native flags cannot make this adapter claim that it opened an OS window.
struct HeadlessSink<'a>(&'a mut LayoutService);
impl ActionSink for HeadlessSink<'_> {
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        let availability = if matches!(&action, LayoutAction::Replace { .. }) { AvailabilityMap::new() } else { self.0.inspect().availability };
        let prepared = self.0.prepare_with_context(action, &availability, context)?;
        if prepared
            .effects()
            .iter()
            .any(|effect| matches!(effect, HostEffect::CreateNative { .. } | HostEffect::RebindNative { .. } | HostEffect::HeaderAction { .. }))
        {
            return Err(error(Rule::Capability, "headless cannot realize native or controller actions"));
        }
        Ok(prepared.layout().clone())
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        let availability = if matches!(&action, LayoutAction::Replace { .. }) { AvailabilityMap::new() } else { self.0.inspect().availability };
        let prepared = self.0.prepare_with_context(action, &availability, context)?;
        if prepared
            .effects()
            .iter()
            .any(|effect| matches!(effect, HostEffect::CreateNative { .. } | HostEffect::RebindNative { .. } | HostEffect::HeaderAction { .. }))
        {
            return Err(error(Rule::Capability, "headless cannot realize native or controller actions"));
        }
        let mut reply = self.0.commit(prepared)?;
        reply.capabilities.native_windows = false;
        reply.capabilities.max_native_hosts = 0;
        Ok(reply)
    }
    fn inspect(&self) -> LayoutReply {
        let mut state = self.0.inspect();
        state.capabilities.native_windows = false;
        state.capabilities.max_native_hosts = 0;
        state
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        let prepared = self.0.prepare(action)?;
        if prepared.effects().iter().any(|effect| matches!(effect, HostEffect::CreateNative { .. } | HostEffect::RebindNative { .. })) {
            return Err(error(Rule::Capability, "headless has no native host adapter"));
        }
        if prepared.effects().iter().any(|effect| matches!(effect, HostEffect::HeaderAction { .. })) {
            return Err(error(Rule::Capability, "headless has no owning panel action controller"));
        }
        Ok(prepared.layout().clone())
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        self.project(action.clone())?;
        let mut state = self.0.dispatch(action)?;
        state.capabilities.native_windows = false;
        state.capabilities.max_native_hosts = 0;
        Ok(state)
    }
}
impl InMemoryPanelBackend {
    pub fn new(service: LayoutService, catalogue: InMemoryCatalogue) -> Self {
        Self { service, catalogue }
    }
    pub fn empty() -> PanelResult<Self> {
        Ok(Self::new(
            LayoutService::new(PanelRegistry::default(), Layout::empty(WorkspaceId::new("headless")?), HostCapabilities::default())?,
            InMemoryCatalogue::default(),
        ))
    }
    pub fn execute(&mut self, id: &str, params: &Value) -> PanelResult<Value> {
        let registry = self.service.registry().clone();
        execute(
            id,
            params,
            &mut PanelCommandContext { sink: &mut HeadlessSink(&mut self.service), registry: &registry, catalogue: &mut self.catalogue, web: false },
        )
    }
    pub fn batch(&mut self, steps: &[Value]) -> PanelResult<Value> {
        let registry = self.service.registry().clone();
        execute_batch(
            steps,
            &mut PanelCommandContext { sink: &mut HeadlessSink(&mut self.service), registry: &registry, catalogue: &mut self.catalogue, web: false },
        )
    }
}
