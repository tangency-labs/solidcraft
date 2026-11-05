use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

/// Joining an existing tab group and creating a split are deliberately distinct.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Destination {
    NewSection { section: SectionId, index: usize, location: SectionLocation, size: Size },
    JoinTab { section: SectionId, index: usize },
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
    /// Both neighbours change together; their shared axis extent is conserved.
    ResizePair {
        first: SectionId,
        second: SectionId,
        first_size: Size,
        second_size: Size,
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

/// Same boundary for GUI and programmatic callers. No mutable Session reference.
pub trait ActionSink {
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply>;
    fn inspect(&self) -> LayoutReply;
    /// Pure projection on the authoritative service. Adapters stage bounded
    /// envelope/storage changes from this before guarded dispatch publishes.
    fn project(&self, _action: LayoutAction) -> PanelResult<Layout> {
        Err(error(Rule::Capability, "layout sink cannot stage commands"))
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
        let mut next = self.layout.clone();
        let mut interaction = self.interaction.clone();
        let mut effects = Vec::new();
        let mut availability = self.availability.clone();
        let mut destroyed = std::collections::BTreeSet::new();
        if let LayoutAction::Replace { expected_revision, layout } = &action {
            if expected_revision.is_some_and(|r| r != self.revision) {
                return Err(error(Rule::Revision, "stale layout revision"));
            }
            validate(layout, &self.registry)?;
            next = layout.clone();
            availability.clear();
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
            for mutation in &mutations {
                if let Mutation::Destroy { instance } = mutation {
                    destroyed.insert(instance.clone());
                }
            }
            for mutation in mutations {
                apply(&mut next, &self.registry, &self.availability, &self.capabilities, &mut interaction, &mut effects, mutation)?;
                // A bounded intermediate state cannot smuggle oversized collections into
                // a later mutation. Semantic invariants are checked at atomic commit.
                if next.instances.len() > MAX_INSTANCES || next.sections.len() > MAX_SECTIONS || next.sections.iter().any(|s| s.tabs.len() > MAX_TABS)
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
    let p = layout.instances.iter_mut().find(|p| p.id == *id).ok_or_else(|| error(Rule::MissingTarget, "instance not found"))?;
    p.memory = PlacementMemory {
        section: section.id,
        location: section.location,
        section_index,
        tab_index,
        size: section.size,
        collapsed: section.collapsed,
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

fn place(layout: &mut Layout, id: &InstanceId, destination: Destination) -> PanelResult<()> {
    match destination {
        Destination::NewSection { section, index, location, size } => {
            valid_id(&section.0)?;
            location.validate()?;
            size.validate()?;
            if layout.sections.iter().any(|s| s.id == section) {
                return Err(error(Rule::DuplicateId, "new-section identity already exists"));
            }
            if index > layout.sections.len() {
                return Err(error(Rule::Bounds, "section insertion index out of bounds"));
            }
            layout.sections.insert(index, Section { id: section, location, tabs: vec![id.clone()], active: id.clone(), size, collapsed: false });
        }
        Destination::JoinTab { section, index } => {
            let s = section_mut(layout, &section)?;
            if index > s.tabs.len() {
                return Err(error(Rule::Bounds, "tab insertion index out of bounds"));
            }
            s.tabs.insert(index, id.clone());
            // Joining never steals active tab or resizes the group.
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

fn apply(
    layout: &mut Layout,
    registry: &PanelRegistry,
    availability: &AvailabilityMap,
    caps: &HostCapabilities,
    interaction: &mut InteractionState,
    effects: &mut Vec<HostEffect>,
    mutation: Mutation,
) -> PanelResult<()> {
    match mutation {
        Mutation::Open { kind, instance, destination } => {
            valid_id(&instance.0)?;
            registry.get(&kind)?;
            available(availability, &instance)?;
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
                let destination = destination.unwrap_or_else(|| reopen_destination(layout, &p));
                place(layout, &instance, destination)?;
                if let Some(entry) = layout.instances.iter_mut().find(|p| p.id == instance) {
                    entry.requested_open = true;
                }
                if restore_collapsed && let Some(s) = layout.sections.iter_mut().find(|s| s.id == p.memory.section) {
                    s.collapsed = p.memory.collapsed;
                }
            } else {
                let destination = destination.ok_or_else(|| error(Rule::Placement, "new instance requires explicit destination"))?;
                place(layout, &instance, destination)?;
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
        Mutation::Move { instance, destination } => {
            if !layout.instance(&instance)?.requested_open {
                return Err(error(Rule::Placement, "cannot move a closed instance"));
            }
            let collapsed = layout.containing(&instance)?.collapsed;
            remember(layout, &instance)?;
            detach(layout, &instance);
            let new_section = match &destination {
                Destination::NewSection { section, .. } => Some(section.clone()),
                _ => None,
            };
            place(layout, &instance, destination)?;
            if let Some(id) = new_section {
                section_mut(layout, &id)?.collapsed = collapsed;
            }
        }
        Mutation::Activate { instance } => {
            available(availability, &instance)?;
            let section = layout.containing(&instance)?.id.clone();
            section_mut(layout, &section)?.active = instance;
        }
        Mutation::Focus { instance } => {
            available(availability, &instance)?;
            let p = layout.instance(&instance)?.clone();
            if !p.requested_open {
                let destination = reopen_destination(layout, &p);
                place(layout, &instance, destination)?;
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
            available(availability, &instance)?;
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
            section_mut(layout, &section)?.size = size;
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
            section_mut(layout, &first)?.size = first_size;
            section_mut(layout, &second)?.size = second_size;
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
            if zone == Zone::Bottom {
                a_size.width = split;
                b_size.width = extent - split;
            } else {
                a_size.height = split;
                b_size.height = extent - split;
            }
            section_mut(layout, &first)?.size = a_size;
            section_mut(layout, &second)?.size = b_size;
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
                && let Some(target) = layout.sections.iter_mut().find(|s| s.id == join.section && s.location.zone() == return_to.zone)
            {
                if target.tabs.len().saturating_add(section.tabs.len()) > MAX_TABS {
                    return Err(error(Rule::CollectionLimit, "native return tab capacity"));
                }
                let index = join.index.min(target.tabs.len());
                drop(target.tabs.splice(index..index, section.tabs));
                return Ok(());
            }
            section.location = SectionLocation::Dock { zone: return_to.zone };
            section.size = return_to.size;
            section.collapsed = return_to.collapsed;
            layout.sections.insert(return_to.index.min(layout.sections.len()), section);
        }
    }
    let windows = native_hosts(layout);
    // Retained unsupported windows can be closed/redocked without native capability.
    if caps.native_windows && windows.len() > caps.max_native_hosts {
        return Err(error(Rule::Capability, "native host limit"));
    }
    Ok(())
}

fn pair<'a>(layout: &'a Layout, first: &SectionId, second: &SectionId) -> PanelResult<(&'a Section, &'a Section, Zone)> {
    if first == second {
        return Err(error(Rule::Placement, "sash requires two distinct neighbours"));
    }
    let a = layout.section(first)?;
    let b = layout.section(second)?;
    let zone = a.location.zone();
    let edge: Vec<_> = layout.sections.iter().filter(|s| s.location.zone() == zone).collect();
    if !zone.docked()
        || b.location.zone() != zone
        || !edge.windows(2).any(|p| p.first().is_some_and(|s| s.id == *first) && p.get(1).is_some_and(|s| s.id == *second))
    {
        return Err(error(Rule::Placement, "sash requires ordered neighbours in one dock"));
    }
    Ok((a, b, zone))
}
