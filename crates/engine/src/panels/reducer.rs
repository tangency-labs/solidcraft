use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::*;

/// Joining an existing tab group and creating a split are deliberately distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelativePlacement {
    BeforeColumn,
    AfterColumn,
    Above,
    Below,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Destination {
    NewSection { section: SectionId, index: usize, location: SectionLocation, size: Size },
    JoinTab { section: SectionId, index: usize },
    Relative { section: SectionId, placement: RelativePlacement, new_section: SectionId, new_column: Option<ColumnId>, size: Size },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mutation {
    Open {
        kind: PanelKind,
        instance: InstanceId,
        destination: Option<Destination>,
    },
    Close {
        instance: InstanceId,
    },
    /// Explicit owner cleanup, only after close. Hiding is never destruction.
    Destroy {
        instance: InstanceId,
    },
    Move {
        instance: InstanceId,
        destination: Destination,
    },
    Activate {
        instance: InstanceId,
    },
    Focus {
        instance: InstanceId,
    },
    Peek {
        instance: InstanceId,
    },
    DismissPeek,
    Collapse {
        section: SectionId,
        collapsed: bool,
    },
    SetSize {
        section: SectionId,
        size: Size,
    },
    SetColumnWidth {
        column: ColumnId,
        width: f32,
    },
    ResetSideHeight {
        section: SectionId,
    },
    SetBottomHeight {
        section: SectionId,
        height: f32,
    },
    HeaderAction {
        instance: InstanceId,
        action: ControlId,
    },
    /// Resize/reposition an existing floating section as one retained group.
    /// This changes geometry and placement memories, never parent or focus.
    SetFloatingRect {
        section: SectionId,
        origin: Point,
        size: Size,
    },
    /// Both neighbours change together; their shared axis extent is conserved.
    ResizePair {
        first: SectionId,
        second: SectionId,
        first_size: Size,
        second_size: Size,
    },
    /// Host-admitted actual geometry and normalized shares. Unlike ResizePair,
    /// actual extent conservation is checked against the captured allocator.
    ResizeAllocatedPair {
        first: SectionId,
        second: SectionId,
        first_size: Size,
        second_size: Size,
        first_weight: f32,
        second_weight: f32,
        #[serde(default = "unit_weight_scale")]
        other_weight_scale: f32,
        #[serde(default)]
        prefer_allocated: bool,
    },
    ResetPair {
        first: SectionId,
        second: SectionId,
    },
    /// A native window's close button redocks its whole section.
    NativeClose {
        host: HostId,
    },
}

fn unit_weight_scale() -> f32 {
    1.0
}

/// Content actions have their own controller transaction, never placement admission.
pub fn validate_header_action_transaction(mutations: &[Mutation]) -> PanelResult<()> {
    if mutations.len() > MAX_MUTATIONS {
        return Err(error(Rule::CollectionLimit, "header action transaction cap"));
    }
    if mutations.len() != 1 && mutations.iter().any(|m| matches!(m, Mutation::HeaderAction { .. })) {
        return Err(error(Rule::Capability, "header actions require a standalone transaction"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutAction {
    Inspect,
    Apply {
        expected_revision: Option<u64>,
        mutations: Vec<Mutation>,
    },
    /// Apply a complete saved layout/preset, including a workspace change.
    /// Omission and requested_open=false hide content without lifecycle intent.
    Replace {
        expected_revision: Option<u64>,
        layout: Layout,
    },
}

/// Stable targets for a later command adapter; none are registered with Session yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum LayoutTarget {
    Workspace(WorkspaceId),
    Instance(InstanceId),
    Section(SectionId),
    Column(ColumnId),
    NativeHost(HostId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostEffect {
    Focus { instance: InstanceId },
    Peek { instance: InstanceId },
    DismissPeek,
    CreateNative { host: HostId, section: SectionId },
    RebindNative { host: HostId, section: SectionId },
    ReleaseNative { host: HostId },
    HeaderAction { instance: InstanceId, action: ControlId },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InteractionState {
    pub focused: Option<InstanceId>,
    pub peek: Option<InstanceId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutReply {
    pub api_version: u32,
    pub revision: u64,
    pub layout: Layout,
    pub interaction: InteractionState,
    pub effects: Vec<HostEffect>,
    pub capabilities: HostCapabilities,
    pub availability: AvailabilityMap,
}

/// Keyboard actions whose target belongs to the live panel host, never the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualPanelAction {
    FocusNext,
    FocusPrevious,
    Toggle,
    MoveNextZone,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    ResetSash,
}
impl ContextualPanelAction {
    pub const ALL: [Self; 9] = [
        Self::FocusNext,
        Self::FocusPrevious,
        Self::Toggle,
        Self::MoveNextZone,
        Self::ResizeLeft,
        Self::ResizeRight,
        Self::ResizeUp,
        Self::ResizeDown,
        Self::ResetSash,
    ];
    pub const fn command_id(self) -> &'static str {
        match self {
            Self::FocusNext => "panel.focus.next",
            Self::FocusPrevious => "panel.focus.previous",
            Self::Toggle => "panel.toggle",
            Self::MoveNextZone => "panel.move.next_zone",
            Self::ResizeLeft => "panel.sash.left",
            Self::ResizeRight => "panel.sash.right",
            Self::ResizeUp => "panel.sash.up",
            Self::ResizeDown => "panel.sash.down",
            Self::ResetSash => "panel.sash.reset",
        }
    }
    pub fn from_command_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.command_id() == id)
    }
}

/// Same boundary for GUI and programmatic callers. No mutable Session reference.
pub trait ActionSink {
    /// Optional live-host target resolution. Headless sinks cannot infer UI focus.
    fn resolve_contextual(&self, _action: ContextualPanelAction) -> PanelResult<LayoutAction> {
        Err(error(Rule::Availability, "live panel host keyboard context is unavailable"))
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply>;
    fn inspect(&self) -> LayoutReply;
    /// Pure projection on the authoritative service. Adapters stage bounded
    /// envelope/storage changes from this before guarded dispatch publishes.
    fn project(&self, _action: LayoutAction) -> PanelResult<Layout> {
        Err(error(Rule::Capability, "layout sink cannot stage commands"))
    }
    fn project_in_context(&self, action: LayoutAction, _context: &LayoutPreparationContext) -> PanelResult<Layout> {
        self.project(action)
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, _context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        self.dispatch(action)
    }
}

#[derive(Debug)]
pub struct LayoutService {
    identity: std::sync::Arc<()>,
    registry: PanelRegistry,
    layout: Layout,
    revision: u64,
    availability: AvailabilityMap,
    interaction: InteractionState,
    capabilities: HostCapabilities,
    identities: std::collections::BTreeMap<InstanceId, PanelKind>,
}

/// An optimistic, fully validated candidate. Fields stay private so it cannot be forged.
pub struct PreparedLayout {
    service_identity: std::sync::Arc<()>,
    base_revision: u64,
    next: Layout,
    interaction: InteractionState,
    effects: Vec<HostEffect>,
    changed: bool,
    availability: AvailabilityMap,
    identities: std::collections::BTreeMap<InstanceId, PanelKind>,
}

impl PreparedLayout {
    pub fn layout(&self) -> &Layout {
        &self.next
    }
    pub fn effects(&self) -> &[HostEffect] {
        &self.effects
    }
    /// Runtime availability is staged with the same atomic layout commit.
    pub fn set_availability(&mut self, availability: AvailabilityMap) -> PanelResult<()> {
        validate_availability(&availability)?;
        self.changed |= self.availability != availability;
        self.availability = availability;
        Ok(())
    }
}

impl LayoutService {
    pub fn new(registry: PanelRegistry, layout: Layout, capabilities: HostCapabilities) -> PanelResult<Self> {
        validate(&layout, &registry)?;
        if capabilities.max_native_hosts > MAX_SECTIONS {
            return Err(error(Rule::CollectionLimit, "native capability exceeds section cap"));
        }
        // Restored native entries remain data even on web; resolve_geometry marks them
        // unpresented. New native mutations still require a capable adapter.
        Ok(Self {
            identity: std::sync::Arc::new(()),
            registry,
            identities: layout.instances.iter().map(|p| (p.id.clone(), p.kind.clone())).collect(),
            layout,
            revision: 0,
            availability: AvailabilityMap::new(),
            interaction: InteractionState::default(),
            capabilities,
        })
    }
    pub fn registry(&self) -> &PanelRegistry {
        &self.registry
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn interaction(&self) -> &InteractionState {
        &self.interaction
    }
    pub fn capabilities(&self) -> &HostCapabilities {
        &self.capabilities
    }
    pub fn availability(&self, id: &InstanceId) -> Availability {
        self.availability.get(id).cloned().unwrap_or(Availability::Available)
    }
    pub fn set_availability(&mut self, values: AvailabilityMap) -> PanelResult<()> {
        validate_availability(&values)?;
        if self.availability != values {
            let revision = self.revision.checked_add(1).ok_or_else(|| error(Rule::Revision, "layout revision exhausted"))?;
            self.availability = values;
            self.revision = revision;
            self.reconcile_interaction();
        }
        Ok(())
    }
    fn reconcile_interaction(&mut self) {
        let layout = &self.layout;
        let availability = &self.availability;
        let visible = |id: &InstanceId| {
            layout.instance(id).is_ok_and(|p| p.requested_open) && availability.get(id).is_none_or(|state| *state == Availability::Available)
        };
        if self.interaction.focused.as_ref().is_some_and(|id| !visible(id)) {
            self.interaction.focused = None;
        }
        if self.interaction.peek.as_ref().is_some_and(|id| !visible(id)) {
            self.interaction.peek = None;
        }
    }
    pub fn prepare(&self, action: LayoutAction) -> PanelResult<PreparedLayout> {
        self.prepare_with_reservations(action, &BTreeSet::new(), &BTreeSet::new())
    }
    /// Borrowed dormant identities participate in generation and collision checks
    /// without entering the proposed layout or consuming IDs during preparation.
    pub fn prepare_with_reservations(
        &self,
        action: LayoutAction,
        sections: &BTreeSet<SectionId>,
        columns: &BTreeSet<ColumnId>,
    ) -> PanelResult<PreparedLayout> {
        self.prepare_internal(action, sections, columns, None, false, None)
    }
    /// Structural data for one proposed-owner capture, never an unchecked
    /// committable candidate. Presentation metadata still uses the current map.
    pub fn project_with_deferred_availability(
        &self,
        action: LayoutAction,
        sections: &BTreeSet<SectionId>,
        columns: &BTreeSet<ColumnId>,
    ) -> PanelResult<Layout> {
        Ok(self.prepare_internal(action, sections, columns, None, true, None)?.next)
    }
    /// Final pure admission against the captured proposed owner's availability.
    /// Replace retains this map rather than clearing it as the legacy path does.
    pub fn prepare_with_availability_and_reservations(
        &self,
        action: LayoutAction,
        availability: &AvailabilityMap,
        sections: &BTreeSet<SectionId>,
        columns: &BTreeSet<ColumnId>,
    ) -> PanelResult<PreparedLayout> {
        self.prepare_internal(action, sections, columns, Some(availability), false, None)
    }
    pub fn project_with_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        Ok(self.prepare_internal(action, &context.sections, &context.columns, None, true, Some(context))?.next)
    }
    pub fn prepare_with_context(
        &self,
        action: LayoutAction,
        availability: &AvailabilityMap,
        context: &LayoutPreparationContext,
    ) -> PanelResult<PreparedLayout> {
        self.prepare_internal(action, &context.sections, &context.columns, Some(availability), false, Some(context))
    }
    fn prepare_internal(
        &self,
        action: LayoutAction,
        sections: &BTreeSet<SectionId>,
        columns: &BTreeSet<ColumnId>,
        captured_availability: Option<&AvailabilityMap>,
        defer_availability: bool,
        full_context: Option<&LayoutPreparationContext>,
    ) -> PanelResult<PreparedLayout> {
        let reservations = Reservations { sections, columns };
        reservations.validate()?;
        if let Some(ownership) = full_context.and_then(|context| context.replacement_ownership.as_ref()) {
            ownership.validate()?;
        }
        if let LayoutAction::Apply { mutations, .. } = &action
            && (mutations.is_empty() || mutations.len() > MAX_MUTATIONS)
        {
            return Err(error(Rule::CollectionLimit, "transaction requires 1..64 mutations"));
        }
        if let Some(availability) = captured_availability {
            validate_availability(availability)?;
        }
        let action_availability = captured_availability.unwrap_or(&self.availability);
        let mut next = self.layout.clone();
        let content_only =
            matches!(&action, LayoutAction::Apply { mutations, .. } if matches!(mutations.as_slice(), [Mutation::HeaderAction { .. }]));
        let authors_priority = matches!(&action, LayoutAction::Apply { mutations, .. } if mutations.iter().any(|m|
            matches!(m, Mutation::SetSize { .. } | Mutation::ResizeAllocatedPair { prefer_allocated: true, .. })));
        if let Some(context) = full_context.filter(|_| !content_only) {
            context.extent_priorities.validate()?;
            if !context.extent_priorities.complete {
                return Err(error(Rule::Capability, "full recovery priority context required"));
            }
            if authors_priority
                && self.layout.extent_priority_context.records.iter().any(|old| {
                    super::persistence::typed_priority(&self.layout, &old.target).is_none()
                        && !context.extent_priorities.records.iter().any(|record| {
                            record.target == old.target
                                && record.zone == old.zone
                                && record.column == old.column
                                && record.stored_priority == old.stored_priority
                        })
                })
            {
                return Err(error(Rule::Capability, "preparation context omits retained priority records"));
            }
            if authors_priority
                && self.layout.extent_priority_context.records.iter().any(|old| {
                    super::persistence::typed_priority(&self.layout, &old.target).is_none()
                        && context.extent_priorities.records.iter().any(|record| {
                            record.target == old.target
                                && record.zone == old.zone
                                && record.column == old.column
                                && record.stored_priority == old.stored_priority
                                && record.priority != old.priority
                        })
                })
            {
                return Err(error(Rule::Revision, "recovery priority context became stale"));
            }
            next.extent_priority_context = context.extent_priorities.clone();
        } else if authors_priority && (!sections.is_empty() || !columns.is_empty()) {
            return Err(error(Rule::Capability, "external reservations require full priority context for authored sizing"));
        }
        let mut interaction = self.interaction.clone();
        let mut effects = Vec::new();
        let mut availability = action_availability.clone();
        let mut destroyed = std::collections::BTreeSet::new();
        let synchronize_priorities = matches!(&action, LayoutAction::Apply { mutations, .. }
            if !matches!(mutations.as_slice(), [Mutation::HeaderAction { .. }]))
            || matches!(&action, LayoutAction::Replace { .. });
        if let LayoutAction::Replace { expected_revision, layout } = &action {
            if expected_revision.is_some_and(|r| r != self.revision) {
                return Err(error(Rule::Revision, "stale layout revision"));
            }
            validate(layout, &self.registry)?;
            reservations.validate_replacement(&self.layout, layout, full_context.and_then(|context| context.replacement_ownership.as_ref()))?;
            next = layout.clone();
            if !next.extent_priority_context.complete
                && let Some(context) = full_context
            {
                if next.workspace != self.layout.workspace && !context.extent_priorities.records.is_empty() {
                    return Err(error(Rule::Capability, "workspace replacement requires incoming recovery context"));
                }
                next.extent_priority_context = context.extent_priorities.clone();
            }
            if captured_availability.is_none() {
                availability.clear();
            }
            interaction = InteractionState::default();
        }
        if let LayoutAction::Apply { expected_revision, mutations } = action {
            if expected_revision.is_some_and(|r| r != self.revision) {
                return Err(error(Rule::Revision, "stale layout revision"));
            }
            if mutations.is_empty() || mutations.len() > MAX_MUTATIONS {
                return Err(error(Rule::CollectionLimit, "transaction requires 1..64 mutations"));
            }
            validate_lifecycle_intents(&mutations)?;
            validate_header_action_transaction(&mutations)?;
            for mutation in &mutations {
                if let Mutation::Destroy { instance } = mutation {
                    destroyed.insert(instance.clone());
                }
            }
            let context = MutationContext {
                registry: &self.registry,
                availability: action_availability,
                caps: &self.capabilities,
                reservations: &reservations,
                defer_availability,
            };
            for mutation in mutations {
                reservations.validate_mutation(&next, &mutation)?;
                apply(&mut next, &context, &mut interaction, &mut effects, mutation)?;
                if !content_only {
                    super::persistence::synchronize_priority_context(&mut next)?;
                }
                // A bounded intermediate state cannot smuggle oversized collections into
                // a later mutation. Semantic invariants are checked at atomic commit.
                if next.instances.len() > MAX_INSTANCES
                    || next.sections.len() > MAX_SECTIONS
                    || next.columns.len() > MAX_SECTIONS
                    || next.sections.iter().any(|s| s.tabs.len() > MAX_TABS)
                {
                    return Err(error(Rule::CollectionLimit, "transaction collection limit"));
                }
            }
            validate(&next, &self.registry)?;
            for before in &self.layout.instances {
                if next.instances.iter().any(|after| after.id == before.id && after.kind != before.kind) {
                    return Err(error(Rule::DuplicateId, "transaction cannot reuse an instance for another kind"));
                }
            }
        }
        if synchronize_priorities {
            super::persistence::synchronize_priority_context(&mut next)?;
        }
        let mut identities = self.identities.clone();
        for id in destroyed {
            identities.remove(&id);
        }
        for p in &next.instances {
            if identities.get(&p.id).is_some_and(|kind| *kind != p.kind) {
                return Err(error(Rule::DuplicateId, "instance identity cannot change kind across layouts/workspaces"));
            }
            identities.insert(p.id.clone(), p.kind.clone());
        }
        if identities.len() > MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "retained instance identity cap"));
        }
        // Native effects describe the committed difference, never intermediate windows.
        let old = native_hosts(&self.layout);
        let new = native_hosts(&next);
        for (host, section) in &new {
            if !old.iter().any(|(h, _)| h == host) {
                if !self.capabilities.native_windows || new.len() > self.capabilities.max_native_hosts {
                    return Err(error(Rule::Capability, "native hosts unavailable or at capacity"));
                }
                effects.push(HostEffect::CreateNative { host: host.clone(), section: section.clone() });
            } else if self.layout.workspace != next.workspace || old.iter().any(|(h, s)| h == host && s != section) {
                if !self.capabilities.native_windows {
                    return Err(error(Rule::Capability, "cannot rebind unsupported native host"));
                }
                effects.push(HostEffect::RebindNative { host: host.clone(), section: section.clone() });
            }
        }
        for (host, _) in old {
            if !new.iter().any(|(h, _)| *h == host) {
                effects.push(HostEffect::ReleaseNative { host });
            }
        }
        if interaction.focused.as_ref().is_some_and(|id| !next.containing(id).is_ok_and(|s| !s.collapsed && s.active == *id)) {
            interaction.focused = None;
        }
        if interaction
            .peek
            .as_ref()
            .is_some_and(|id| !next.instance(id).is_ok_and(|p| p.requested_open) || !next.containing(id).is_ok_and(|s| s.collapsed))
        {
            interaction.peek = None;
        }
        effects.retain(|effect| match effect {
            HostEffect::Focus { instance } => interaction.focused.as_ref() == Some(instance),
            HostEffect::Peek { instance } => interaction.peek.as_ref() == Some(instance),
            _ => true,
        });
        let changed = next != self.layout || interaction != self.interaction || availability != self.availability || !effects.is_empty();
        if changed && self.revision == u64::MAX {
            return Err(error(Rule::Revision, "layout revision exhausted"));
        }
        Ok(PreparedLayout {
            service_identity: self.identity.clone(),
            base_revision: self.revision,
            next,
            interaction,
            effects,
            changed,
            availability,
            identities,
        })
    }
    /// Geometry and environment remain caller-owned. Failed admission cannot
    /// publish a candidate or consume its proposed identities.
    pub fn prepare_admitted(&self, action: LayoutAction, admission: impl FnOnce(&Layout, &Layout) -> PanelResult<()>) -> PanelResult<PreparedLayout> {
        let prepared = self.prepare(action)?;
        admission(&self.layout, prepared.layout())?;
        Ok(prepared)
    }
    pub fn commit(&mut self, prepared: PreparedLayout) -> PanelResult<LayoutReply> {
        if !std::sync::Arc::ptr_eq(&prepared.service_identity, &self.identity) || prepared.base_revision != self.revision {
            return Err(error(Rule::Revision, "candidate became stale"));
        }
        if prepared.changed {
            self.revision = self.revision.checked_add(1).ok_or_else(|| error(Rule::Revision, "layout revision exhausted"))?;
        }
        self.layout = prepared.next;
        self.interaction = prepared.interaction;
        self.availability = prepared.availability;
        self.identities = prepared.identities;
        self.reconcile_interaction();
        let mut reply = self.inspect();
        reply.effects = prepared.effects;
        Ok(reply)
    }
}

fn validate_availability(values: &AvailabilityMap) -> PanelResult<()> {
    if values.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "availability map full"));
    }
    for (id, state) in values {
        valid_id(&id.0)?;
        if let Availability::Unavailable { reason } = state
            && (reason.is_empty() || reason.len() > 256)
        {
            return Err(error(Rule::Availability, "availability reason must contain 1..256 bytes"));
        }
    }
    Ok(())
}

/// Close/Destroy are explicit, irreversible content intents. Do not infer them
/// solely from net layout differences, or permit later mutations to erase them.
pub fn validate_lifecycle_intents(mutations: &[Mutation]) -> PanelResult<()> {
    if mutations.len() > MAX_MUTATIONS {
        return Err(error(Rule::CollectionLimit, "lifecycle intent batch cap"));
    }
    let mut counts = std::collections::BTreeMap::<&InstanceId, (usize, bool)>::new();
    for mutation in mutations {
        let (id, terminal) = match mutation {
            Mutation::Close { instance } | Mutation::Destroy { instance } => (instance, true),
            Mutation::Open { instance, .. }
            | Mutation::Move { instance, .. }
            | Mutation::Activate { instance }
            | Mutation::Focus { instance }
            | Mutation::HeaderAction { instance, .. }
            | Mutation::Peek { instance } => (instance, false),
            _ => continue,
        };
        let entry = counts.entry(id).or_default();
        entry.0 += 1;
        entry.1 |= terminal;
        if entry.1 && entry.0 > 1 {
            return Err(error(Rule::Lifecycle, "Close/Destroy cannot share a batch with another operation on that instance"));
        }
    }
    Ok(())
}

impl ActionSink for LayoutService {
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        Ok(self.prepare_internal(action, &context.sections, &context.columns, None, false, Some(context))?.next)
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        let prepared = self.prepare_internal(action, &context.sections, &context.columns, None, false, Some(context))?;
        self.commit(prepared)
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        Ok(self.prepare(action)?.layout().clone())
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        let prepared = self.prepare(action)?;
        self.commit(prepared)
    }
    fn inspect(&self) -> LayoutReply {
        LayoutReply {
            api_version: PANEL_API_VERSION,
            revision: self.revision,
            layout: self.layout.clone(),
            interaction: self.interaction.clone(),
            effects: Vec::new(),
            capabilities: self.capabilities.clone(),
            availability: self.availability.clone(),
        }
    }
}

fn native_hosts(layout: &Layout) -> Vec<(HostId, SectionId)> {
    layout
        .sections
        .iter()
        .filter_map(|s| match &s.location {
            SectionLocation::Window { host, .. } => Some((host.clone(), s.id.clone())),
            _ => None,
        })
        .collect()
}

fn available(map: &AvailabilityMap, id: &InstanceId) -> PanelResult<()> {
    if let Some(state) = map.get(id) {
        state.require()?;
    }
    Ok(())
}

fn section_mut<'a>(layout: &'a mut Layout, id: &SectionId) -> PanelResult<&'a mut Section> {
    layout.sections.iter_mut().find(|s| s.id == *id).ok_or_else(|| error(Rule::MissingTarget, "section not found"))
}

fn remember(layout: &mut Layout, id: &InstanceId) -> PanelResult<()> {
    let section = layout.containing(id)?.clone();
    let section_index = layout.sections.iter().position(|s| s.id == section.id).ok_or_else(|| error(Rule::MissingTarget, "section not found"))?;
    let tab_index = section.tabs.iter().position(|p| p == id).ok_or_else(|| error(Rule::MissingTarget, "tab not found"))?;
    let column = section.column.as_ref().map(|id| layout.column(id)).transpose()?;
    let column_index =
        column.map_or(0, |column| layout.columns.iter().filter(|c| c.zone == column.zone).position(|c| c.id == column.id).unwrap_or(0));
    let column_width = column.map(|column| column.width);
    let p = layout.instances.iter_mut().find(|p| p.id == *id).ok_or_else(|| error(Rule::MissingTarget, "instance not found"))?;
    p.memory = PlacementMemory {
        section: section.id,
        location: section.location,
        section_index,
        tab_index,
        size: section.size,
        collapsed: section.collapsed,
        column: section.column,
        column_index,
        column_width,
        weight: section.weight,
        preferred_extent: section.preferred_extent,
        extent_priority: section.extent_priority,
    };
    Ok(())
}

fn detach(layout: &mut Layout, id: &InstanceId) {
    for s in &mut layout.sections {
        s.tabs.retain(|p| p != id);
        if s.active == *id
            && let Some(first) = s.tabs.first()
        {
            s.active = first.clone();
        }
    }
    layout.sections.retain(|s| !s.tabs.is_empty());
}

fn restore_column(layout: &mut Layout, id: ColumnId, zone: Zone, rank: usize, width: f32) -> PanelResult<ColumnId> {
    valid_id(&id.0)?;
    if let Ok(column) = layout.column(&id) {
        if column.zone != zone {
            return Err(error(Rule::Placement, "remembered column moved to a different side"));
        }
        return Ok(id);
    }
    if layout.columns.len() >= MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "column cap"));
    }
    let index = layout.columns.iter().enumerate().filter(|(_, c)| c.zone == zone).nth(rank).map(|(i, _)| i).unwrap_or(layout.columns.len());
    layout.columns.insert(index, Column { id: id.clone(), zone, width });
    Ok(id)
}

fn fresh_section(layout: &Layout, id: &InstanceId, section: &SectionId) -> PanelResult<()> {
    valid_id(&section.0)?;
    if layout.sections.iter().any(|s| s.id == *section) {
        return Err(error(Rule::DuplicateId, "new-section identity already exists"));
    }
    let own = layout.instance(id).ok().map(|p| &p.memory.section);
    let native_own = layout.instance(id).ok().is_some_and(|p| {
        matches!(&p.memory.location,
        SectionLocation::Window { return_to, .. } if return_to.section.as_ref() == Some(section))
    });
    if own != Some(section) && !native_own && layout.instances.iter().any(|p| p.memory.section == *section) {
        return Err(error(Rule::DuplicateId, "new-section identity reserved by placement memory"));
    }
    for location in layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location
            && (return_to.section.as_ref() == Some(section) || return_to.join.as_ref().is_some_and(|join| join.section == *section))
            && own != Some(section)
            && !native_own
        {
            return Err(error(Rule::DuplicateId, "new-section identity reserved by native return"));
        }
    }
    Ok(())
}

fn column_insert_index(layout: &Layout, column: &ColumnId, requested: usize) -> PanelResult<usize> {
    let target = layout.column(column)?;
    let rank = layout.columns.iter().position(|c| c.id == *column).ok_or_else(|| error(Rule::MissingTarget, "column absent"))?;
    let members: Vec<_> = layout.sections.iter().enumerate().filter(|(_, s)| s.column.as_ref() == Some(column)).map(|(i, _)| i).collect();
    if let (Some(first), Some(last)) = (members.first(), members.last()) {
        return Ok(requested.clamp(*first, last.saturating_add(1)));
    }
    for (index, section) in layout.sections.iter().enumerate().filter(|(_, s)| s.location.zone() == target.zone) {
        if section.column.as_ref().and_then(|id| layout.columns.iter().position(|c| c.id == *id)).is_some_and(|other| other > rank) {
            return Ok(index);
        }
    }
    Ok(layout.sections.iter().rposition(|s| s.location.zone() == target.zone).map_or(requested.min(layout.sections.len()), |index| index + 1))
}

struct Reservations<'a> {
    sections: &'a BTreeSet<SectionId>,
    columns: &'a BTreeSet<ColumnId>,
}

impl Reservations<'_> {
    fn validate(&self) -> PanelResult<()> {
        if self.sections.len() > MAX_RESERVED_IDENTITIES || self.columns.len() > MAX_RESERVED_IDENTITIES {
            return Err(error(Rule::CollectionLimit, "external identity reservation cap"));
        }
        for id in self.sections {
            valid_id(&id.0)?;
        }
        for id in self.columns {
            valid_id(&id.0)?;
        }
        Ok(())
    }
    fn owns_section(layout: &Layout, instance: &InstanceId, section: &SectionId) -> bool {
        let Ok(panel) = layout.instance(instance) else {
            return false;
        };
        panel.memory.section == *section
            || layout.containing(instance).is_ok_and(|s| s.id == *section)
            || [Some(&panel.memory.location), layout.containing(instance).ok().map(|s| &s.location)].into_iter().flatten().any(|location| {
                matches!(location, SectionLocation::Window { return_to, .. }
                    if return_to.section.as_ref() == Some(section) || return_to.join.as_ref().is_some_and(|join| join.section == *section))
            })
    }
    fn owns_column(layout: &Layout, instance: &InstanceId, column: &ColumnId) -> bool {
        let Ok(panel) = layout.instance(instance) else {
            return false;
        };
        panel.memory.column.as_ref() == Some(column)
            || layout.containing(instance).is_ok_and(|s| s.column.as_ref() == Some(column))
            || [Some(&panel.memory.location), layout.containing(instance).ok().map(|s| &s.location)]
                .into_iter()
                .flatten()
                .any(|location| matches!(location, SectionLocation::Window { return_to, .. } if return_to.column.as_ref() == Some(column)))
    }
    fn section(&self, layout: &Layout, instance: &InstanceId, id: &SectionId) -> PanelResult<()> {
        if self.sections.contains(id) && !Self::owns_section(layout, instance, id) {
            return Err(error(Rule::DuplicateId, "section identity externally reserved"));
        }
        Ok(())
    }
    fn column(&self, layout: &Layout, instance: &InstanceId, id: &ColumnId) -> PanelResult<()> {
        if self.columns.contains(id) && !Self::owns_column(layout, instance, id) {
            return Err(error(Rule::DuplicateId, "column identity externally reserved"));
        }
        Ok(())
    }
    fn destination(&self, layout: &Layout, instance: &InstanceId, destination: &Destination) -> PanelResult<()> {
        match destination {
            Destination::NewSection { section, location, .. } => {
                self.section(layout, instance, section)?;
                if let SectionLocation::Window { return_to, .. } = location {
                    if let Some(id) = &return_to.section {
                        self.section(layout, instance, id)?;
                    }
                    if let Some(join) = &return_to.join
                        && layout.section(&join.section).is_err()
                    {
                        self.section(layout, instance, &join.section)?;
                    }
                    if let Some(id) = &return_to.column {
                        self.column(layout, instance, id)?;
                    }
                }
            }
            Destination::Relative { new_section, new_column, .. } => {
                self.section(layout, instance, new_section)?;
                if let Some(id) = new_column {
                    self.column(layout, instance, id)?;
                }
            }
            Destination::JoinTab { .. } => {}
        }
        Ok(())
    }
    fn validate_mutation(&self, layout: &Layout, mutation: &Mutation) -> PanelResult<()> {
        match mutation {
            Mutation::Open { instance, destination: Some(destination), .. } | Mutation::Move { instance, destination } => {
                self.destination(layout, instance, destination)
            }
            _ => Ok(()),
        }
    }
    fn validate_replacement(&self, before: &Layout, after: &Layout, captured: Option<&ReplacementIdentityOwnership>) -> PanelResult<()> {
        let after_ownership = ReplacementIdentityOwnership::from_layout(after)?;
        let captured_section = |id: &SectionId| {
            captured.is_some_and(|captured| captured.sections.get(id).is_some_and(|owners| after_ownership.sections.get(id) == Some(owners)))
        };
        let captured_column = |id: &ColumnId| {
            captured.is_some_and(|captured| captured.columns.get(id).is_some_and(|owners| after_ownership.columns.get(id) == Some(owners)))
        };
        for section in &after.sections {
            if before.section(&section.id).is_err()
                && self.sections.contains(&section.id)
                && !section.tabs.iter().any(|id| Self::owns_section(before, id, &section.id))
                && !captured_section(&section.id)
            {
                return Err(error(Rule::DuplicateId, "replacement steals reserved section"));
            }
            if let Some(column) = &section.column
                && before.column(column).is_err()
                && self.columns.contains(column)
                && !section.tabs.iter().any(|id| Self::owns_column(before, id, column))
                && !captured_column(column)
            {
                return Err(error(Rule::DuplicateId, "replacement steals reserved column"));
            }
            for instance in &section.tabs {
                if let SectionLocation::Window { return_to, .. } = &section.location {
                    if let Some(id) = &return_to.section
                        && !captured_section(id)
                    {
                        self.section(before, instance, id)?;
                    }
                    if let Some(join) = &return_to.join
                        && before.section(&join.section).is_err()
                        && !captured_section(&join.section)
                    {
                        self.section(before, instance, &join.section)?;
                    }
                    if let Some(id) = &return_to.column
                        && !captured_column(id)
                    {
                        self.column(before, instance, id)?;
                    }
                }
            }
        }
        for column in &after.columns {
            if before.column(&column.id).is_err()
                && self.columns.contains(&column.id)
                && !after.instances.iter().any(|p| Self::owns_column(before, &p.id, &column.id))
                && !captured_column(&column.id)
            {
                return Err(error(Rule::DuplicateId, "replacement steals reserved dormant column"));
            }
        }
        for panel in &after.instances {
            let memory = &panel.memory;
            if before.section(&memory.section).is_err() && !captured_section(&memory.section) {
                self.section(before, &panel.id, &memory.section)?;
            }
            if let Some(column) = &memory.column
                && before.column(column).is_err()
                && !captured_column(column)
            {
                self.column(before, &panel.id, column)?;
            }
            if let SectionLocation::Window { return_to, .. } = &memory.location {
                if let Some(id) = &return_to.section
                    && before.section(id).is_err()
                    && !captured_section(id)
                {
                    self.section(before, &panel.id, id)?;
                }
                if let Some(join) = &return_to.join
                    && before.section(&join.section).is_err()
                    && !captured_section(&join.section)
                {
                    self.section(before, &panel.id, &join.section)?;
                }
                if let Some(id) = &return_to.column
                    && before.column(id).is_err()
                    && !captured_column(id)
                {
                    self.column(before, &panel.id, id)?;
                }
            }
        }
        Ok(())
    }
}

fn place(
    layout: &mut Layout,
    id: &InstanceId,
    destination: Destination,
    reservations: &Reservations<'_>,
    new_column_width: Option<f32>,
) -> PanelResult<()> {
    match destination {
        Destination::NewSection { section, index, mut location, mut size } => {
            fresh_section(layout, id, &section)?;
            if let SectionLocation::Window { return_to, .. } = &mut location {
                if return_to.zone.side() && return_to.column.is_none() {
                    let existing = layout.sections.iter().filter(|s| s.location.zone() == return_to.zone).find_map(|s| s.column.clone());
                    let column = match existing {
                        Some(column) => column,
                        None => {
                            let column = next_column_id_with_reservations(layout, reservations.columns)?;
                            restore_column(
                                layout,
                                column,
                                return_to.zone,
                                return_to.column_index,
                                return_to.column_width.unwrap_or(return_to.size.width),
                            )?
                        }
                    };
                    return_to.column_width = Some(layout.column(&column)?.width);
                    return_to.column_index = layout.columns.iter().filter(|c| c.zone == return_to.zone).position(|c| c.id == column).unwrap_or(0);
                    return_to.column = Some(column);
                }
                if return_to.section.is_none() {
                    return_to.section = Some(section.clone());
                }
            }
            location.validate()?;
            size.validate()?;
            if index > layout.sections.len() {
                return Err(error(Rule::Bounds, "section insertion index out of bounds"));
            }
            let memory = layout
                .instance(id)
                .ok()
                .filter(|p| p.memory.section == section && p.memory.location.zone() == location.zone())
                .map(|p| p.memory.clone());
            let zone = location.zone();
            let mut created_column = false;
            let column = if zone.side() {
                if let Some(memory) = &memory
                    && let Some(column) = &memory.column
                {
                    created_column = layout.column(column).is_err();
                    Some(restore_column(
                        layout,
                        column.clone(),
                        zone,
                        memory.column_index,
                        new_column_width.unwrap_or_else(|| memory.column_width.unwrap_or(size.width)),
                    )?)
                } else {
                    let existing = layout
                        .sections
                        .iter()
                        .skip(index)
                        .find(|s| s.location.zone() == zone)
                        .and_then(|s| s.column.clone())
                        .or_else(|| layout.sections.iter().rev().find(|s| s.location.zone() == zone).and_then(|s| s.column.clone()));
                    Some(match existing {
                        Some(column) => column,
                        None => {
                            created_column = true;
                            let column = next_column_id_with_reservations(layout, reservations.columns)?;
                            restore_column(layout, column, zone, MAX_SECTIONS, new_column_width.unwrap_or(size.width))?
                        }
                    })
                }
            } else {
                None
            };
            if created_column
                && new_column_width.is_some()
                && let Some(column) = &column
            {
                size.width = layout.column(column)?.width;
            }
            let index = if let Some(column) = &column { column_insert_index(layout, column, index)? } else { index };
            layout.sections.insert(
                index,
                Section {
                    id: section,
                    location,
                    tabs: vec![id.clone()],
                    active: id.clone(),
                    size,
                    collapsed: memory.as_ref().is_some_and(|m| m.collapsed),
                    column,
                    preferred_extent: memory.as_ref().and_then(|m| m.preferred_extent),
                    extent_priority: memory.as_ref().map_or(0, |m| m.extent_priority),
                    weight: memory.map_or(if zone == Zone::Bottom { size.width } else { size.height }, |m| m.weight),
                },
            );
        }
        Destination::JoinTab { section, index } => {
            let s = section_mut(layout, &section)?;
            if index > s.tabs.len() {
                return Err(error(Rule::Bounds, "tab insertion index out of bounds"));
            }
            s.tabs.insert(index, id.clone());
            // Joining never steals active tab or resizes the group.
        }
        Destination::Relative { section, placement, new_section, new_column, mut size } => {
            fresh_section(layout, id, &new_section)?;
            size.validate()?;
            let target = layout.section(&section)?.clone();
            let zone = target.location.zone();
            let target_column = target.column.as_ref().ok_or_else(|| error(Rule::Placement, "relative placement requires side column"))?;
            let target_index =
                layout.sections.iter().position(|s| s.id == section).ok_or_else(|| error(Rule::MissingTarget, "relative target absent"))?;
            let (column, index, weight) = match placement {
                RelativePlacement::Above | RelativePlacement::Below => {
                    if new_column.is_some() {
                        return Err(error(Rule::Placement, "stack placement reuses target column"));
                    }
                    validate_weight(target.weight)?;
                    // Stack weights are relative, not logical heights. A zero
                    // anchor stays at minimum; the new section can use surplus.
                    let weight = if target.weight > 0.0 { target.weight } else { default_weight() };
                    (target_column.clone(), target_index + usize::from(placement == RelativePlacement::Below), weight)
                }
                RelativePlacement::BeforeColumn | RelativePlacement::AfterColumn => {
                    if let Some(width) = new_column_width {
                        size.width = width;
                    }
                    let column = new_column.ok_or_else(|| error(Rule::Placement, "column placement requires identity"))?;
                    valid_id(&column.0)?;
                    if layout.columns.iter().any(|c| c.id == column)
                        || layout.instances.iter().any(|p| p.memory.column.as_ref() == Some(&column))
                        || layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)).any(
                            |location| matches!(location, SectionLocation::Window { return_to, .. } if return_to.column.as_ref() == Some(&column)),
                        )
                    {
                        return Err(error(Rule::DuplicateId, "column identity already reserved"));
                    }
                    let after = placement == RelativePlacement::AfterColumn;
                    let rank = layout
                        .columns
                        .iter()
                        .position(|c| c.id == *target_column)
                        .ok_or_else(|| error(Rule::MissingTarget, "target column absent"))?;
                    let index = if after {
                        layout.sections.iter().rposition(|s| s.column.as_ref() == Some(target_column)).map(|i| i + 1)
                    } else {
                        layout.sections.iter().position(|s| s.column.as_ref() == Some(target_column))
                    }
                    .ok_or_else(|| error(Rule::MissingTarget, "target column empty"))?;
                    layout.columns.insert(rank + usize::from(after), Column { id: column.clone(), zone, width: size.width });
                    (column, index, size.height)
                }
            };
            layout.sections.insert(
                index,
                Section {
                    id: new_section,
                    location: SectionLocation::Dock { zone },
                    tabs: vec![id.clone()],
                    active: id.clone(),
                    size,
                    collapsed: false,
                    column: Some(column),
                    weight,
                    preferred_extent: None,
                    extent_priority: 0,
                },
            );
        }
    }
    Ok(())
}

fn reopen_destination(layout: &Layout, p: &PanelInstance) -> Destination {
    if let Ok(s) = layout.section(&p.memory.section) {
        Destination::JoinTab { section: s.id.clone(), index: p.memory.tab_index.min(s.tabs.len()) }
    } else {
        Destination::NewSection {
            section: p.memory.section.clone(),
            index: p.memory.section_index.min(layout.sections.len()),
            location: p.memory.location.clone(),
            size: p.memory.size,
        }
    }
}

fn inherited_extent_preference(source: Zone, target: Zone, preferred_extent: Option<f32>, extent_priority: u16) -> (Option<f32>, u16) {
    // Free placements retain latent intent; only concrete dock axes establish incompatibility.
    if source.docked() && target.docked() && source.side() != target.side() { (None, 0) } else { (preferred_extent, extent_priority) }
}

fn inherit_native_preference(destination: &mut Destination, source: Zone, preferred_extent: Option<f32>, extent_priority: u16) -> PanelResult<()> {
    if let Destination::NewSection { location: SectionLocation::Window { return_to, .. }, .. } = destination {
        validate_extent_priority(return_to.preferred_extent, return_to.extent_priority)?;
        if return_to.preferred_extent.is_none() {
            (return_to.preferred_extent, return_to.extent_priority) =
                inherited_extent_preference(source, return_to.zone, preferred_extent, extent_priority);
        }
    }
    Ok(())
}

struct MutationContext<'a, 'r> {
    registry: &'a PanelRegistry,
    availability: &'a AvailabilityMap,
    caps: &'a HostCapabilities,
    reservations: &'a Reservations<'r>,
    defer_availability: bool,
}

impl MutationContext<'_, '_> {
    fn require_available(&self, instance: &InstanceId) -> PanelResult<()> {
        if self.defer_availability { Ok(()) } else { available(self.availability, instance) }
    }
}

fn apply(
    layout: &mut Layout,
    context: &MutationContext<'_, '_>,
    interaction: &mut InteractionState,
    effects: &mut Vec<HostEffect>,
    mutation: Mutation,
) -> PanelResult<()> {
    let MutationContext { registry, availability, caps, reservations, defer_availability } = *context;
    let changes_placement =
        matches!(&mutation, Mutation::Open { .. } | Mutation::Move { .. } | Mutation::Focus { .. } | Mutation::NativeClose { .. });
    match mutation {
        Mutation::Open { kind, instance, destination } => {
            valid_id(&instance.0)?;
            registry.get(&kind)?;
            context.require_available(&instance)?;
            if let Ok(existing) = layout.instance(&instance) {
                if existing.kind != kind {
                    return Err(error(Rule::DuplicateId, "instance belongs to another kind"));
                }
                if existing.requested_open {
                    if destination.is_some() {
                        return Err(error(Rule::Placement, "use Move for an open instance"));
                    }
                    return Ok(());
                }
                let p = existing.clone();
                let restore_collapsed = destination.is_none() && layout.section(&p.memory.section).is_err();
                let mut destination = destination.unwrap_or_else(|| reopen_destination(layout, &p));
                inherit_native_preference(&mut destination, p.memory.location.zone(), p.memory.preferred_extent, p.memory.extent_priority)?;
                let created = !matches!(&destination, Destination::JoinTab { .. });
                place(layout, &instance, destination, reservations, None)?;
                if created {
                    let section = layout.containing(&instance)?.id.clone();
                    let section = section_mut(layout, &section)?;
                    (section.preferred_extent, section.extent_priority) = inherited_extent_preference(
                        p.memory.location.zone(),
                        section.location.zone(),
                        p.memory.preferred_extent,
                        p.memory.extent_priority,
                    );
                }
                if let Some(entry) = layout.instances.iter_mut().find(|p| p.id == instance) {
                    entry.requested_open = true;
                }
                if restore_collapsed && let Some(s) = layout.sections.iter_mut().find(|s| s.id == p.memory.section) {
                    s.collapsed = p.memory.collapsed;
                }
            } else {
                let destination = destination.ok_or_else(|| error(Rule::Placement, "new instance requires explicit destination"))?;
                place(layout, &instance, destination, reservations, None)?;
                let s = layout.containing(&instance)?.clone();
                layout.instances.push(PanelInstance {
                    id: instance.clone(),
                    kind,
                    requested_open: true,
                    memory: PlacementMemory {
                        section: s.id,
                        location: s.location,
                        section_index: 0,
                        tab_index: 0,
                        size: s.size,
                        collapsed: s.collapsed,
                        column: s.column,
                        column_index: 0,
                        column_width: None,
                        weight: s.weight,
                        preferred_extent: s.preferred_extent,
                        extent_priority: s.extent_priority,
                    },
                });
            }
        }
        Mutation::Close { instance } => {
            let p = layout.instance(&instance)?;
            if !registry.get(&p.kind)?.features.closable {
                return Err(error(Rule::FeatureDisabled, "panel is not closable"));
            }
            if p.requested_open {
                remember(layout, &instance)?;
                detach(layout, &instance);
                if let Some(p) = layout.instances.iter_mut().find(|p| p.id == instance) {
                    p.requested_open = false;
                }
            }
        }
        Mutation::Destroy { instance } => {
            if layout.instance(&instance)?.requested_open {
                return Err(error(Rule::Lifecycle, "close before owner destruction"));
            }
            layout.instances.retain(|p| p.id != instance);
        }
        Mutation::Move { instance, mut destination } => {
            context.require_available(&instance)?;
            if !layout.instance(&instance)?.requested_open {
                return Err(error(Rule::Placement, "cannot move a closed instance"));
            }
            let collapsed = layout.containing(&instance)?.collapsed;
            let source = layout.containing(&instance)?.clone();
            let column_width = match &source.column {
                Some(column) => layout.column(column)?.width,
                None => registry.get(&layout.instance(&instance)?.kind)?.default_size.width,
            };
            if let Destination::NewSection { location: SectionLocation::Window { return_to, .. }, .. } = &mut destination
                && source.location.zone() == return_to.zone
                && source.location.zone().docked()
            {
                let column = source.column.as_ref().map(|id| layout.column(id)).transpose()?;
                if return_to.column.is_none() {
                    return_to.column = source.column.clone();
                    return_to.column_width = column.map(|c| c.width);
                    return_to.column_index = column
                        .map_or(0, |column| layout.columns.iter().filter(|c| c.zone == column.zone).position(|c| c.id == column.id).unwrap_or(0));
                    return_to.weight = source.weight;
                }
                if return_to.section.is_none() {
                    return_to.section = Some(source.id.clone());
                }
                if return_to.join.is_none() && source.tabs.len() > 1 {
                    return_to.join =
                        Some(ReturnTab { section: source.id.clone(), index: source.tabs.iter().position(|p| *p == instance).unwrap_or(0) });
                }
            }
            inherit_native_preference(&mut destination, source.location.zone(), source.preferred_extent, source.extent_priority)?;
            if let Destination::Relative { section, .. } = &destination
                && layout.section(section)?.tabs == [instance.clone()]
            {
                return Err(error(Rule::Placement, "relative destination cannot target its sole moving tab"));
            }
            remember(layout, &instance)?;
            detach(layout, &instance);
            let new_section = match &destination {
                Destination::NewSection { section, .. } => Some(section.clone()),
                Destination::Relative { new_section, .. } => Some(new_section.clone()),
                _ => None,
            };
            place(layout, &instance, destination, reservations, Some(column_width))?;
            if let Some(id) = new_section {
                let section = section_mut(layout, &id)?;
                section.collapsed = collapsed;
                (section.preferred_extent, section.extent_priority) =
                    inherited_extent_preference(source.location.zone(), section.location.zone(), source.preferred_extent, source.extent_priority);
            }
        }
        Mutation::Activate { instance } => {
            context.require_available(&instance)?;
            let section = layout.containing(&instance)?.id.clone();
            section_mut(layout, &section)?.active = instance;
        }
        Mutation::Focus { instance } => {
            context.require_available(&instance)?;
            let p = layout.instance(&instance)?.clone();
            if !p.requested_open {
                let mut destination = reopen_destination(layout, &p);
                inherit_native_preference(&mut destination, p.memory.location.zone(), p.memory.preferred_extent, p.memory.extent_priority)?;
                place(layout, &instance, destination, reservations, None)?;
                if let Some(entry) = layout.instances.iter_mut().find(|p| p.id == instance) {
                    entry.requested_open = true;
                }
            }
            let section = layout.containing(&instance)?.id.clone();
            let s = section_mut(layout, &section)?;
            s.active = instance.clone();
            s.collapsed = false;
            interaction.peek = None;
            interaction.focused = Some(instance.clone());
            effects.push(HostEffect::Focus { instance });
        }
        Mutation::Peek { instance } => {
            context.require_available(&instance)?;
            let s = layout.containing(&instance)?;
            if !s.collapsed {
                return Err(error(Rule::Placement, "peek requires a collapsed section"));
            }
            interaction.peek = Some(instance.clone());
            effects.push(HostEffect::Peek { instance });
        }
        Mutation::DismissPeek => {
            interaction.peek = None;
            effects.push(HostEffect::DismissPeek);
        }
        Mutation::Collapse { section, collapsed } => {
            section_mut(layout, &section)?.collapsed = collapsed;
        }
        Mutation::SetSize { section, size } => {
            size.validate()?;
            let source = layout.section(&section)?;
            let zone = source.location.zone();
            let column = source.column.clone();
            let priority = if zone.docked() { Some(next_extent_priority(layout, zone, &column)?) } else { None };
            if let Some(column) = layout.section(&section)?.column.clone() {
                set_column_width(layout, registry, &column, size.width)?;
            }
            let target = section_mut(layout, &section)?;
            target.size = size;
            if let Some(extent) = dock_stack_extent(target.location.zone(), size) {
                target.preferred_extent = Some(extent);
                target.extent_priority = priority.unwrap_or(0);
            }
        }
        Mutation::SetColumnWidth { column, width } => {
            set_column_width(layout, registry, &column, width)?;
        }
        Mutation::ResetSideHeight { section } => {
            let target = section_mut(layout, &section)?;
            if !target.location.zone().side() {
                return Err(error(Rule::Placement, "reset height requires a side section"));
            }
            target.preferred_extent = None;
            target.extent_priority = 0;
        }
        Mutation::SetBottomHeight { section, height } => {
            if layout.section(&section)?.location.zone() != Zone::Bottom {
                return Err(error(Rule::Placement, "row height requires a Bottom section"));
            }
            let (min, max) = bottom_height_bounds(layout, registry)?;
            if !height.is_finite() || height <= 0.0 || height > MAX_LOGICAL || height < min || height > max {
                return Err(error(Rule::Bounds, "Bottom height outside common row bounds"));
            }
            let members: BTreeSet<_> =
                layout.sections.iter().filter(|s| s.location.zone() == Zone::Bottom).flat_map(|s| s.tabs.iter().cloned()).collect();
            for target in layout.sections.iter_mut().filter(|s| s.location.zone() == Zone::Bottom) {
                target.size.height = height;
            }
            for panel in &mut layout.instances {
                if members.contains(&panel.id) {
                    panel.memory.size.height = height;
                }
            }
            // This orthogonal edit must not refresh divergent saved stack intent
            // or prune unrelated retained columns through structural maintenance.
            return Ok(());
        }
        Mutation::HeaderAction { instance, action } => {
            valid_id(&action.0)?;
            context.require_available(&instance)?;
            let panel = layout.instance(&instance)?;
            let spec = registry.get(&panel.kind)?;
            if !spec.header_actions.iter().any(|registered| registered.id == action) {
                return Err(error(Rule::Capability, "header action is not registered for panel"));
            }
            if !panel.requested_open {
                return Err(error(Rule::Availability, "header action requires an open presented tab"));
            }
            let section = layout.containing(&instance)?;
            if !defer_availability && presented_tab(section, availability) != Some(&instance) {
                return Err(error(Rule::Availability, "header action requires the presented available tab"));
            }
            effects.push(HostEffect::HeaderAction { instance, action });
            // Content-only dispatch deliberately bypasses catalogue staging.
            // Preserve even divergent imported memories and retained columns.
            return Ok(());
        }
        Mutation::SetFloatingRect { section, origin, size } => {
            valid_id(&section.0)?;
            LogicalRect { origin, size }.validate()?;
            let source = layout.section(&section)?;
            if !matches!(&source.location, SectionLocation::Floating { .. }) {
                return Err(error(Rule::Placement, "section rect requires an existing floating section"));
            }
            let (min, max) = tab_bounds(layout, registry, source)?;
            if !size.within(min, max) {
                return Err(error(Rule::Bounds, "floating rect outside intersected tab bounds"));
            }
            let members = source.tabs.clone();
            let target = section_mut(layout, &section)?;
            target.location = SectionLocation::Floating { origin };
            target.size = size;
            for instance in members {
                remember(layout, &instance)?;
            }
        }
        Mutation::ResizePair { first, second, first_size, second_size } => {
            first_size.validate()?;
            second_size.validate()?;
            let (a, b, zone) = pair(layout, &first, &second)?;
            let sum = |a: Size, b: Size| if zone == Zone::Bottom { a.width + b.width } else { a.height + b.height };
            if (sum(a.size, b.size) - sum(first_size, second_size)).abs() > 0.01 {
                return Err(error(Rule::Bounds, "paired sash must conserve shared extent"));
            }
            if zone == Zone::Bottom && (first_size.height != a.size.height || second_size.height != b.size.height)
                || zone != Zone::Bottom && (first_size.width != a.size.width || second_size.width != b.size.width)
            {
                return Err(error(Rule::Bounds, "paired sash changes only split axis"));
            }
            let total_weight = a.weight + b.weight;
            let extent = sum(first_size, second_size);
            let first_weight = total_weight * (if zone == Zone::Bottom { first_size.width } else { first_size.height }) / extent;
            section_mut(layout, &first)?.size = first_size;
            section_mut(layout, &first)?.weight = first_weight;
            section_mut(layout, &first)?.preferred_extent = None;
            section_mut(layout, &first)?.extent_priority = 0;
            section_mut(layout, &second)?.size = second_size;
            section_mut(layout, &second)?.weight = total_weight - first_weight;
            section_mut(layout, &second)?.preferred_extent = None;
            section_mut(layout, &second)?.extent_priority = 0;
        }
        Mutation::ResizeAllocatedPair {
            first,
            second,
            first_size,
            second_size,
            first_weight,
            second_weight,
            other_weight_scale,
            prefer_allocated,
        } => {
            first_size.validate()?;
            second_size.validate()?;
            for weight in [first_weight, second_weight] {
                if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
                    return Err(error(Rule::Bounds, "allocated pair shares must be finite fractions"));
                }
            }
            if !other_weight_scale.is_finite() || other_weight_scale <= 0.0 || other_weight_scale > MAX_LOGICAL {
                return Err(error(Rule::Bounds, "unpaired weight scale must be finite positive bounded"));
            }
            let (a, b, zone) = presented_pair(layout, &first, &second, availability)?;
            let (a_min, a_max) = tab_bounds(layout, registry, a)?;
            let (b_min, b_max) = tab_bounds(layout, registry, b)?;
            if !first_size.within(a_min, a_max) || !second_size.within(b_min, b_max) {
                return Err(error(Rule::Bounds, "allocated pair outside composite tab bounds"));
            }
            let cross = |size: Size| if zone == Zone::Bottom { size.height } else { size.width };
            if (cross(first_size) - cross(second_size)).abs() > 0.01 {
                return Err(error(Rule::Bounds, "allocated pair must retain a shared cross axis"));
            }
            if let Some(column) = &a.column {
                let (min, max) = column_bounds(layout, registry, column)?;
                if first_size.width < min || first_size.width > max {
                    return Err(error(Rule::Bounds, "allocated cross axis outside column bounds"));
                }
            }
            let column = a.column.clone();
            let total = saved_group_total(layout, zone, &column)?;
            let priority = if prefer_allocated { next_extent_priority(layout, zone, &column)? } else { 0 };
            let axis = |size: Size| if zone == Zone::Bottom { size.width } else { size.height };
            let authored_extents = prefer_allocated.then_some((axis(first_size), axis(second_size), priority));
            reweight_group(
                layout,
                zone,
                &column,
                total,
                PairReweight { first: &first, second: &second, first_weight, second_weight, other_weight_scale, authored_extents },
            )?;
            let target = section_mut(layout, &first)?;
            if zone == Zone::Bottom {
                target.size.width = first_size.width;
            } else {
                target.size.height = first_size.height;
            }
            target.weight = first_weight;
            target.preferred_extent = prefer_allocated.then_some(axis(first_size));
            target.extent_priority = priority;
            let target = section_mut(layout, &second)?;
            if zone == Zone::Bottom {
                target.size.width = second_size.width;
            } else {
                target.size.height = second_size.height;
            }
            target.weight = second_weight;
            target.preferred_extent = prefer_allocated.then_some(axis(second_size));
            target.extent_priority = priority;
        }
        Mutation::ResetPair { first, second } => {
            let (a, b, zone) = pair(layout, &first, &second)?;
            let mut a_size = a.size;
            let mut b_size = b.size;
            let (a_min, a_max) = tab_bounds(layout, registry, a)?;
            let (b_min, b_max) = tab_bounds(layout, registry, b)?;
            let axis = |s: Size| if zone == Zone::Bottom { s.width } else { s.height };
            let extent = axis(a_size) + axis(b_size);
            let lo = axis(a_min).max(extent - axis(b_max));
            let hi = axis(a_max).min(extent - axis(b_min));
            if lo > hi {
                return Err(error(Rule::Bounds, "pair cannot satisfy defaults within bounds"));
            }
            let a_default = registry.get(&layout.instance(&a.active)?.kind)?.default_size;
            let b_default = registry.get(&layout.instance(&b.active)?.kind)?.default_size;
            let preferred = extent * axis(a_default) / (axis(a_default) + axis(b_default));
            let split = preferred.clamp(lo, hi);
            let total_weight = a.weight + b.weight;
            if zone == Zone::Bottom {
                a_size.width = split;
                b_size.width = extent - split;
            } else {
                a_size.height = split;
                b_size.height = extent - split;
            }
            section_mut(layout, &first)?.size = a_size;
            section_mut(layout, &first)?.weight = total_weight * split / extent;
            section_mut(layout, &first)?.preferred_extent = None;
            section_mut(layout, &first)?.extent_priority = 0;
            section_mut(layout, &second)?.size = b_size;
            section_mut(layout, &second)?.weight = total_weight * (extent - split) / extent;
            section_mut(layout, &second)?.preferred_extent = None;
            section_mut(layout, &second)?.extent_priority = 0;
        }
        Mutation::NativeClose { host } => {
            valid_id(&host.0)?;
            let index = layout
                .sections
                .iter()
                .position(|s| matches!(&s.location, SectionLocation::Window { host: h, .. } if *h == host))
                .ok_or_else(|| error(Rule::MissingTarget, "native host not found"))?;
            let mut section = layout.sections.remove(index);
            let SectionLocation::Window { return_to, .. } = section.location else {
                return Err(error(Rule::Placement, "not a native section"));
            };
            if let Some(join) = &return_to.join
                && let Some(target) = layout.sections.iter_mut().find(|s| {
                    s.id == join.section
                        && s.location.zone() == return_to.zone
                        && return_to.column.as_ref().is_none_or(|column| s.column.as_ref() == Some(column))
                })
            {
                if target.tabs.len().saturating_add(section.tabs.len()) > MAX_TABS {
                    return Err(error(Rule::CollectionLimit, "native return tab capacity"));
                }
                let index = join.index.min(target.tabs.len());
                drop(target.tabs.splice(index..index, section.tabs));
                let members = target.tabs.clone();
                normalize_fixed_bottom_rows(layout, registry)?;
                for instance in members {
                    remember(layout, &instance)?;
                }
                prune_columns(layout);
                return Ok(());
            }
            section.location = SectionLocation::Dock { zone: return_to.zone };
            section.size = return_to.size;
            section.collapsed = return_to.collapsed;
            section.weight = return_to.weight;
            section.preferred_extent = return_to.preferred_extent;
            section.extent_priority = return_to.extent_priority;
            section.column = if return_to.zone.side() {
                let column = match return_to.column {
                    Some(column) => column,
                    None => next_column_id_with_reservations(layout, reservations.columns)?,
                };
                Some(restore_column(layout, column, return_to.zone, return_to.column_index, return_to.column_width.unwrap_or(return_to.size.width))?)
            } else {
                None
            };
            if let Some(id) = return_to.section {
                section.id = if layout.section(&id).is_ok() { next_section_id_with_reservations(layout, reservations.sections)? } else { id };
            }
            let index = match &section.column {
                Some(column) => column_insert_index(layout, column, return_to.index)?,
                None => return_to.index.min(layout.sections.len()),
            };
            layout.sections.insert(index, section);
        }
    }
    if changes_placement {
        normalize_fixed_bottom_rows(layout, registry)?;
    }
    // Every live member gets the same committed group geometry/proportions.
    // Closed memories are intentionally independent until their panel reopens.
    let members: Vec<_> = layout.sections.iter().flat_map(|s| s.tabs.iter().cloned()).collect();
    for instance in members {
        remember(layout, &instance)?;
    }
    prune_columns(layout);
    let windows = native_hosts(layout);
    // Retained unsupported windows can be closed/redocked without native capability.
    if caps.native_windows && windows.len() > caps.max_native_hosts {
        return Err(error(Rule::Capability, "native host limit"));
    }
    Ok(())
}

fn normalize_fixed_bottom_rows(layout: &mut Layout, registry: &PanelRegistry) -> PanelResult<()> {
    let mut fixed = Vec::new();
    for section in layout.sections.iter().filter(|section| section.location.zone() == Zone::Bottom) {
        let mut declared = false;
        for instance in &section.tabs {
            declared |= registry.get(&layout.instance(instance)?.kind)?.fixed_bottom_height.is_some();
        }
        if declared {
            let (min, _) = tab_bounds(layout, registry, section)?;
            fixed.push((section.id.clone(), min.height));
        }
    }
    for (id, height) in fixed {
        let section = section_mut(layout, &id)?;
        section.size.height = height;
        section.collapsed = false;
    }
    Ok(())
}

fn prune_columns(layout: &mut Layout) {
    let mut used = std::collections::BTreeSet::new();
    for column in layout.sections.iter().filter_map(|s| s.column.clone()).chain(layout.instances.iter().filter_map(|p| p.memory.column.clone())) {
        used.insert(column);
    }
    for location in layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location
            && let Some(column) = &return_to.column
        {
            used.insert(column.clone());
        }
    }
    layout.columns.retain(|column| used.contains(&column.id));
}

fn set_column_width(layout: &mut Layout, registry: &PanelRegistry, id: &ColumnId, width: f32) -> PanelResult<()> {
    valid_id(&id.0)?;
    let (min, max) = column_bounds(layout, registry, id)?;
    if !width.is_finite() || width <= 0.0 || width < min || width > max {
        return Err(error(Rule::Bounds, "column width outside composite stack bounds"));
    }
    let column = layout.columns.iter_mut().find(|c| c.id == *id).ok_or_else(|| error(Rule::MissingTarget, "column absent"))?;
    column.width = width;
    for section in layout.sections.iter_mut().filter(|s| s.column.as_ref() == Some(id)) {
        section.size.width = width;
    }
    Ok(())
}

/// Comparison tolerance for callers inspecting normalized saved weights.
pub const WEIGHT_SHARE_TOLERANCE: f32 = 0.00001;

fn same_group(zone: Zone, column: &Option<ColumnId>, location: &SectionLocation, other: &Option<ColumnId>) -> bool {
    zone.docked() && location.zone() == zone && (zone == Zone::Bottom || (column.is_some() && column == other))
}

/// Assign an authored tier only within the bounded private proposed layout.
/// Validate every matching record before compaction can write any rank.
fn next_extent_priority(layout: &mut Layout, zone: Zone, column: &Option<ColumnId>) -> PanelResult<u16> {
    if !zone.docked() || (zone.side() && column.is_none()) {
        return Err(error(Rule::Placement, "extent priority requires one dock group"));
    }
    if layout.sections.len() > MAX_SECTIONS || layout.instances.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "extent priority group cap"));
    }
    let mut ranks = BTreeSet::new();
    let mut collect = |extent: Option<f32>, priority: u16| -> PanelResult<()> {
        validate_extent_priority(extent, priority)?;
        if priority > 0 {
            ranks.insert(priority);
        }
        Ok(())
    };
    for section in &layout.sections {
        collect(section.preferred_extent, section.extent_priority)?;
    }
    for panel in &layout.instances {
        let memory = &panel.memory;
        collect(memory.preferred_extent, memory.extent_priority)?;
    }
    for location in layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location {
            collect(return_to.preferred_extent, return_to.extent_priority)?;
        }
    }
    for record in &layout.extent_priority_context.records {
        if record.priority > 0 {
            ranks.insert(record.priority);
        }
    }
    let highest = ranks.last().copied().unwrap_or(0);
    if let Some(next) = highest.checked_add(1) {
        return Ok(next);
    }
    let next = u16::try_from(ranks.len())
        .ok()
        .and_then(|rank| rank.checked_add(1))
        .ok_or_else(|| error(Rule::CollectionLimit, "extent priority tiers exhausted"))?;
    let mut compact = std::collections::BTreeMap::new();
    let mut ordinal = 0_u16;
    for rank in ranks {
        ordinal = ordinal.checked_add(1).ok_or_else(|| error(Rule::CollectionLimit, "extent priority compaction exhausted"))?;
        compact.insert(rank, ordinal);
    }
    for section in &mut layout.sections {
        section.extent_priority = compact.get(&section.extent_priority).copied().unwrap_or(section.extent_priority);
    }
    for panel in &mut layout.instances {
        let memory = &mut panel.memory;
        memory.extent_priority = compact.get(&memory.extent_priority).copied().unwrap_or(memory.extent_priority);
    }
    for location in layout.sections.iter_mut().map(|s| &mut s.location).chain(layout.instances.iter_mut().map(|p| &mut p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location {
            return_to.extent_priority = compact.get(&return_to.extent_priority).copied().unwrap_or(return_to.extent_priority);
        }
    }
    for record in &mut layout.extent_priority_context.records {
        record.priority = compact.get(&record.priority).copied().unwrap_or(record.priority);
    }
    Ok(next)
}

fn saved_group_total(layout: &Layout, zone: Zone, column: &Option<ColumnId>) -> PanelResult<f64> {
    if layout.sections.len() > MAX_SECTIONS || layout.instances.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "saved resize group cap"));
    }
    let mut members = std::collections::BTreeMap::new();
    for section in &layout.sections {
        if same_group(zone, column, &section.location, &section.column) {
            validate_weight(section.weight)?;
            members.insert(section.id.clone(), section.weight);
        }
    }
    for instance in &layout.instances {
        let memory = &instance.memory;
        if same_group(zone, column, &memory.location, &memory.column) {
            validate_weight(memory.weight)?;
            members.entry(memory.section.clone()).and_modify(|weight| *weight = (*weight).max(memory.weight)).or_insert(memory.weight);
        }
    }
    for (location, fallback) in
        layout.sections.iter().map(|s| (&s.location, &s.id)).chain(layout.instances.iter().map(|p| (&p.memory.location, &p.memory.section)))
    {
        if let SectionLocation::Window { return_to, .. } = location
            && return_to.zone == zone
            && (zone == Zone::Bottom || &return_to.column == column)
        {
            validate_weight(return_to.weight)?;
            let section = return_to.join.as_ref().map(|join| &join.section).or(return_to.section.as_ref()).unwrap_or(fallback);
            members.entry(section.clone()).and_modify(|weight| *weight = (*weight).max(return_to.weight)).or_insert(return_to.weight);
        }
    }
    let total: f64 = members.values().map(|weight| f64::from(*weight)).sum();
    if !total.is_finite() || total <= 0.0 {
        return Err(error(Rule::Bounds, "resize group must have positive total saved weight"));
    }
    Ok(total)
}

/// Pure shares for host inversion of actual minimum-first allocation. Closed,
/// collapsed and unavailable members retain the same normalization factor.
pub fn normalized_pair_weights(layout: &Layout, first: &SectionId, second: &SectionId) -> PanelResult<(f32, f32)> {
    valid_id(&first.0)?;
    valid_id(&second.0)?;
    if layout.sections.len() > MAX_SECTIONS || layout.instances.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "saved resize group cap"));
    }
    let (a, b, zone) = expanded_group_pair(layout, first, second)?;
    let total = saved_group_total(layout, zone, &a.column)?;
    Ok(((f64::from(a.weight) / total) as f32, (f64::from(b.weight) / total) as f32))
}

struct PairReweight<'a> {
    first: &'a SectionId,
    second: &'a SectionId,
    first_weight: f32,
    second_weight: f32,
    other_weight_scale: f32,
    authored_extents: Option<(f32, f32, u16)>,
}

fn reweight_group(layout: &mut Layout, zone: Zone, column: &Option<ColumnId>, total: f64, plan: PairReweight<'_>) -> PanelResult<()> {
    let PairReweight { first, second, first_weight, second_weight, other_weight_scale, authored_extents } = plan;
    let fraction = |id: &SectionId, weight: f32| {
        if id == first {
            first_weight
        } else if id == second {
            second_weight
        } else {
            (f64::from(weight) / total * f64::from(other_weight_scale)) as f32
        }
    };
    let preference = |id: &SectionId, extent: &mut Option<f32>, priority: &mut u16| {
        if id == first || id == second {
            *extent = authored_extents.map(|(a, b, _)| if id == first { a } else { b });
            *priority = authored_extents.map_or(0, |(_, _, priority)| priority);
        }
    };
    for section in &mut layout.sections {
        if same_group(zone, column, &section.location, &section.column) {
            section.weight = fraction(&section.id, section.weight);
            preference(&section.id, &mut section.preferred_extent, &mut section.extent_priority);
        }
    }
    for instance in &mut layout.instances {
        let memory = &mut instance.memory;
        if same_group(zone, column, &memory.location, &memory.column) {
            memory.weight = fraction(&memory.section, memory.weight);
            preference(&memory.section, &mut memory.preferred_extent, &mut memory.extent_priority);
        }
    }
    for (location, fallback) in layout
        .sections
        .iter_mut()
        .map(|s| (&mut s.location, &s.id))
        .chain(layout.instances.iter_mut().map(|p| (&mut p.memory.location, &p.memory.section)))
    {
        if let SectionLocation::Window { return_to, .. } = location
            && return_to.zone == zone
            && (zone == Zone::Bottom || &return_to.column == column)
        {
            let id = return_to.join.as_ref().map(|join| &join.section).or(return_to.section.as_ref()).unwrap_or(fallback);
            return_to.weight = fraction(id, return_to.weight);
            preference(id, &mut return_to.preferred_extent, &mut return_to.extent_priority);
        }
    }
    let new_total = saved_group_total(layout, zone, column)?;
    if new_total > f64::from(MAX_LOGICAL) {
        return Err(error(Rule::Bounds, "reweighted group total exceeds bound"));
    }
    Ok(())
}

fn expanded_group_pair<'a>(layout: &'a Layout, first: &SectionId, second: &SectionId) -> PanelResult<(&'a Section, &'a Section, Zone)> {
    if first == second {
        return Err(error(Rule::Placement, "sash requires distinct sections"));
    }
    let a = layout.section(first)?;
    let b = layout.section(second)?;
    let zone = a.location.zone();
    if !same_group(zone, &a.column, &b.location, &b.column) || a.collapsed || b.collapsed {
        return Err(error(Rule::Placement, "sash requires expanded sections in one dock group"));
    }
    Ok((a, b, zone))
}

fn presented_pair<'a>(
    layout: &'a Layout,
    first: &SectionId,
    second: &SectionId,
    availability: &AvailabilityMap,
) -> PanelResult<(&'a Section, &'a Section, Zone)> {
    let (a, b, zone) = expanded_group_pair(layout, first, second)?;
    let mut previous = None;
    for section in &layout.sections {
        if same_group(zone, &a.column, &section.location, &section.column) && presented_tab(section, availability).is_some() {
            if previous == Some(first) && section.id == *second {
                return Ok((a, b, zone));
            }
            previous = Some(&section.id);
        }
    }
    Err(error(Rule::Placement, "allocated sash requires ordered presented neighbours"))
}

fn pair<'a>(layout: &'a Layout, first: &SectionId, second: &SectionId) -> PanelResult<(&'a Section, &'a Section, Zone)> {
    if first == second {
        return Err(error(Rule::Placement, "sash requires two distinct neighbours"));
    }
    let a = layout.section(first)?;
    let b = layout.section(second)?;
    let zone = a.location.zone();
    let edge: Vec<_> = layout.sections.iter().filter(|s| s.location.zone() == zone && s.column == a.column).collect();
    if !zone.docked()
        || (zone.side() && a.column.is_none())
        || b.location.zone() != zone
        || a.column != b.column
        || a.collapsed
        || b.collapsed
        || !edge.windows(2).any(|p| p.first().is_some_and(|s| s.id == *first) && p.get(1).is_some_and(|s| s.id == *second))
    {
        return Err(error(Rule::Placement, "sash requires ordered neighbours in one dock"));
    }
    Ok((a, b, zone))
}
